import os
import sys
import ctypes
import ctypes.wintypes
import webbrowser
import subprocess
from src.utils import resource_path
from pathlib import Path
from PyQt6.QtWidgets import (
    QMainWindow, QWidget, QVBoxLayout, QHBoxLayout,
    QPushButton, QLabel, QFrame, QLineEdit,
    QComboBox, QStackedWidget, QFileDialog, QGraphicsOpacityEffect,
    QTextEdit, QSizePolicy, QScrollArea, 
)
from PyQt6.QtCore import Qt, QSize, QThread, QPoint, QTimer, QPropertyAnimation, pyqtSignal, QVariant, QVariantAnimation, QEasingCurve
from PyQt6.QtGui import QPixmap, QIcon, QPainter, QColor
from src.logger import logger, dev_console_handler, file_handler
from src.path_finder import validate_path, get_game_directory, get_local_version
from src.styles import MAIN_STYLE, SETTING_STYLE, TOAST_STYLE, POPUP_STYLE, MOD_MANAGER_STYLE
from src import config_manager as cfg
from src.translator import Translator, t
from src.engine import get_app_dir
from src.mod_manager import ModManager
import urllib.request

def parse_version(v):
    try:
        return tuple(int(x) for x in v.strip().split("."))
    except (ValueError, AttributeError):
        return (0, 0, 0)

def GetOnlineVersion():
    try:
        with urllib.request.urlopen("https://raw.githubusercontent.com/Daturaxoxo/Aurora/refs/heads/main/dev/VERSION") as response:
            version_info = response.read().decode('utf-8').strip()
        return version_info or "9.9.9"
    except Exception as e:
        logger.warning("Couldn't get version information GitHub ")

# ENGINE THREAD
class GameMonitorThread(QThread):
    game_started = pyqtSignal()  # emitted the moment HTGame.exe is detected
    access_denied = pyqtSignal() # emitted when AV/UAC blocks a file operation (or if the user somehow ran it without Administrator priviledges)

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
                logger.info("Launcher started, waiting for manual game start. (HTGame.exe)")
                subprocess.Popen([str(launcher_path)], cwd=str(self.engine.game_path))
                # Patch monitor_game to signal us when the game actually starts
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

# TOAST NOTIFICATION
class ToastNotification(QFrame):
    def __init__(self, parent, message, is_error=False):
        super().__init__(parent)
        self.setObjectName("ToastContainer")
        self.setFixedSize(340, 70)
        self.setStyleSheet(TOAST_STYLE)
        self.move(parent.width() - self.width() - 20, 30)

        layout = QHBoxLayout(self)
        layout.setContentsMargins(12, 8, 12, 8)
        layout.setSpacing(10)

        icon_label = QLabel()
        icon_path = resource_path("Bin/Assets/warning192.png") if is_error else resource_path("Bin/Assets/checkmark.png")
        icon_label.setPixmap(QIcon(icon_path).pixmap(28, 28))

        msg_label = QLabel(message)
        msg_label.setStyleSheet("color: #D7D7D7; font-size: 13px;")
        msg_label.setWordWrap(True)

        layout.addWidget(icon_label)
        layout.addWidget(msg_label)

        self.opacity_effect = QGraphicsOpacityEffect(self)
        self.setGraphicsEffect(self.opacity_effect)
        self.anim = QPropertyAnimation(self.opacity_effect, b"opacity")
        self.anim.setDuration(400)
        self.anim.setStartValue(0)
        self.anim.setEndValue(1)
        self.show()
        self.raise_()
        self.anim.start()
        QTimer.singleShot(4000, self.fade_out)

    def fade_out(self):
        self.anim.setDirection(QPropertyAnimation.Direction.Backward)
        self.anim.finished.connect(self.deleteLater)
        self.anim.start()


# POPUP DIALOG
class PopupDialog(QWidget):
    def __init__(self, parent, title, message, confirm_text="Confirm",
                 cancel_text="Cancel", on_confirm=None, on_cancel=None):
        super().__init__(parent)
        self.on_confirm = on_confirm
        self.on_cancel = on_cancel

        self.setObjectName("DimOverlay")
        self.setFixedSize(parent.size())
        self.move(0, 0)
        self.setAttribute(Qt.WidgetAttribute.WA_StyledBackground)
        self.setStyleSheet(POPUP_STYLE)

        card = QFrame(self)
        card.setObjectName("PopupContainer")
        card.setFixedWidth(460)
        card.setMinimumHeight(220)
        card.setAttribute(Qt.WidgetAttribute.WA_StyledBackground)
        card.setStyleSheet(POPUP_STYLE)
        card.move(
            (self.width() - card.width()) // 2,
            (self.height() - card.height()) // 2
        )

        card_layout = QVBoxLayout(card)
        card_layout.setContentsMargins(32, 28, 32, 28)
        card_layout.setSpacing(10)

        lbl_title = QLabel(title)
        lbl_title.setObjectName("PopupTitle")
        lbl_msg = QLabel(message)
        lbl_msg.setObjectName("PopupMessage")
        lbl_msg.setWordWrap(True)
        lbl_msg.setFixedWidth(460 - 64)

        btn_row = QHBoxLayout()
        btn_row.setSpacing(12)
        btn_row.addStretch()

        btn_cancel = QPushButton(cancel_text)
        btn_cancel.setObjectName("PopupCancelButton")
        btn_cancel.setFixedHeight(36)
        btn_cancel.setCursor(Qt.CursorShape.PointingHandCursor)
        btn_cancel.clicked.connect(self._handle_cancel)
        if not cancel_text:
            btn_cancel.hide()

        btn_confirm = QPushButton(confirm_text)
        btn_confirm.setObjectName("PopupConfirmButton")
        btn_confirm.setFixedHeight(36)
        btn_confirm.setCursor(Qt.CursorShape.PointingHandCursor)
        btn_confirm.clicked.connect(self._handle_confirm)

        btn_row.addWidget(btn_cancel)
        btn_row.addWidget(btn_confirm)

        card_layout.addWidget(lbl_title)
        card_layout.addWidget(lbl_msg)
        card_layout.addStretch()
        card_layout.addLayout(btn_row)

        card.adjustSize()
        card.move(
            (self.width() - card.width()) // 2,
            (self.height() - card.height()) // 2
        )

        self.opacity_effect = QGraphicsOpacityEffect(self)
        self.setGraphicsEffect(self.opacity_effect)
        self.anim = QPropertyAnimation(self.opacity_effect, b"opacity")
        self.anim.setDuration(200)
        self.anim.setStartValue(0)
        self.anim.setEndValue(1)
        self.show()
        self.raise_()
        self.anim.start()

    def _handle_confirm(self):
        if self.on_confirm:
            self.on_confirm()
        self._close()

    def _handle_cancel(self):
        if self.on_cancel:
            self.on_cancel()
        self._close()

    def _close(self):
        self.anim.setDirection(QPropertyAnimation.Direction.Backward)
        self.anim.finished.connect(self.deleteLater)
        self.anim.start()


