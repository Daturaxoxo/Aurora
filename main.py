import sys, ctypes, traceback
from pathlib import Path
from PyQt6.QtWidgets import QApplication
from PyQt6.QtCore import QTimer
from src.frontend.ui_main import AuroraUI
from src.backend.engine import AuroraEngine
from src.path_finder import validate_path
from src import config_manager as cfg
from src.discord_rpc import DiscordRPC
ARCHIVES = (
    "\\Temp\\7z",
    "\\Temp\\Rar$",
    "\\Temp\\wz",
    "\\Temp\\peazip",
    "\\Temp\\BandZip",
    "\\Temp\\$",
)

def compute_scale(base_w=1920, base_h=1080) -> float:
    app = QApplication.instance()
    screen = app.primaryScreen() if app else None
    if screen is None:
        return 1.0

    geo = screen.geometry()
    scale_w = geo.width() / base_w
    scale_h = geo.height() / base_h
    raw = min(scale_w, scale_h)

    snapped = round(raw * 4) / 4
    return max(0.75, min(2.5, snapped))

def check_archive_status():
    if not getattr(sys, "frozen", False): return  # Skip development environments
    exe_path = sys.executable.replace("/", "\\")
    mei_path = (getattr(sys, "_MEIPASS", "") or "").replace("/", "\\")
    in_temp = any(
        marker.lower() in path.lower()
        for path in (exe_path, mei_path)
        for marker in ARCHIVES
    )
    exe_dir = Path(sys.executable).parent
    probe = exe_dir / ".aurora_write_probe"
    try:
        probe.write_bytes(b"")
        probe.unlink()
        read_only = False
    except OSError: read_only = True

    if in_temp or read_only:
        ctypes.windll.user32.MessageBoxW(
            0,
            "Aurora can not launch because of the following issue:\n\n"
            "Aurora is running inside a compressed archive file.\n\n"
            "Please extract the archive before running.\n"
            "This is to prevent saving errors, mod loading issues, etc.\n\n",
            "Aurora - Prelaunch Error",
            0x10,
        )
        sys.exit(1)

def handle_exception(exc_type, exc_value, exc_tb):
    error = "".join(traceback.format_exception(exc_type, exc_value, exc_tb))
    ctypes.windll.user32.MessageBoxW(0, error, "Aurora - Fatal Error", 0x10)
    sys.exit(1)

sys.excepthook = handle_exception
myappid = 'datura.aurora.nte.1000'
ctypes.windll.shell32.SetCurrentProcessExplicitAppUserModelID(myappid)

def run_as_admin():
    if ctypes.windll.shell32.IsUserAnAdmin(): return True
    if getattr(sys, 'frozen', False):
        exe = sys.executable
        params = " ".join(f'"{a}"' for a in sys.argv[1:])
    else:
        exe = sys.executable
        params = " ".join(f'"{a}"' for a in sys.argv)

    ctypes.windll.shell32.ShellExecuteW(None, "runas", exe, params, None, 1)
    sys.exit(0)

def main():
    app = QApplication(sys.argv)
    scale = compute_scale()
    saved_path = cfg.get(cfg.Key.GAME_PATH)
    initial_path = saved_path if (saved_path and validate_path(saved_path)) else None
    engine = AuroraEngine(initial_path) if initial_path else None
    window = AuroraUI(engine, initial_path, scale=scale)

    if cfg.get(cfg.Key.DISCORD_RPC):
        window.rpc = DiscordRPC()
        window.rpc.set_idle()
        window.rpc.start()

    window.show()
    if not initial_path: QTimer.singleShot(500, window._prompt_drive_search)

    sys.exit(app.exec())

if __name__ == "__main__":
    check_archive_status()
    if run_as_admin():  main()