# ui_main.py
import ctypes
import ctypes.wintypes
import time
import webbrowser
import subprocess

import psutil
from src.utils import resource_path
from pathlib import Path
from PyQt6.QtWidgets import (
    QMainWindow, QWidget, QVBoxLayout, QHBoxLayout,
    QPushButton, QLabel, QFrame, QGraphicsOpacityEffect, QSystemTrayIcon, QMenu
)
from PyQt6.QtCore import Qt, QSize, QThread, QPoint, QTimer, QPropertyAnimation, pyqtSignal
from PyQt6.QtGui import QPixmap, QIcon, QPainter, QAction
from src.logger import logger, dev_console_handler
from src.path_finder import validate_path, get_game_directory, get_local_version
from src.styles import MAIN_STYLE
from src import config_manager as cfg
from src.translator import Translator, t
from src.engine import get_app_dir
from src.mod_manager import ModManager
from src.ui.elements import PopupDialog, AuroraOverlayWindow
from src.ui.settings import SettingsOverlay
from src.utils import GetOnlineVersion, parse_version
from src.ui.dev_console import DevConsolePanel
from src.ui.widgets import BackgroundWidget, OverlayWidget
from src.ui.notification import ToastNotification
from src.ui.mod_manager import ModManagerOverlay

# ENGINE THREAD
class GameMonitorThread(QThread):
    game_started = pyqtSignal()  # emitted the moment HTGame.exe is detected
    access_denied = pyqtSignal() # emitted when AV/UAC blocks a file operation (or if the user somehow ran it without Administrator priviledges)
    launcher_detected = pyqtSignal() 

    def __init__(self, engine):
        super().__init__()
        self.engine = engine

    def run(self):
        try:
            result = self.engine.inject() if self.engine else False
            if result == "access_denied":
                self.access_denied.emit()
                return
            if result:
                launcher_path = self.engine.game_path / "NTEGlobalLauncher.exe"
                logger.info("Launcher started, waiting for manual game start. (HTGame.exe)", extra={"el": True})
                subprocess.Popen([str(launcher_path)], cwd=str(self.engine.game_path))
                self.engine.on_launcher_detected = lambda: self.launcher_detected.emit()  # <-- ADD
                self.engine.on_game_started = lambda: self.game_started.emit()
                self.engine.monitor_game()
                logger.info("Session was ended successfully.")
        except PermissionError:
            logger.error("Launch failed: Access Denied (missing admin privileges).")
            self.access_denied.emit()
        except Exception as e:
            logger.error(f"Launch failed with unexpected error: {e}", exc_info=True)
            self.access_denied.emit()


# DRIVE SEARCH THREAD
class DriveSearchThread(QThread):
    finished = pyqtSignal(str)

    def run(self):
        result = get_game_directory()
        self.finished.emit(result or "")