# DEV CONSOLE PANEL
class DevConsolePanel(QFrame):
    def __init__(self, parent=None):
        super().__init__(parent)
        self.setObjectName("DevConsole")
        self.setFixedHeight(180)
        self.setStyleSheet("""
            #DevConsole {
                background-color: rgba(10, 8, 18, 220);
                border-top: 1px solid #333333;
            }
            QTextEdit {
                background-color: transparent;
                color: #D7D7D7;
                font-family: 'Consolas', 'Courier New', monospace;
                font-size: 12px;
                border: none;
                padding: 6px 12px;
            }
        """)

        layout = QVBoxLayout(self)
        layout.setContentsMargins(0, 0, 0, 0)
        layout.setSpacing(0)

        header = QHBoxLayout()
        header.setContentsMargins(12, 6, 12, 0)
        lbl = QLabel("Developer Console")
        lbl.setStyleSheet("color: #969696; font-size: 11px; font-weight: bold;")
        btn_clear = QPushButton("Clear")
        btn_clear.setFixedSize(50, 22)
        btn_clear.setStyleSheet(
            "color: #969696; font-size: 11px; border: 1px solid #333; border-radius: 4px; padding: 0;"
        )

        header.addWidget(lbl)
        header.addStretch()
        header.addWidget(btn_clear)

        self.log_view = QTextEdit()
        self.log_view.setReadOnly(True)
        self.log_view.document().setMaximumBlockCount(500)  # cap at 500 lines to prevent performance drops

        btn_clear.clicked.connect(self.log_view.clear)

        layout.addLayout(header)
        layout.addWidget(self.log_view)

        # Attach to the logger and replay the session buffer
        if dev_console_handler:
            history = file_handler.buffer if file_handler else []
            dev_console_handler.attach(self.log_view, history)

    def closeEvent(self, event):
        if dev_console_handler:
            dev_console_handler.detach()
        super().closeEvent(event)


# SETTINGS ROW
class SettingRow(QFrame):
    def __init__(self, title, description, checked=False, on_toggle=None, parent=None):
        super().__init__(parent)
        self._on_toggle = on_toggle
        self.setObjectName("SettingRow")
        self.setFixedHeight(68)
        self.setStyleSheet("""
            #SettingRow {
                background-color: rgba(255, 255, 255, 4);
                border: 1px solid rgba(255, 255, 255, 7);
                border-radius: 10px;
            }
            #SettingRow:hover {
                background-color: rgba(255, 255, 255, 7);
                border: 1px solid rgba(255, 255, 255, 12);
            }
        """)

        layout = QHBoxLayout(self)
        layout.setContentsMargins(20, 0, 20, 0)
        layout.setSpacing(16)

        text_col = QVBoxLayout()
        text_col.setSpacing(3)

        self._lbl_title = QLabel(title)
        self._lbl_title.setStyleSheet("color: #E8E8E8; font-size: 14px; font-weight: 500; background: transparent; border: none;")

        self._lbl_desc = QLabel(description)
        self._lbl_desc.setStyleSheet("color: #707070; font-size: 12px; background: transparent; border: none;")

        text_col.addStretch()
        text_col.addWidget(self._lbl_title)
        text_col.addWidget(self._lbl_desc)
        text_col.addStretch()

        self.toggle = AnimatedToggle(self)
        self.toggle.setChecked(checked)

        layout.addLayout(text_col)
        layout.addStretch()
        layout.addWidget(self.toggle)

    def handle_toggle(self):
        if self._on_toggle:
            self._on_toggle(self.toggle.isChecked())

    def set_title(self, text):
        self._lbl_title.setText(text)

    def set_description(self, text):
        self._lbl_desc.setText(text)


# SETTINGS OVERLAY
_SIDEBAR_STYLE = """
    QFrame#SettingsSidebar {
        background-color: rgba(255, 255, 255, 3);
        border-right: 1px solid rgba(255, 255, 255, 8);
        border-radius: 0px;
    }
    QPushButton#SidebarBtn {
        background: transparent;
        border: none;
        border-radius: 8px;
        color: #707070;
        font-size: 13px;
        font-weight: 400;
        text-align: left;
        padding: 0px 14px;
    }
    QPushButton#SidebarBtn:hover {
        background-color: rgba(255, 255, 255, 6);
        color: #C8C8C8;
    }
    QPushButton#SidebarBtn[active=true] {
        background-color: rgba(255, 255, 255, 10);
        color: #FFFFFF;
        font-weight: 500;
    }
"""

_SECTION_LABEL_STYLE = "color: #484848; font-size: 11px; font-weight: 600; letter-spacing: 1px;"
_PAGE_TITLE_STYLE    = "color: #D7D7D7; font-size: 20px; font-weight: 500;"

