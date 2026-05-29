from pathlib import Path
import shutil

from PyQt6.QtWidgets import (
    QLayout, QScrollArea, QWidget, QVBoxLayout, QHBoxLayout,
    QPushButton, QLabel, QFrame, QComboBox, QStackedWidget, QFileDialog, QSlider
)
from PyQt6.QtCore import Qt, QSize
from PyQt6.QtGui import QIcon
from src.utils import resource_path
from src.styles import SETTING_STYLE
from src import config_manager as cfg
from src.translator import Translator, t
from src.logger import dev_console_handler, get_app_dir
from src.ui.elements import AnimatedToggle
from src.helpers import ui_scaling
import os
from datetime import datetime
import logging

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

        root = QVBoxLayout(self)
        root.setContentsMargins(0, 0, 0, 0)
        root.setSpacing(0)

        # Top bar with close button
        top_bar = QWidget()
        top_bar.setFixedHeight(44)
        top_bar.setAttribute(Qt.WidgetAttribute.WA_TranslucentBackground)
        top_bar_layout = QHBoxLayout(top_bar)
        top_bar_layout.setContentsMargins(0, 10, 14, 0)
        top_bar_layout.setSpacing(0)
        top_bar_layout.addStretch()

        btn_close = QPushButton()
        btn_close.setIcon(QIcon(resource_path("Bin/Assets/close.png")))
        btn_close.setIconSize(QSize(24, 24))
        btn_close.setFixedSize(24, 24)
        btn_close.setCursor(Qt.CursorShape.PointingHandCursor)
        btn_close.setStyleSheet(
            "QPushButton { background: transparent; border: none; }"
        )
        btn_close.clicked.connect(self.hide)
        top_bar_layout.addWidget(btn_close)

        top_bar.setParent(self)
        top_bar.move(0, 0)
        top_bar.resize(800, 44)

        # Sidebar + content body
        body = QWidget()
        body.setAttribute(Qt.WidgetAttribute.WA_TranslucentBackground)
        body_layout = QHBoxLayout(body)
        body_layout.setContentsMargins(0, 0, 0, 0)
        body_layout.setSpacing(0)

        # Sidebar
        sidebar = QFrame()
        sidebar.setObjectName("SettingsSidebar")
        sidebar.setFixedWidth(180)
        sidebar.setStyleSheet(_SIDEBAR_STYLE)

        sidebar_layout = QVBoxLayout(sidebar)
        sidebar_layout.setContentsMargins(16, 28, 16, 16)
        sidebar_layout.setSpacing(4)

        self.lbl_title = QLabel()
        self.lbl_title.setStyleSheet("color: #969696; font-size: 11px; font-weight: 600; letter-spacing: 1px;")
        sidebar_layout.addWidget(self.lbl_title)
        sidebar_layout.addSpacing(12)

        self.btn_general  = QPushButton()
        self.btn_launcher = QPushButton()
        self.btn_developer = QPushButton()
        self.btn_addons = QPushButton()
        self._sidebar_btns = [self.btn_general, self.btn_launcher, self.btn_addons, self.btn_developer]

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
        self.stack.addWidget(self._create_addons_page())    # 2
        self.stack.addWidget(self._create_developer_page()) # 3

        body_layout.addWidget(sidebar)
        body_layout.addWidget(self.stack, 1)
        root.addWidget(body, 1)
        top_bar.raise_()

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
        self.btn_addons.setText(t("addons"))
        self.btn_developer.setText(t("developer"))
        self.general_page_title.setText(t("general"))
        self.launcher_page_title.setText(t("launcher"))
        self.addons_page_title.setText(t("addons"))
        self.developer_page_title.setText(t("developer"))
        self._lbl_language.setText(t("language"))
        self._lbl_language_desc.setText(t("language_desc"))
        self._lbl_game_dir.setText(t("game_directory"))
        self._btn_browse.setText(t("browse"))
        self._row_cr.set_title(t("censorship_removal"))
        self._row_cr.set_description(t("censorship_removal_desc"))
        self._row_ndl.set_title(t("no_drive_line"))
        self._row_ndl.set_description(t("no_drive_line_desc"))
        self._row_uid.set_title(t("hide_uid_title"))
        self._row_uid.set_description(t("hide_uid_desc"))
        self._row_hide_dots.set_title(t("hide_dots_title"))
        self._row_hide_dots.set_description(t("hide_dots_desc"))
        self._row_dev.set_title(t("developer_mode"))
        self._row_dev.set_description(t("developer_mode_desc"))
        self._row_min.set_title(t("ui_minimization_title"))
        self._row_min.set_description(t("ui_minimization_desc"))
        self._row_EL.set_title(t("extensive_logging"))
        self._row_EL.set_description(t("extensive_logging_desc"))
        self._lbl_export.setText(t("export_file_title"))
        self._lbl_export_desc.setText(t("export_file_desc"))
        self._btn_export_console.setText(t("export_file_button"))
        self._row_rpc.set_title(t("discord_rpc"))
        self._row_rpc.set_description(t("discord_rpc_desc"))
        self._lbl_ui_scale.setText(t("ui_scaling"))
        self._lbl_ui_scale_desc.setText(t("ui_scaling_desc"))
        self._row_hard_links.set_title(t("hard_links"))
        self._row_hard_links.set_description(t("hard_links_desc"))

    # Helpers
    def _make_page(self):
        scroll = QScrollArea()
        scroll.setWidgetResizable(True)
        scroll.setFrameShape(QScrollArea.Shape.NoFrame)
        scroll.setStyleSheet("QScrollArea { background: transparent; }")
        
        page = QWidget()
        page.setStyleSheet("QWidget { background: transparent; }")
        layout = QVBoxLayout(page)
        layout.setContentsMargins(32, 32, 32, 32)
        layout.setSpacing(0)
        layout.setAlignment(Qt.AlignmentFlag.AlignTop)
        
        scroll.setWidget(page)
        
        return scroll, layout

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
    
    def _make_slider(self, minimum, maximum, step, width, value, color="#C8A8FF"):
        slider = QSlider(Qt.Orientation.Horizontal)
        slider.setMinimum(minimum)
        slider.setMaximum(maximum)
        slider.setSingleStep(step)
        slider.setPageStep(step)
        slider.setTickInterval(step)
        slider.setFixedWidth(width)
        slider.setValue(value)
        slider.setStyleSheet(f"""
            QSlider::groove:horizontal {{
                height: 4px;
                background: rgba(255, 255, 255, 12);
                border-radius: 2px;
            }}
            QSlider::sub-page:horizontal {{
                background: rgba(180, 140, 255, 180);
                border-radius: 2px;
            }}
            QSlider::handle:horizontal {{
                width: 14px;
                height: 14px;
                margin: -5px 0;
                border-radius: 7px;
                background: {color};
            }}
            QSlider::handle:horizontal:hover {{
                background: #DFC0FF;
            }}
        """)
        return slider

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
        self._lang_box.addItems(["English", "中文", "日本語", "Español", "Português (Brasil)", "Deutsch", "Türkçe", "Tiếng Việt", "Nederlands", "Pусский", "Bahasa Indonesia", "Italiano"])
        self._lang_box.setFixedWidth(160)
        self._lang_box.setStyleSheet("""
            QComboBox {
                background-color: #1e1e1e;
                color: #C8C8C8;
                border: 1px solid rgba(255, 255, 255, 12);
                border-radius: 7px;
                padding: 4px 10px;
                font-size: 12px;
            }
            QComboBox:hover {
                background-color: #2a2a2a;
                color: #FFFFFF;
            }
            QComboBox::drop-down {
                border: none;
                width: 0px;
                background-color: transparent;
            }
            QComboBox::down-arrow {
                width: 0;
                height: 0;
            }
            QComboBox QAbstractItemView {
                background-color: #1e1e1e;
                color: #C8C8C8;
                border: 1px solid rgba(255, 255, 255, 12);
                selection-background-color: rgba(255, 255, 255, 10);
                selection-color: #FFFFFF;
                outline: none;
            }
        """)
        current_code = cfg.get(cfg.Key.LANGUAGE)
        display = LANG_NAMES.get(current_code, "English")
        idx = self._lang_box.findText(display)
        if idx >= 0:
            self._lang_box.setCurrentIndex(idx)
        self._lang_box.currentTextChanged.connect(self._on_language_changed)

        self._row_min = SettingRow(
            title="Launcher Minimization",
            description="",
            checked=cfg.get(cfg.Key.UI_MINIMIZATION),
            on_toggle=lambda v: self._toggle(cfg.Key.UI_MINIMIZATION, v),
        )

        lang_row.addLayout(lang_text)
        lang_row.addStretch()
        lang_row.addWidget(self._lang_box)

        layout.addWidget(lang_card)
        layout.addSpacing(10)
        layout.addWidget(self._row_min)
        layout.addSpacing(24)

        # Integration
        layout.addWidget(self._section_label("Integration"))
        layout.addSpacing(10)

        self._row_rpc = SettingRow(
            title="",
            description="",
            checked=cfg.get(cfg.Key.DISCORD_RPC),
            on_toggle=self._toggle_rpc,
        )
        layout.addWidget(self._row_rpc)
        layout.addStretch()
        return page

    def _on_language_changed(self, display_name):
        from src.config_manager import LANG_CODES
        code = LANG_CODES.get(display_name, "en")
        cfg.set(cfg.Key.LANGUAGE, code)
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

        # Mods
        layout.addWidget(self._section_label("Mods"))
        layout.addSpacing(10)
        self._row_hard_links = SettingRow(
            title="Use Hard Links",
            description="",
            checked=cfg.get(cfg.Key.USE_HARD_LINKS),
            on_toggle=self._toggle_hard_links,
        )
        layout.addWidget(self._row_hard_links)

        layout.addStretch()

        return page

    def _handle_browse(self):
        folder = QFileDialog.getExistingDirectory(self, t("game_directory"))
        if folder:
            self.path_display.setText(folder)
            main_ui = self.parent().parent()
            main_ui.current_path = folder
            cfg.set(cfg.Key.GAME_PATH, folder)
            main_ui.refresh_launch_state()

    def _toggle_cr_mode(self, new_state):
        self._toggle(cfg.Key.CENSORSHIP_REMOVE, new_state)
        engine = self.parent().parent().engine
        if engine:
            engine.censorship_removal = new_state

    def _toggle_ndl_mode(self, new_state):
        self._toggle(cfg.Key.NO_DRIVE_LINE, new_state)
        engine = self.parent().parent().engine
        if engine:
            engine.no_drive_line = new_state
            
    def _toggle_hard_links(self, new_state):
        from src.logger import logger
        logger.info("Toggling hard links...")
        cfg.set(cfg.Key.USE_HARD_LINKS, new_state)
        nte_mod_folder = Path(cfg.get(cfg.Key.GAME_PATH) + "/Client/WindowsNoEditor/HT/Content/Paks/AuroraMods")
        aurora_mod_folder = Path(get_app_dir() + "/Mods")
        logger.info(f"Mod folder: {aurora_mod_folder}")
        logger.info(f"NTE mod folder: {nte_mod_folder}")
        if new_state:
            if nte_mod_folder.exists():
                logger.info("Moving files from NTE mod folder to Aurora mod folder...")
                for file in nte_mod_folder.iterdir():
                    shutil.move(file, aurora_mod_folder)
                shutil.rmtree(nte_mod_folder)
        else:
            logger.info("Moving files from Aurora mod folder to NTE mod folder...")
            nte_mod_folder.mkdir(exist_ok=True)
            if aurora_mod_folder.exists():
                for file in aurora_mod_folder.iterdir():
                    shutil.move(file, nte_mod_folder)

    def _toggle_rpc(self, new_state):
        self._toggle(cfg.Key.DISCORD_RPC, new_state)
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

    def _on_scale_slider_moved(self, value: int):
        snapped = round(value / 5) * 5 # Snap to the nearest 5 to make it look smooth
        if self._scale_slider.value() != snapped:
            self._scale_slider.blockSignals(True)
            self._scale_slider.setValue(snapped)
            self._scale_slider.blockSignals(False)
        self._scale_value_lbl.setText(f"{snapped}%")
 
    def _on_scale_committed(self):
        value   = self._scale_slider.value()
        scale_f = round(value / 100, 2)
 
        cfg.set(cfg.Key.UI_SCALING, scale_f)
 
        ok = ui_scaling.apply_scale(scale_f)
        if not ok:
            from src.logger import logger
            logger.warning(
                f"UI Scaling: failed to write Engine.ini "
                f"({ui_scaling.ini_path()}). "
                "Check file permissions."
            )

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
            checked=cfg.get(cfg.Key.DEV_MODE),
            on_toggle=self._toggle_dev_mode,
        )
        self._row_EL = SettingRow(
            title="",
            description="",
            checked=cfg.get(cfg.Key.EXTENSIVE_LOGGING),
            on_toggle=lambda v: self._toggle(cfg.Key.EXTENSIVE_LOGGING, v),
        )
        export_card = QFrame()
        export_card.setObjectName("ExportCard")
        export_card.setFixedHeight(68)
        export_card.setStyleSheet("""
            #ExportCard {
                background-color: rgba(255, 255, 255, 4);
                border: 1px solid rgba(255, 255, 255, 7);
                border-radius: 10px;
            }
        """)
        export_row = QHBoxLayout(export_card)
        export_row.setContentsMargins(20, 0, 14, 0)
        export_row.setSpacing(12)

        # Export file button
        export_text = QVBoxLayout()
        export_text.setSpacing(3)
        self._lbl_export = QLabel()
        self._lbl_export.setStyleSheet("color: #E8E8E8; font-size: 14px; font-weight: 500; background: transparent; border: none;")
        self._lbl_export_desc = QLabel()
        self._lbl_export_desc.setStyleSheet("color: #707070; font-size: 12px; background: transparent; border: none;")
        export_text.addStretch()
        export_text.addWidget(self._lbl_export)
        export_text.addWidget(self._lbl_export_desc)
        export_text.addStretch()

        self._btn_export_console = QPushButton()
        self._btn_export_console.setFixedSize(72, 32)
        self._btn_export_console.setCursor(Qt.CursorShape.PointingHandCursor)
        self._btn_export_console.setStyleSheet("""
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
        self._btn_export_console.clicked.connect(self._handle_export_console)
        layout.addWidget(self._row_dev)
        layout.addSpacing(10)
        layout.addWidget(self._row_EL)
        layout.addSpacing(10)
        export_row.addLayout(export_text)
        export_row.addStretch()
        export_row.addWidget(self._btn_export_console)

        layout.addWidget(export_card)
        layout.addStretch()
        
        return page
    
    # Addons Page
    def _create_addons_page(self):
        page, layout = self._make_page()

        self.addons_page_title = QLabel(t("addons"))
        self.addons_page_title.setStyleSheet(_PAGE_TITLE_STYLE)
        layout.addWidget(self.addons_page_title)
        layout.addSpacing(24)

        layout.addWidget(self._section_label("Builtin Addons"))
        layout.addSpacing(10)

        self._row_cr = SettingRow(
            title="Censorship Removal",
            description="",
            checked=cfg.get(cfg.Key.CENSORSHIP_REMOVE),
            on_toggle=self._toggle_cr_mode,
        )
        self._row_ndl = SettingRow(
            title="No Drive Line",
            description="",
            checked=cfg.get(cfg.Key.NO_DRIVE_LINE),
            on_toggle=self._toggle_ndl_mode,
        )
        self._row_uid = SettingRow(
            title="Hide UID",
            description="",
            checked=cfg.get(cfg.Key.HIDE_UID),
            on_toggle=lambda v: self._toggle(cfg.Key.HIDE_UID, v),
        )
        self._row_hide_dots = SettingRow(
            title="Hide Red Dots",
            description="",
            checked=cfg.get(cfg.Key.HIDE_NOTIF_DOTS),
            on_toggle=lambda v: self._toggle(cfg.Key.HIDE_NOTIF_DOTS, v),
        )

        # Slider (still have to make it prettier)
        saved_scale = cfg.get(cfg.Key.UI_SCALING)
        if saved_scale is None:
            saved_scale = 1.0
        initial_val = int(round(float(saved_scale) * 100))

        scale_card = QFrame()
        scale_card.setObjectName("ScaleCard")
        scale_card.setMinimumHeight(68)
        scale_card.setStyleSheet("""
            #ScaleCard {
                background-color: rgba(255, 255, 255, 4);
                border: 1px solid rgba(255, 255, 255, 7);
                border-radius: 10px;
            }
        """)
        scale_row = QHBoxLayout(scale_card)
        scale_row.setContentsMargins(20, 0, 20, 0)
        scale_row.setSpacing(16)

        scale_text = QVBoxLayout()
        scale_text.setSpacing(3)
        self._lbl_ui_scale = QLabel()
        self._lbl_ui_scale.setStyleSheet("color: #E8E8E8; font-size: 14px; font-weight: 500; background: transparent; border: none;")
        self._lbl_ui_scale_desc = QLabel()
        self._lbl_ui_scale_desc.setStyleSheet("color: #707070; font-size: 12px; background: transparent; border: none;")
        self._lbl_ui_scale_desc.setWordWrap(True)
        self._lbl_ui_scale_desc.setMaximumWidth(280)
        scale_text.addStretch()
        scale_text.addWidget(self._lbl_ui_scale)
        scale_text.addWidget(self._lbl_ui_scale_desc)
        scale_text.addStretch()

        self._scale_slider = self._make_slider(50, 200, 25, 160, initial_val)
        self._scale_slider.valueChanged.connect(self._on_scale_slider_moved)
        self._scale_slider.sliderReleased.connect(self._on_scale_committed)

        self._scale_value_lbl = QLabel(f"{initial_val}%")
        self._scale_value_lbl.setFixedWidth(36)
        self._scale_value_lbl.setAlignment(Qt.AlignmentFlag.AlignRight | Qt.AlignmentFlag.AlignVCenter)
        self._scale_value_lbl.setStyleSheet("color: #C8A8FF; font-size: 12px; font-weight: 500; background: transparent; border: none;")

        layout.addWidget(self._row_cr)
        layout.addSpacing(6)
        layout.addWidget(self._row_ndl)
        layout.addSpacing(6)
        layout.addWidget(self._row_uid)
        layout.addSpacing(6)
        layout.addWidget(self._row_hide_dots)
        layout.addSpacing(6)
        scale_row.addLayout(scale_text)
        scale_row.addStretch()
        scale_row.addWidget(self._scale_value_lbl)
        scale_row.addWidget(self._scale_slider)
        layout.addWidget(scale_card)
        layout.addStretch()

        return page
    
    def _toggle(self, key, new_state):
        cfg.set(key, new_state)

    def _toggle_dev_mode(self, new_state):
        self._toggle(cfg.Key.DEV_MODE, new_state)
        self.parent().parent().set_dev_console(new_state)

    def _handle_export_console(self):
        
        records = getattr(dev_console_handler, 'session_buffer', [])

        log_dir = os.path.join(get_app_dir(), "Logs")
        os.makedirs(log_dir, exist_ok=True)

        filename = datetime.now().strftime("aurora_console_%Y-%m-%d_%H-%M-%S.aulog")
        filepath = os.path.join(log_dir, filename)

        formatter = logging.Formatter(
            '[%(asctime)s] [%(levelname)s] %(message)s',
            datefmt='%H:%M:%S'
        )
        try:
            with open(filepath, 'w', encoding='utf-8') as f:
                f.write(f"--- Aurora Console Export — {datetime.now().strftime('%Y-%m-%d %H:%M:%S')} ---\n\n")
                for record in records:
                    f.write(formatter.format(record) + '\n')
            self._btn_export_console.setText(t("export_file_done"))
            from PyQt6.QtCore import QTimer
            QTimer.singleShot(2500, lambda: self._btn_export_console.setText(t("export_file_button")))
        except OSError:
            self._btn_export_console.setText("Error")
