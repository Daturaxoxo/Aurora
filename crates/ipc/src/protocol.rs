use std::io::{self, Read, Write};

use interprocess::local_socket::{
    traits::{Listener as _, Stream as _},
    GenericNamespaced, Listener, ListenerOptions, Stream, ToNsName,
};
use serde::{Deserialize, Serialize};

// Re-exported so dependents don't need their own interprocess dependency.
pub use interprocess::local_socket::{Listener as IpcListener, Stream as IpcStream};

const MAX_FRAME_LEN: u32 = 64 * 1024;

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Message {
    /// Updater -> Aurora: update in progress, lock the UI.
    Lock,
    /// Updater -> Aurora: update finished (non-exe files), UI back to normal.
    Unlock,
    /// Updater -> Aurora: Aurora.exe itself is being replaced, exit cleanly.
    CloseNow,
    /// Updater -> Aurora: periodic liveness ping while locked.
    Heartbeat,
    /// Aurora -> Updater: new Aurora launched successfully after an exe swap.
    InitConfirmed,
    /// Updater -> Aurora: update failed.
    Error { message: String },
    /// Updater -> Aurora: manifest matches local state, updater exits.
    NoUpdate,
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

pub fn connect(pipe: &str) -> io::Result<Stream> {
    let name = pipe.to_ns_name::<GenericNamespaced>()?;
    Stream::connect(name)
}

pub fn accept(listener: &Listener) -> io::Result<Stream> {
    listener.accept()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn framing_round_trips_every_message() {
        let messages = [
            Message::Lock,
            Message::Unlock,
            Message::CloseNow,
            Message::Heartbeat,
            Message::InitConfirmed,
            Message::Error {
                message: "boom".into(),
            },
            Message::NoUpdate,
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
}