class SettingsOverlay(QFrame):
    def __init__(self, parent=None):
        super().__init__(parent)
        self.setObjectName("SettingsContainer")
        self.setFixedSize(800, 500)
        self.move(240, 110)
        self.setAttribute(Qt.WidgetAttribute.WA_StyledBackground)
        self.setStyleSheet(SETTING_STYLE)
        self.hide()

        root = QHBoxLayout(self)
        root.setContentsMargins(0, 0, 0, 0)
        root.setSpacing(0)

        # Sidebar
        sidebar = QFrame()
        sidebar.setObjectName("SettingsSidebar")
        sidebar.setFixedWidth(180)
        sidebar.setStyleSheet(_SIDEBAR_STYLE)

        sidebar_layout = QVBoxLayout(sidebar)
        sidebar_layout.setContentsMargins(16, 28, 16, 28)
        sidebar_layout.setSpacing(4)

        self.lbl_title = QLabel()
        self.lbl_title.setStyleSheet("color: #969696; font-size: 11px; font-weight: 600; letter-spacing: 1px;")
        sidebar_layout.addWidget(self.lbl_title)
        sidebar_layout.addSpacing(12)

        self.btn_general  = QPushButton()
        self.btn_launcher = QPushButton()
        self.btn_developer = QPushButton()
        self._sidebar_btns = [self.btn_general, self.btn_launcher, self.btn_developer]

        for b in self._sidebar_btns:
            b.setObjectName("SidebarBtn")
            b.setFixedHeight(36)
            b.setCursor(Qt.CursorShape.PointingHandCursor)
            b.setCheckable(False)
            sidebar_layout.addWidget(b)

        sidebar_layout.addStretch()

        # Content Area
        self.stack = QStackedWidget()
        self.stack.setContentsMargins(0, 0, 0, 0)

        self.stack.addWidget(self._create_general_page())   # 0
        self.stack.addWidget(self._create_launcher_page())  # 1
        self.stack.addWidget(self._create_developer_page()) # 2

        root.addWidget(sidebar)
        root.addWidget(self.stack, 1)

        # Connect side buttons
        for i, b in enumerate(self._sidebar_btns):
            b.clicked.connect(lambda _, idx=i: self._switch_page(idx))
        self._switch_page(0)

        Translator.language_changed.connect(self.retranslate_ui)
        self.retranslate_ui()

    def _switch_page(self, index):
        self.stack.setCurrentIndex(index)
        for i, b in enumerate(self._sidebar_btns):
            b.setProperty("active", i == index)
            b.style().unpolish(b)
            b.style().polish(b)

    def retranslate_ui(self):
        self.lbl_title.setText(t("settings").upper())
        self.btn_general.setText(t("general"))
        self.btn_launcher.setText(t("launcher"))
        self.btn_developer.setText(t("developer"))
        self.general_page_title.setText(t("general"))
        self.launcher_page_title.setText(t("launcher"))
        self.developer_page_title.setText(t("developer"))
        self._lbl_language.setText(t("language"))
        self._lbl_language_desc.setText(t("language_desc"))
        self._lbl_game_dir.setText(t("game_directory"))
        self._btn_browse.setText(t("browse"))
        self._row_cr.set_title(t("censorship_removal"))
        self._row_cr.set_description(t("censorship_removal_desc"))
        self._row_ndl.set_title(t("no_drive_line"))
        self._row_ndl.set_description(t("no_drive_line_desc"))
        self._row_dev.set_title(t("developer_mode"))
        self._row_dev.set_description(t("developer_mode_desc"))
        self._row_rpc.set_title(t("discord_rpc"))
        self._row_rpc.set_description(t("discord_rpc_desc"))

    # Helpers
    def _make_page(self):
        page = QWidget()
        layout = QVBoxLayout(page)
        layout.setContentsMargins(32, 32, 32, 32)
        layout.setSpacing(0)
        return page, layout

    def _section_label(self, text):
        lbl = QLabel(text.upper())
        lbl.setStyleSheet(_SECTION_LABEL_STYLE)
        lbl.setContentsMargins(4, 0, 0, 0)
        return lbl

    def _divider(self):
        line = QFrame()
        line.setFrameShape(QFrame.Shape.HLine)
        line.setStyleSheet("background-color: rgba(255,255,255,6); border: none; max-height: 1px;")
        return line

    # General Page
    def _create_general_page(self):
        page, layout = self._make_page()

        self.general_page_title = QLabel(t("general"))
        self.general_page_title.setStyleSheet(_PAGE_TITLE_STYLE)
        layout.addWidget(self.general_page_title)
        layout.addSpacing(24)

        layout.addWidget(self._section_label("Appearance"))
        layout.addSpacing(10)

        # Language row
        lang_card = QFrame()
        lang_card.setObjectName("LangCard")
        lang_card.setFixedHeight(68)
        lang_card.setStyleSheet("""
            #LangCard {
                background-color: rgba(255, 255, 255, 4);
                border: 1px solid rgba(255, 255, 255, 7);
                border-radius: 10px;
            }
        """)
        lang_row = QHBoxLayout(lang_card)
        lang_row.setContentsMargins(20, 0, 20, 0)

        lang_text = QVBoxLayout()
        lang_text.setSpacing(3)
        self._lbl_language = QLabel()
        self._lbl_language.setStyleSheet("color: #E8E8E8; font-size: 14px; font-weight: 500; background: transparent; border: none;")
        lang_sub = QLabel(t("language_desc"))
        self._lbl_language_desc = lang_sub
        lang_sub.setStyleSheet("color: #707070; font-size: 12px; background: transparent; border: none;")
        lang_text.addStretch()
        lang_text.addWidget(self._lbl_language)
        lang_text.addWidget(lang_sub)
        lang_text.addStretch()

        from src.config_manager import LANG_NAMES
        self._lang_box = QComboBox()
        self._lang_box.addItems(["English", "中文", "日本語", "Español", "Deutsch", "Türkçe", "Tiếng Việt"])
        self._lang_box.setFixedWidth(160)
        current_code = cfg.get_language()
        display = LANG_NAMES.get(current_code, "English")
        idx = self._lang_box.findText(display)
        if idx >= 0:
            self._lang_box.setCurrentIndex(idx)
        self._lang_box.currentTextChanged.connect(self._on_language_changed)

        lang_row.addLayout(lang_text)
        lang_row.addStretch()
        lang_row.addWidget(self._lang_box)

        layout.addWidget(lang_card)
        layout.addSpacing(24)

        # Integration
        layout.addWidget(self._section_label("Integration"))
        layout.addSpacing(10)

        self._row_rpc = SettingRow(
            title="",
            description="",
            checked=cfg.get_discord_rpc(),
            on_toggle=self._toggle_rpc,
        )
        layout.addWidget(self._row_rpc)
        layout.addStretch()
        return page

    def _on_language_changed(self, display_name):
        from src.config_manager import LANG_CODES
        code = LANG_CODES.get(display_name, "en")
        cfg.set_language(code)
        Translator.load(code)

    # Launcher Page
    def _create_launcher_page(self):
        page, layout = self._make_page()

        self.launcher_page_title = QLabel(t("launcher"))
        self.launcher_page_title.setStyleSheet(_PAGE_TITLE_STYLE)
        layout.addWidget(self.launcher_page_title)
        layout.addSpacing(24)

        # Game Directory
        layout.addWidget(self._section_label("Game Directory"))
        layout.addSpacing(10)

        path_card = QFrame()
        path_card.setObjectName("PathCard")
        path_card.setFixedHeight(68)
        path_card.setStyleSheet("""
            #PathCard {
                background-color: rgba(255, 255, 255, 4);
                border: 1px solid rgba(255, 255, 255, 7);
                border-radius: 10px;
            }
        """)
        path_row = QHBoxLayout(path_card)
        path_row.setContentsMargins(20, 0, 14, 0)
        path_row.setSpacing(12)

        path_text = QVBoxLayout()
        path_text.setSpacing(3)
        self._lbl_game_dir = QLabel()
        self._lbl_game_dir.setStyleSheet("color: #E8E8E8; font-size: 14px; font-weight: 500; background: transparent; border: none;")
        initial_path = self.parent().parent().current_path if self.parent() else ""
        self.path_display = QLabel(str(initial_path) if initial_path else "Not set")
        self.path_display.setStyleSheet("color: #585858; font-size: 11px; font-family: 'Consolas', monospace; background: transparent; border: none;")
        self.path_display.setMaximumWidth(380)
        self.path_display.setWordWrap(False)
        path_text.addStretch()
        path_text.addWidget(self._lbl_game_dir)
        path_text.addWidget(self.path_display)
        path_text.addStretch()

        self._btn_browse = QPushButton()
        self._btn_browse.setFixedSize(72, 32)
        self._btn_browse.setStyleSheet("""
            QPushButton {
                background-color: rgba(255, 255, 255, 8);
                color: #C8C8C8;
                border: 1px solid rgba(255, 255, 255, 12);
                border-radius: 7px;
                font-size: 12px;
            }
            QPushButton:hover {
                background-color: rgba(255, 255, 255, 14);
                color: #FFFFFF;
            }
            QPushButton:pressed {
                background-color: rgba(255, 255, 255, 5);
            }
        """)
        self._btn_browse.clicked.connect(self._handle_browse)

        path_row.addLayout(path_text)
        path_row.addStretch()
        path_row.addWidget(self._btn_browse)

        layout.addWidget(path_card)
        layout.addSpacing(24)

        # Gameplay
        layout.addWidget(self._section_label("Gameplay"))
        layout.addSpacing(10)

        self._row_cr = SettingRow(
            title="Censorship Removal",
            description="",
            checked=cfg.get_censorship_removal(),
            on_toggle=self._toggle_cr_mode,
        )
        layout.addWidget(self._row_cr)
        layout.addSpacing(8)

        self._row_ndl = SettingRow(
            title="No Drive Line",
            description="",
            checked=cfg.get_no_drive_line(),
            on_toggle=self._toggle_ndl_mode,
        )
        layout.addWidget(self._row_ndl)
        layout.addStretch()
        return page

    def _handle_browse(self):
        folder = QFileDialog.getExistingDirectory(self, t("game_directory"))
        if folder:
            self.path_display.setText(folder)
            main_ui = self.parent().parent()
            main_ui.current_path = folder
            cfg.set_game_path(folder)
            main_ui.refresh_launch_state()

    def _toggle_cr_mode(self, new_state):
        cfg.set_censorship_removal(new_state)
        main_ui = self.parent().parent()
        if main_ui.engine:
            main_ui.engine.censorship_removal = new_state

    def _toggle_ndl_mode(self, new_state):
        cfg.set_no_drive_line(new_state)
        main_ui = self.parent().parent()
        if main_ui.engine:
            main_ui.engine.no_drive_line = new_state

    def _toggle_rpc(self, new_state):
        cfg.set_discord_rpc(new_state)
        main_ui = self.parent().parent()
        if new_state:
            from src.discord_rpc import DiscordRPC
            if hasattr(main_ui, 'rpc'):
                main_ui.rpc.stop()
            main_ui.rpc = DiscordRPC()
            main_ui.rpc.set_idle()
            main_ui.rpc.start()
        else:
            if hasattr(main_ui, 'rpc'):
                main_ui.rpc.stop()

    # Developer Page
    def _create_developer_page(self):
        page, layout = self._make_page()

        self.developer_page_title = QLabel(t("developer"))
        self.developer_page_title.setStyleSheet(_PAGE_TITLE_STYLE)
        layout.addWidget(self.developer_page_title)
        layout.addSpacing(24)

        layout.addWidget(self._section_label("Debug"))
        layout.addSpacing(10)

        self._row_dev = SettingRow(
            title="",
            description="",
            checked=cfg.get_dev_mode(),
            on_toggle=self._toggle_dev_mode,
        )
        layout.addWidget(self._row_dev)
        layout.addStretch()
        return page

    def _toggle_dev_mode(self, new_state):
        cfg.set_dev_mode(new_state)
        main_ui = self.parent().parent()
        main_ui.set_dev_console(new_state)

