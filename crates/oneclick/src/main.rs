#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]
use ipc::oneclick::OneClick;
use ipc::protocol::{self, Message};
use std::process::ExitCode;
use std::time::{Duration, Instant};
const CONNECT_ATTEMPTS: u32 = 4;
const CONNECT_RETRY_DELAY: Duration = Duration::from_millis(500);
const STARTUP_GRACE: Duration = Duration::from_secs(30);

fn main() -> ExitCode {
    let Some(request) = ipc::oneclick::from_args() else {
        return ExitCode::from(2);
    };

    if forward(&request) {
        return ExitCode::SUCCESS;
    }

    match launch_aurora() {
        Ok(()) => ExitCode::SUCCESS,
        #[cfg(windows)]
        Err(LaunchError::Declined) => ExitCode::FAILURE,
        Err(LaunchError::Failed(e)) => {
            report(&format!(
                "Aurora could not be started to install this mod.\n\n{e}"
            ));
            ExitCode::FAILURE
        }
    }
}

fn forward(request: &OneClick) -> bool {
    let deadline = Instant::now() + STARTUP_GRACE;
    let mut attempt = 0;

    let message = Message::OneClick {
        url: request.url.clone(),
        model: request.model.clone(),
        item_id: request.item_id,
    };

    loop {
        attempt += 1;
        if protocol::send_and_confirm(
            &ipc::oneclick_pipe_name(),
            &message,
            ipc::ONECLICK_ACK_TIMEOUT,
        )
        .is_ok()
        {
            return true;
        }

        let starting = aurora_is_starting() && Instant::now() < deadline;
        if attempt >= CONNECT_ATTEMPTS && !starting {
            return false;
        }
        std::thread::sleep(CONNECT_RETRY_DELAY);
    }
}

#[cfg(windows)]
fn aurora_is_starting() -> bool {
    ipc::lock::holder_pid(&ipc::instance_root().join(ipc::AURORA_LOCK_FILE)).is_some()
}

#[cfg(not(windows))]
const fn aurora_is_starting() -> bool {
    false
}

enum LaunchError {
    #[cfg(windows)]
    Declined,
    Failed(String),
}

#[cfg(windows)]
fn launch_aurora() -> Result<(), LaunchError> {
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::UI::Shell::ShellExecuteW;
    use windows_sys::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;

    fn wide(text: &std::ffi::OsStr) -> Vec<u16> {
        text.encode_wide().chain(std::iter::once(0)).collect()
    }
    const SE_ERR_ACCESSDENIED: isize = 5;

    let root = ipc::install_root();
    let exe = root.join(ipc::AURORA_EXE);
    if !exe.is_file() {
        return Err(LaunchError::Failed(format!(
            "{} was not found.",
            exe.display()
        )));
    }

    let file = wide(exe.as_os_str());
    let params = wide(std::ffi::OsStr::new(&raw_uri()));
    let dir = wide(root.as_os_str());
    let result = unsafe {
        ShellExecuteW(
            std::ptr::null_mut(),
            std::ptr::null(),
            file.as_ptr(),
            params.as_ptr(),
            dir.as_ptr(),
            SW_SHOWNORMAL,
        )
    };

    match result as isize {
        code if code > 32 => Ok(()),
        SE_ERR_ACCESSDENIED => Err(LaunchError::Declined),
        code => Err(LaunchError::Failed(format!(
            "ShellExecuteW failed (code {code})"
        ))),
    }
}

#[cfg(not(windows))]
fn launch_aurora() -> Result<(), LaunchError> {
    let exe = ipc::install_root().join(ipc::AURORA_EXE);
    if !exe.is_file() {
        return Err(LaunchError::Failed(format!(
            "{} was not found.",
            exe.display()
        )));
    }
    std::process::Command::new(&exe)
        .arg(raw_uri())
        .spawn()
        .map(|_| ())
        .map_err(|e| LaunchError::Failed(format!("could not start {}: {e}", exe.display())))
}

fn raw_uri() -> String {
    let prefix = format!("{}:", ipc::oneclick::SCHEME);
    std::env::args()
        .find(|arg| arg.starts_with(&prefix))
        .unwrap_or_default()
}

#[cfg(windows)]
fn report(message: &str) {
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::UI::WindowsAndMessaging::{MB_ICONERROR, MB_OK, MessageBoxW};

    fn wide(text: &str) -> Vec<u16> {
        std::ffi::OsStr::new(text)
            .encode_wide()
            .chain(std::iter::once(0))
            .collect()
    }

    unsafe {
        MessageBoxW(
            std::ptr::null_mut(),
            wide(message).as_ptr(),
            wide("Aurora 1-Click").as_ptr(),
            MB_OK | MB_ICONERROR,
        );
    }
}

#[cfg(not(windows))]
fn report(message: &str) {
    eprintln!("Aurora 1-Click: {message}");
}
