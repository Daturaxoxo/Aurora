import json
import os
import struct
import time
import threading
import uuid
from PyQt6.QtCore import QThread
from src.logger import logger

OP_HANDSHAKE = 0
OP_FRAME     = 1
OP_CLOSE     = 2


def _open_pipe():
    import ctypes
    import ctypes.wintypes

    for i in range(10):
        path = f"\\\\.\\pipe\\discord-ipc-{i}"
        try:
            handle = ctypes.windll.kernel32.CreateFileW(
                path,
                0x40000000 | 0x80000000,  # GENERIC_READ | GENERIC_WRITE
                0,                         # no sharing
                None,
                3,                         # OPEN_EXISTING
                0,
                None,
            ) # thx stack overflow
            INVALID_HANDLE = ctypes.wintypes.HANDLE(-1).value
            if handle != INVALID_HANDLE:
                return handle
        except Exception:
            continue
    raise OSError("Discord IPC pipe not found. Is Discord running?")


def _read_pipe(handle, size: int) -> bytes:
    import ctypes
    buf   = ctypes.create_string_buffer(size)
    read  = ctypes.c_ulong(0)
    ok    = ctypes.windll.kernel32.ReadFile(handle, buf, size, ctypes.byref(read), None)
    if not ok:
        raise OSError(f"RPC:ReadFile failed (error {ctypes.windll.kernel32.GetLastError()})")
    return buf.raw[:read.value]


def _write_pipe(handle, data: bytes):
    import ctypes
    written = ctypes.c_ulong(0)
    ok = ctypes.windll.kernel32.WriteFile(handle, data, len(data), ctypes.byref(written), None)
    if not ok:
        raise OSError(f"RPC:WriteFile failed (error {ctypes.windll.kernel32.GetLastError()})")


def _encode(op: int, payload: dict) -> bytes:
    data = json.dumps(payload).encode("utf-8")
    return struct.pack("<II", op, len(data)) + data


def _decode(handle) -> dict:
    header = _read_pipe(handle, 8)
    _op, length = struct.unpack("<II", header)
    body = _read_pipe(handle, length)
    return json.loads(body)


def _close_pipe(handle):
    import ctypes
    ctypes.windll.kernel32.CloseHandle(handle)

# MAIN
class DiscordRPC(QThread):
    CID = "1505644188060876920"

    def __init__(self, client_id: str = None, parent=None):
        super().__init__(parent)
        self._client_id  = client_id or self.CID
        self._handle     = None
        self._connected  = False
        self._stop_event = threading.Event()
        self._lock       = threading.Lock()

        self._pending_activity: dict | None = None
        self._start_timestamp = int(time.time())

    def set_idle(self):
        self._queue_activity({
            "state":   "Idle",
            "details": "In launcher",
            "timestamps": {"start": self._start_timestamp},
            "assets": {
                "large_image": "aurora_logo",
                "large_text":  "Aurora Mod Launcher",
            },
        })

    def set_launching(self):
        self._queue_activity({
            "state":   "Launching…",
            "details": "Starting NTE",
            "timestamps": {"start": int(time.time())},
            "assets": {
                "large_image": "aurora_logo",
                "large_text":  "Aurora Mod Launcher",
            },
        })

    def set_in_game(self):
        self._queue_activity({
            "state":   "In-game",
            "details": "Playing NTE",
            "timestamps": {"start": int(time.time())},
            "assets": {
                "large_image": "background",
                "large_text":  "Aurora Mod Launcher",
                "small_image": "playing",
                "small_text":  "In-game",
            },
        })

    def stop(self):
        self._stop_event.set()
        self._clear_presence()
        self.quit()
        self.wait(2000)

    # Internal helpers

    def _queue_activity(self, activity: dict):
        with self._lock:
            self._pending_activity = activity

    def _send_activity(self, activity: dict):
        if not self._connected or self._handle is None:
            return
        payload = {
            "cmd":   "SET_ACTIVITY",
            "args":  {
                "pid":      os.getpid(),
                "activity": activity,
            },
            "nonce": str(uuid.uuid4()),
        }
        try:
            _write_pipe(self._handle, _encode(OP_FRAME, payload))
            _decode(self._handle)
        except Exception as e:
            logger.warning(f"[RPC] Failed to send activity: {e}", extra={'el': True})
            self._connected = False

    def _clear_presence(self):
        if not self._connected or self._handle is None:
            return
        payload = {
            "cmd":   "SET_ACTIVITY",
            "args":  {"pid": os.getpid(), "activity": None},
            "nonce": str(uuid.uuid4()),
        }
        try:
            _write_pipe(self._handle, _encode(OP_FRAME, payload))
        except Exception:
            pass

    def _connect(self) -> bool:
        try:
            self._handle = _open_pipe()
            # Handshake
            _write_pipe(self._handle, _encode(OP_HANDSHAKE, {
                "v":         1,
                "client_id": self._client_id,
            }))
            resp = _decode(self._handle)
            if resp.get("cmd") == "DISPATCH" and resp.get("evt") == "READY":
                self._connected = True
                logger.info("[RPC] Connected to Discord.", extra={'el': True})
                return True
        except Exception as e:
            logger.warning(f"[RPC] Connection failed: {e}", extra={'el': True})
        return False

    # QThread entry point

    def run(self):
        while not self._stop_event.is_set():
            if not self._connected:
                if not self._connect():
                    # Discord not running
                    self._stop_event.wait(15)
                    continue
                with self._lock:
                    pending = self._pending_activity
                if pending:
                    self._send_activity(pending)

            # Flush pending update
            with self._lock:
                pending = self._pending_activity
                self._pending_activity = None   # clear so we don't spam and cause rate limit to go grrr

            if pending:
                self._send_activity(pending)

            self._stop_event.wait(1)

        if self._handle:
            try:
                _write_pipe(self._handle, _encode(OP_CLOSE, {}))
            except Exception:
                pass
            _close_pipe(self._handle)
            self._handle = None
        logger.info("[RPC] Disconnected.", extra={'el': True})