class AnimatedToggle(QWidget):
    def __init__(self, parent=None):
        super().__init__(parent)
        self.setFixedSize(50, 26)
        self._checked = False
        self._handle_position = 3
        
        self._active_color = QColor("#00AD5C") 
        self._inactive_color = QColor("#3E3E42")
        self._handle_color = QColor("#FFFFFF")

        self.animation = QVariantAnimation(self)
        self.animation.setDuration(150)
        self.animation.setEasingCurve(QEasingCurve.Type.InOutQuad)
        self.animation.valueChanged.connect(self._update_position)

    def _update_position(self, v):
        self._handle_position = v
        self.update()

    def isChecked(self): return self._checked

    def setChecked(self, checked):
        self._checked = checked
        self._handle_position = 27 if checked else 3
        self.update()

    def mousePressEvent(self, event):
        self._checked = not self._checked
        start = self._handle_position
        end = 27 if self._checked else 3
        self.animation.setStartValue(start)
        self.animation.setEndValue(end)
        self.animation.start()
        self.parent().handle_toggle()

    def paintEvent(self, event):
        painter = QPainter(self)
        painter.setRenderHint(QPainter.RenderHint.Antialiasing)
        
        color = self._active_color if self._checked else self._inactive_color
        painter.setBrush(color)
        painter.setPen(Qt.PenStyle.NoPen)
        painter.drawRoundedRect(0, 0, self.width(), self.height(), 13, 13)

        painter.setBrush(self._handle_color)
        painter.drawEllipse(self._handle_position, 3, 20, 20)

