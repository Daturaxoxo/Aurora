use slint::Weak;
use crate::MainWindow;

pub struct ToastHandler;

impl ToastHandler {
    pub fn setup(window_weak: Weak<MainWindow>) {
        if let Some(w) = window_weak.upgrade() {
            let weak = window_weak.clone();
            w.on_trigger_test_toast(move || {
                let w = weak.unwrap();
                w.set_toast_text("Operation completed successfully.".into());
                w.set_toast_kind("success".into());
                w.set_toast_active(true);
            });
        }
    }
}