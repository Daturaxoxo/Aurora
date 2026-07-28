use crate::MainWindow;
use slint::Weak;

pub struct ToastHandler;

impl ToastHandler {
    pub fn setup(window_weak: Weak<MainWindow>) {
        if let Some(w) = window_weak.upgrade() {
            let weak = window_weak;
            w.on_trigger_toast(move || {
                let w = weak.unwrap();
                w.set_toast_text("Operation completed successfully.".into());
                w.set_toast_kind("success".into());
                w.set_toast_active(true);
            });
        }
    }

    pub fn show(window_weak: &Weak<MainWindow>, text: impl Into<slint::SharedString>, kind: &str) {
        let text = text.into();
        let kind = kind.to_string();
        let ww = window_weak.clone();
        let _ = slint::invoke_from_event_loop(move || {
            if let Some(w) = ww.upgrade() {
                w.set_toast_text(text);
                w.set_toast_kind(kind.into());
                w.set_toast_active(true);
            }
        });
    }
}