def _get_mod_image(mod_folder_name: str, mod_display_name: str) -> QPixmap:
    images_dir = Path(resource_path("Bin/Assets/ModImages"))
    if not images_dir.exists():
        return QPixmap()

    images = sorted(
        p for p in images_dir.iterdir()
        if p.suffix.lower() in (".png", ".jpg", ".jpeg")
    )
    if not images:
        return QPixmap()

    name_lower = mod_display_name.lower()

    # Character match (Longest character name will be picked)
    best_match = None
    best_length = 0
    for img_path in images:
        character = img_path.stem.lower()
        if character in name_lower and len(character) > best_length:
            best_match = img_path
            best_length = len(character)

    if best_match:
        return QPixmap(str(best_match))

    # Stable fallback
    idx = hash(mod_folder_name) % len(images)
    return QPixmap(str(images[idx]))


class ModImage(QLabel):
    RADIUS = 6

    def __init__(self, pixmap: QPixmap, size: int, parent=None):
        super().__init__(parent)
        self.setObjectName("ModImage")
        self.setFixedSize(size, size)
        self._source = pixmap

    def paintEvent(self, event):
        painter = QPainter(self)
        painter.setRenderHint(QPainter.RenderHint.Antialiasing)

        from PyQt6.QtGui import QPainterPath
        path = QPainterPath()
        path.addRoundedRect(0, 0, self.width(), self.height(), self.RADIUS, self.RADIUS)
        painter.setClipPath(path)

        if not self._source.isNull():
            scaled = self._source.scaled(
                self.size(),
                Qt.AspectRatioMode.KeepAspectRatioByExpanding,
                Qt.TransformationMode.SmoothTransformation,
            )
            x = (self.width()  - scaled.width())  // 2
            y = (self.height() - scaled.height()) // 2
            painter.drawPixmap(x, y, scaled)
        else:
            # Fallback: filled rect so the slot isn't invisible
            painter.fillRect(self.rect(), QColor(40, 40, 50))

        painter.end()


class ModCard(QFrame):
    def __init__(self, mod, manager, parent_overlay):
        super().__init__()
        self.setObjectName("ModCard")
        self.setFixedHeight(72)
        self.setAttribute(Qt.WidgetAttribute.WA_StyledBackground)

        self.mod = mod
        self.manager = manager
        self.parent_overlay = parent_overlay

        layout = QHBoxLayout(self)
        layout.setContentsMargins(14, 0, 20, 0)
        layout.setSpacing(14)

        # Mod Thumbnail
        pixmap = _get_mod_image(mod.folder_name, mod.display_name)
        thumb = ModImage(pixmap, 44)
        layout.addWidget(thumb)

        # Mod Info
        info_vbox = QVBoxLayout()
        info_vbox.setSpacing(3)

        self.title = QLabel(mod.display_name)
        self.title.setObjectName("ModTitle")

        meta_row = QHBoxLayout()
        meta_row.setSpacing(10)
        meta_row.setContentsMargins(0, 0, 0, 0)

        author_text = f"{t('mod_manager_author')}{mod.author}"
        has_link = mod.support_link.startswith("https://")

        if has_link:
            meta = QLabel(author_text)
            meta.setObjectName("ModAuthorLink")
            meta.setCursor(Qt.CursorShape.PointingHandCursor)
            meta.mousePressEvent = lambda _e, url=mod.support_link: self._open_support_link(url)
        else:
            meta = QLabel(author_text)
            meta.setObjectName("ModMeta")

        version_lbl = QLabel(mod.version)
        version_lbl.setObjectName("ModVersion")

        meta_row.addWidget(meta)
        meta_row.addWidget(version_lbl)
        meta_row.addStretch()

        info_vbox.addStretch()
        info_vbox.addWidget(self.title)
        info_vbox.addLayout(meta_row)
        info_vbox.addStretch()

        layout.addLayout(info_vbox)
        layout.addStretch()

        # Toggle
        self.toggle = AnimatedToggle(self)
        self.toggle.setChecked(mod.is_enabled)
        layout.addWidget(self.toggle)

    def _open_support_link(self, url: str):
        if not url.startswith("https://"):
            return
        PopupDialog(
            parent=self.parent_overlay,
            title="Open Support Link",
            message=f"{url}",
            confirm_text="Open in Browser",
            cancel_text=t("cancel"),
            on_confirm=lambda: webbrowser.open(url),
        )

    def handle_toggle(self):
        new_state = self.toggle.isChecked()

        new_folder_name = self.manager.toggle_mod(self.mod)
        if new_folder_name:
            self.mod.folder_name = new_folder_name
            self.mod.is_enabled = new_state

        self.parent_overlay._update_mod_count()

