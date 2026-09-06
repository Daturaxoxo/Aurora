use std::fs::{self, File};
use std::io::{self, Write};
use std::path::{Path, PathBuf};
#[cfg(windows)]
use std::time::{Duration, Instant};

#[derive(Debug)]
pub struct SingletonLock {
    path: PathBuf,
    file: File,
}

#[cfg(windows)]
impl SingletonLock {
    pub fn acquire(path: &Path) -> io::Result<Option<Self>> {
        for _ in 0..5 {
            match Self::try_create(path) {
                Ok(lock) => {
                    if owns_lock_file(&lock.path, &lock.file) {
                        return Ok(Some(lock));
                    }
                    drop(lock);
                }
                Err(e) if e.kind() == io::ErrorKind::AlreadyExists => {
                    let owner = fs::read_to_string(path)
                        .ok()
                        .and_then(|s| s.trim().parse::<u32>().ok());
                    if let Some(pid) = owner
                        && pid_alive(pid)
                    {
                        return Ok(None);
                    }
                    if let Err(e) = fs::remove_file(path)
                        && e.kind() != io::ErrorKind::NotFound
                    {
                        return Err(e);
                    }
                }
                Err(e) => return Err(e),
            }
        }

        Err(io::Error::new(
            io::ErrorKind::WouldBlock,
            "could not stabilize singleton lock file",
        ))
    }

    fn try_create(path: &Path) -> io::Result<Self> {
        use std::os::windows::fs::OpenOptionsExt;

        // https://learn.microsoft.com/en-us/windows/win32/api/fileapi/nf-fileapi-createfilew
        const FILE_SHARE_READ: u32 = 0x0000_0001;
        const FILE_SHARE_DELETE: u32 = 0x0000_0004;
        const FILE_FLAG_DELETE_ON_CLOSE: u32 = 0x0400_0000;

        let mut file = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .share_mode(FILE_SHARE_READ | FILE_SHARE_DELETE)
            .custom_flags(FILE_FLAG_DELETE_ON_CLOSE)
            .open(path)?;
        write!(file, "{}", std::process::id())?;
        file.flush()?;
        Ok(Self {
            path: path.to_path_buf(),
            file,
        })
    }
}

#[cfg(not(windows))]
impl SingletonLock {
    pub fn acquire(path: &Path) -> io::Result<Option<Self>> {
        use std::os::unix::fs::MetadataExt;

        for _ in 0..5 {
            let file = fs::OpenOptions::new()
                .write(true)
                .create(true)
                .truncate(false)
                .open(path)?;

            if !flock_nonblocking(&file)? {
                return Ok(None); // another live process holds the lock
            }

            let locked_ino = file.metadata()?.ino();
            match fs::metadata(path) {
                Ok(meta) if meta.ino() == locked_ino => {}
                _ => continue,
            }

            file.set_len(0)?;
            (&file).write_all(std::process::id().to_string().as_bytes())?;

            return Ok(Some(Self {
                path: path.to_path_buf(),
                file,
            }));
        }

        Err(io::Error::new(
            io::ErrorKind::WouldBlock,
            "could not stabilize singleton lock file",
        ))
    }
}

impl Drop for SingletonLock {
    fn drop(&mut self) {
        if owns_lock_file(&self.path, &self.file) {
            let _ = fs::remove_file(&self.path);
        }
    }
}

#[cfg(windows)]
fn owns_lock_file(path: &Path, _file: &File) -> bool {
    fs::read_to_string(path)
        .ok()
        .and_then(|s| s.trim().parse::<u32>().ok())
        == Some(std::process::id())
}

#[cfg(not(windows))]
fn owns_lock_file(path: &Path, file: &File) -> bool {
    use std::os::unix::fs::MetadataExt;

    let Ok(locked) = file.metadata() else {
        return false;
    };
    fs::metadata(path).is_ok_and(|meta| meta.ino() == locked.ino())
}

#[cfg(not(windows))]
fn flock_nonblocking(file: &File) -> io::Result<bool> {
    use std::os::unix::io::AsRawFd;

    let rc = unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) };
    if rc == 0 {
        return Ok(true);
    }
    let err = io::Error::last_os_error();
    match err.raw_os_error() {
        Some(libc::EWOULDBLOCK) => Ok(false),
        _ => Err(err),
    }
}

#[cfg(windows)]
pub fn holder_pid(path: &Path) -> Option<u32> {
    let pid = fs::read_to_string(path).ok()?.trim().parse::<u32>().ok()?;
    pid_alive(pid).then_some(pid)
}

#[cfg(windows)]
pub fn wait_until_released(path: &Path, timeout: Duration) -> bool {
    const POLL_INTERVAL: Duration = Duration::from_millis(100);

    let deadline = Instant::now() + timeout;
    loop {
        if holder_pid(path).is_none() {
            return true;
        }
        if Instant::now() >= deadline {
            return false;
        }
        std::thread::sleep(POLL_INTERVAL);
    }
}

#[cfg(windows)]
fn pid_alive(pid: u32) -> bool {
    use windows_sys::Win32::Foundation::{CloseHandle, ERROR_ACCESS_DENIED, GetLastError};
    use windows_sys::Win32::System::Threading::{
        GetExitCodeProcess, OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION,
    };

    // https://learn.microsoft.com/en-us/windows/win32/api/processthreadsapi/nf-processthreadsapi-getexitcodeprocess
    const STILL_ACTIVE: u32 = 259;

    unsafe {
        let handle = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid);
        if handle.is_null() {
            // Access denied means the process exists but we can't open it.
            return GetLastError() == ERROR_ACCESS_DENIED;
        }

        let mut code: u32 = 0;
        let queried = GetExitCodeProcess(handle, &raw mut code) != 0;
        CloseHandle(handle);
        !queried || code == STILL_ACTIVE
    }
}
