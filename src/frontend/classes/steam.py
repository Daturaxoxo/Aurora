import json
import os
import platform
from pathlib import Path
from src.path_finder import get_local_version

from PyQt6.QtCore    import Qt, QThread, pyqtSignal, QObject
from PyQt6.QtWidgets import (
    QDialog, QFrame, QHBoxLayout, QLabel,
    QPushButton, QVBoxLayout, QWidget,
)

from src import config_manager as cfg
from src.backend.helpers.steam import apply_steam_wrapper, remove_steam_wrapper, check_steam_async
from src.frontend.classes.elements import AnimatedToggle
from src.frontend.classes.notification import ToastNotification
from src.logger import logger
from src.translator import t

class SteamSignalBridge(QObject):
    steam_detected = pyqtSignal()


# Cache Functions (for v2, move to utils.rs)
def cache_path() -> Path:
    system = platform.system()
    if system == "Windows":
        base = Path(os.environ.get("APPDATA", Path.home()))
    else:
        base = Path.home() / ".config"
    return base / "Aurora" / "Cache" / "storage.json"

def load_cache() -> dict:
    p = cache_path()
    if not p.exists() or p.stat().st_size == 0: return {}
    try: return json.loads(p.read_text(encoding="utf-8"))
    except (json.JSONDecodeError, OSError): return {}


def save_cache(data: dict):
    p = cache_path()
    p.parent.mkdir(parents=True, exist_ok=True)
    try:
        p.write_text(json.dumps(data, indent=2, ensure_ascii=False), encoding="utf-8")
    except OSError: pass


def set_cache(key: str, value):
    d = load_cache()
    d[key] = value
    save_cache(d)


def get_cache(key: str, default=None): return load_cache().get(key, default)


class WrapperInstallThread(QThread):
    success = pyqtSignal()
    failure = pyqtSignal()

    def run(self):
        try:
            ok = apply_steam_wrapper()
            (self.success if ok else self.failure).emit()
        except Exception: self.failure.emit()


def check_steam(main_window, game_path: str):
    from src.frontend.classes.elements import PopupDialog
    from PyQt6.QtCore import QObject, QThread, pyqtSignal

    cache = load_cache()
    modify_steam = cache.get("modify_steam")
    installed_by_steam = cache.get("installed_by_steam")

    if installed_by_steam is True:
        settings = getattr(main_window, "settings_menu", None)
        if settings:
            settings.btn_steam.setVisible(True)
        return

    if modify_steam is False:
        last_check = cache.get("last_steam_check", "")
        if last_check == get_local_version(): return

    class InstallThread(QThread):
        success = pyqtSignal()
        failure = pyqtSignal()
        def run(self):
            try:
                (self.success if apply_steam_wrapper() else self.failure).emit()
            except Exception:
                self.failure.emit()

    def show_popup():
        def _on_confirm():
            t = InstallThread()

            def _ok():
                set_cache("modify_steam", True)
                set_cache("installed_by_steam", True)
                settings = getattr(main_window, "settings_menu", None)
                if settings:
                    settings.btn_steam.setVisible(True)
                ToastNotification(main_window, "Steam Wrapper installed!", False, "success")

            def _fail():
                ToastNotification(main_window, "Failed to install Steam Wrapper.", False, "error")

            t.success.connect(_ok)
            t.failure.connect(_fail)
            main_window._steam_install_thread = t
            t.start()

        def on_cancel():
            set_cache("modify_steam", False)
            set_cache("installed_by_steam", True)
            set_cache("last_steam_check", get_local_version())

        popup = PopupDialog(
            parent=main_window,
            title="Modify Steam's Play Button?", # edit later
            message="Aurora has detected you have installed the game through Steam and can update the Play button to use our Steam Wrapper instead of running the game without mods.",
            confirm_text="Confirm",
            cancel_text="Not Now",
            on_confirm=_on_confirm,
            on_cancel=on_cancel,
        )

    class SteamBridge(QObject):
        detected = pyqtSignal()

    bridge = SteamBridge()
    bridge.detected.connect(show_popup)
    main_window._steam_bridge = bridge

    def on_result(is_steam: bool):
        set_cache("installed_by_steam", is_steam)
        if is_steam:
            bridge.detected.emit()

    check_steam_async(game_path, on_result)