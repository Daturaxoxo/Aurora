from functools import partial
from typing import Dict, List, Optional
import ctypes, json, webbrowser, shutil, os, hashlib, urllib.request
from src.backend.helpers.api import NTEMod, NTEModFile
from src.utils import bytes_to_human_readable, resource_path
from pathlib import Path
from PyQt6.QtWidgets import (
    QWidget, QVBoxLayout, QHBoxLayout,
    QPushButton, QLabel, QFrame, QGraphicsOpacityEffect, QLineEdit,
    QScrollArea, QGridLayout, QFileDialog,
)
from PyQt6.QtCore import Qt, QPropertyAnimation, QVariantAnimation, QEasingCurve, QTimer, QSize, QRectF, QThread, pyqtSignal
from PyQt6.QtGui import QPixmap, QPainter, QColor, QIcon, QPainterPath, QPen
from src.frontend.styles import POPUP_STYLE
from src.logger import logger
from src.translator import t

def custom_icons_dir() -> Path:
    d = Path(os.environ["APPDATA"]) / "Aurora" / "UserData" / "icons"
    d.mkdir(parents=True, exist_ok=True)
    return d

def icon_map_path() -> Path: return custom_icons_dir() / "icon_map.json"

def load_icon_map() -> dict:
    p = icon_map_path()
    if p.exists():
        try: return json.loads(p.read_text(encoding="utf-8"))
        except Exception: pass
    return {}

def _save_icon_map(mapping: dict): icon_map_path().write_text(json.dumps(mapping, ensure_ascii=False, indent=2), encoding="utf-8")

def _get_icon_cache_path(url: str) -> Path:
    cache_dir = Path(os.environ["APPDATA"]) / "Aurora" / "UserData" / "icon_cache"
    cache_dir.mkdir(parents=True, exist_ok=True)
    ext = url.split("?")[0].rsplit(".", 1)[-1].lower() or "png"
    filename = hashlib.md5(url.encode()).hexdigest() + f".{ext}"
    return cache_dir / filename

def _url_to_cached_pixmap(url: str) -> QPixmap:
    cached = _get_icon_cache_path(url)
    if not cached.exists():
        try:
            req = urllib.request.Request(
                url,
                headers={"User-Agent": "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 Chrome/124.0.0.0 Safari/537.36"}
            )
            with urllib.request.urlopen(req) as response:
                cached.write_bytes(response.read())
        except Exception as e:
            logger.warning(f"Failed to download custom icon URL: {e}")
            return QPixmap()
    return QPixmap(str(cached))

class _IconFetchThread(QThread):
    icon_ready = pyqtSignal(str, QPixmap)

    def __init__(self, mod_folder_name: str, url: str):
        super().__init__()
        self._mod_folder_name = mod_folder_name
        self._url = url

    def run(self):
        pix = _url_to_cached_pixmap(self._url)
        self.icon_ready.emit(self._mod_folder_name, pix)

def _get_mod_image(mod_folder_name: str, mod_display_name: str, mod_icon: str = "") -> QPixmap:
    if mod_icon and mod_icon.startswith(("https://", "http://")):
        cached = _get_icon_cache_path(mod_icon)
        if cached.exists():
            return QPixmap(str(cached))
        return QPixmap()

    icon_map = load_icon_map()
    if mod_folder_name in icon_map:
        entry = icon_map[mod_folder_name]
        if entry.startswith("builtin:"):
            builtin_path = Path(resource_path("Bin/Assets/ModImages")) / entry[len("builtin:"):]
            if builtin_path.exists(): return QPixmap(str(builtin_path))
        else:
            custom_path = custom_icons_dir() / entry
            if custom_path.exists(): return QPixmap(str(custom_path))

    if mod_icon:
        icon_filename = f"{mod_icon.lower()}.png"
        images_dir = Path(resource_path("Bin/Assets/ModImages"))
        for img_path in images_dir.iterdir():
            if img_path.name.lower() == icon_filename: return QPixmap(str(img_path))

    images_dir = Path(resource_path("Bin/Assets/ModImages"))
    if not images_dir.exists(): return QPixmap()

    images = sorted(
        p for p in images_dir.iterdir()
        if p.suffix.lower() in (".png", ".jpg", ".jpeg") and p.is_file()
    )
    if not images: return QPixmap()

    name_lower = mod_display_name.lower()
    best_match = None
    best_length = 0
    for img_path in images:
        character = img_path.stem.lower()
        if character in name_lower and len(character) > best_length:
            best_match = img_path
            best_length = len(character)

    if best_match: return QPixmap(str(best_match))

    idx = hash(mod_folder_name) % len(images)
    return QPixmap(str(images[idx]))

