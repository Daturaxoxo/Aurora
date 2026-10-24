// Copyright © SixtyFPS GmbH <info@slint.dev>
// SPDX-License-Identifier: GPL-3.0-only OR LicenseRef-Slint-Royalty-free-2.0 OR LicenseRef-Slint-Software-3.0

use std::{cell::RefCell, num::NonZeroU32, rc::Rc};
use i_slint_core::api::PlatformError;
use crate::{
    BeginRendering, FemtoVGRenderer, GraphicsBackend, WindowSurface, itemrenderer::CanvasRc,
};

#[allow(unsafe_code)]
pub unsafe trait OpenGLInterface {
    fn ensure_current(&self) -> Result<(), Box<dyn std::error::Error + Send + Sync>>;
    fn swap_buffers(&self) -> Result<(), Box<dyn std::error::Error + Send + Sync>>;
    fn resize(
        &self,
        width: NonZeroU32,
        height: NonZeroU32,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>>;
    fn get_proc_address(&self, name: &std::ffi::CStr) -> *const std::ffi::c_void;
}

#[cfg(target_arch = "wasm32")]
struct WebGLNeedsNoCurrentContext;
#[cfg(target_arch = "wasm32")]
unsafe impl OpenGLInterface for WebGLNeedsNoCurrentContext {
    fn ensure_current(&self) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        Ok(())
    }

    fn swap_buffers(&self) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        Ok(())
    }

    fn resize(
        &self,
        _width: NonZeroU32,
        _height: NonZeroU32,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        Ok(())
    }

    fn get_proc_address(&self, _: &std::ffi::CStr) -> *const std::ffi::c_void {
        unreachable!()
    }
}

struct SuspendedRenderer {}

unsafe impl OpenGLInterface for SuspendedRenderer {
    fn ensure_current(&self) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        Err("ensure current called on suspended renderer".to_string().into())
    }

    fn swap_buffers(&self) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        Err("swap_buffers called on suspended renderer".to_string().into())
    }

    fn resize(
        &self,
        _: NonZeroU32,
        _: NonZeroU32,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        Ok(())
    }

    fn get_proc_address(&self, _: &std::ffi::CStr) -> *const std::ffi::c_void {
        panic!("get_proc_address called on suspended renderer")
    }
}
struct SnapshotTargetGuard<'a>(&'a RefCell<Option<femtovg::ImageId>>);

impl Drop for SnapshotTargetGuard<'_> {
    fn drop(&mut self) {
        self.0.borrow_mut().take();
    }
}

pub struct OpenGLBackend {
    opengl_context: RefCell<Box<dyn OpenGLInterface>>,
    snapshot_target: RefCell<Option<femtovg::ImageId>>,
    #[cfg(target_family = "wasm")]
    html_canvas: RefCell<Option<web_sys::HtmlCanvasElement>>,
}