class ModManagerOverlay(QFrame):
    def __init__(self, parent, mod_manager):
        super().__init__(parent)
        self.setObjectName("ModManagerOverlay")
        self.manager = mod_manager

        self.setGeometry(240, 80, 800, 560)
        self.setAttribute(Qt.WidgetAttribute.WA_StyledBackground)
        self.setStyleSheet(MOD_MANAGER_STYLE)

        root = QVBoxLayout(self)
        root.setContentsMargins(0, 0, 0, 0)
        root.setSpacing(0)

        # Header
        header = QFrame()
        header.setObjectName("ModManagerHeader")
        header.setFixedHeight(64)
        header.setAttribute(Qt.WidgetAttribute.WA_StyledBackground)

        header_layout = QHBoxLayout(header)
        header_layout.setContentsMargins(28, 0, 20, 0)
        header_layout.setSpacing(12)

        title_col = QVBoxLayout()
        title_col.setSpacing(2)
        lbl_title = QLabel(t("mod_manager"))
        lbl_title.setObjectName("ModManagerTitle")
        self._lbl_mod_count = QLabel("")
        self._lbl_mod_count.setObjectName("ModCount")
        title_col.addStretch()
        title_col.addWidget(lbl_title)
        title_col.addWidget(self._lbl_mod_count)
        title_col.addStretch()

        btn_close = QPushButton("✕")
        btn_close.setObjectName("ModManagerClose")
        btn_close.setFixedSize(32, 32)
        btn_close.setCursor(Qt.CursorShape.PointingHandCursor)
        btn_close.clicked.connect(self.hide)

        header_layout.addLayout(title_col)
        header_layout.addStretch()
        header_layout.addWidget(btn_close)

        root.addWidget(header)

        #  Body
        body = QWidget()
        body_layout = QVBoxLayout(body)
        body_layout.setContentsMargins(28, 20, 28, 24)
        body_layout.setSpacing(16)

        # Search Row
        search_row = QFrame()
        search_row.setObjectName("SearchRow")
        search_row.setFixedHeight(42)
        search_row.setAttribute(Qt.WidgetAttribute.WA_StyledBackground)

        sr_layout = QHBoxLayout(search_row)
        sr_layout.setContentsMargins(14, 0, 8, 0)
        sr_layout.setSpacing(6)

        # Search icon
        icon_lbl = QLabel()
        icon_lbl.setObjectName("SearchIcon")
        icon_lbl.setFixedSize(18, 18)
        icon_lbl.setPixmap(QIcon(resource_path("Bin/Assets/search.png")).pixmap(16, 16))

        # Text input
        self.search_bar = QLineEdit()
        self.search_bar.setObjectName("ModSearch")
        self.search_bar.setPlaceholderText(t("search_mods"))
        self.search_bar.textChanged.connect(self.refresh_list)

        # Vertical divider
        divider = QFrame()
        divider.setObjectName("SearchDivider")
        divider.setFrameShape(QFrame.Shape.VLine)
        divider.setFixedHeight(22)

        # Refresh button
        btn_refresh = QPushButton()
        btn_refresh.setObjectName("SearchActionBtn")
        btn_refresh.setFixedSize(30, 30)
        btn_refresh.setToolTip("Refresh mod list")
        btn_refresh.setCursor(Qt.CursorShape.PointingHandCursor)
        btn_refresh.setIcon(QIcon(resource_path("Bin/Assets/refresh.png")))
        btn_refresh.setIconSize(QSize(16, 16))
        btn_refresh.clicked.connect(self.refresh_list)

        # Open folder button
        btn_folder = QPushButton()
        btn_folder.setObjectName("SearchActionBtn")
        btn_folder.setFixedSize(30, 30)
        btn_folder.setToolTip("Open Mods folder")
        btn_folder.setCursor(Qt.CursorShape.PointingHandCursor)
        btn_folder.setIcon(QIcon(resource_path("Bin/Assets/folder.png")))
        btn_folder.setIconSize(QSize(16, 16))
        btn_folder.clicked.connect(self._open_mods_folder)

        sr_layout.addWidget(icon_lbl)
        sr_layout.addWidget(self.search_bar, 1)
        sr_layout.addWidget(divider)
        sr_layout.addWidget(btn_refresh)
        sr_layout.addWidget(btn_folder)

        # Scroll area
        self.scroll = QScrollArea()
        self.scroll.setWidgetResizable(True)
        self.list_container = QWidget()
        self.list_container.setObjectName("ScrollContent")
        self.list_layout = QVBoxLayout(self.list_container)
        self.list_layout.setSpacing(8)
        self.list_layout.setContentsMargins(0, 0, 4, 0)
        self.list_layout.setAlignment(Qt.AlignmentFlag.AlignTop)
        self.scroll.setWidget(self.list_container)

        body_layout.addWidget(search_row)
        body_layout.addWidget(self.scroll, 1)

        root.addWidget(body, 1)
        self.refresh_list()

    def _open_mods_folder(self):
        mods_path = Path(get_app_dir()) / "Mods"
        if not mods_path.exists():
            mods_path.mkdir(parents=True, exist_ok=True)
        if sys.platform == "win32":
            os.startfile(str(mods_path))
        elif sys.platform == "darwin":
            subprocess.Popen(["open", str(mods_path)])
        else:
            subprocess.Popen(["xdg-open", str(mods_path)])

    def _update_mod_count(self):
        mods = self.manager.scan_mods()
        total = len(mods)
        enabled = sum(1 for m in mods if m.is_enabled)
        TMP_desc_a = t("mod_manager_desc_a") or "OF"
        TMP_desc_b = t("mod_manager_desc_b") or "ENABLED"
        self._lbl_mod_count.setText(f"{enabled} {TMP_desc_a} {total} {TMP_desc_b}")

    def refresh_list(self):
        while self.list_layout.count():
            child = self.list_layout.takeAt(0)
            if child.widget():
                child.widget().deleteLater()

        search_text = self.search_bar.text().lower()
        mods = self.manager.scan_mods()
        visible = [
            m for m in mods
            if search_text in m.display_name.lower() or search_text in m.author.lower()
        ]

        self._update_mod_count()

        if not visible:
            empty = QLabel("No mods found" if search_text else "No mods installed")
            empty.setObjectName("EmptyLabel")
            empty.setAlignment(Qt.AlignmentFlag.AlignCenter)
            self.list_layout.addStretch()
            self.list_layout.addWidget(empty)
            self.list_layout.addStretch()
            return

        for mod in visible:
            card = ModCard(mod, self.manager, self)
            self.list_layout.addWidget(card)

# BACKGROUND WIDGET
class BackgroundWidget(QWidget):
    def __init__(self, image_path, parent=None):
        super().__init__(parent)
        self.setAttribute(Qt.WidgetAttribute.WA_StyledBackground)
        self._pixmap = QPixmap(image_path) if os.path.exists(image_path) else QPixmap()

    def paintEvent(self, event):
        if self._pixmap.isNull():
            return
        painter = QPainter(self)
        scaled = self._pixmap.scaled(
            self.size(),
            Qt.AspectRatioMode.KeepAspectRatioByExpanding,
            Qt.TransformationMode.SmoothTransformation
        )
        x = (self.width() - scaled.width()) // 2
        y = (self.height() - scaled.height()) // 2
        painter.drawPixmap(x, y, scaled)
        painter.end()


# OVERLAY WIDGET
class OverlayWidget(QWidget):
    TOP_BAR_HEIGHT = 60

    def __init__(self, parent=None):
        super().__init__(parent)
        self.setAttribute(Qt.WidgetAttribute.WA_TransparentForMouseEvents)
        self.setAttribute(Qt.WidgetAttribute.WA_NoSystemBackground)
        self.setAttribute(Qt.WidgetAttribute.WA_TranslucentBackground)

    def paintEvent(self, event):
        from PyQt6.QtGui import QLinearGradient, QColor, QBrush
        painter = QPainter(self)
        painter.setRenderHint(QPainter.RenderHint.Antialiasing)
        w, h = self.width(), self.height()

        # Bottom Bar
        gradient_top = int(h * 0.45)
        grad = QLinearGradient(0, gradient_top, 0, h)
        grad.setColorAt(0.0, QColor(10, 8, 18, 0))
        grad.setColorAt(0.55, QColor(10, 8, 18, 160))
        grad.setColorAt(1.0, QColor(10, 8, 18, 230))
        painter.fillRect(0, gradient_top, w, h - gradient_top, QBrush(grad))

        # Frosted Panel
        painter.fillRect(0, 0, w, self.TOP_BAR_HEIGHT, QColor(16, 10, 27, 160))
        painter.end()


