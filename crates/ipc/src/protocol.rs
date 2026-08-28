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
    /// Updater -> Aurora: sent once on connect, before any other message.
    Hello,
    /// Updater -> Aurora: update in progress, lock the UI.
    Lock,
    /// Updater -> Aurora: update finished (non-exe files), UI back to normal.
    Unlock,
    /// Updater -> Aurora: Aurora.exe itself is being replaced, exit cleanly.
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
    /// Second Aurora instance -> running Aurora: browser 1-click request.
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

/// Opens a listener that a process running unelevated can still connect to.
///
/// Aurora may be running elevated while the peer — `oneclick.exe`, launched by
/// the browser — is not. A pipe created by an elevated process is unreachable
/// from a medium-integrity one twice over: the elevated token's default DACL
/// does not grant the filtered token, and the object's high mandatory label
/// blocks the write access that connecting requires. An explicit descriptor
/// fixes both, dropping the label to medium and naming the user directly.
///
/// The result is a pipe reachable by any process running as this user, so use
/// it only where every message is safe to accept from that boundary. The
/// updater pipes deliberately keep the restrictive default.
#[cfg(windows)]
pub fn listen_cross_elevation(pipe: &str) -> io::Result<Listener> {
    use interprocess::os::windows::local_socket::ListenerOptionsExt as _;
    use interprocess::os::windows::security_descriptor::SecurityDescriptor;

    // SYSTEM and Administrators keep full control; the interactive user gets
    // enough to both create pipe instances (Aurora, elevated or not) and
    // connect to them (oneclick.exe). `ME` is the medium integrity label, and
    // `NW` is the usual no-write-up policy: at medium, a medium-integrity
    // client is no longer writing up, which is the whole point.
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
pub fn listen_cross_elevation(pipe: &str) -> io::Result<Listener> {
    // Nothing to relax: there is no elevation split, and the socket already
    // belongs to the user.
    listen(pipe)
}

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
        if OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &raw mut raw) == 0 {
            return Err(io::Error::last_os_error());
        }
        let token = Handle(raw);

        // Sized in a first call that is expected to fail with "buffer too small".
        let mut needed: u32 = 0;
        GetTokenInformation(token.0, TokenUser, std::ptr::null_mut(), 0, &raw mut needed);
        if needed == 0 {
            return Err(io::Error::last_os_error());
        }

        let mut buffer = vec![0u8; needed as usize];
        if GetTokenInformation(
            token.0,
            TokenUser,
            buffer.as_mut_ptr().cast(),
            needed,
            &raw mut needed,
        ) == 0
        {
            return Err(io::Error::last_os_error());
        }

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