impl OpenGLBackend {
    pub fn set_opengl_context(
        &self,
        renderer: &FemtoVGRenderer<Self>,
        #[cfg(not(target_arch = "wasm32"))] opengl_context: impl OpenGLInterface + 'static,
        #[cfg(target_arch = "wasm32")] html_canvas: web_sys::HtmlCanvasElement,
    ) -> Result<(), i_slint_core::platform::PlatformError> {
        #[cfg(target_arch = "wasm32")]
        let opengl_context = WebGLNeedsNoCurrentContext {};

        let opengl_context = Box::new(opengl_context);
        #[cfg(not(target_arch = "wasm32"))]
        let gl_renderer = unsafe {
            femtovg::renderer::OpenGl::new_from_function_cstr(|name| {
                opengl_context.get_proc_address(name)
            })
            .unwrap()
        };

        #[cfg(target_arch = "wasm32")]
        let gl_renderer = match femtovg::renderer::OpenGl::new_from_html_canvas(&html_canvas) {
            Ok(gl_renderer) => gl_renderer,
            Err(_) => {
                use wasm_bindgen::JsCast;
                let context_2d = html_canvas
                    .get_context("2d")
                    .unwrap()
                    .unwrap()
                    .dyn_into::<web_sys::CanvasRenderingContext2d>()
                    .unwrap();
                context_2d.set_font("20px serif");
                context_2d.set_fill_style_str("red");
                context_2d
                    .fill_text("Slint requires WebGL to be enabled in your browser", 0., 30.)
                    .unwrap();
                panic!("Cannot proceed without WebGL - aborting")
            }
        };

        let femtovg_canvas = femtovg::Canvas::new_with_text_context(
            gl_renderer,
            crate::font_cache::FONT_CACHE.with(|cache| cache.borrow().text_context.clone()),
        )
        .unwrap();

        *self.opengl_context.borrow_mut() = opengl_context;
        #[cfg(target_family = "wasm")]
        {
            *self.html_canvas.borrow_mut() = Some(html_canvas);
        }

        let canvas = Rc::new(RefCell::new(femtovg_canvas));
        renderer.reset_canvas(canvas);
        Ok(())
    }

    fn render_snapshot(
        &self,
        canvas: &CanvasRc<femtovg::renderer::OpenGl>,
        image_id: femtovg::ImageId,
        width: u32,
        height: u32,
        render: &dyn Fn() -> Result<(), PlatformError>,
    ) -> Result<
        i_slint_core::graphics::SharedPixelBuffer<i_slint_core::graphics::Rgba8Pixel>,
        PlatformError,
    > {
        *self.snapshot_target.borrow_mut() = Some(image_id);
        let guard = SnapshotTargetGuard(&self.snapshot_target);
        let render_result = render();
        drop(guard);
        render_result?;
        let mut canvas = canvas.borrow_mut();
        crate::select_render_target(&mut canvas, femtovg::RenderTarget::Image(image_id));
        canvas.flush_to_output(());

        let screenshot = canvas
            .screenshot()
            .map_err(|e| format!("FemtoVG error reading back snapshot texture: {e}"))?;

        if screenshot.width() as u32 != width || screenshot.height() as u32 != height {
            return Err(format!(
                "take_snapshot: read back {}x{} pixels instead of the requested {width}x{height}",
                screenshot.width(),
                screenshot.height()
            )
            .into());
        }

        use rgb::ComponentBytes;
        Ok(i_slint_core::graphics::SharedPixelBuffer::clone_from_slice(
            screenshot.buf().as_bytes(),
            width,
            height,
        ))
    }
}

pub enum GLWindowSurface {
    Screen,
    Snapshot(femtovg::ImageId),
}

impl WindowSurface<femtovg::renderer::OpenGl> for GLWindowSurface {
    fn render_output(
        &self,
    ) -> impl Into<<femtovg::renderer::OpenGl as femtovg::Renderer>::RenderOutput> {
    }

    fn initial_render_target(&self) -> femtovg::RenderTarget {
        match self {
            Self::Screen => femtovg::RenderTarget::Screen,
            Self::Snapshot(image_id) => femtovg::RenderTarget::Image(*image_id),
        }
    }
}

impl GraphicsBackend for OpenGLBackend {
    type Renderer = femtovg::renderer::OpenGl;
    type WindowSurface = GLWindowSurface;
    const NAME: &'static str = "OpenGL";

    fn new_suspended() -> Self {
        Self {
            opengl_context: RefCell::new(Box::new(SuspendedRenderer {})),
            snapshot_target: RefCell::new(None),
            #[cfg(target_family = "wasm")]
            html_canvas: RefCell::new(None),
        }
    }

    fn clear_graphics_context(&self) {
        *self.opengl_context.borrow_mut() = Box::new(SuspendedRenderer {});
    }

