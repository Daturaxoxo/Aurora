use std::io::{self, Read, Write};
use std::sync::Arc;
use std::sync::mpsc::{self, Receiver};
use std::thread;
use std::time::{Duration, Instant};

use interprocess::local_socket::{
    GenericNamespaced, Listener, ListenerNonblockingMode, ListenerOptions, Stream, ToNsName,
    traits::{Listener as _, Stream as _},
};
use serde::{Deserialize, Serialize};

pub use interprocess::local_socket::{Listener as IpcListener, Stream as IpcStream};

const MAX_FRAME_LEN: u32 = 64 * 1024;

const ACCEPT_POLL_INTERVAL: Duration = Duration::from_millis(20);

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Message {
    /// Updater -> Aurora: connection message.
    Hello,
    /// Updater -> Aurora: update in progress, lock the UI.
    Lock,
    /// Updater -> Aurora: update finished (non-exe files!), UI back to normal.
    Unlock,
    /// Updater -> Aurora: Aurora.exe is being replaced, exit
    CloseNow,
    /// Updater -> Aurora: periodic liveness ping while locked.
    Heartbeat,
    /// Updater -> Aurora: download progress while locked.
    Progress {
        file_index: u32,
        file_count: u32,
        bytes_done: u64,
        bytes_total: u64,
    },
    /// Aurora -> Updater: new Aurora launched successfully after an exe swap.
    InitConfirmed,
    /// Updater -> Aurora: update failed.
    Error { message: String },
    /// Updater -> Aurora: manifest matches local state, updater exits.
    NoUpdate,
    /// One-click -> Aurora: browser 1-click request.
    OneClick {
        url: String,
        model: String,
        item_id: u32,
    },
}

pub fn write_message<W: Write>(writer: &mut W, msg: &Message) -> io::Result<()> {
    let payload = serde_json::to_vec(msg).map_err(io::Error::other)?;
    let len = u32::try_from(payload.len()).map_err(io::Error::other)?;
    writer.write_all(&len.to_le_bytes())?;
    writer.write_all(&payload)?;
    writer.flush()
}

pub fn read_message<R: Read>(reader: &mut R) -> io::Result<Message> {
    let mut len_buf = [0u8; 4];
    reader.read_exact(&mut len_buf)?;
    let len = u32::from_le_bytes(len_buf);
    if len > MAX_FRAME_LEN {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!("frame length {len} exceeds limit"),
        ));
    }
    let mut payload = vec![0u8; len as usize];
    reader.read_exact(&mut payload)?;
    serde_json::from_slice(&payload).map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))
}

pub fn listen(pipe: &str) -> io::Result<Listener> {
    let name = pipe.to_ns_name::<GenericNamespaced>()?;
    ListenerOptions::new().name(name).create_sync()
}

#[cfg(windows)]
pub fn listen_cross_elevation(pipe: &str) -> io::Result<Listener> {
    use interprocess::os::windows::local_socket::ListenerOptionsExt as _;
    use interprocess::os::windows::security_descriptor::SecurityDescriptor;
    let sddl = format!(
        "D:P(A;;GA;;;SY)(A;;GA;;;BA)(A;;GA;;;{})S:(ML;;NW;;;ME)",
        current_user_sid()?
    );
    let sddl = widestring::U16CString::from_str(&sddl).map_err(io::Error::other)?;
    let sd = SecurityDescriptor::deserialize(&sddl)?;

    let name = pipe.to_ns_name::<GenericNamespaced>()?;
    ListenerOptions::new()
        .name(name)
        .security_descriptor(sd)
        .create_sync()
}

#[cfg(not(windows))]
pub fn listen_cross_elevation(pipe: &str) -> io::Result<Listener> {listen(pipe)}

