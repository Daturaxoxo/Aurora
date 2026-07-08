from functools import partial
import os, shutil, subprocess, sys, time, tempfile, webbrowser
from pathlib import Path
from typing import List, Optional

from PyQt6.QtCore import (
    QObject, QPoint, QPointF, QPropertyAnimation, QRectF, QSize,
    Qt, QThread, QTimer, pyqtSignal
)
from PyQt6.QtGui import (
    QBrush, QColor, QIcon, QLinearGradient, QPainter,
    QPainterPath, QPen, QPixmap
)
from PyQt6.QtWidgets import (
    QFrame, QGraphicsOpacityEffect, QGridLayout, QHBoxLayout,
    QLabel, QLineEdit, QPushButton, QScrollArea, QVBoxLayout, QWidget, QMainWindow, QApplication
)
from src import config_manager as cfg
from src.backend.helpers.api import (
    NTEMod, NTEModFile, get_mod_files, clear_cache,
    get_nte_mods, search_nte_mods,
)
from src.logger import logger
from src.frontend.styles import GB_STYLE
from src.translator import t
from src.frontend.classes.elements import AnimatedToggle, PopupDialog, _rounded_pixmap, show_image
from src.utils import bytes_to_human_readable, get_mods_path, resource_path, get_seven_zip_path, hidden_subprocess_kwargs

# Display names, for the icons to show the display name must match the png names in \ModImages
_NTE_CHARACTERS: list[str] = [
    "Adler",
    "Aurelia",
    "Baicang",
    "Chaos",
    "Chiz",
    "Daffodill",
    "Edgar",
    "Fadia",
    "Haniel",
    "Hathor",
    "Hotori",
    "Iroi",
    "Jiuyuan",
    "Lacrimosa",
    "Mint",
    "Nanally",
    "Sakiri",
    "Shinku",
    "Skia",
    "Female Zero",
    "Male Zero",
]

_CHAR_CHIP_QSS = """
    QPushButton#CharChip {
        background:    #1c2030;
        color:         #8b949e;
        border:        1px solid #2a2d35;
        border-radius: 13px;
        font-size:     11px;
        font-weight:   600;
        padding:       0 10px 0 4px;
        text-align:    left;
    }
    QPushButton#CharChip:hover {
        background:  #232840;
        border-color:#4493f8;
        color:       #c9d1d9;
    }
    QPushButton#CharChip[active="true"] {
        background:  #17304f;
        border-color:#4493f8;
        color:       #79c0ff;
    }
    QPushButton#CharChip[active="true"]:hover {
        background: #1c3a60;
    }
"""

class _CharacterFilterBar(QWidget):
    filter_changed = pyqtSignal(str)

    def __init__(self, parent=None):
        super().__init__(parent)
        self.setFixedHeight(46)
        self.setAttribute(Qt.WidgetAttribute.WA_StyledBackground)
        self.setStyleSheet(_CHAR_CHIP_QSS)

        self._active: str = ""
        self._chips: dict[str, QPushButton] = {}

        scroll = QScrollArea(self)
        scroll.setFrameShape(QScrollArea.Shape.NoFrame)
        scroll.setHorizontalScrollBarPolicy(Qt.ScrollBarPolicy.ScrollBarAsNeeded)
        scroll.setVerticalScrollBarPolicy(Qt.ScrollBarPolicy.ScrollBarAlwaysOff)
        scroll.setWidgetResizable(True)
        scroll.setStyleSheet("""
            QScrollArea { background: transparent; border: none; }
            QScrollBar:horizontal {
                height: 4px;
                background: transparent;
                margin: 0;
            }
            QScrollBar::handle:horizontal {
                background: #3a3f4b;
                border-radius: 2px;
                min-width: 24px;
            }
            QScrollBar::handle:horizontal:hover {
                background: #4493f8;
            }
            QScrollBar::add-line:horizontal,
            QScrollBar::sub-line:horizontal {
                width: 0;
            }
        """)

        inner = QWidget()
        inner.setStyleSheet("background: transparent;")
        row = QHBoxLayout(inner)
        row.setContentsMargins(2, 4, 2, 4)
        row.setSpacing(6)

        for name in _NTE_CHARACTERS:
            btn = QPushButton(f"  {name}")
            btn.setObjectName("CharChip")
            btn.setFixedHeight(26)
            btn.setCursor(Qt.CursorShape.PointingHandCursor)
            btn.setProperty("active", "false")

            icon_path = resource_path(f"Bin/Assets/ModImages/{name.lower()}.png")
            pix = QPixmap(icon_path)
            if not pix.isNull():
                btn.setIcon(QIcon(pix.scaled(16, 16, Qt.AspectRatioMode.KeepAspectRatio, Qt.TransformationMode.SmoothTransformation)))
                btn.setIconSize(QSize(16, 16))

            btn.clicked.connect(partial(self._on_chip, name))
            row.addWidget(btn)
            self._chips[name] = btn

        row.addStretch()
        scroll.setWidget(inner)

        outer = QHBoxLayout(self)
        outer.setContentsMargins(0, 0, 0, 0)
        outer.setSpacing(0)
        outer.addWidget(scroll)

    @property
    def active_character(self) -> str: return self._active

    def clear_selection(self, *, silent: bool = True):
        if self._active:
            self._set_chip(self._active, False)
            self._active = ""
        if not silent: self.filter_changed.emit("")

    def _on_chip(self, name: str):
        if self._active == name:
            self._set_chip(name, False)
            self._active = ""
            self.filter_changed.emit("")
        else:
            if self._active: self._set_chip(self._active, False)
            self._set_chip(name, True)
            self._active = name
            self.filter_changed.emit(name)

    def _set_chip(self, name: str, active: bool):
        btn = self._chips.get(name)
        if btn:
            btn.setProperty("active", "true" if active else "false")
            btn.style().unpolish(btn)
            btn.style().polish(btn)

class _ModFetcher(QObject):
    mod_ready = pyqtSignal(object, int)
    page_done = pyqtSignal(bool, int)
    finished  = pyqtSignal()

    def __init__(self, page: int, force_refresh: bool = False, generation: int = 0):
        super().__init__()
        self._page  = page
        self._force = force_refresh
        self._cancelled = False
        self.generation = generation

    def cancel(self): self._cancelled = True

    def run(self):
        had_results_ref = []

        def _on_mod(mod):
            if self._cancelled: return
            had_results_ref.append(True)
            self.mod_ready.emit(mod, self.generation)

        try:
            get_nte_mods(force_refresh=self._force, page=self._page, on_mod_ready=_on_mod)
            if not self._cancelled: self.page_done.emit(bool(had_results_ref), self.generation)
        finally: self.finished.emit()