# MAIN UI
class AuroraUI(QMainWindow):
    def __init__(self, engine, current_path):
        super().__init__()
        self.engine = engine
        self.current_path = current_path
        self.old_pos = None
        self._search_thread = None
        self.is_valid = validate_path(self.current_path) if self.current_path else False

        self.setWindowFlags(Qt.WindowType.FramelessWindowHint)
        self.setFixedSize(1280, 720)
        self.setStyleSheet(MAIN_STYLE)
        self.setWindowTitle("Aurora Launcher")
        self.setWindowIcon(QIcon(resource_path("Bin/Assets/logo.ico")))
        
        Translator.load(cfg.get(cfg.Key.LANGUAGE))
        Translator.language_changed.connect(self.retranslate_ui)
        self.retranslate_ui()

        self.central_widget = QWidget()
        self.central_widget.setObjectName("CentralWidget")
        self.setCentralWidget(self.central_widget)

        self.bg_widget = BackgroundWidget(resource_path("Bin/Assets/background.jpg"), self.central_widget)
        self.bg_widget.setGeometry(0, 0, 1280, 720)
        self.bg_widget.lower()

        self._overlay = OverlayWidget(self.central_widget)
        self._overlay.setGeometry(0, 0, 1280, 720)

        self.top_bar = QFrame(self.central_widget)
        self.top_bar.setObjectName("TopBar")
        self.top_bar.setGeometry(0, 0, 1280, 60)
        self.setup_top_bar()

        self.setup_bottom_bar()

        self._dev_console = DevConsolePanel(self.central_widget)
        self._dev_console.hide()

        self._overlay.raise_()
        self.top_bar.raise_()
        self._bottom_bar.raise_()

        self.settings_menu = SettingsOverlay(self.central_widget)
        self.btn_settings.clicked.connect(self.toggle_settings)

        if cfg.get(cfg.Key.DEV_MODE):
            self.set_dev_console(True)

        self.check_for_updates()
        self.refresh_launch_state()

        self._tray = QSystemTrayIcon(QIcon(resource_path("Bin/Assets/logo.ico")), self)
        tray_menu = QMenu()
        restore_action = QAction("Open Aurora", self)
        restore_action.triggered.connect(self._restore_from_tray)
        tray_menu.addAction(restore_action)
        self._tray.setContextMenu(tray_menu)
        self._tray.activated.connect(self._on_tray_activated)

    # Translation
    def retranslate_ui(self):
        new_lang = cfg.get(cfg.Key.LANGUAGE)
        if hasattr(self, "_current_lang") and self._current_lang == new_lang:
            return
        self._current_lang = new_lang
        self._retranslate_tooltips()
        self.refresh_launch_state() 

    def _retranslate_tooltips(self):
        buttons = {
            "btn_settings":  "settings_tooltip",
            "btn_folder":    "mod_manager_tooltip",
            "btn_coffee":    "ko-fi_tooltip",
            "btn_discord":   "discord_tooltip",
            "btn_gamebanana":"gamebanana_tooltip",
            "btn_search":    "search_tooltip",
        }
        for attr, key in buttons.items():
            btn = getattr(self, attr, None)
            if btn:
                btn.setToolTip(t(key))
                btn.style().unpolish(btn)
                btn.style().polish(btn)

    def toggle_mod_manager(self):
        if hasattr(self, 'mod_overlay') and self.mod_overlay.isVisible():
            self.mod_overlay.hide()
        else:
            self.mod_overlay = ModManagerOverlay(self, self.mod_manager)
            self.mod_overlay.show()
            self.mod_overlay.raise_()
    
    # Checking for updates
    def check_for_updates(self):
        self.current_version = get_local_version()
        self.online_version = GetOnlineVersion() or "9.9.9"
        logger.info(self.current_version, extra={'el': True})
        logger.info(self.online_version, extra={'el': True})
        if parse_version(self.current_version) < parse_version(self.online_version):
            TMP_msg_a = t("update_available_message_a")
            TMP_msg_b = t("update_available_message_b")
            TMP_msg_c = t("update_current_version")
            TMP_msg_d = t("update_new_version")
            PopupDialog(
                parent=self,
                title=t("update_available_title"),
                message=(
                    f"{TMP_msg_a}\n\n"
                    f"{TMP_msg_c}: {self.current_version}\n"
                    f"{TMP_msg_d}: {self.online_version}\n\n"
                    f"{TMP_msg_b}"
                ),
                confirm_text=t("update_available_confirm"),
                cancel_text=t("cancel"),
                on_confirm=lambda: webbrowser.open("https://github.com/Daturaxoxo/Aurora/releases/latest"),
            )

    # Top Bar
    def setup_top_bar(self):
        tb_layout = QHBoxLayout(self.top_bar)
        tb_layout.setContentsMargins(15, 0, 15, 0)

        self.btn_settings = QPushButton()
        self.btn_settings.setIcon(QIcon(resource_path("Bin/Assets/settings.png")))
        self.btn_settings.setIconSize(QSize(32, 32))
        self.btn_settings.setToolTip(t("settings_tooltip"))

        self.btn_faq = QPushButton()
        self.btn_faq.setIcon(QIcon(resource_path("Bin/Assets/favourite.png")))
        self.btn_faq.setIconSize(QSize(32, 32))
        self.btn_faq.setToolTip(t("settings_tooltip"))

        self.logo = QLabel()
        logo_pix = QPixmap(resource_path("Bin/Assets/logo1024_wn.png"))
        if not logo_pix.isNull():
            self.logo.setPixmap(logo_pix.scaled(
                40, 40,
                Qt.AspectRatioMode.KeepAspectRatio,
                Qt.TransformationMode.SmoothTransformation
            ))

        self.btn_min = QPushButton()
        self.btn_min.setIcon(QIcon(resource_path("Bin/Assets/minimise.png")))
        self.btn_min.setIconSize(QSize(24, 24))
        self.btn_min.clicked.connect(self.showMinimized)

        self.btn_close = QPushButton()
        self.btn_close.setIcon(QIcon(resource_path("Bin/Assets/close.png")))
        self.btn_close.setIconSize(QSize(24, 24))
        self.btn_close.clicked.connect(self._on_close)

        tb_layout.addWidget(self.btn_settings)
        tb_layout.addWidget(self.btn_faq)
        tb_layout.addStretch()
        tb_layout.addWidget(self.logo)
        tb_layout.addStretch()
        tb_layout.addWidget(self.btn_min)
        tb_layout.addWidget(self.btn_close)

    # Bottom Bar
    def setup_bottom_bar(self):
        BOTTOM_H = 90
        self._bottom_bar = QWidget(self.central_widget)
        self._bottom_bar.setGeometry(0, 720 - BOTTOM_H, 1280, BOTTOM_H)
        self._bottom_bar.setAttribute(Qt.WidgetAttribute.WA_TranslucentBackground)

        bottom_layout = QHBoxLayout(self._bottom_bar)
        bottom_layout.setContentsMargins(30, 15, 30, 20)

        if cfg.get(cfg.Key.USE_HARD_LINKS):
            mods_dir = Path(get_app_dir()) / "Mods"
        else:
            mods_dir = Path(cfg.get(cfg.Key.GAME_PATH)) / "Client/WindowsNoEditor/HT/Content/Paks/AuroraMods"
        self.mod_manager = ModManager(
            mods_dir=mods_dir,
            state_file=Path(get_app_dir()) / "Bin" / "disabled_mods.json"
        )

        self.btn_folder = QPushButton()
        self.btn_folder.setIcon(QIcon(resource_path("Bin/Assets/folder.png")))
        self.btn_folder.setIconSize(QSize(42, 42))
        self.btn_folder.clicked.connect(self.toggle_mod_manager)
        self.btn_folder.setToolTip(t("mod_manager_tooltip"))

        self.btn_coffee = QPushButton()
        self.btn_coffee.setIcon(QIcon(resource_path("Bin/Assets/coffee.png")))
        self.btn_coffee.setIconSize(QSize(42, 42))
        self.btn_coffee.clicked.connect(lambda: webbrowser.open("https://ko-fi.com/daturaxoxo"))
        self.btn_coffee.setToolTip(t("ko-fi_tooltip"))

        self.btn_discord = QPushButton()
        self.btn_discord.setIcon(QIcon(resource_path("Bin/Assets/discord.png")))
        self.btn_discord.setIconSize(QSize(42, 42))
        self.btn_discord.clicked.connect(lambda: webbrowser.open("https://discord.gg/565jfeYsbp"))
        self.btn_discord.setToolTip(t("discord_tooltip"))

        self.btn_gamebanana = QPushButton()
        self.btn_gamebanana.setIcon(QIcon(resource_path("Bin/Assets/marketplace.png")))
        self.btn_gamebanana.setIconSize(QSize(42, 42))
        self.btn_gamebanana.clicked.connect(lambda: webbrowser.open("https://gamebanana.com/games/23012"))
        self.btn_gamebanana.setToolTip(t("gamebanana_tooltip"))

        self.btn_search = QPushButton()
        self.btn_search.setObjectName("SearchButton")
        self.btn_search.setIcon(QIcon(resource_path("Bin/Assets/refresh.png")))
        self.btn_search.setIconSize(QSize(28, 28))
        self.btn_search.setFixedSize(60, 60)
        self.btn_search.setCursor(Qt.CursorShape.PointingHandCursor)
        self.btn_search.clicked.connect(self._prompt_drive_search)
        self.btn_search.setToolTip(t("search_tooltip"))
        if self.is_valid:
            self.btn_search.hide()
        else:
            self.btn_search.show()

        self.btn_launch = QPushButton()
        self.btn_launch.setObjectName("LaunchButton")
        self.btn_launch.setFixedSize(240, 60)
        self.btn_launch.setIconSize(QSize(28, 28))
        if self.is_valid:
            self.btn_launch.setIcon(QIcon(resource_path("Bin/Assets/checkmark.png")))
            self.btn_launch.setEnabled(True)
            self.btn_launch.setText(f"    {t('launch')}")
        else:
            self.btn_launch.setIcon(QIcon(resource_path("Bin/Assets/cancel.png")))
            self.btn_launch.setEnabled(False)
            self.btn_launch.setText(f"    {t('launch_invalid')}")
        self.btn_launch.clicked.connect(self.handle_launch)

        bottom_layout.addWidget(self.btn_coffee)
        bottom_layout.addWidget(self.btn_folder)
        bottom_layout.addWidget(self.btn_discord)
        bottom_layout.addWidget(self.btn_gamebanana)
        bottom_layout.addStretch()
        bottom_layout.addWidget(self.btn_search)
        bottom_layout.addSpacing(10)
        bottom_layout.addWidget(self.btn_launch)

        self._bottom_bar.raise_()

    # HELPERS
    def toggle_settings(self):
        if self.settings_menu.isHidden():
            self.settings_menu.show()
            self.settings_menu.raise_()
        else:
            self.settings_menu.hide()

    def set_dev_console(self, enabled: bool):
        console_h = self._dev_console.height()
        if enabled:
            total_h = 720 + console_h
            self.setFixedSize(1280, total_h)
            self.bg_widget.setGeometry(0, 0, 1280, 720)
            self._overlay.setGeometry(0, 0, 1280, 720)
            self._dev_console.setGeometry(0, 720, 1280, console_h)
            self._dev_console.show()
            self._dev_console.raise_()
        else:
            self._dev_console.hide()
            if dev_console_handler:
                dev_console_handler.detach()
            self.setFixedSize(1280, 720)
            self.bg_widget.setGeometry(0, 0, 1280, 720)
            self._overlay.setGeometry(0, 0, 1280, 720)

    def refresh_launch_state(self):
        self.is_valid = validate_path(self.current_path) if self.current_path else False
        if hasattr(self, "btn_launch"):
            if self.is_valid:
                self.btn_launch.setIcon(QIcon(resource_path("Bin/Assets/checkmark.png")))
                self.btn_launch.setEnabled(True)
                self.btn_launch.setText(f"    {t('launch')}")
            else:
                self.btn_launch.setIcon(QIcon(resource_path("Bin/Assets/cancel.png")))
                self.btn_launch.setEnabled(False)
                self.btn_launch.setText(f"    {t('launch_invalid')}")
                
        if hasattr(self, "btn_search"):
            if self.is_valid:
                self.btn_search.hide()
            else:
                self.btn_search.show()

    # DRIVE SEARCH
    def _prompt_drive_search(self):
        PopupDialog(
            parent=self.central_widget,
            title=t("search_drives_title"),
            message=t("search_drives_message"),
            confirm_text=t("search"),
            cancel_text=t("cancel"),
            on_confirm=self._start_drive_search
        )

    def _start_drive_search(self):
        logger.info("User initiated drive search.")
        self.btn_search.setEnabled(False)
        self._search_thread = DriveSearchThread()
        self._search_thread.finished.connect(self._on_search_finished)
        self._search_thread.start()

    def _on_search_finished(self, found_path):
        self.btn_search.setEnabled(True)
        if found_path:
            logger.info(f"Drive search found NTE at: {found_path}")
            self.current_path = found_path
            cfg.set(cfg.Key.GAME_PATH, found_path)
            from src.engine import AuroraEngine
            self.engine = AuroraEngine(found_path, censorship_removal=cfg.get(cfg.Key.CENSORSHIP_REMOVE), no_drive_line=cfg.get(cfg.Key.NO_DRIVE_LINE))
            try:
                self.settings_menu.path_display.setText(found_path)
            except Exception:
                pass
            self.refresh_launch_state()
            ToastNotification(self.central_widget, t("toast_game_found"), False)
        else:
            logger.warning("Aurora did not find NTE using Drive Search.")
            ToastNotification(self.central_widget, t("toast_game_not_found"), True)

    # LAUNCH
    def handle_launch(self):
        logger.info("Launch button was clicked, initialising engine...", extra={"el": True})
        if not self.engine:
            logger.warning("Launch aborted: Engine not initialized.")
            ToastNotification(self.central_widget, t("toast_engine_error"), True)
            return
        logger.info(f"Target Path: {self.current_path}", extra={"el": True})
        self.btn_launch.setEnabled(False)
        self.btn_launch.setText(f"    {t('launch_running')}")
        self.monitor_thread = GameMonitorThread(self.engine)
        self.monitor_thread.finished.connect(self.refresh_launch_state)
        self.monitor_thread.finished.connect(lambda: setattr(self, 'monitor_thread', None))
        self.monitor_thread.finished.connect(self._on_session_ended)
        self.monitor_thread.game_started.connect(self._show_game_overlay)
        self.monitor_thread.launcher_detected.connect(self._send_to_tray)
        self.monitor_thread.access_denied.connect(self._show_access_denied_popup)
        self.monitor_thread.start()

        if hasattr(self, 'rpc'):
            self.rpc.set_launching()
        self.monitor_thread.game_started.connect(lambda: hasattr(self, 'rpc') and self.rpc.set_in_game())
        self.monitor_thread.finished.connect(lambda: hasattr(self, 'rpc') and self.rpc.set_idle())

        ToastNotification(self.central_widget, t("toast_launching"), False)

    def _show_access_denied_popup(self):
        self.refresh_launch_state()
        PopupDialog(
            parent=self.central_widget,
            title="Access Denied",
            message=(
                "Aurora was blocked from copying files to the game directory.\n\n"
                "This is commonly caused by Antivirus software inteference. "
                "Try adding the Aurora folder to your antivirus "
                "whitelist (exclusion list) and re-launching.\n"
            ),
            confirm_text="OK",
            cancel_text="",
            on_confirm=None,
        )

    def _show_game_overlay(self):
        logger.info("HTGame.exe detected. Starting window polling...", extra={"el": True})
        
        # Initialize polling state
        self._poll_count = 0
        self._poll_timer = QTimer(self)
        self._poll_timer.timeout.connect(self._check_window_ready)
        self._poll_timer.start(500)  # Default: 500ms

    def _get_game_hwnd(self):
        hwnd_result = [None]
        
        def enum_cb(hwnd, _):
            # Check 1. Basic Visibility check
            if not ctypes.windll.user32.IsWindowVisible(hwnd):
                return True
                
            # Check 2. Cloaked check (Windows 10/11 ghost windows)
            cloaked = ctypes.c_int(0)
            ctypes.windll.dwmapi.DwmGetWindowAttribute(
                hwnd, 14, ctypes.byref(cloaked), ctypes.sizeof(cloaked)
            )
            if cloaked.value != 0:
                return True

            length = ctypes.windll.user32.GetWindowTextLengthW(hwnd)
            if length > 0:
                buf = ctypes.create_unicode_buffer(length + 1)
                ctypes.windll.user32.GetWindowTextW(hwnd, buf, length + 1)
                title = buf.value.lower()
                
                # Filter specifically for NTE
                if any(name in title for name in ["neverness", "htgame", "nte"]):
                    # Check 3. Final Sanity: Does it have an actual area?
                    rect = ctypes.wintypes.RECT()
                    ctypes.windll.user32.GetWindowRect(hwnd, ctypes.byref(rect))
                    if (rect.right - rect.left) > 100:
                        hwnd_result[0] = hwnd
                        return False 
            return True

        # FIX: Use HWND and LPARAM for 64-bit safety otherwise Aurora craps itself (I Love Windows!)
        WNDENUMPROC = ctypes.WINFUNCTYPE(ctypes.c_bool, ctypes.wintypes.HWND, ctypes.wintypes.LPARAM)
        ctypes.windll.user32.EnumWindows(WNDENUMPROC(enum_cb), 0)
        return hwnd_result[0]

    def _check_window_ready(self):
        self._poll_count += 1
        
        if self._poll_count > 300: # 150s timeout (300 polls * 500ms)
            logger.warning("Overlay polling timed out.", extra={"el": True})
            self._poll_timer.stop()
            return

        hwnd = self._get_game_hwnd()
        if hwnd:
            # Check 1: Get the current active foreground window.
            foreground_hwnd = ctypes.windll.user32.GetForegroundWindow()
            
            # Check 2: Get window dimensions
            rect = ctypes.wintypes.RECT()
            ctypes.windll.user32.GetWindowRect(hwnd, ctypes.byref(rect))
            width = rect.right - rect.left

            # Check 3: Checks if NTE is focused and on the foreground before showing overlay (New Check) -Datura
            is_focused = (hwnd == foreground_hwnd)
            
            if is_focused and width > 100:
                logger.info(f"NTE has loaded, showing game overlay...", extra={"el": True})
                self._poll_timer.stop()
                self._spawn_overlay(rect)
            else:
                if self._poll_count % 10 == 0 and not is_focused:
                    logger.info("NTE (HTGame.exe) is detected but not focused, yielding game overlay until focused.", extra={"el": True})

    def _spawn_overlay(self, game_rect=None):
        self._overlay_win = AuroraOverlayWindow()
        self._overlay_win.show_over_game(game_rect)

    # WINDOW CHROME
    def closeEvent(self, event):
        if hasattr(self, '_poll_timer') and self._poll_timer is not None:
            self._poll_timer.stop()
        if hasattr(self, '_tray'):
            self._tray.hide()
        logger.info("Aurora was closed normally by the user.")
        event.accept()

    def _send_to_tray(self):
        if not cfg.get(cfg.Key.UI_MINIMIZATION):
            return
        
        self._tray.show()
        self.hide()
        self._overlay_win = AuroraOverlayWindow(
            title="Aurora Mod Loader",
            subtitle="Minimised to tray"
        )
        self._overlay_win.show_over_game()

    def _restore_from_tray(self):
        self._tray.hide()
        self.showNormal()
        self.activateWindow()
        self.raise_()

    def _on_tray_activated(self, reason):
        if reason == QSystemTrayIcon.ActivationReason.DoubleClick:
            self._restore_from_tray()

    def _on_session_ended(self):
        if not cfg.get(cfg.Key.UI_MINIMIZATION):
            return
        self._restore_from_tray()
        logger.info("Session ended, Aurora restored from tray.", extra={"el": True})
        
    def _on_close(self):
        logger.info("Closing aurora...", extra={"el": True})
        
        active = {p.name().lower() for p in psutil.process_iter(['name'])}
        if "htgame.exe" in active:
            self._tray.show()
            self.hide()
            logger.info("NTE Process (HTGame.exe) was detected while closing, waiting...", extra={"el": True})
            ht_procs = [p for p in psutil.process_iter(['name']) if p.name().lower() == "htgame.exe"]
            if ht_procs:
                psutil.wait_procs(ht_procs, timeout=None)
            else:
                while True:
                    time.sleep(0.5)
                    active = {p.name().lower() for p in psutil.process_iter(['name'])}
                    if "htgame.exe" not in active:
                        break
        
        logger.info("Sanitizing and closing...", extra={"el": True})
        self.engine.sanitize(False)
        self.close()

    def mousePressEvent(self, event):
        if event.button() == Qt.MouseButton.LeftButton:
            self.old_pos = event.globalPosition().toPoint()

    def mouseMoveEvent(self, event):
        if self.old_pos is not None:
            delta = QPoint(event.globalPosition().toPoint() - self.old_pos)
            self.move(self.x() + delta.x(), self.y() + delta.y())
            self.old_pos = event.globalPosition().toPoint()

    def mouseReleaseEvent(self, event):
        self.old_pos = None
