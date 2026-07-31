// classes/mod.rs
pub mod buttons;
#[cfg(target_os = "linux")]
pub mod desktop_entry;
#[cfg(target_os = "windows")]
pub mod filedrop;
pub mod logwindow;
pub mod pages;
pub mod popup;
pub mod repair;
pub mod toast;
pub mod updater;
