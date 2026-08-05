// classes/mod.rs
pub mod buttons;
pub mod characters;
#[cfg(target_os = "linux")]
pub mod desktop;
#[cfg(target_os = "windows")]
pub mod filedrop;
pub mod logwindow;
pub mod modicons;
pub mod pages;
pub mod popup;
pub mod repair;
pub mod toast;
pub mod tray;
pub mod updater;
#[cfg(target_os = "linux")]
pub mod windowdrag;
