use crate::MainWindow;
pub struct PopupHandler;

impl PopupHandler {
    pub fn setup(window: &slint::Weak<MainWindow>) {
        let w = window.clone();
        window.unwrap().on_popup_confirm_callback(move |id| {
            if let Some(_w) = w.upgrade() {
                match id.as_str() {
                    "discord-popup" => {
                        let _ = open::that("https://discord.gg/565jfeYsbp");
                    }
                    "gamebanana-popup" => {
                        let _ = open::that("https://gamebanana.com/games/23012");
                    }
                    _ => {}
                }
            }
        });
    }
}
