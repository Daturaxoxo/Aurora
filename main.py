import sys
import ctypes
import traceback
from PyQt6.QtWidgets import QApplication
from PyQt6.QtCore import QTimer
from src.ui_main import AuroraUI
from src.engine import AuroraEngine
from src.path_finder import validate_path
from src import config_manager as cfg
from src.discord_rpc import DiscordRPC

def handle_exception(exc_type, exc_value, exc_tb):
    error = "".join(traceback.format_exception(exc_type, exc_value, exc_tb))
    import ctypes
    ctypes.windll.user32.MessageBoxW(0, error, "Aurora - Fatal Error", 0x10)
    sys.exit(1)

sys.excepthook = handle_exception

myappid = 'datura.aurora.nte.1000'
ctypes.windll.shell32.SetCurrentProcessExplicitAppUserModelID(myappid)

def run_as_admin():
    if ctypes.windll.shell32.IsUserAnAdmin():
        return True

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
    saved_path = cfg.get(cfg.Key.GAME_PATH)
    initial_path = saved_path if (saved_path and validate_path(saved_path)) else None

    engine = AuroraEngine(
        initial_path,
        censorship_removal=cfg.get(cfg.Key.CENSORSHIP_REMOVE),
        no_drive_line=cfg.get(cfg.Key.NO_DRIVE_LINE)
    ) if initial_path else None
    
    window = AuroraUI(engine, initial_path)

    if cfg.get(cfg.Key.DISCORD_RPC):
        window.rpc = DiscordRPC()
        window.rpc.set_idle()
        window.rpc.start()

    window.show()
    
    # Trigger the visual drive search UI automatically if no valid path is found
    if not initial_path:
        QTimer.singleShot(500, window._prompt_drive_search)

    sys.exit(app.exec())

if __name__ == "__main__":
    if run_as_admin():
        main()