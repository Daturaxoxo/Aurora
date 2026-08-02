#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

#[cfg(windows)]
mod logfile;
#[cfg(windows)]
mod net;
#[cfg(windows)]
mod run;

#[cfg(windows)]
fn main() {
    run::main();
}

#[cfg(not(windows))]
fn main() {
    eprintln!("The Aurora updater is Windows-only; Linux builds update in-place as an AppImage.");
    std::process::exit(1);
}