    fn begin_surface_rendering(
        &self,
    ) -> Result<BeginRendering<GLWindowSurface>, Box<dyn std::error::Error + Send + Sync>> {
        self.opengl_context.borrow().ensure_current()?;
        Ok(BeginRendering::Acquired(match *self.snapshot_target.borrow() {
            Some(image_id) => GLWindowSurface::Snapshot(image_id),
            None => GLWindowSurface::Screen,
        }))
    }

    fn submit_commands(&self, _commands: <Self::Renderer as femtovg::Renderer>::CommandBuffer) {}
    fn present_surface(
        &self,
        surface: GLWindowSurface,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        match surface {
            GLWindowSurface::Screen => self.opengl_context.borrow().swap_buffers(),
            GLWindowSurface::Snapshot(_) => Ok(()),
        }
    }

    #[cfg(not(target_family = "wasm"))]
    fn with_graphics_api<R>(
        &self,
        callback: impl FnOnce(Option<i_slint_core::api::GraphicsAPI<'_>>) -> R,
    ) -> Result<R, i_slint_core::platform::PlatformError> {
        use i_slint_core::api::GraphicsAPI;

        self.opengl_context.borrow().ensure_current()?;
        let api = GraphicsAPI::NativeOpenGL {
            get_proc_address: &|name| self.opengl_context.borrow().get_proc_address(name),
        };
        Ok(callback(Some(api)))
    }

    #[cfg(target_family = "wasm")]
    fn with_graphics_api<R>(
        &self,
        callback: impl FnOnce(Option<i_slint_core::api::GraphicsAPI<'_>>) -> R,
    ) -> Result<R, i_slint_core::platform::PlatformError> {
        use i_slint_core::api::GraphicsAPI;

        let id =
            self.html_canvas.borrow().as_ref().map_or_else(|| String::new(), |canvas| canvas.id());

        let api = GraphicsAPI::WebGL { canvas_element_id: &id, context_type: "webgl2" };
        Ok(callback(Some(api)))
    }

    fn resize(
        &self,
        width: NonZeroU32,
        height: NonZeroU32,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        self.opengl_context.borrow().resize(width, height)
    }

    fn take_snapshot_pixels(
        &self,
        canvas: Option<CanvasRc<Self::Renderer>>,
        width: u32,
        height: u32,
        render: &dyn Fn() -> Result<(), PlatformError>,
    ) -> Option<
        Result<
            i_slint_core::graphics::SharedPixelBuffer<i_slint_core::graphics::Rgba8Pixel>,
            PlatformError,
        >,
    > {
        let canvas = canvas?;
        Some((|| {
            self.opengl_context
                .borrow()
                .ensure_current()
                .map_err(|e| PlatformError::Other(e.to_string()))?;

            let image_id = canvas
                .borrow_mut()
                .create_image_empty(
                    width as usize,
                    height as usize,
                    femtovg::PixelFormat::Rgba8,
                    femtovg::ImageFlags::empty(),
                )
                .map_err(|e| format!("FemtoVG error allocating snapshot texture: {e}"))?;

            let snapshot = self.render_snapshot(&canvas, image_id, width, height, render);
            let mut canvas = canvas.borrow_mut();
            canvas.set_render_target(femtovg::RenderTarget::Screen);
            canvas.delete_image(image_id);

            snapshot
        })())
    }
}

impl FemtoVGRenderer<OpenGLBackend> {
    pub fn new(
        #[cfg(not(target_arch = "wasm32"))] opengl_context: impl OpenGLInterface + 'static,
        #[cfg(target_arch = "wasm32")] html_canvas: web_sys::HtmlCanvasElement,
    ) -> Result<Self, PlatformError> {
        use super::FemtoVGRendererExt;
        let this = Self::new_suspended();
        this.graphics_backend.set_opengl_context(
            &this,
            #[cfg(not(target_arch = "wasm32"))]
            opengl_context,
            #[cfg(target_arch = "wasm32")]
            html_canvas,
        )?;
        Ok(this)
    }
}