def _rounded_pixmap(pixmap: QPixmap, width: int, height: int, radius: int = 10) -> QPixmap:
    if pixmap.isNull(): return QPixmap()

    scaled = pixmap.scaled(width, height, Qt.AspectRatioMode.KeepAspectRatioByExpanding, Qt.TransformationMode.SmoothTransformation)

    crop_x = max(0, (scaled.width() - width) // 2)
    crop_y = max(0, (scaled.height() - height) // 2)
    cropped = scaled.copy(crop_x, crop_y, width, height)

    rounded = QPixmap(width, height)
    rounded.fill(Qt.GlobalColor.transparent)

    painter = QPainter(rounded)
    painter.setRenderHint(QPainter.RenderHint.Antialiasing)
    path = QPainterPath()
    path.moveTo(0, radius)
    path.quadTo(0, 0, radius, 0)
    path.lineTo(width - radius, 0)
    path.quadTo(width, 0, width, radius)
    path.lineTo(width, height)
    path.lineTo(0, height)
    path.closeSubpath()
    painter.setClipPath(path)
    painter.drawPixmap(0, 0, cropped)
    painter.end()

    return rounded

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
        target = self.parent()
        while target and not hasattr(target, "handle_toggle"): target = target.parent();

        if target: target.handle_toggle();

    def paintEvent(self, event):
        painter = QPainter(self)
        painter.setRenderHint(QPainter.RenderHint.Antialiasing)

        color = self._active_color if self._checked else self._inactive_color
        painter.setBrush(color)
        painter.setPen(Qt.PenStyle.NoPen)
        painter.drawRoundedRect(0, 0, self.width(), self.height(), 13, 13)

        painter.setBrush(self._handle_color)
        painter.drawEllipse(self._handle_position, 3, 20, 20)

class ModImage(QLabel):
    RADIUS = 6

    def __init__(self, pixmap: QPixmap, size: int, mod_folder_name: str = "", parent=None):
        super().__init__(parent)
        self.setObjectName("ModImage")
        self.setFixedSize(size, size)
        self._source = pixmap
        self._mod_folder_name = mod_folder_name
        self._hovered = False

        self._btn = QPushButton(self)
        self._btn.setFixedSize(size, size)
        self._btn.setIcon(QIcon(resource_path("Bin/Assets/rename.png")))
        self._btn.setIconSize(QSize(16, 16))
        self._btn.setCursor(Qt.CursorShape.PointingHandCursor)
        self._btn.setToolTip(t("change_icon_tooltip"))
        self._btn.setStyleSheet("QPushButton { background: transparent; border: none; }")
        self._btn.move(0, 0)
        self._btn.hide()
        self._btn.clicked.connect(self._open_icon_picker)

        self.setMouseTracking(True)

    def set_pixmap_source(self, pixmap: QPixmap):
        self._source = pixmap
        self.update()

    def enterEvent(self, event):
        self._hovered = True
        self._btn.show()
        self._btn.raise_()
        self.update()
        super().enterEvent(event)

    def leaveEvent(self, event):
        self._hovered = False
        self._btn.hide()
        self.update()
        super().leaveEvent(event)

    def _open_icon_picker(self):
        overlay = self.parent()
        while overlay is not None and not hasattr(overlay, "refresh_list"):
            overlay = overlay.parent()
        if overlay is None:
            return

        card = self.parent()
        while card is not None and card.objectName() != "ModCard":
            card = card.parent()

        IconPickerDialog(
            parent=overlay,
            mod_folder_name=self._mod_folder_name,
            on_confirm=lambda path: self._apply_icon(path, card),
        )

    def _apply_icon(self, icon_path: Path | None, card):
        if icon_path is None:
            mapping = load_icon_map()
            mapping.pop(self._mod_folder_name, None)
            _save_icon_map(mapping)
        else:
            mapping = load_icon_map()
            name = str(icon_path)
            if not name.startswith("builtin:"):
                name = icon_path.name
            mapping[self._mod_folder_name] = name
            _save_icon_map(mapping)

        new_pixmap = _get_mod_image(
            self._mod_folder_name,
            card.mod.display_name if card else "",
            getattr(card.mod, "icon", "") if card else "",
        )
        self.set_pixmap_source(new_pixmap)

    def paintEvent(self, event):
        painter = QPainter(self)
        painter.setRenderHint(QPainter.RenderHint.Antialiasing)

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
            painter.fillRect(self.rect(), QColor(40, 40, 50))

        if self._hovered:
            painter.fillRect(self.rect(), QColor(0, 0, 0, 120))

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
        self._icon_thread = None

        layout = QHBoxLayout(self)
        layout.setContentsMargins(14, 0, 20, 0)
        layout.setSpacing(14)

        # Mod Thumbnail
        mod_icon = getattr(self.mod, 'icon', "")
        pixmap = _get_mod_image(
            self.mod.folder_name,
            self.mod.display_name,
            mod_icon,
        )
        self.thumb = ModImage(pixmap, 44, mod_folder_name=self.mod.folder_name)
        layout.addWidget(self.thumb)

        if mod_icon and mod_icon.startswith(("https://", "http://")):
            cached = _get_icon_cache_path(mod_icon)
            if not cached.exists():
                self._icon_thread = _IconFetchThread(self.mod.folder_name, mod_icon)
                self._icon_thread.icon_ready.connect(self._on_icon_ready)
                self._icon_thread.start()

        # Mod Info
        info_vbox = QVBoxLayout()
        info_vbox.setSpacing(3)

        title_row = QHBoxLayout()
        title_row.setSpacing(6)
        title_row.setContentsMargins(0, 0, 0, 0)

        self.title = QLabel(mod.display_name)
        self.title.setObjectName("ModTitle")

        btn_rename = QPushButton()
        btn_rename.setObjectName("ModRenameBtn")
        btn_rename.setFixedSize(20, 20)
        btn_rename.setIcon(QIcon(resource_path("Bin/Assets/rename.png")))
        btn_rename.setIconSize(QSize(13, 13))
        btn_rename.setCursor(Qt.CursorShape.PointingHandCursor)
        btn_rename.setToolTip(t("rename_mod_tooltip"))
        btn_rename.clicked.connect(self._open_rename_dialog)

        title_row.addWidget(self.title)
        title_row.addWidget(btn_rename)
        title_row.addStretch()

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
        info_vbox.addLayout(title_row)
        info_vbox.addLayout(meta_row)
        info_vbox.addStretch()

        layout.addLayout(info_vbox)
        layout.addStretch()

        # Delete Button
        btn_delete = QPushButton()
        btn_delete.setObjectName("ModDeleteBtn")
        btn_delete.setFixedSize(30, 30)
        btn_delete.setIcon(QIcon(resource_path("Bin/Assets/delete.png")))
        btn_delete.setIconSize(QSize(16, 16))
        btn_delete.setCursor(Qt.CursorShape.PointingHandCursor)
        btn_delete.setToolTip(t("delete_mod_tooltip"))
        btn_delete.clicked.connect(self._confirm_delete)
        layout.addWidget(btn_delete)

        # Enable Toggle
        self.toggle = AnimatedToggle(self)
        self.toggle.setChecked(mod.is_enabled)
        layout.addWidget(self.toggle)

    def _on_icon_ready(self, folder_name: str, pixmap: QPixmap):
        if folder_name == self.mod.folder_name and not pixmap.isNull(): self.thumb.set_pixmap_source(pixmap)

    def _confirm_delete(self):
        message = t("mod_delete_message")
        PopupDialog(
            parent=self.parent_overlay,
            title=t("mod_delete_title"),
            message=f"{message}\n- {self.mod.display_name}",
            confirm_text=t("confirm"),
            cancel_text=t("cancel"),
            on_confirm=self._delete_mod,
        )

    def _delete_mod(self):
        try:
            candidate = (
                self.mod.folder_path
                if getattr(self.mod, "folder_path", None)
                else self.manager.mods_dir / self.mod.folder_name
            )
            if not candidate or not candidate.exists():
                logger.warning(f"Delete target not found for mod '{self.mod.display_name}'")
                return
            shutil.rmtree(candidate) if candidate.is_dir() else candidate.unlink()
            logger.info(f"Deleted mod: {candidate}", extra={"el": True})
            parent = candidate.parent
            if parent != self.manager.mods_dir and parent.exists() and not any(parent.iterdir()):
                parent.rmdir()
                logger.info(f"Removed empty group folder: {parent.name}", extra={"el": True})
            self.parent_overlay.refresh_list()
        except Exception as e:
            logger.error(f"Failed to delete mod '{self.mod.display_name}': {e}")

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

    def _open_rename_dialog(self):
        RenameDialog(
            parent=self.parent_overlay,
            current_name=self.mod.display_name,
            on_confirm=self._apply_rename,
        )

    def _apply_rename(self, new_name: str):
        new_name = new_name.strip()
        if not new_name or new_name == self.mod.display_name:
            return
        try:
            candidate = (
                self.mod.folder_path
                if getattr(self.mod, "folder_path", None)
                else self.manager.mods_dir / self.mod.folder_name
            )
            if not candidate or not candidate.exists():
                logger.warning(f"Rename target not found for mod '{self.mod.display_name}'")
                return

            new_folder = candidate.parent / new_name
            candidate.rename(new_folder)
            logger.info(f"Renamed mod folder: '{candidate.name}' → '{new_name}'", extra={"el": True})

            self.mod.folder_name = new_name
            self.mod.display_name = new_name
            if hasattr(self.mod, "folder_path"):
                self.mod.folder_path = new_folder

            self.title.setText(new_name)
        except Exception as e:
            logger.error(f"Failed to rename mod '{self.mod.display_name}': {e}")

    def handle_toggle(self):
        new_state = self.toggle.isChecked()
        success = self.manager.toggle_mod(self.mod)
        if success:
            self.mod.is_enabled = new_state
        self.parent_overlay._update_mod_count()

class _IconCell(QFrame):
    CELL_SIZE = 72

    def __init__(self, pixmap: QPixmap | None, label: str,
                 is_custom: bool, is_add: bool,
                 on_select=None, on_delete=None, parent=None):
        super().__init__(parent)
        self.setFixedSize(self.CELL_SIZE, self.CELL_SIZE)
        self.setAttribute(Qt.WidgetAttribute.WA_StyledBackground)
        self.setCursor(Qt.CursorShape.PointingHandCursor)
        self.setObjectName("IconCell")
        self.setStyleSheet("""
            QFrame#IconCell {
                border: 2px solid #3d444d;
                border-radius: 8px;
                background: #161b22;
            }
            QFrame#IconCell:hover {
                border-color: #4493f8;
                background: #1c2333;
            }
        """)

        self._on_select = on_select

        if pixmap and not pixmap.isNull():
            img_lbl = QLabel(self)
            img_lbl.setFixedSize(self.CELL_SIZE - 4, self.CELL_SIZE - 4)
            img_lbl.move(2, 2)
            scaled = pixmap.scaled(
                img_lbl.size(),
                Qt.AspectRatioMode.KeepAspectRatioByExpanding,
                Qt.TransformationMode.SmoothTransformation,
            )
            img_lbl.setPixmap(scaled)
            img_lbl.setScaledContents(False)
            img_lbl.setAttribute(Qt.WidgetAttribute.WA_TransparentForMouseEvents)
        elif is_add:
            add_lbl = QLabel(self)
            add_lbl.setFixedSize(self.CELL_SIZE, self.CELL_SIZE)
            add_lbl.move(0, 0)
            add_lbl.setAlignment(Qt.AlignmentFlag.AlignCenter)
            add_icon = QIcon(resource_path("Bin/Assets/add.png")).pixmap(28, 28)
            if not add_icon.isNull():
                add_lbl.setPixmap(add_icon)
            else:
                add_lbl.setText("+")
                add_lbl.setStyleSheet("color:#848d97; font-size:24px;")
            add_lbl.setAttribute(Qt.WidgetAttribute.WA_TransparentForMouseEvents)

        # Delete badge for custom icons
        if is_custom and on_delete:
            del_btn = QPushButton("✕", self)
            del_btn.setFixedSize(18, 18)
            del_btn.move(self.CELL_SIZE - 20, 2)
            del_btn.setStyleSheet("""
                QPushButton {
                    background: #3d444d;
                    color: #e6edf3;
                    border: none;
                    border-radius: 9px;
                    font-size: 10px;
                }
                QPushButton:hover { background: #c93c37; }
            """)
            del_btn.setCursor(Qt.CursorShape.PointingHandCursor)
            del_btn.clicked.connect(lambda _checked, cb=on_delete: cb())
            del_btn.raise_()

        if label:
            self.setToolTip(label)

    def mousePressEvent(self, event):
        if event.button() == Qt.MouseButton.LeftButton and self._on_select:
            self._on_select()
        super().mousePressEvent(event)

class IconPickerDialog(QWidget):
    COLS = 6

    def __init__(self, parent, mod_folder_name: str, on_confirm=None):
        super().__init__(parent)
        self._mod_folder_name = mod_folder_name
        self._on_confirm = on_confirm

        self.setObjectName("DimOverlay")
        self.setFixedSize(parent.size())
        self.move(0, 0)
        self.setAttribute(Qt.WidgetAttribute.WA_StyledBackground)
        self.setStyleSheet(POPUP_STYLE)

        # Card (width = 6 cols × 72px + 5 gaps × 8px + 2 × 28px padding + 12px scrollbar clearance)
        self._card = QFrame(self)
        self._card.setObjectName("PopupContainer")
        self._card.setFixedWidth(548)
        self._card.setAttribute(Qt.WidgetAttribute.WA_StyledBackground)
        self._card.setStyleSheet(POPUP_STYLE)

        self._card_layout = QVBoxLayout(self._card)
        self._card_layout.setContentsMargins(28, 24, 28, 24)
        self._card_layout.setSpacing(14)

        lbl_title = QLabel(t("choose_icon_title"))
        lbl_title.setObjectName("PopupTitle")
        self._card_layout.addWidget(lbl_title)

        lbl_desc = QLabel("Custom icons are shown first. Click an icon to assign it.")
        lbl_desc.setObjectName("PopupMessage")
        lbl_desc.setWordWrap(True)
        self._card_layout.addWidget(lbl_desc)

        # Icon Grid
        self._scroll = QScrollArea()
        self._scroll.setWidgetResizable(True)
        self._scroll.setFixedHeight(300)
        self._scroll.setHorizontalScrollBarPolicy(Qt.ScrollBarPolicy.ScrollBarAlwaysOff)
        self._scroll.setStyleSheet("""
            QScrollArea { border: none; background: transparent; }
            QScrollBar:vertical {
                background: #161b22; width: 6px; border-radius: 3px;
            }
            QScrollBar::handle:vertical {
                background: #3d444d; border-radius: 3px; min-height: 20px;
            }
        """)

        self._grid_widget = QWidget()
        self._grid_widget.setStyleSheet("background: transparent;")
        self._grid = QGridLayout(self._grid_widget)
        self._grid.setSpacing(8)
        self._grid.setContentsMargins(0, 0, 0, 0)
        self._scroll.setWidget(self._grid_widget)
        self._card_layout.addWidget(self._scroll)

        # Cancel Button
        btn_row = QHBoxLayout()
        btn_row.addStretch()
        btn_cancel = QPushButton(t("cancel") or "Cancel")
        btn_cancel.setObjectName("PopupCancelButton")
        btn_cancel.setFixedHeight(36)
        btn_cancel.setCursor(Qt.CursorShape.PointingHandCursor)
        btn_cancel.clicked.connect(self._close)
        btn_row.addWidget(btn_cancel)
        self._card_layout.addLayout(btn_row)

        self._populate_grid()

        self._card.adjustSize()
        self._card.move(
            (self.width()  - self._card.width())  // 2,
            (self.height() - self._card.height()) // 2,
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

    def _populate_grid(self):
        while self._grid.count():
            item = self._grid.takeAt(0)
            w = item.widget()
            if w:
                w.deleteLater()

        col = 0
        row = 0

        def _next_pos():
            nonlocal row, col
            r, c = row, col
            col += 1
            if col >= self.COLS:
                col = 0
                row += 1
            return r, c

        add_cell = _IconCell(
            pixmap=None, label=t("custom_icon_tooltip"),
            is_custom=False, is_add=True,
            on_select=self._import_custom_icon,
        )
        r, c = _next_pos()
        self._grid.addWidget(add_cell, r, c)

        # Custom Icons
        custom_dir = custom_icons_dir()
        custom_images = sorted(
            p for p in custom_dir.iterdir()
            if p.suffix.lower() in (".png", ".jpg", ".jpeg", ".webp") and p.is_file()
        )
        for img_path in custom_images:
            path_capture = img_path
            cell = _IconCell(
                pixmap=QPixmap(str(img_path)),
                label=img_path.stem,
                is_custom=True,
                is_add=False,
                on_select=lambda p=path_capture: self._select(p),
                on_delete=lambda p=path_capture: self._delete_custom(p),
            )
            r, c = _next_pos()
            self._grid.addWidget(cell, r, c)

        # Built-in Icons
        builtin_dir = Path(resource_path("Bin/Assets/ModImages"))
        if builtin_dir.exists():
            builtin_images = sorted(
                p for p in builtin_dir.iterdir()
                if p.suffix.lower() in (".png", ".jpg", ".jpeg") and p.is_file()
            )
            for img_path in builtin_images:
                path_capture = img_path
                cell = _IconCell(
                    pixmap=QPixmap(str(img_path)),
                    label=img_path.stem,
                    is_custom=False,
                    is_add=False,
                    on_select=lambda p=path_capture: self._select_builtin(p),
                )
                r, c = _next_pos()
                self._grid.addWidget(cell, r, c)

    def _import_custom_icon(self):
        file_path, _ = QFileDialog.getOpenFileName(
            self, "Select icon image", "", "Image files (*.png *.jpg *.jpeg *.webp)"
        )
        if not file_path:
            return
        src = Path(file_path)
        dest = custom_icons_dir() / src.name
        counter = 2
        while dest.exists():
            dest = custom_icons_dir() / f"{src.stem}_{counter}{src.suffix}"
            counter += 1
        try:
            shutil.copy2(src, dest)
        except Exception as e:
            logger.error(f"Failed to copy custom icon: {e}")
            return
        self._select(dest)

    def _select(self, path: Path):
        if self._on_confirm:
            self._on_confirm(path)
        self._close()

    def _select_builtin(self, path: Path):
        if self._on_confirm:
            self._on_confirm(Path("builtin:" + path.name))
        self._close()

    def _delete_custom(self, path: Path):
        try:
            mapping = load_icon_map()
            for key, val in list(mapping.items()):
                if val == path.name:
                    del mapping[key]
            _save_icon_map(mapping)
            path.unlink(missing_ok=True)
        except Exception as e:
            logger.error(f"Failed to delete custom icon: {e}")
        self._populate_grid()

    def _close(self):
        self.anim.setDirection(QPropertyAnimation.Direction.Backward)
        self.anim.finished.connect(self.deleteLater)
        self.anim.start()

class RenameDialog(QWidget):
    def __init__(self, parent, current_name: str, on_confirm=None):
        super().__init__(parent)
        self.on_confirm = on_confirm

        self.setObjectName("DimOverlay")
        self.setFixedSize(parent.size())
        self.move(0, 0)
        self.setAttribute(Qt.WidgetAttribute.WA_StyledBackground)
        self.setStyleSheet(POPUP_STYLE)

        card = QFrame(self)
        card.setObjectName("PopupContainer")
        card.setFixedWidth(460)
        card.setAttribute(Qt.WidgetAttribute.WA_StyledBackground)
        card.setStyleSheet(POPUP_STYLE)

        card_layout = QVBoxLayout(card)
        card_layout.setContentsMargins(32, 28, 32, 28)
        card_layout.setSpacing(12)

        lbl_title = QLabel(t("rename_mod_title"))
        lbl_title.setObjectName("PopupTitle")

        lbl_desc = QLabel(t("rename_mod_desc"))
        lbl_desc.setObjectName("PopupMessage")
        lbl_desc.setWordWrap(True)

        self._input = QLineEdit()
        self._input.setText(current_name)
        self._input.selectAll()
        self._input.setStyleSheet("""
            QLineEdit {
                background-color: #1a1a1a;
                color: #D7D7D7;
                border: 1px solid #333333;
                border-radius: 8px;
                font-size: 13px;
                padding: 8px 12px;
            }
            QLineEdit:focus {
                border-color: #555555;
            }
        """)
        self._input.returnPressed.connect(self._handle_confirm)

        btn_row = QHBoxLayout()
        btn_row.setSpacing(12)
        btn_row.addStretch()

        btn_cancel = QPushButton(t("cancel"))
        btn_cancel.setObjectName("PopupCancelButton")
        btn_cancel.setFixedHeight(36)
        btn_cancel.setCursor(Qt.CursorShape.PointingHandCursor)
        btn_cancel.clicked.connect(self._close)

        btn_confirm = QPushButton(t("rename_button"))
        btn_confirm.setObjectName("PopupConfirmButton")
        btn_confirm.setFixedHeight(36)
        btn_confirm.setCursor(Qt.CursorShape.PointingHandCursor)
        btn_confirm.clicked.connect(self._handle_confirm)

        btn_row.addWidget(btn_cancel)
        btn_row.addWidget(btn_confirm)

        card_layout.addWidget(lbl_title)
        card_layout.addWidget(lbl_desc)
        card_layout.addWidget(self._input)
        card_layout.addStretch()
        card_layout.addLayout(btn_row)

        card.adjustSize()
        card.move(
            (self.width()  - card.width())  // 2,
            (self.height() - card.height()) // 2,
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
        self._input.setFocus()

    def _handle_confirm(self):
        if self.on_confirm:
            self.on_confirm(self._input.text())
        self._close()

    def _close(self):
        self.anim.setDirection(QPropertyAnimation.Direction.Backward)
        self.anim.finished.connect(self.deleteLater)
        self.anim.start()

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

class AuroraOverlayWindow(QWidget):
    DISPLAY_MS = 6000
    FADE_MS    = 1000

    def __init__(self, title="Aurora Mod Loader", subtitle="Mods are active"):
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

        icon_lbl = QLabel()
        icon_pix = QIcon(resource_path("Bin/Assets/logo1024_wn.png")).pixmap(30, 30)
        icon_lbl.setPixmap(icon_pix)
        icon_lbl.setFixedSize(30, 30)

        text_col = QVBoxLayout()
        text_col.setSpacing(1)
        text_col.addStretch()

        lbl_title = QLabel(title)
        lbl_title.setStyleSheet("""
            color: #E0E0E0;
            font-size: 14px;
            font-weight: 600;
            font-family: 'Segoe UI', system-ui, sans-serif;
        """)

        lbl_sub = QLabel(subtitle)
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

        self.opacity_effect = QGraphicsOpacityEffect(self)
        self.setGraphicsEffect(self.opacity_effect)
        self.opacity_effect.setOpacity(0)

    def paintEvent(self, event):
        painter = QPainter(self)
        painter.setRenderHint(QPainter.RenderHint.Antialiasing)
        painter.setBrush(QColor(10, 8, 18, 210))
        painter.setPen(QColor(60, 60, 80, 200))
        painter.drawRoundedRect(self.rect().adjusted(1, 1, -1, -1), 12, 12)
        painter.end()

    def show_over_game(self, game_rect=None):
        if game_rect is not None:
            x, y = game_rect.left, game_rect.top
        else:
            x, y = self._find_game_position()
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

        total_ms = self.DISPLAY_MS + self.FADE_MS + 1000
        QTimer.singleShot(total_ms, self._force_close)

    def _fade_out(self):
        self._anim = QPropertyAnimation(self.opacity_effect, b"opacity")
        self._anim.setDuration(self.FADE_MS)
        self._anim.setStartValue(1.0)
        self._anim.setEndValue(0.0)
        self._anim.finished.connect(self.hide)
        self._anim.finished.connect(self.deleteLater)
        self._anim.start()

    def _force_close(self):
        if not self.isHidden():
            self.hide()
            self.deleteLater()

class LoadingSpinner(QWidget):
    def __init__(self, size: int = 32, color: QColor = None, parent=None):
        super().__init__(parent)
        self._size = size
        self._color = color or QColor("#4493f8")
        self._angle = 0
        self.setFixedSize(size, size)
        self._timer = QTimer(self)
        self._timer.timeout.connect(self._tick)

    def start(self):
        self._timer.start(16)

    def stop(self):
        self._timer.stop()

    def _tick(self):
        self._angle = (self._angle + 8) % 360
        self.update()

    def paintEvent(self, event):
        painter = QPainter(self)
        painter.setRenderHint(QPainter.RenderHint.Antialiasing)
        painter.translate(self._size / 2, self._size / 2)
        painter.rotate(self._angle)

        track_pen = QPen(QColor(255, 255, 255, 30))
        track_pen.setWidth(3)
        track_pen.setCapStyle(Qt.PenCapStyle.RoundCap)
        painter.setPen(track_pen)
        r = self._size / 2 - 4
        painter.drawEllipse(QRectF(-r, -r, r * 2, r * 2))

        arc_pen = QPen(self._color)
        arc_pen.setWidth(3)
        arc_pen.setCapStyle(Qt.PenCapStyle.RoundCap)
        painter.setPen(arc_pen)
        painter.drawArc(QRectF(-r, -r, r * 2, r * 2), 0, 100 * 16)
        painter.end()

class _ImageViewerOverlay(QWidget):
    def __init__(self, parent: QWidget, pixmap: QPixmap):
        super().__init__(parent)
        self.setObjectName("DimOverlay")
        self.setFixedSize(parent.size())
        self.move(0, 0)
        self.setAttribute(Qt.WidgetAttribute.WA_StyledBackground)
        self.setStyleSheet("""
            QWidget#DimOverlay {
                background: rgba(0, 0, 0, 180);
            }
        """)

        layout = QVBoxLayout(self)
        layout.setAlignment(Qt.AlignmentFlag.AlignCenter)
        layout.setContentsMargins(30, 30, 30, 30)

        img = QLabel()
        img.setAlignment(Qt.AlignmentFlag.AlignCenter)
        img.setCursor(Qt.CursorShape.PointingHandCursor)

        max_w = min(pixmap.width(), int(parent.width() * 0.88))
        max_h = min(pixmap.height(), int(parent.height() * 0.88))
        scaled = pixmap.scaled(
            max_w, max_h,
            Qt.AspectRatioMode.KeepAspectRatio,
            Qt.TransformationMode.SmoothTransformation,
        )
        img.setPixmap(scaled)

        layout.addWidget(img)

        def _dismiss(e):
            self.close()
        img.mousePressEvent = _dismiss
        self.mousePressEvent = _dismiss


def show_image(pixmap: QPixmap, parent_widget: QWidget):
    if pixmap.isNull():
        return
    overlay = _ImageViewerOverlay(parent_widget.window(), pixmap)
    overlay.show()
    overlay.raise_()