# OVERLAY WINDOW
class AuroraOverlayWindow(QWidget):
    DISPLAY_MS = 6000
    FADE_MS    = 1000

    def __init__(self):
        super().__init__(None)
        self.setWindowFlags(
            Qt.WindowType.FramelessWindowHint |
            Qt.WindowType.WindowStaysOnTopHint |
            Qt.WindowType.Tool |
            Qt.WindowType.WindowTransparentForInput
        )
        self.setAttribute(Qt.WidgetAttribute.WA_TranslucentBackground)
        self.setAttribute(Qt.WidgetAttribute.WA_ShowWithoutActivating)
        self.setFixedSize(300, 64)

        layout = QHBoxLayout(self)
        layout.setContentsMargins(18, 0, 18, 0)
        layout.setSpacing(12)
        layout.setAlignment(Qt.AlignmentFlag.AlignVCenter)

        # Icon
        icon_lbl = QLabel()
        icon_pix = QIcon(resource_path("Bin/Assets/logo1024_wn.png")).pixmap(30, 30)
        icon_lbl.setPixmap(icon_pix)
        icon_lbl.setFixedSize(30, 30)

        text_col = QVBoxLayout()
        text_col.setSpacing(1)
        text_col.addStretch()

        lbl_title = QLabel("Aurora Mod Loader")
        lbl_title.setStyleSheet("""
            color: #E0E0E0; 
            font-size: 14px; 
            font-weight: 600; 
            font-family: 'Segoe UI', system-ui, sans-serif;
        """)

        lbl_sub = QLabel("Mods are active")
        lbl_sub.setStyleSheet("""
            color: #AAAAAA; 
            font-size: 12px;
            font-family: 'Segoe UI', system-ui, sans-serif;
        """)

        text_col.addWidget(lbl_title)
        text_col.addWidget(lbl_sub)
        text_col.addStretch()

        layout.addWidget(icon_lbl)
        layout.addLayout(text_col)
        layout.addStretch() 

        # Fade effect
        self.opacity_effect = QGraphicsOpacityEffect(self)
        self.setGraphicsEffect(self.opacity_effect)
        self.opacity_effect.setOpacity(0)

    def paintEvent(self, event):
        painter = QPainter(self)
        painter.setRenderHint(QPainter.RenderHint.Antialiasing)
        from PyQt6.QtGui import QColor
        painter.setBrush(QColor(10, 8, 18, 210))
        painter.setPen(QColor(60, 60, 80, 200))
        painter.drawRoundedRect(self.rect().adjusted(1, 1, -1, -1), 12, 12)
        painter.end()

    def show_over_game(self, game_rect=None):
        if game_rect is not None:
            x, y = game_rect.left, game_rect.top
        else:
            x, y = self._find_game_position()
        # Top-left corner of the game window with a small margin
        self.move(x + 20, y + 20)
        self.show()
        self._fade_in()

    def _find_game_position(self):
        try:
            result = [20, 20]
            def enum_cb(hwnd, _):
                length = ctypes.windll.user32.GetWindowTextLengthW(hwnd)
                if length > 0:
                    buf = ctypes.create_unicode_buffer(length + 1)
                    ctypes.windll.user32.GetWindowTextW(hwnd, buf, length + 1)
                    title = buf.value.lower()
                    if any(x in title for x in ["neverness", "htgame", "nte"]):
                        rect = ctypes.wintypes.RECT()
                        ctypes.windll.user32.GetWindowRect(hwnd, ctypes.byref(rect))
                        result[0], result[1] = rect.left, rect.top
                        return False
                return True

            WNDENUMPROC = ctypes.WINFUNCTYPE(ctypes.c_bool, ctypes.wintypes.HWND, ctypes.wintypes.LPARAM)
            ctypes.windll.user32.EnumWindows(WNDENUMPROC(enum_cb), 0)
            return result[0], result[1]
        except Exception as e:
            logger.error(f"Failed to find game position: {e}")
            return 20, 20

    def _fade_in(self):
        self._anim = QPropertyAnimation(self.opacity_effect, b"opacity")
        self._anim.setDuration(400)
        self._anim.setStartValue(0.0)
        self._anim.setEndValue(1.0)
        self._anim.finished.connect(lambda: QTimer.singleShot(self.DISPLAY_MS, self._fade_out))
        self._anim.start()

    def _fade_out(self):
        self._anim = QPropertyAnimation(self.opacity_effect, b"opacity")
        self._anim.setDuration(self.FADE_MS)
        self._anim.setStartValue(1.0)
        self._anim.setEndValue(0.0)
        self._anim.finished.connect(self.close)
        self._anim.start()