class _SearchFetcher(QObject):
    mod_ready = pyqtSignal(object, int)
    page_done = pyqtSignal(bool, int)
    finished  = pyqtSignal()

    def __init__(self, query: str, page: int, generation: int = 0):
        super().__init__()
        self._query = query
        self._page  = page
        self._cancelled = False
        self.generation = generation

    def cancel(self):
        self._cancelled = True

    def run(self):
        had_results_ref = []

        def _on_mod(mod):
            if self._cancelled: return
            had_results_ref.append(True)
            self.mod_ready.emit(mod, self.generation)

        try:
            search_nte_mods(query=self._query, page=self._page, on_mod_ready=_on_mod,)
            if not self._cancelled: self.page_done.emit(bool(had_results_ref), self.generation)
        finally: self.finished.emit()


class GameBananaBrowserOverlay(QFrame):
    def __init__(self, parent, mod_manager, scale: float=1.0):
        super().__init__(parent)
        self.setObjectName("GameBananaBrowserOverlay")
        self.manager = mod_manager

        s=scale
        self.setGeometry(240, 80, 800, 560)
        self.setGeometry(int(240*s), int(80*s), int(800*s), int(560*s))
        self.setAttribute(Qt.WidgetAttribute.WA_StyledBackground)
        self.setStyleSheet(GB_STYLE)
        self.closeEvent = self._on_close

        self._all_mods: list            = []
        self._cached_mods: list         = []
        self._current_page              = 0
        self._has_more                  = True
        self._loading                   = False
        self._thread: QThread | None    = None
        self._fetcher                   = None
        self._threads: set[QThread]     = set()
        self._fetch_generation          = 0

        self._search_mode               = False
        self._search_query              = ""
        self._search_page               = 0
        self._search_has_more           = True
        self._char_filter: str          = ""

        self._search_timer = QTimer(self)
        self._search_timer.setSingleShot(True)
        self._search_timer.setInterval(400)
        self._search_timer.timeout.connect(self._commit_search)

        root = QVBoxLayout(self)
        root.setContentsMargins(0, 0, 0, 0)
        root.setSpacing(0)

        header = QFrame()
        header.setObjectName("GBModManagerHeader")
        header.setFixedHeight(64)
        header.setAttribute(Qt.WidgetAttribute.WA_StyledBackground)

        header_layout = QHBoxLayout(header)
        header_layout.setContentsMargins(28, 0, 20, 0)
        header_layout.setSpacing(12)

        title_col = QVBoxLayout()
        lbl_title = QLabel(t("gamebanana_mods"))
        lbl_title.setObjectName("GBModManagerTitle")
        self._lbl_gb_status = QLabel("")
        self._lbl_gb_status.setObjectName("GBStatus")
        self._lbl_gb_status.setStyleSheet("color: #8b949e; font-size: 11px;")
        title_col.addStretch()
        title_col.addSpacing(24)
        title_col.addWidget(lbl_title)
        title_col.addWidget(self._lbl_gb_status)
        title_col.addStretch()

        lbl_nsfw_mods = QLabel(t("show_nsfw_mods"))
        lbl_nsfw_mods.setStyleSheet("color: #8b949e; font-size: 11px;")

        self.toggle_nsfw_mods = AnimatedToggle(self)
        self.toggle_nsfw_mods.setChecked(cfg.get(cfg.Key.SHOW_NSFW_MODS))
        self.toggle_nsfw_mods.setCursor(Qt.CursorShape.PointingHandCursor)
        self.toggle_nsfw_mods.setToolTip(t("show_nsfw_mods_tooltip"))

        btn_clear_cache = QPushButton()
        btn_clear_cache.setObjectName("SearchActionBtn")
        btn_clear_cache.setFixedSize(30, 30)
        btn_clear_cache.setToolTip(t("clear_cache_tooltip"))
        btn_clear_cache.setCursor(Qt.CursorShape.PointingHandCursor)
        btn_clear_cache.setIcon(QIcon(resource_path("Bin/Assets/delete.png")))
        btn_clear_cache.setIconSize(QSize(14, 14))
        btn_clear_cache.clicked.connect(self._confirm_clear_cache)

        btn_close = QPushButton("✕")
        btn_close.setObjectName("GBModManagerClose")
        btn_close.setFixedSize(32, 32)
        btn_close.setCursor(Qt.CursorShape.PointingHandCursor)
        btn_close.clicked.connect(self.hide)

        header_layout.addLayout(title_col)
        header_layout.addStretch()
        header_layout.addWidget(lbl_nsfw_mods)
        header_layout.addWidget(self.toggle_nsfw_mods)
        header_layout.addWidget(btn_clear_cache)
        header_layout.addWidget(btn_close)
        root.addWidget(header)

        # Body
        body = QWidget()
        body_layout = QVBoxLayout(body)
        body_layout.setContentsMargins(28, 16, 28, 24)
        body_layout.setSpacing(12)

        # Search bar
        search_row = QFrame()
        search_row.setObjectName("GBSearchRow")
        search_row.setFixedHeight(42)
        search_row.setAttribute(Qt.WidgetAttribute.WA_StyledBackground)

        sr_layout = QHBoxLayout(search_row)
        sr_layout.setContentsMargins(14, 0, 8, 0)
        sr_layout.setSpacing(6)

        search_icon = QLabel()
        search_icon.setObjectName("GBSearchIcon")
        search_icon.setFixedSize(18, 18)
        search_icon.setPixmap(QIcon(resource_path("Bin/Assets/search.png")).pixmap(16, 16))

        self._search_input = QLineEdit()
        self._search_input.setObjectName("GBSearchInput")
        self._search_input.setPlaceholderText(t("gb_search_title"))
        self._search_input.textChanged.connect(self._on_search_text_changed)

        self._search_clear_btn = QPushButton("✕")
        self._search_clear_btn.setObjectName("GBSearchClear")
        self._search_clear_btn.setFixedSize(24, 24)
        self._search_clear_btn.setCursor(Qt.CursorShape.PointingHandCursor)
        self._search_clear_btn.setToolTip("Clear search")
        self._search_clear_btn.clicked.connect(self._clear_search)
        self._search_clear_btn.hide()

        sr_layout.addWidget(search_icon)
        sr_layout.addWidget(self._search_input, 1)
        sr_layout.addWidget(self._search_clear_btn)
        body_layout.addWidget(search_row)

        self._char_filter_bar = _CharacterFilterBar()
        self._char_filter_bar.filter_changed.connect(self._on_char_filter_changed)
        body_layout.addWidget(self._char_filter_bar)

        self.gb_scroll = QScrollArea()
        self.gb_scroll.setWidgetResizable(True)
        self.gb_scroll.setFrameShape(QScrollArea.Shape.NoFrame)
        self.gb_scroll.setStyleSheet("QScrollArea { background: transparent; }")
        self.gb_container = QWidget()
        self.gb_container.setObjectName("GBContainer")
        self.gb_container.setStyleSheet("background: transparent;")
        self.gb_grid = QGridLayout(self.gb_container)
        self.gb_grid.setSpacing(10)
        self.gb_grid.setContentsMargins(0, 0, 0, 0)
        self.gb_grid.setAlignment(Qt.AlignmentFlag.AlignCenter)
        self.gb_scroll.setWidget(self.gb_container)

        body_layout.addWidget(self.gb_scroll, 1)
        root.addWidget(body, 1)

        self.gb_scroll.verticalScrollBar().valueChanged.connect(self._on_scroll)

        self._load_next_page()

    def _on_search_text_changed(self, text: str):
        self._search_clear_btn.setVisible(bool(text))
        stripped = text.strip()

        if not stripped:
            self._search_timer.stop()
            self._lbl_gb_status.setText("")
            if self._search_mode: self._exit_search_mode()
            return

        if len(stripped) < 3:
            self._search_timer.stop()
            self._lbl_gb_status.setText(t("gb_search_min_chars"))
            return

        self._lbl_gb_status.setText("")
        self._search_timer.start()

    def _commit_search(self):
        query = self._search_input.text().strip()
        if not query: return
        if len(query) < 3: return
        if query == self._search_query and self._search_mode: return
        self._enter_search_mode(query)

    def _clear_search(self):
        self._search_input.clear()

    def _enter_search_mode(self, query: str):
        self._search_mode     = True
        self._search_query    = query
        self._search_page     = 0
        self._search_has_more = True

        self._fetch_generation += 1
        self._stop_all_fetches()
        self._clear_grid()

        TMP_trans = t("searching")
        self._lbl_gb_status.setText(f"{TMP_trans} \"{query}\"")
        self._load_next_search_page()

    def _exit_search_mode(self):
        self._search_mode     = False
        self._search_query    = ""
        self._search_page     = 0
        self._search_has_more = True

        self._fetch_generation += 1
        self._stop_all_fetches()
        self._clear_grid()
        self._current_page = 0
        self._has_more     = True
        self._loading      = False

        self._load_next_page()

    def _load_next_page(self, force_refresh: bool = False):
        if self._search_mode or self._loading or not self._has_more: return

        self._loading = True
        self._current_page += 1
        self._lbl_gb_status.setText(t("loading"))

        generation = self._fetch_generation
        thread  = QThread()
        fetcher = _ModFetcher(self._current_page, force_refresh, generation)
        self._start_thread(thread, fetcher)

    def _load_next_search_page(self):
        if not self._search_mode or self._loading or not self._search_has_more: return

        self._loading = True
        self._search_page += 1

        generation = self._fetch_generation
        thread  = QThread()
        fetcher = _SearchFetcher(self._search_query, self._search_page, generation)
        self._start_thread(thread, fetcher)

    def _start_thread(self, thread: QThread, fetcher):
        fetcher.moveToThread(thread)
        thread.started.connect(fetcher.run)
        fetcher.mod_ready.connect(self._on_mod_ready)
        fetcher.page_done.connect(self._on_page_done)
        fetcher.finished.connect(thread.quit)
        fetcher.finished.connect(fetcher.deleteLater)
        thread.finished.connect(thread.deleteLater)
        thread.finished.connect(lambda t=thread: self._threads.discard(t))
        self._threads.add(thread)
        self._thread  = thread
        self._fetcher = fetcher
        thread.start()


    def _on_mod_ready(self, mod: NTEMod, generation: int):
        if generation != self._fetch_generation: return
        self._cached_mods.append(mod)
        if not self._mod_passes_filters(mod): return
        self._all_mods.append(mod)
        cols = max(1, (self.gb_scroll.width() - 40) // 148)
        i = len(self._all_mods) - 1
        self.gb_grid.addWidget(GameBananaMod(mod), i // cols, i % cols)

    def _mod_passes_filters(self, mod: NTEMod) -> bool:
        if mod.is_nsfw and not self.toggle_nsfw_mods.isChecked(): return False
        if self._char_filter and mod.sub_category.lower() != self._char_filter.lower(): return False
        return True


    def _on_char_filter_changed(self, name: str):
        self._char_filter = name
        self._clear_grid(keep_cache=True)
        for mod in self._cached_mods:
            if not self._mod_passes_filters(mod): continue
            self._all_mods.append(mod)
            cols = max(1, (self.gb_scroll.width() - 40) // 148)
            i = len(self._all_mods) - 1
            self.gb_grid.addWidget(GameBananaMod(mod), i // cols, i % cols)
        if not self._search_mode: self._load_next_page()

    def _on_page_done(self, had_results: bool, generation: int):
        if generation != self._fetch_generation: return

        self._loading = False

        if self._search_mode:
            self._lbl_gb_status.setText(f"\"{self._search_query}\"" if had_results else "")
            if not had_results:
                self._search_has_more = False
                if not self._all_mods:
                    TMP_trans = t("gb_no_result")
                    self._show_empty(f"{TMP_trans} \"{self._search_query}\".")
        else:
            self._lbl_gb_status.setText("")
            if not had_results:
                self._has_more = False
                if not self._all_mods: self._show_empty(t("gb_no_mods"))

        self._check_fill()

    def _show_empty(self, message: str):
        empty = QLabel(message)
        empty.setStyleSheet("color: #8b949e; font-size: 13px; border: none;")
        empty.setAlignment(Qt.AlignmentFlag.AlignCenter)
        self.gb_grid.addWidget(empty, 0, 0, 1, 1, Qt.AlignmentFlag.AlignCenter)

    def _check_fill(self): QTimer.singleShot(0, self._auto_load_if_needed)
    def _auto_load_if_needed(self):
        if self._loading: return
        content  = self.gb_container.sizeHint().height()
        viewport = self.gb_scroll.viewport().height()
        if content <= viewport:
            if self._search_mode:   self._load_next_search_page()
            else:                   self._load_next_page()

    def _on_scroll(self, value: int):
        if self._loading: return
        vbar = self.gb_scroll.verticalScrollBar()
        if vbar.maximum() > 0 and value >= vbar.maximum() - 50:
            if self._search_mode: self._load_next_search_page()
            else: self._load_next_page()

    def _clear_grid(self, keep_cache: bool = False):
        while self.gb_grid.count():
            item = self.gb_grid.takeAt(0)
            w = item.widget()
            if w: w.deleteLater()
        self._all_mods.clear()
        if not keep_cache: self._cached_mods.clear()

    def _on_close(self, event):
        self._stop_all_fetches()
        event.accept()

    def _stop_all_fetches(self):
        self._search_timer.stop()
        if self._fetcher: self._fetcher.cancel()
        for thread in list(self._threads):
            try:
                if thread.isRunning():
                    thread.quit()
                    thread.wait(5000)
            except RuntimeError: pass
        self._threads.clear()
        self._thread  = None
        self._fetcher = None
        
    def hideEvent(self, event):
        self._stop_all_fetches()
        self._clear_grid()
        for mod in self._cached_mods: mod.thumbnail = b""
        self._cached_mods.clear()
        self._current_page  = 0
        self._has_more      = True
        self._loading       = False
        self._search_mode   = False
        self._search_query  = ""
        self._search_page   = 0
        self._search_has_more = True
        super().hideEvent(event)
        
    def showEvent(self, event):
        super().showEvent(event)
        if not self._all_mods and not self._loading: self._load_next_page()

    def handle_toggle(self, checked=None):
        cfg.set(cfg.Key.SHOW_NSFW_MODS, self.toggle_nsfw_mods.isChecked())
        self._fetch_generation += 1
        self._stop_all_fetches()
        self._clear_grid()
        self._search_mode  = False
        self._search_query = ""
        self._search_input.blockSignals(True)
        self._search_input.clear()
        self._search_input.blockSignals(False)
        self._search_clear_btn.hide()
        self._char_filter  = ""
        self._char_filter_bar.clear_selection()
        self._current_page = 0
        self._has_more = True
        self._loading = False
        self._load_next_page(force_refresh=False)

    def _confirm_clear_cache(self):
        PopupDialog(
            parent=self.window(),
            title=t("clear_cache_title"),
            message=t("clear_cache_message"),
            confirm_text=t("confirm"),
            cancel_text=t("cancel"),
            on_confirm=self._do_clear_cache,
        )

    def _do_clear_cache(self):
        self._fetch_generation += 1
        self._stop_all_fetches()
        clear_cache()
        self._clear_grid()
        self._search_mode  = False
        self._search_query = ""
        self._search_input.blockSignals(True)
        self._search_input.clear()
        self._search_input.blockSignals(False)
        self._search_clear_btn.hide()
        self._char_filter  = ""
        self._char_filter_bar.clear_selection()
        self._current_page = 0
        self._has_more     = True
        self._loading      = False
        self._load_next_page(force_refresh=True)

class GameBananaMod(QFrame):
    CARD_W  = 160
    THUMB_H = 90

    _CARD_QSS = """
        QFrame#GameBananaModCard {
            background: #13151a;
            border: 1px solid #2a2d35;
            border-radius: 10px;
        }
        QFrame#GameBananaModCard:hover {
            border-color: #4493f8;
            background: #181c24;
        }
        QLabel#GBStat {
            color: #8b949e; font-size: 11px;
            background: transparent; border: none;
        }
        QLabel#GBName {
            color: #e6edf3; font-size: 12px; font-weight: 700;
            background: transparent; border: none;
        }
        QLabel#GBAuthor {
            color: #8b949e; font-size: 11px;
            background: transparent; border: none;
        }
        QLabel#GBCategory {
            color: #6e7681; font-size: 10px;
            background: transparent; border: none;
        }
        QLabel#GBRatingNSFW {
            color: #f85149; font-size: 10px; font-weight: 700;
            background: #3d1c1c; border: 1px solid #6e2a2a;
            border-radius: 4px; padding: 1px 5px;
        }
        QLabel#GBRatingSFW {
            color: #3fb950; font-size: 10px; font-weight: 700;
            background: #0f2b18; border: 1px solid #196130;
            border-radius: 4px; padding: 1px 5px;
        }
        QPushButton#GBInstallBtn {
            background: #238636; color: #ffffff;
            border: none; border-radius: 6px;
            font-size: 11px; font-weight: 700; padding: 0 8px;
        }
        QPushButton#GBInstallBtn:hover  { background: #2ea043; }
        QPushButton#GBInstallBtn:pressed { background: #1a6b2a; }
        QPushButton#GBOpenBtn {
            background: #21262d; color: #c9d1d9;
            border: 1px solid #30363d; border-radius: 6px;
            font-size: 11px; font-weight: 600; padding: 0 8px;
        }
        QPushButton#GBOpenBtn:hover  { background: #30363d; border-color: #4493f8; color: #e6edf3; }
        QPushButton#GBOpenBtn:pressed { background: #161b22; }
        QFrame#GBSep { background: #21262d; }
    """

    def __init__(self, mod: NTEMod, parent=None):
        super().__init__(parent)
        self.mod = mod
        self.setObjectName("GameBananaModCard")
        self.setFixedWidth(self.CARD_W)
        self.setAttribute(Qt.WidgetAttribute.WA_StyledBackground)
        self.setCursor(Qt.CursorShape.ArrowCursor)
        self.setStyleSheet(self._CARD_QSS)

        root = QVBoxLayout(self)
        root.setContentsMargins(0, 0, 0, 0)
        root.setSpacing(0)

        thumbnail_pix = QPixmap()
        thumbnail_pix.loadFromData(mod.thumbnail)

        thumb = QLabel()
        thumb.setObjectName("GBThumbnail")
        thumb.setFixedSize(self.CARD_W, self.THUMB_H)
        thumb.setAlignment(Qt.AlignmentFlag.AlignCenter)
        thumb.setCursor(Qt.CursorShape.PointingHandCursor)
        thumb.setPixmap(_rounded_pixmap(thumbnail_pix, self.CARD_W, self.THUMB_H, radius=10))
        thumb.setStyleSheet("border-top-left-radius: 10px; border-top-right-radius: 10px;")

        def _thumb_click(e, pix=thumbnail_pix):
            if e.button() == Qt.MouseButton.LeftButton and not pix.isNull(): show_image(pix, self)
        thumb.mousePressEvent = _thumb_click
        root.addWidget(thumb)

        body = QVBoxLayout()
        body.setContentsMargins(10, 8, 10, 10)
        body.setSpacing(6)
        root.addLayout(body)

        name_lbl = QLabel(mod.name)
        name_lbl.setObjectName("GBName")
        name_lbl.setWordWrap(True)
        name_lbl.setMaximumHeight(38)
        body.addWidget(name_lbl)

        author_row = QHBoxLayout()
        author_row.setSpacing(4)
        author_row.setContentsMargins(0, 0, 0, 0)
        author_icon = QLabel()
        author_icon.setFixedSize(13, 13)
        _pix_author = QPixmap(resource_path("Bin/Assets/author.png"))
        if not _pix_author.isNull():
            author_icon.setPixmap(_pix_author.scaled(13, 13, Qt.AspectRatioMode.KeepAspectRatio, Qt.TransformationMode.SmoothTransformation))
        author_icon.setStyleSheet("background: transparent; border: none;")
        author_lbl = QLabel(mod.author)
        author_lbl.setObjectName("GBAuthor")
        author_row.addWidget(author_icon)
        author_row.addWidget(author_lbl)
        author_row.addStretch()
        body.addLayout(author_row)

        sep1 = QFrame(); sep1.setObjectName("GBSep"); sep1.setFixedHeight(1)
        body.addWidget(sep1)

        cat_row = QHBoxLayout()
        cat_row.setSpacing(5)
        cat_row.setContentsMargins(0, 0, 0, 0)
        cat_text = mod.root_category
        if mod.sub_category: cat_text = f"{mod.root_category} › {mod.sub_category}" if cat_text else mod.sub_category
        cat_lbl = QLabel(cat_text or "—")
        cat_lbl.setObjectName("GBCategory")
        cat_lbl.setWordWrap(False)
        if mod.is_nsfw:
            rating_icon_path = resource_path("Bin/Assets/rating_nsfw.png")
            rating_lbl = QLabel("NSFW"); rating_lbl.setObjectName("GBRatingNSFW")
        else:
            rating_icon_path = resource_path("Bin/Assets/rating_sfw.png")
            rating_lbl = QLabel("SFW"); rating_lbl.setObjectName("GBRatingSFW")
        rating_icon_lbl = QLabel()
        rating_icon_lbl.setFixedSize(13, 13)
        _pix_rating = QPixmap(rating_icon_path)
        if not _pix_rating.isNull():
            rating_icon_lbl.setPixmap(
                _pix_rating.scaled(13, 13, Qt.AspectRatioMode.KeepAspectRatio,
                                   Qt.TransformationMode.SmoothTransformation)
            )
        rating_icon_lbl.setStyleSheet("background: transparent; border: none;")
        cat_row.addWidget(cat_lbl, stretch=1)
        cat_row.addWidget(rating_icon_lbl)
        cat_row.addWidget(rating_lbl)
        body.addLayout(cat_row)

        sep2 = QFrame(); sep2.setObjectName("GBSep"); sep2.setFixedHeight(1)
        body.addWidget(sep2)

        # Stats row
        def _fmt(n: int) -> str:
            if n >= 1_000_000: return f"{n / 1_000_000:.1f}M"
            if n >= 1_000:     return f"{n / 1_000:.1f}K"
            return str(n)

        def _stat_widget(icon_path: str, value: int) -> QWidget:
            w = QWidget(); w.setStyleSheet("background: transparent;")
            h = QHBoxLayout(w)
            h.setContentsMargins(0, 0, 0, 0); h.setSpacing(3)
            icon_lbl = QLabel(); icon_lbl.setFixedSize(12, 12)
            pix = QPixmap(resource_path(icon_path))
            if not pix.isNull():
                icon_lbl.setPixmap(
                    pix.scaled(12, 12, Qt.AspectRatioMode.KeepAspectRatio,
                               Qt.TransformationMode.SmoothTransformation)
                )
            icon_lbl.setStyleSheet("background: transparent; border: none;")
            val_lbl = QLabel(_fmt(value)); val_lbl.setObjectName("GBStat")
            h.addWidget(icon_lbl); h.addWidget(val_lbl)
            return w

        stats_row = QHBoxLayout()
        stats_row.setSpacing(0); stats_row.setContentsMargins(0, 0, 0, 0)
        stats_row.addWidget(_stat_widget("Bin/Assets/views.png",    mod.view_count))
        stats_row.addStretch()
        stats_row.addWidget(_stat_widget("Bin/Assets/download.png", mod.download_count))
        stats_row.addStretch()
        stats_row.addWidget(_stat_widget("Bin/Assets/like.png",     mod.like_count))
        body.addLayout(stats_row)

        # Buttons
        btn_row = QHBoxLayout()
        btn_row.setSpacing(6); btn_row.setContentsMargins(0, 0, 0, 0)

        install_btn = QPushButton()
        install_btn.setObjectName("GBInstallBtn")
        install_btn.setFixedHeight(28)
        install_btn.setCursor(Qt.CursorShape.PointingHandCursor)
        install_btn.setToolTip(t("gamebanana_install_btn") or "Download & Install")
        _dl_icon = QPixmap(resource_path("Bin/Assets/download.png"))
        if not _dl_icon.isNull():
            install_btn.setIcon(QIcon(_dl_icon))
            install_btn.setIconSize(QSize(13, 13))
        install_btn.setText(t("gamebanana_install_btn") or "Install")
        install_btn.clicked.connect(self._install)

        open_btn = QPushButton()
        open_btn.setObjectName("GBOpenBtn")
        open_btn.setFixedHeight(28); open_btn.setFixedWidth(34)
        open_btn.setCursor(Qt.CursorShape.PointingHandCursor)
        open_btn.setToolTip(t("gamebanana_open_btn") or "Open on GameBanana")
        _gb_icon = QPixmap(resource_path("Bin/Assets/marketplace.png"))
        if not _gb_icon.isNull():
            open_btn.setIcon(QIcon(_gb_icon))
            open_btn.setIconSize(QSize(16, 16))
        open_btn.clicked.connect(self._open_gamebanana)

        btn_row.addWidget(install_btn, stretch=1)
        btn_row.addWidget(open_btn)
        body.addLayout(btn_row)

    def _install(self):
        files = get_mod_files(self.mod.id)
        if len(files) == 1:
            file = files[0]
            self.window().__dict__["mod_overlay"].__dict__["gamebanana_install_zone"].install_file(
                file.name, file.url
            )
            return
        self.overlay = _InstallSelectionOverlay(self.window(), files)
        self.overlay.show()
        self.overlay.raise_()

    def _open_gamebanana(self):
        url  = self.mod.mod_url or f"https://gamebanana.com/mods/{self.mod.id}"
        host = self.window()
        if host is None or host is self:
            webbrowser.open(url)
            return
        PopupDialog(
            parent=host,
            title="Open GameBanana",
            message=(
                f"You are about to open an external link in your browser:\n\n"
                f"{url}\n\nContinue?"
            ),
            confirm_text="Open in Browser",
            cancel_text=t("cancel") or "Cancel",
            on_confirm=lambda: webbrowser.open(url),
        )

class _InstallSelectionOverlay(QWidget):
    def __init__(self, parent: QWidget, files: Optional[List[NTEModFile]]):
        super().__init__(parent)
        self.setObjectName("InstallSelectionOverlay")
        self.setFixedSize(parent.size())
        self.move(0, 0)
        self.setAttribute(Qt.WidgetAttribute.WA_StyledBackground)
        self.setStyleSheet("""QWidget#InstallSelectionOverlay { background: rgba(0, 0, 0, 180); }""")

        self.card = QFrame(self)
        self.card.setObjectName("InstallCard")
        self.card.setFixedWidth(460)
        self.card.setAttribute(Qt.WidgetAttribute.WA_StyledBackground)
        self.card.setStyleSheet("""
            QFrame#InstallCard {
                background: #161b22;
                border: 1px solid #30363d;
                border-radius: 12px;
            }
        """)

        card_layout = QVBoxLayout(self.card)
        card_layout.setContentsMargins(24, 24, 24, 24)
        card_layout.setSpacing(16)

        title = QLabel(t("gamebanana_install_title"))
        title.setAlignment(Qt.AlignmentFlag.AlignCenter)
        title.setStyleSheet(
            "font-size: 16px; font-weight: bold; color: #e6edf3;"
            "background: transparent; border: none;"
        )
        card_layout.addWidget(title)

        scroll = QScrollArea()
        scroll.setWidgetResizable(True)
        scroll.setStyleSheet("""
            QScrollArea { border: none; background: transparent; }
            QScrollBar:vertical { background: #161b22; width: 6px; border-radius: 3px; }
            QScrollBar::handle:vertical { background: #3d444d; border-radius: 3px; min-height: 20px; }
        """)

        files_widget = QWidget()
        files_widget.setStyleSheet("background: transparent;")
        files_layout = QVBoxLayout(files_widget)
        files_layout.setSpacing(8)
        files_layout.setContentsMargins(0, 0, 0, 0)

        if files:
            for file in files:
                btn = QPushButton(f"{file.name} ({bytes_to_human_readable(file.size)})")
                btn.setCursor(Qt.CursorShape.PointingHandCursor)
                btn.setStyleSheet("""
                    QPushButton {
                        background: #238636; color: #ffffff;
                        border: none; border-radius: 6px;
                        font-size: 13px; font-weight: 600;
                        padding: 12px 16px; text-align: left;
                    }
                    QPushButton:hover  { background: #2ea043; }
                    QPushButton:pressed { background: #1a6b2a; }
                """)
                btn.clicked.connect(
                    partial(
                        parent.__dict__["mod_overlay"].__dict__["gamebanana_install_zone"].install_file,
                        file.name,
                        file.url,
                    )
                )
                files_layout.addWidget(btn)
        else:
            empty_lbl = QLabel(t("gamebanana_install_empty") or "No files available.")
            empty_lbl.setStyleSheet("color: #8b949e; font-size: 13px;")
            empty_lbl.setAlignment(Qt.AlignmentFlag.AlignCenter)
            files_layout.addWidget(empty_lbl)

        scroll.setWidget(files_widget)
        files_widget.adjustSize()
        scroll.setFixedHeight(min(files_widget.sizeHint().height(), 250))
        card_layout.addWidget(scroll)

        btn_row = QHBoxLayout()
        btn_row.addStretch()
        cancel_btn = QPushButton(t("cancel"))
        cancel_btn.setCursor(Qt.CursorShape.PointingHandCursor)
        cancel_btn.setStyleSheet("""
            QPushButton {
                background: #21262d; color: #c9d1d9;
                border: 1px solid #30363d; border-radius: 6px;
                font-size: 13px; font-weight: 600; padding: 8px 24px;
            }
            QPushButton:hover  { background: #30363d; border-color: #8b949e; color: #e6edf3; }
            QPushButton:pressed { background: #161b22; }
        """)
        cancel_btn.clicked.connect(self._close_overlay)
        btn_row.addWidget(cancel_btn)
        card_layout.addLayout(btn_row)

        self.card.adjustSize()
        self.card.move(
            (self.width() - self.card.width()) // 2,
            (self.height() - self.card.height()) // 2,
        )

        self.opacity_effect = QGraphicsOpacityEffect(self)
        self.setGraphicsEffect(self.opacity_effect)
        self.anim = QPropertyAnimation(self.opacity_effect, b"opacity")
        self.anim.setDuration(200)
        self.anim.setStartValue(0)
        self.anim.setEndValue(1)
        self.show(); self.raise_(); self.anim.start()

    def mousePressEvent(self, e):
        if e.button() == Qt.MouseButton.LeftButton:
            if not self.card.geometry().contains(e.pos()): self._close_overlay()

    def _close_overlay(self):
        self.anim.setDirection(QPropertyAnimation.Direction.Backward)
        self.anim.finished.connect(self.deleteLater)
        self.anim.start()

_BG          = "#101010"
_BORDER      = "#282828"
_TEXT_PRI    = "#D7D7D7"
_TEXT_SEC    = "#707070"
_TEXT_MUTED  = "#484848"
_ACCENT_FULL = "#C8A8FF"

_WIN_W = 420
_WIN_H = 230

_WINDOW_STYLE = f"""
QWidget#InstallProgressWindow {{
    background-color: {_BG};
    border: 1px solid {_BORDER};
    border-radius: 16px;
}}
QLabel {{
    background: transparent;
    border: none;
}}
QPushButton#CancelBtn {{
    background-color: transparent;
    color: {_TEXT_MUTED};
    border: 1px solid #282828;
    border-radius: 8px;
    font-size: 12px;
    padding: 5px 18px;
}}
QPushButton#CancelBtn:hover {{
    background-color: rgba(255, 255, 255, 7);
    color: {_TEXT_SEC};
    border-color: #383838;
}}
QPushButton#CancelBtn:pressed {{
    background-color: rgba(255, 255, 255, 3);
}}
"""


class _SmartProgressBar(QWidget):
    _TRACK_H   = 3
    _GLOW_FRAC = 0.38
    _SPEED     = 0.0022

    def __init__(self, parent=None):
        super().__init__(parent)
        self.setFixedHeight(self._TRACK_H + 2)
        self._indeterminate = False
        self._progress      = 0.0
        self._pos           = 0.0
        self._last_tick     = time.monotonic()
        self._timer = QTimer(self)
        self._timer.setInterval(16)
        self._timer.timeout.connect(self._tick)

    def start_indeterminate(self):
        self._indeterminate = True
        self._last_tick     = time.monotonic()
        if not self._timer.isActive(): self._timer.start()

    def set_progress(self, value: float):
        self._indeterminate = False
        self._progress      = max(0.0, min(1.0, value))
        if self._timer.isActive():
            self._timer.stop()
        self.update()

    def stop(self): self._timer.stop()

    def _tick(self):
        now             = time.monotonic()
        dt              = (now - self._last_tick) * 1000
        self._last_tick = now
        self._pos       = (self._pos + self._SPEED * dt) % 1.0
        self.update()

    def paintEvent(self, _event):
        p = QPainter(self)
        p.setRenderHint(QPainter.RenderHint.Antialiasing)
        w, h = self.width(), self._TRACK_H
        y    = (self.height() - h) // 2
        p.setPen(Qt.PenStyle.NoPen)
        track = QPainterPath()
        track.addRoundedRect(QRectF(0, y, w, h), h / 2, h / 2)
        p.fillPath(track, QBrush(QColor(40, 40, 40)))
        accent = QColor(_ACCENT_FULL)
        if self._indeterminate:
            frac   = self._GLOW_FRAC
            center = self._pos
            x0     = (center - frac / 2) * w
            x1     = (center + frac / 2) * w
            grad        = QLinearGradient(QPointF(x0, 0), QPointF(x1, 0))
            transparent = QColor(accent); transparent.setAlpha(0)
            dim         = QColor(accent); dim.setAlpha(90)
            grad.setColorAt(0.0,  transparent)
            grad.setColorAt(0.25, dim)
            grad.setColorAt(0.5,  accent)
            grad.setColorAt(0.75, dim)
            grad.setColorAt(1.0,  transparent)
            p.save()
            p.setClipPath(track)
            p.fillRect(int(x0), y, int(x1 - x0) + 1, h, QBrush(grad))
            p.restore()
        else:
            fill_w = w * self._progress
            if fill_w > 0:
                fill = QPainterPath()
                fill.addRoundedRect(QRectF(0, y, fill_w, h), h / 2, h / 2)
                grad = QLinearGradient(QPointF(0, 0), QPointF(fill_w, 0))
                dim  = QColor(accent); dim.setAlpha(160)
                grad.setColorAt(0.0, dim)
                grad.setColorAt(1.0, accent)
                p.save()
                p.setClipPath(track)
                p.fillPath(fill, QBrush(grad))
                p.restore()
        p.end()


class _InstallWorker(QObject):
    download_progress = pyqtSignal(int, int)
    install_started   = pyqtSignal()
    finished          = pyqtSignal(list)
    error             = pyqtSignal(str)
    ini_warning       = pyqtSignal(str)

    def __init__(self, filename: str, url: str):
        super().__init__()
        self._filename  = filename
        self._url       = url
        self._cancelled = False

    def cancel(self): self._cancelled = True

    @staticmethod
    def _has_ini_only(folder: Path) -> bool:
        subdirs = [p for p in folder.rglob("*") if p.is_dir()]
        dirs_to_check = subdirs if subdirs else [folder]
        for d in dirs_to_check:
            files = [f for f in d.iterdir() if f.is_file()]
            if not files: continue
            suffixes = {f.suffix.lower() for f in files}
            if ".ini" in suffixes and ".pak" not in suffixes: return True
        return False

    def run(self):
        try: self._download_and_install()
        except Exception as exc:
            if not self._cancelled: self.error.emit(str(exc))

    def _download_and_install(self):
        import requests
        tmp_dir  = Path(tempfile.gettempdir()) / "nte_mod_install"
        tmp_dir.mkdir(parents=True, exist_ok=True)
        tmp_path = tmp_dir / self._filename
        try:
            resp = requests.get(self._url, stream=True, timeout=30)
            resp.raise_for_status()
        except requests.RequestException as exc:
            self.error.emit(f"Download failed: {exc}")
            return
        total = int(resp.headers.get("content-length", 0))
        done  = 0
        with open(tmp_path, "wb") as fh:
            for chunk in resp.iter_content(chunk_size=65_536):
                if self._cancelled: return
                if chunk:
                    fh.write(chunk)
                    done += len(chunk)
                    self.download_progress.emit(done, total)
        if self._cancelled: return
        mods_dir       = get_mods_path()
        seven_zip_path = get_seven_zip_path()
        installed: list[str] = []
        path   = tmp_path
        suffix = path.suffix.lower()
        try:
            if suffix in (".zip", ".rar", ".7z"):
                if not seven_zip_path:
                    self.error.emit("Extraction tool (7z or 7za) missing on system.")
                    return
                out_dir = mods_dir / path.stem
                cmd     = [str(seven_zip_path), "x", str(path), f"-o{out_dir}", "-y"]
                result = subprocess.run(
                    cmd,
                    capture_output=True,
                    text=True,
                    **hidden_subprocess_kwargs(),
                )
                if result.returncode == 0:
                    installed.append(path.stem)
                    try: os.remove(path)
                    except OSError: pass
                    if self._has_ini_only(out_dir):
                        self.ini_warning.emit(path.stem)
                        shutil.rmtree(out_dir, ignore_errors=True)
                        self.error.emit("")
                        return
                else:
                    self.error.emit(f"Extraction failed for {path.name}:\n{result.stderr.strip()}")
                    return
            elif path.is_dir():
                dest = mods_dir / path.name
                if dest.exists(): shutil.rmtree(dest)
                shutil.copytree(path, dest)
                installed.append(path.name)
            else:
                dest = mods_dir / path.name
                shutil.copy2(path, dest)
                installed.append(path.name)
        except Exception as exc:
            self.error.emit(f"Install error: {exc}")
            return
        if not self._cancelled:
            self.finished.emit(installed)


class InstallProgressWindow(QWidget):
    install_finished = pyqtSignal(list)
    cancelled        = pyqtSignal()

    def __init__(self, filename: str, url: str, parent=None, overlay_parent=None):
        super().__init__(
            parent,
            Qt.WindowType.FramelessWindowHint |
            Qt.WindowType.WindowStaysOnTopHint |
            Qt.WindowType.Tool,
        )
        self.setAttribute(Qt.WidgetAttribute.WA_TranslucentBackground)
        self.setFixedSize(_WIN_W, _WIN_H)
        self.setObjectName("InstallProgressWindow")
        self.setStyleSheet(_WINDOW_STYLE)
        self._filename : str                   = filename
        self._url      : str                   = url
        self._worker   : _InstallWorker | None = None
        self._thread   : QThread        | None = None
        self._old_pos  : QPoint         | None = None
        self._build_ui()
        geo = QApplication.primaryScreen().availableGeometry()
        self.move(geo.center().x() - _WIN_W // 2, geo.center().y() - _WIN_H // 2)
        self._overlay_parent = overlay_parent

    def _build_ui(self):
        root = QVBoxLayout(self)
        root.setContentsMargins(28, 24, 28, 20)
        root.setSpacing(0)

        top_row = QHBoxLayout(); top_row.setSpacing(10)
        lbl_icon = QLabel()
        _pix = QPixmap(resource_path("Bin/Assets/install_zip.png"))
        if not _pix.isNull():
            lbl_icon.setPixmap(_pix.scaled(20, 20, Qt.AspectRatioMode.KeepAspectRatio,
                                           Qt.TransformationMode.SmoothTransformation))
        lbl_icon.setFixedSize(20, 20)
        lbl_title = QLabel("Installing Mod")
        lbl_title.setStyleSheet(
            f"color: {_TEXT_PRI}; font-size: 15px; font-weight: 600;"
            " font-family: 'Segoe UI', system-ui, sans-serif;"
        )
        btn_close = QPushButton()
        btn_close.setIcon(QIcon(resource_path("Bin/Assets/close.png")))
        btn_close.setIconSize(QSize(24, 24))
        btn_close.setFixedSize(24, 24)
        btn_close.setCursor(Qt.CursorShape.PointingHandCursor)
        btn_close.setStyleSheet(
            "QPushButton { background: transparent; border: none; }"
            "QPushButton:hover { background-color: rgba(255,255,255,20); border-radius: 5px; }"
        )
        btn_close.clicked.connect(self._on_cancel)
        top_row.addWidget(lbl_icon); top_row.addWidget(lbl_title)
        top_row.addStretch(); top_row.addWidget(btn_close)
        root.addLayout(top_row); root.addSpacing(6)

        self._lbl_filename = QLabel(self._filename)
        self._lbl_filename.setStyleSheet(
            f"color: {_TEXT_MUTED}; font-size: 10px;"
            " font-family: 'Consolas', 'Cascadia Code', monospace;"
        )
        self._lbl_filename.setFixedHeight(14)
        root.addWidget(self._lbl_filename); root.addSpacing(16)

        self._lbl_status = QLabel("Preparing download…")
        self._lbl_status.setStyleSheet(
            f"color: {_TEXT_SEC}; font-size: 12px;"
            " font-family: 'Segoe UI', system-ui, sans-serif;"
        )
        self._lbl_status.setWordWrap(True)
        root.addWidget(self._lbl_status); root.addSpacing(12)

        self._bar = _SmartProgressBar(self)
        root.addWidget(self._bar); root.addSpacing(8)

        self._lbl_detail = QLabel("")
        self._lbl_detail.setStyleSheet(
            f"color: {_TEXT_MUTED}; font-size: 10px;"
            " font-family: 'Segoe UI', system-ui, sans-serif;"
        )
        self._lbl_detail.setFixedHeight(14)
        root.addWidget(self._lbl_detail); root.addStretch()

        divider = QFrame()
        divider.setFrameShape(QFrame.Shape.HLine)
        divider.setStyleSheet("background-color: rgba(255,255,255,6); border: none; max-height: 1px;")
        root.addWidget(divider); root.addSpacing(12)

        bottom_row = QHBoxLayout(); bottom_row.setSpacing(0)
        self._lbl_phase = QLabel("Downloading")
        self._lbl_phase.setStyleSheet(
            f"color: {_TEXT_MUTED}; font-size: 11px;"
            " font-family: 'Segoe UI', system-ui, sans-serif;"
        )
        self._btn_cancel = QPushButton("Cancel")
        self._btn_cancel.setObjectName("CancelBtn")
        self._btn_cancel.setCursor(Qt.CursorShape.PointingHandCursor)
        self._btn_cancel.clicked.connect(self._on_cancel)
        bottom_row.addWidget(self._lbl_phase); bottom_row.addStretch()
        bottom_row.addWidget(self._btn_cancel)
        root.addLayout(bottom_row)

    def start(self):
        self._worker = _InstallWorker(self._filename, self._url)
        self._thread = QThread()
        self._worker.moveToThread(self._thread)
        self._thread.started.connect(self._worker.run)
        self._worker.download_progress.connect(self._on_download_progress)
        self._worker.install_started.connect(self._on_install_started)
        self._worker.finished.connect(self._on_finished)
        self._worker.error.connect(self._on_error)
        self._worker.ini_warning.connect(self._on_ini_warning)
        self._worker.finished.connect(self._thread.quit)
        self._worker.error.connect(self._thread.quit)
        self._worker.finished.connect(self._worker.deleteLater)
        self._thread.finished.connect(self._thread.deleteLater)
        self._thread.start()

    def _on_download_progress(self, done: int, total: int):
        if total > 0:
            frac = done / total
            self._bar.set_progress(frac)
            self._lbl_status.setText(f"Downloading… {int(frac * 100)}%")
            self._lbl_detail.setText(
                f"{bytes_to_human_readable(done)}  /  {bytes_to_human_readable(total)}"
            )
        else:
            self._bar.start_indeterminate()
            self._lbl_status.setText("Downloading…")
            self._lbl_detail.setText(bytes_to_human_readable(done))

    def _on_install_started(self):
        self._bar.start_indeterminate()
        self._lbl_status.setText("Extracting and installing mod…")
        self._lbl_detail.setText("")
        self._lbl_phase.setText("Installing")

    def _on_finished(self, installed: list):
        self._bar.stop()
        self.install_finished.emit(installed)
        self.close()

    def _on_error(self, msg: str):
        self._bar.stop()
        if not msg: return
        self._lbl_status.setText(f"Error: {msg}")
        self._lbl_detail.setText("")
        self._lbl_phase.setText("Failed")
        self._btn_cancel.setText("Close")
        logger.error(f"Install PW error: {msg}")

    def _on_ini_warning(self, folder_name: str):
        self._bar.stop()
        target = self._overlay_parent
        if target:
            popup = PopupDialog(
                parent=target,
                title="Incompatible Mod",
                message=(
                    f"\"{folder_name}\" contains only INI files and is not "
                    "compatible with Aurora.\n\n"
                    "Aurora only supports PAK mods. INI mods require a "
                    "different tool and cannot be loaded by this launcher.\n\n"
                    "The mod has been removed automatically."
                ),
                confirm_text=t("confirm"),
                cancel_text="",
            )
            popup.raise_()
        self.close()

    def _on_cancel(self):
        if self._worker: self._worker.cancel()
        if self._thread and self._thread.isRunning():
            self._thread.quit()
        self._bar.stop()
        self.cancelled.emit()
        self.close()

    def mousePressEvent(self, event):
        if event.button() == Qt.MouseButton.LeftButton:
            self._old_pos = event.globalPosition().toPoint()

    def mouseMoveEvent(self, event):
        if self._old_pos is not None:
            delta         = event.globalPosition().toPoint() - self._old_pos
            self.move(self.x() + delta.x(), self.y() + delta.y())
            self._old_pos = event.globalPosition().toPoint()

    def mouseReleaseEvent(self, _event):
        self._old_pos = None

    def paintEvent(self, _event):
        p = QPainter(self)
        p.setRenderHint(QPainter.RenderHint.Antialiasing)
        p.setPen(Qt.PenStyle.NoPen)
        p.setBrush(QBrush(QColor(_BG)))
        p.drawRoundedRect(self.rect(), 16, 16)
        pen = QPen(QColor(_BORDER)); pen.setWidth(1)
        p.setPen(pen); p.setBrush(Qt.BrushStyle.NoBrush)
        p.drawRoundedRect(self.rect().adjusted(1, 1, -1, -1), 15, 15)
        glow = QLinearGradient(QPointF(0, 0), QPointF(0, 40))
        glow.setColorAt(0.0, QColor(200, 168, 255, 18))
        glow.setColorAt(1.0, QColor(200, 168, 255, 0))
        p.setPen(Qt.PenStyle.NoPen); p.setBrush(QBrush(glow))
        p.drawRoundedRect(self.rect(), 16, 16)
        p.end()