use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};

/// Held for the lifetime of the process. The lock file is removed on drop.
#[derive(Debug)]
pub struct SingletonLock {
    path: PathBuf,
}

impl SingletonLock {
    /// Try to acquire the lock file at `path`.
    ///
    /// Returns `Ok(None)` if another live process already holds it.
    pub fn acquire(path: &Path) -> io::Result<Option<Self>> {
        match Self::try_create(path) {
            Ok(lock) => Ok(Some(lock)),
            Err(e) if e.kind() == io::ErrorKind::AlreadyExists => {
                let owner = fs::read_to_string(path)
                    .ok()
                    .and_then(|s| s.trim().parse::<u32>().ok());
                match owner {
                    Some(pid) if pid_alive(pid) => Ok(None),
                    // Retry once
                    _ => {
                        fs::remove_file(path)?;
                        match Self::try_create(path) {
                            Ok(lock) => Ok(Some(lock)),
                            Err(e) if e.kind() == io::ErrorKind::AlreadyExists => Ok(None),
                            Err(e) => Err(e),
                        }
                    }
                }
            }
            Err(e) => Err(e),
        }
    }

    fn try_create(path: &Path) -> io::Result<Self> {
        let mut file = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(path)?;
        write!(file, "{}", std::process::id())?;
        Ok(Self {
            path: path.to_path_buf(),
        })
    }
}

impl Drop for SingletonLock {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.path);
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

#[cfg(not(windows))]
fn pid_alive(pid: u32) -> bool {
    Path::new(&format!("/proc/{pid}")).exists()
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
        // Held by our own (live) PID: second acquire fails.
        assert!(SingletonLock::acquire(&path).unwrap().is_none());

        drop(lock);
        assert!(!path.exists());
    }

    #[test]
    fn stale_lock_is_reclaimed() {
        let path = temp_lock_path("aurora_lock_test_stale.lock");
        let _ = fs::remove_file(&path);

        // u32::MAX is not a plausible live PID on either platform.
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