/// Sends one message and stays connected until the far end has taken it.
///
/// Writing only fills the pipe buffer. A sender that exits immediately
/// afterwards tears the pipe down before a listener polling with
/// [`accept_timeout`] has accepted the connection, and the message is lost with
/// no error on either side — the write succeeded, so the sender believes it
/// delivered. Blocking until the listener closes the stream, which it does once
/// it has read, is what makes delivery something either side can rely on.
///
/// A timeout is not treated as failure: the connection was established and held
/// open long enough to be accepted, so the far end is there. It just did not
/// close in time.
pub fn send_and_confirm(pipe: &str, msg: &Message, timeout: Duration) -> io::Result<()> {
    let mut stream = connect(pipe)?;
    write_message(&mut stream, msg)?;

    // Best effort: several platforms, Windows named pipes included, refuse it.
    let _ = set_read_timeout(&stream, Some(timeout));

    // Windows named pipes reject read timeouts outright, so the wait is bounded
    // with a channel around a blocking read rather than by the socket itself.
    let stream = Arc::new(stream);
    let reader = Arc::clone(&stream);
    let (tx, rx) = mpsc::channel();
    thread::spawn(move || {
        let mut reader = &*reader;
        let mut sink = [0u8; 1];
        let _ = tx.send(reader.read(&mut sink).map(|_| ()));
    });

    match rx.recv_timeout(timeout) {
        // EOF, or the broken pipe Windows reports in its place: either way the
        // listener took the message and hung up.
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
        // No answer, but the connection stayed open for the whole timeout —
        // far longer than a listener needs to accept it, which is the part that
        // actually matters.
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn framing_round_trips_every_message() {
        let messages = [
            Message::Hello,
            Message::Lock,
            Message::Unlock,
            Message::CloseNow,
            Message::Heartbeat,
            Message::Progress {
                file_index: 2,
                file_count: 7,
                bytes_done: 1024,
                bytes_total: 4096,
            },
            Message::InitConfirmed,
            Message::Error {
                message: "boom".into(),
            },
            Message::NoUpdate,
            Message::OneClick {
                url: "https://gamebanana.com/mmdl/1".into(),
                model: "Mod".into(),
                item_id: 2,
            },
        ];

        let mut buf = Vec::new();
        for msg in &messages {
            write_message(&mut buf, msg).unwrap();
        }

        let mut cursor = std::io::Cursor::new(buf);
        for msg in &messages {
            assert_eq!(&read_message(&mut cursor).unwrap(), msg);
        }
    }

    #[test]
    fn oversized_frame_is_rejected() {
        let mut buf = Vec::new();
        buf.extend_from_slice(&(MAX_FRAME_LEN + 1).to_le_bytes());
        let err = read_message(&mut std::io::Cursor::new(buf)).unwrap_err();
        assert_eq!(err.kind(), std::io::ErrorKind::InvalidData);
    }

    #[test]
    fn messages_round_trip_over_a_real_pipe() {
        let pipe = "aurora-updater-proto-test";
        let listener = listen(pipe).unwrap();

        let server = std::thread::spawn(move || {
            let mut stream = accept(&listener).unwrap();
            let received = read_message(&mut stream).unwrap();
            write_message(&mut stream, &Message::Lock).unwrap();
            received
        });

        let mut client = connect(pipe).unwrap();
        write_message(&mut client, &Message::InitConfirmed).unwrap();
        assert_eq!(read_message(&mut client).unwrap(), Message::Lock);
        assert_eq!(server.join().unwrap(), Message::InitConfirmed);
    }

    #[test]
    fn truncated_frame_is_an_error() {
        let mut buf = Vec::new();
        write_message(&mut buf, &Message::Lock).unwrap();
        buf.truncate(buf.len() - 1);
        assert!(read_message(&mut std::io::Cursor::new(buf)).is_err());
    }

    /// The 1-click pipe is the one place Aurora accepts a peer that may be at a
    /// lower integrity level than itself. Read the descriptor back off the live
    /// pipe and prove both halves of that actually landed, since getting either
    /// wrong fails silently as "Aurora just doesn't respond to Install".
    #[cfg(windows)]
    #[test]
    fn cross_elevation_pipe_is_reachable_from_medium_integrity() {
        use windows_sys::Win32::Foundation::LocalFree;
        use windows_sys::Win32::Security::Authorization::{
            ConvertSecurityDescriptorToStringSecurityDescriptorW, GetNamedSecurityInfoW,
            SE_FILE_OBJECT,
        };
        use windows_sys::Win32::Security::{
            DACL_SECURITY_INFORMATION, LABEL_SECURITY_INFORMATION, PSECURITY_DESCRIPTOR,
        };

        let pipe = format!("aurora-sd-test-{}", std::process::id());
        let listener = listen_cross_elevation(&pipe).unwrap();

        let object: Vec<u16> = format!(r"\\.\pipe\{pipe}")
            .encode_utf16()
            .chain(std::iter::once(0))
            .collect();

        let sddl = unsafe {
            let mut sd: PSECURITY_DESCRIPTOR = std::ptr::null_mut();
            let status = GetNamedSecurityInfoW(
                object.as_ptr(),
                SE_FILE_OBJECT,
                DACL_SECURITY_INFORMATION | LABEL_SECURITY_INFORMATION,
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                &raw mut sd,
            );
            assert_eq!(status, 0, "GetNamedSecurityInfoW failed: {status}");

            let mut text: *mut u16 = std::ptr::null_mut();
            let ok = ConvertSecurityDescriptorToStringSecurityDescriptorW(
                sd,
                1, // SDDL_REVISION_1
                DACL_SECURITY_INFORMATION | LABEL_SECURITY_INFORMATION,
                &raw mut text,
                std::ptr::null_mut(),
            );
            assert_ne!(ok, 0, "could not stringify the descriptor");

            let mut len = 0;
            while *text.add(len) != 0 {
                len += 1;
            }
            let sddl = String::from_utf16(std::slice::from_raw_parts(text, len)).unwrap();
            LocalFree(text.cast());
            LocalFree(sd.cast());
            sddl
        };

        // Medium mandatory label: without this, a pipe made by an elevated
        // Aurora carries a high label and no-write-up blocks the connect.
        assert!(
            sddl.contains("(ML;;NW;;;ME)"),
            "mandatory label is not medium: {sddl}"
        );
        // And the user is named explicitly, because the elevated token's
        // default DACL grants Administrators, which is deny-only in the
        // filtered token that oneclick.exe runs under.
        let sid = current_user_sid().unwrap();
        assert!(sddl.contains(&sid), "user {sid} is not in the DACL: {sddl}");

        // Still an ordinary working pipe.
        let server = std::thread::spawn(move || {
            let mut stream = accept(&listener).unwrap();
            read_message(&mut stream).unwrap()
        });
        let mut client = connect(&pipe).unwrap();
        write_message(&mut client, &Message::Hello).unwrap();
        assert_eq!(server.join().unwrap(), Message::Hello);
    }

    /// A sender that writes and drops loses the message when the listener has
    /// not accepted yet — silently, because the write itself succeeds. The
    /// 1-click path is exactly that shape: the shim exits the instant it has
    /// sent. `send_and_confirm` has to outlive a listener that is slow to poll.
    #[test]
    fn send_and_confirm_survives_a_listener_that_accepts_late() {
        let pipe = format!("aurora-confirm-test-{}", std::process::id());
        let listener = listen_cross_elevation(&pipe).unwrap();

        let sender = {
            let pipe = pipe.clone();
            std::thread::spawn(move || {
                send_and_confirm(&pipe, &Message::Hello, Duration::from_secs(5))
            })
        };

        // The listener only gets around to accepting well after the send call
        // has done its writing.
        std::thread::sleep(Duration::from_millis(750));
        let mut stream = accept_timeout(&listener, Duration::from_secs(5)).unwrap();
        assert_eq!(read_message(&mut stream).unwrap(), Message::Hello);
        drop(stream);

        sender.join().unwrap().unwrap();
    }
}
