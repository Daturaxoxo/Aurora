use crate::MainWindow;
use crate::classes::pages::modmanager::ModManagerHandler;
use crate::classes::pages::modules::ModulesHandler;

use i_slint_backend_winit::WinitWindowAccessor;
use i_slint_backend_winit::winit::raw_window_handle::{HasWindowHandle, RawWindowHandle};
use log::*;
use slint::ComponentHandle;

use std::cell::Cell;
use std::ffi::OsString;
use std::os::windows::ffi::OsStringExt;
use std::path::PathBuf;
use std::ptr::null_mut;

use windows_sys::Win32::Foundation::{HWND, LPARAM, LRESULT, TRUE, WPARAM};
use windows_sys::Win32::System::Ole::RevokeDragDrop;
use windows_sys::Win32::UI::Shell::{
    DefSubclassProc, DragAcceptFiles, DragFinish, DragQueryFileW, HDROP, RemoveWindowSubclass,
    SetWindowSubclass,
};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    ChangeWindowMessageFilterEx, MSGFLT_ALLOW, WM_COPYDATA, WM_DROPFILES, WM_NCDESTROY,
};

const WM_COPYGLOBALDATA: u32 = 0x0049;
const SUBCLASS_ID: usize = 0x4155_5244; // "AURD"

pub fn setup(window: &slint::Weak<MainWindow>) {
    let ww = window.clone();
    let installed = Cell::new(false);
    let deferred = Cell::new(false);
    window
        .unwrap()
        .window()
        .on_winit_window_event(move |w, _event| {
            if !installed.get() {
                if w.with_winit_window(|winit_win| install(winit_win, &ww)) == Some(true) {
                    installed.set(true);
                } else if !deferred.get() {
                    deferred.set(true);
                    warn!("[FileDrop] the window is not ready yet, retrying on the next event");
                }
            }
            i_slint_backend_winit::EventResult::Propagate
        });
}

fn install(
    winit_win: &i_slint_backend_winit::winit::window::Window,
    weak: &slint::Weak<MainWindow>,
) -> bool {
    let Ok(RawWindowHandle::Win32(handle)) = winit_win.window_handle().map(|h| h.as_raw()) else {
        error!("[FileDrop] could not get a Win32 window handle");
        return false;
    };
    let hwnd = handle.hwnd.get() as HWND;

    let ctx = Box::into_raw(Box::new(weak.clone()));

    unsafe {
        RevokeDragDrop(hwnd);
        DragAcceptFiles(hwnd, TRUE);

        // Allow the drop messages through from lower-integrity processes
        for msg in [WM_DROPFILES, WM_COPYDATA, WM_COPYGLOBALDATA] {
            if ChangeWindowMessageFilterEx(hwnd, msg, MSGFLT_ALLOW, null_mut()) == 0 {
                warn!("[FileDrop] ChangeWindowMessageFilterEx failed for {msg:#x}");
            }
        }

        if SetWindowSubclass(hwnd, Some(subclass_proc), SUBCLASS_ID, ctx as usize) == 0 {
            error!("[FileDrop] SetWindowSubclass failed");
            drop(Box::from_raw(ctx));
            return false;
        }
    }

    info!("[FileDrop] WM_DROPFILES handler installed");
    true
}

unsafe extern "system" fn subclass_proc(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
    _id: usize,
    refdata: usize,
) -> LRESULT {
    match msg {
        WM_DROPFILES => {
            let hdrop = wparam as HDROP;
            let paths = unsafe { dropped_paths(hdrop) };
            unsafe { DragFinish(hdrop) };

            let weak = unsafe { &*(refdata as *const slint::Weak<MainWindow>) };
            if let Some(win) = weak.upgrade()
                && !paths.is_empty()
            {
                if win.get_show_mod_manager() {
                    info!(
                        "[FileDrop] {} path(s) dropped on the mod manager",
                        paths.len()
                    );
                    ModManagerHandler::install_paths(weak, paths);
                } else if win.get_show_modules() {
                    info!("[FileDrop] {} path(s) dropped on the modules", paths.len());
                    ModulesHandler::install_paths(weak, paths);
                }
            }
            0
        }
        WM_NCDESTROY => unsafe {
            RemoveWindowSubclass(hwnd, Some(subclass_proc), SUBCLASS_ID);
            drop(Box::from_raw(refdata as *mut slint::Weak<MainWindow>));
            DefSubclassProc(hwnd, msg, wparam, lparam)
        },
        _ => unsafe { DefSubclassProc(hwnd, msg, wparam, lparam) },
    }
}

unsafe fn dropped_paths(hdrop: HDROP) -> Vec<PathBuf> {
    unsafe {
        let count = DragQueryFileW(hdrop, u32::MAX, null_mut(), 0);
        let mut paths = Vec::with_capacity(count as usize);
        for i in 0..count {
            let len = DragQueryFileW(hdrop, i, null_mut(), 0);
            if len == 0 {
                continue;
            }
            let mut buf = vec![0u16; len as usize + 1];
            let copied = DragQueryFileW(hdrop, i, buf.as_mut_ptr(), len.saturating_add(1));
            buf.truncate(copied as usize);
            paths.push(PathBuf::from(OsString::from_wide(&buf)));
        }

        paths
    }
}
