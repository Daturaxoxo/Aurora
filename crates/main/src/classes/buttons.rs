use crate::MainWindow;

pub struct ButtonHandler;

impl ButtonHandler {
    pub fn setup(window: &slint::Weak<MainWindow>) {
        let w = window.clone();
        window.unwrap().on_bottom_icon_clicked(move |index| {
            if let Some(w) = w.upgrade() {
                #[allow(clippy::match_same_arms)]
                match index {
                    0 => w.set_show_menu(!w.get_show_menu()),
                    1 => { /* mod manager */ }
                    2 => { /* discord */ }
                    3 => { /* gamebanana */ }
                    _ => {}
                }
            }
        });
    }
}