/// The current process user's SID in string form, for building an SDDL string.
#[cfg(windows)]
fn current_user_sid() -> io::Result<String> {
    use windows_sys::Win32::Foundation::{CloseHandle, HANDLE, LocalFree};
    use windows_sys::Win32::Security::Authorization::ConvertSidToStringSidW;
    use windows_sys::Win32::Security::{GetTokenInformation, TOKEN_QUERY, TOKEN_USER, TokenUser};
    use windows_sys::Win32::System::Threading::{GetCurrentProcess, OpenProcessToken};

    struct Handle(HANDLE);
    impl Drop for Handle {
        fn drop(&mut self) {
            unsafe { CloseHandle(self.0) };
        }
    }

    unsafe {
        let mut raw: HANDLE = std::ptr::null_mut();
        if OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &raw mut raw) == 0 {return Err(io::Error::last_os_error());}
        let token = Handle(raw);
        let mut needed: u32 = 0;
        GetTokenInformation(token.0, TokenUser, std::ptr::null_mut(), 0, &raw mut needed);
        if needed == 0 {return Err(io::Error::last_os_error());}

        let mut buffer = vec![0u8; needed as usize];
        if GetTokenInformation(
            token.0,
            TokenUser,
            buffer.as_mut_ptr().cast(),
            needed,
            &raw mut needed,
        ) == 0
        {return Err(io::Error::last_os_error());}

        let user: *const TOKEN_USER = buffer.as_ptr().cast();
        let mut sid_w: *mut u16 = std::ptr::null_mut();
        if ConvertSidToStringSidW((*user).User.Sid, &raw mut sid_w) == 0 {
            return Err(io::Error::last_os_error());
        }

        let mut len = 0;
        while *sid_w.add(len) != 0 {
            len += 1;
        }
        let sid = String::from_utf16(std::slice::from_raw_parts(sid_w, len))
            .map_err(io::Error::other);
        LocalFree(sid_w.cast());
        sid
    }
}

pub fn connect(pipe: &str) -> io::Result<Stream> {
    let name = pipe.to_ns_name::<GenericNamespaced>()?;
    Stream::connect(name)
}

pub fn send_and_confirm(pipe: &str, msg: &Message, timeout: Duration) -> io::Result<()> {
    let mut stream = connect(pipe)?;
    write_message(&mut stream, msg)?;
    let _ = set_read_timeout(&stream, Some(timeout));
    let stream = Arc::new(stream);
    let reader = Arc::clone(&stream);
    let (tx, rx) = mpsc::channel();
    thread::spawn(move || {
        let mut reader = &*reader;
        let mut sink = [0u8; 1];
        let _ = tx.send(reader.read(&mut sink).map(|_| ()));
    });

    match rx.recv_timeout(timeout) {
        Ok(Ok(())) => Ok(()),
        Ok(Err(e))
            if matches!(
                e.kind(),
                io::ErrorKind::BrokenPipe
                    | io::ErrorKind::ConnectionReset
                    | io::ErrorKind::ConnectionAborted
            ) =>
        {
            Ok(())
        }
        Ok(Err(e)) => Err(e),
        Err(_) => Ok(()),
    }
}

pub fn accept(listener: &Listener) -> io::Result<Stream> {
    listener.accept()
}

pub fn accept_timeout(listener: &Listener, timeout: Duration) -> io::Result<Stream> {
    listener.set_nonblocking(ListenerNonblockingMode::Accept)?;
    let deadline = Instant::now() + timeout;
    let result = loop {
        match listener.accept() {
            Ok(stream) => break Ok(stream),
            Err(e) if e.kind() == io::ErrorKind::WouldBlock => {
                if Instant::now() >= deadline {
                    break Err(io::Error::new(
                        io::ErrorKind::TimedOut,
                        "timed out waiting for a peer to connect",
                    ));
                }
                thread::sleep(ACCEPT_POLL_INTERVAL);
            }
            Err(e) => break Err(e),
        }
    };
    let _ = listener.set_nonblocking(ListenerNonblockingMode::Neither);
    result
}

pub fn set_read_timeout(stream: &Stream, timeout: Option<Duration>) -> io::Result<()> {
    stream.set_recv_timeout(timeout)
}

pub fn spawn_reader(stream: Arc<Stream>) -> Receiver<io::Result<Message>> {
    let (tx, rx) = mpsc::channel();
    thread::spawn(move || {
        let mut reader = &*stream;
        loop {
            let msg = read_message(&mut reader);
            let failed = msg.is_err();
            if tx.send(msg).is_err() || failed {
                break;
            }
        }
    });
    rx
}