use std::fs::{self, File};
use std::io::{self, Write};
use std::path::{Path, PathBuf};

#[derive(Debug)]
pub struct SingletonLock {
    path: PathBuf,
    file: File,
}

#[cfg(windows)]
impl SingletonLock {
    /// Try to acquire the lock file at `path`.
    ///
    /// Returns `Ok(None)` if another live process already holds it.
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
                    if let Some(pid) = owner {
                        if pid_alive(pid) {
                            return Ok(None);
                        }
                    }
                    // Stale or unreadable lock: clear it and retry.
                    if let Err(e) = fs::remove_file(path) {
                        if e.kind() != io::ErrorKind::NotFound {
                            return Err(e);
                        }
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
fn pid_alive(pid: u32) -> bool {
    use windows_sys::Win32::Foundation::{CloseHandle, GetLastError, ERROR_ACCESS_DENIED};
    use windows_sys::Win32::System::Threading::{OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION};

    unsafe {
        let handle = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid);
        if handle.is_null() {
            // Access denied means the process exists but we can't open it.
            return GetLastError() == ERROR_ACCESS_DENIED;
        }
        CloseHandle(handle);
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_lock_path(name: &str) -> PathBuf {
        std::env::temp_dir().join(name)
    }

    #[test]
    fn acquire_and_release() {
        let path = temp_lock_path("aurora_lock_test_basic.lock");
        let _ = fs::remove_file(&path);

        let lock = SingletonLock::acquire(&path).unwrap();
        assert!(lock.is_some());
        assert!(SingletonLock::acquire(&path).unwrap().is_none());

        drop(lock);
        assert!(!path.exists());
    }

    #[test]
    fn stale_lock_is_reclaimed() {
        let path = temp_lock_path("aurora_lock_test_stale.lock");
        let _ = fs::remove_file(&path);

        fs::write(&path, u32::MAX.to_string()).unwrap();
        let lock = SingletonLock::acquire(&path).unwrap();
        assert!(lock.is_some());
        drop(lock);
    }

    #[test]
    fn garbage_lock_is_reclaimed() {
        let path = temp_lock_path("aurora_lock_test_garbage.lock");
        let _ = fs::remove_file(&path);

        fs::write(&path, "not-a-pid").unwrap();
        let lock = SingletonLock::acquire(&path).unwrap();
        assert!(lock.is_some());
        drop(lock);
    }
}