# MAIN UI
class AuroraUI(QMainWindow):
    def __init__(self, engine, current_path):
        super().__init__()
        self.engine = engine
        self.current_path = current_path
        self.old_pos = None
        self._search_thread = None

        self.setWindowFlags(Qt.WindowType.FramelessWindowHint)
        self.setFixedSize(1280, 720)
        self.setStyleSheet(MAIN_STYLE)
        self.setWindowTitle("Aurora Launcher")
        self.setWindowIcon(QIcon(resource_path("Bin/Assets/logo.ico")))

        self.central_widget = QWidget()
        self.central_widget.setObjectName("CentralWidget")
        self.setCentralWidget(self.central_widget)

        # Background image, lowest z-order
        self.bg_widget = BackgroundWidget(resource_path("Bin/Assets/background.jpg"), self.central_widget)
        self.bg_widget.setGeometry(0, 0, 1280, 720)
        self.bg_widget.lower()

        # Gradient + frosted panel overlay
        self._overlay = OverlayWidget(self.central_widget)
        self._overlay.setGeometry(0, 0, 1280, 720)

        # Top bar, pinned to top
        self.top_bar = QFrame(self.central_widget)
        self.top_bar.setObjectName("TopBar")
        self.top_bar.setGeometry(0, 0, 1280, 60)
        self.setup_top_bar()

        # Bottom bar, pinned to bottom of the main window
        self.setup_bottom_bar()

        # Dev console, absolutely positioned overlay below the main window
        self._dev_console = DevConsolePanel(self.central_widget)
        self._dev_console.hide()

        self._overlay.raise_()
        self.top_bar.raise_()
        self._bottom_bar.raise_()

        # Settings overlay
        self.settings_menu = SettingsOverlay(self.central_widget)
        self.btn_settings.clicked.connect(self.toggle_settings)

        # Load saved language and apply
        Translator.load(cfg.get_language())
        Translator.language_changed.connect(self.retranslate_ui)
        self.retranslate_ui()

        # Apply saved dev mode
        if cfg.get_dev_mode():
            self.set_dev_console(True)

        self.check_for_updates()
        self.refresh_launch_state()

    # Translation
    def retranslate_ui(self):
        self.btn_search.setToolTip(t("search_tooltip"))
        self.refresh_launch_state() 

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
        logger.info(self.current_version)
        logger.info(self.online_version)
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
        self.btn_close.clicked.connect(self.close)

        tb_layout.addWidget(self.btn_settings)
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

        self.mod_manager = ModManager(
            mods_dir=Path(get_app_dir()) / "Mods",
            state_file=Path(get_app_dir()) / "Bin" / "disabled_mods.json"
        )

        self.btn_folder = QPushButton()
        self.btn_folder.setIcon(QIcon(resource_path("Bin/Assets/folder.png")))
        self.btn_folder.setIconSize(QSize(42, 42))
        self.btn_folder.clicked.connect(self.toggle_mod_manager)

        self.btn_coffee = QPushButton()
        self.btn_coffee.setIcon(QIcon(resource_path("Bin/Assets/coffee.png")))
        self.btn_coffee.setIconSize(QSize(42, 42))
        self.btn_coffee.clicked.connect(lambda: webbrowser.open("https://ko-fi.com/daturaxoxo"))

        self.btn_discord = QPushButton()
        self.btn_discord.setIcon(QIcon(resource_path("Bin/Assets/discord.png")))
        self.btn_discord.setIconSize(QSize(42, 42))
        self.btn_discord.clicked.connect(lambda: webbrowser.open("https://discord.gg/565jfeYsbp"))

        self.btn_gamebanana = QPushButton()
        self.btn_gamebanana.setIcon(QIcon(resource_path("Bin/Assets/marketplace.png")))
        self.btn_gamebanana.setIconSize(QSize(42, 42))
        self.btn_gamebanana.clicked.connect(lambda: webbrowser.open("https://gamebanana.com/games/23012"))

        self.btn_search = QPushButton()
        self.btn_search.setObjectName("SearchButton")
        self.btn_search.setIcon(QIcon(resource_path("Bin/Assets/refresh.png")))
        self.btn_search.setIconSize(QSize(28, 28))
        self.btn_search.setFixedSize(60, 60)
        self.btn_search.setCursor(Qt.CursorShape.PointingHandCursor)
        self.btn_search.clicked.connect(self._prompt_drive_search)
        self.btn_search.hide()

        self.btn_launch = QPushButton()
        self.btn_launch.setObjectName("LaunchButton")
        self.btn_launch.setFixedSize(240, 60)
        self.btn_launch.setIconSize(QSize(28, 28))
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
        is_valid = validate_path(self.current_path) if self.current_path else False
        if is_valid:
            self.btn_launch.setIcon(QIcon(resource_path("Bin/Assets/checkmark.png")))
            self.btn_launch.setEnabled(True)
            self.btn_launch.setText(f"    {t('launch')}")
            self.btn_search.hide()
        else:
            self.btn_launch.setIcon(QIcon(resource_path("Bin/Assets/cancel.png")))
            self.btn_launch.setEnabled(False)
            self.btn_launch.setText(f"    {t('launch_invalid')}")
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
            cfg.set_game_path(found_path)
            from src.engine import AuroraEngine
            self.engine = AuroraEngine(found_path, censorship_removal=cfg.get_censorship_removal(), no_drive_line=cfg.get_no_drive_line())
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
        logger.info("Launch button was clicked, initialising engine...")
        if not self.engine:
            logger.warning("Launch aborted: Engine not initialized.")
            ToastNotification(self.central_widget, t("toast_engine_error"), True)
            return
        # logger.info(f"Target Path: {self.current_path}")
        self.btn_launch.setEnabled(False)
        self.btn_launch.setText(f"    {t('launch_running')}")
        self.monitor_thread = GameMonitorThread(self.engine)
        self.monitor_thread.finished.connect(self.refresh_launch_state)
        self.monitor_thread.finished.connect(lambda: setattr(self, 'monitor_thread', None))  # ← add this
        self.monitor_thread.game_started.connect(self._show_game_overlay)
        self.monitor_thread.access_denied.connect(self._show_access_denied_popup)
        self.monitor_thread.start()

        if hasattr(self, 'rpc'):
            self.rpc.set_launching()
        self.monitor_thread.game_started.connect(lambda: hasattr(self, 'rpc') and self.rpc.set_in_game())
        self.monitor_thread.finished.connect(lambda: hasattr(self, 'rpc') and self.rpc.set_idle())

        ToastNotification(self.central_widget, t("toast_launching"), False)

    def _show_access_denied_popup(self):
        """Called when engine.inject() returns 'access_denied'.
        Most commonly caused by antivirus software or UAC intercepting
        file copy / junction creation. We can't confirm it's AV specifically,
        but we can give the user an actionable message.
        """
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
        """Called on main thread when HTGame.exe is first detected."""
        logger.info("HTGame.exe detected. Starting window polling...")
        
        # Initialize polling state
        self._poll_count = 0
        self._poll_timer = QTimer(self)
        self._poll_timer.timeout.connect(self._check_window_ready)
        self._poll_timer.start(500)  # Default: 500ms

    def _get_game_hwnd(self):
        """Helper to find the HWND for the game window with non-blocking checks."""
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
        """Checks if the game window is fully initialized AND focused."""
        self._poll_count += 1
        
        if self._poll_count > 300: # 150s timeout (300 polls * 500ms)
            logger.warning("Overlay polling timed out.")
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
                logger.info(f"NTE has loaded, showing game overlay...")
                self._poll_timer.stop()
                self._spawn_overlay(rect)
            else:
                if self._poll_count % 10 == 0 and not is_focused:
                    logger.info("NTE (HTGame.exe) is detected but not focused, yielding game overlay until focused.")

    def _spawn_overlay(self, game_rect=None):
        self._overlay_win = AuroraOverlayWindow()
        self._overlay_win.show_over_game(game_rect)

    # WINDOW CHROME
    def closeEvent(self, event):
        # Guard: stop the overlay poll timer if Aurora is closed while the
        # game is still loading, to prevent the timer firing on a destroyed object.
        if hasattr(self, '_poll_timer') and self._poll_timer is not None:
            self._poll_timer.stop()
        logger.info("Aurora was closed normally by the user.")
        event.accept()

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
