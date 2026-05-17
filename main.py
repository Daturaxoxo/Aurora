import sys
import ctypes
import traceback
import os
from PyQt6.QtWidgets import QApplication
from src.ui_main import AuroraUI
from src.engine import AuroraEngine
from src.utils import resource_path
from src.path_finder import get_game_directory
from src import config_manager as cfg

def handle_exception(exc_type, exc_value, exc_tb):
    error = "".join(traceback.format_exception(exc_type, exc_value, exc_tb))
    import ctypes
    ctypes.windll.user32.MessageBoxW(0, error, "Aurora - Fatal Error", 0x10)
    sys.exit(1)

sys.excepthook = handle_exception

myappid = 'datura.aurora.nte.1000'
ctypes.windll.shell32.SetCurrentProcessExplicitAppUserModelID(myappid)

def run_as_admin():
    """
    Re-launches the process with elevated privileges if not already admin.
    Works correctly for both frozen EXEs (PyInstaller) and plain Python scripts. (mayhaps, i hope so.)
    """
    if ctypes.windll.shell32.IsUserAnAdmin():
        return True

    # sys.executable is the EXE itself when frozen, pass sys.argv[1:] as args. If not, pass sys.argv[0] as args.
    if getattr(sys, 'frozen', False):
        exe = sys.executable
        params = " ".join(f'"{a}"' for a in sys.argv[1:])
    else:
        # Developer Mode: Relaunch python.exe with the script as argument for the console to appear.
        exe = sys.executable
        params = " ".join(f'"{a}"' for a in sys.argv)

    ctypes.windll.shell32.ShellExecuteW(None, "runas", exe, params, None, 1)
    sys.exit(0)

def main():
    app = QApplication(sys.argv)
    initial_path = get_game_directory()

    engine = AuroraEngine(
        initial_path,
        censorship_removal=cfg.get_censorship_removal(),
        no_drive_line=cfg.get_no_drive_line(),
    ) if initial_path else None
    window = AuroraUI(engine, initial_path)

    window.show()
    sys.exit(app.exec())

if __name__ == "__main__":
    if run_as_admin():
        main()
