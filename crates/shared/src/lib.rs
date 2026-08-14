#![feature(integer_casts)]
#![feature(file_buffered)]

#[cfg(target_os = "linux")]
pub mod appimage;
pub mod archive;
pub mod config;
pub mod desktop_entry;
pub mod display;
pub mod logger;
pub mod pathfind;
#[cfg(target_os = "windows")]
pub mod repair;
pub mod telemetry;
pub mod utils;
pub mod classes {
    pub mod export;
    pub mod gamebanana;
    pub mod games;
    pub mod info;
    #[cfg(target_os = "linux")]
    pub mod steam;
}
pub mod api;
