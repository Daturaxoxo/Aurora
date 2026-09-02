#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]
#![cfg_attr(all(test, not(windows)), allow(dead_code))]

#[cfg(any(windows, test))]
mod logfile;
#[cfg(any(windows, test))]
mod net;
#[cfg(any(windows, test))]
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
