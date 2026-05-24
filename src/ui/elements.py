import webbrowser
import ctypes
from src.utils import resource_path
from pathlib import Path
from PyQt6.QtWidgets import (
    QWidget, QVBoxLayout, QHBoxLayout,
    QPushButton, QLabel, QFrame, QGraphicsOpacityEffect
)
from PyQt6.QtCore import Qt, QPropertyAnimation, QVariantAnimation, QEasingCurve, QTimer, QSize
from PyQt6.QtCore import Qt, QPropertyAnimation, QVariantAnimation, QEasingCurve, QTimer
from PyQt6.QtGui import QPixmap, QPainter, QColor, QIcon
from src.styles import POPUP_STYLE
from src.logger import logger
from src.translator import t
from src.path_finder import get_app_dir

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

def _get_mod_image(mod_folder_name: str, mod_display_name: str, mod_icon: str = "") -> QPixmap:
    if mod_icon:
        icon_filename = f"{mod_icon.lower()}.png"
        images_dir = Path(resource_path("Bin/Assets/ModImages"))

        for img_path in images_dir.iterdir():
            if img_path.name.lower() == icon_filename:
                return QPixmap(str(img_path))

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
        pixmap = _get_mod_image(
            self.mod.folder_name, 
            self.mod.display_name, 
            getattr(self.mod, 'icon', "")
        )
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

        # Delete button
        btn_delete = QPushButton()
        btn_delete.setObjectName("ModDeleteBtn")
        btn_delete.setFixedSize(30, 30)
        btn_delete.setIcon(QIcon(resource_path("Bin/Assets/delete.png")))
        btn_delete.setIconSize(QSize(16, 16))
        btn_delete.setCursor(Qt.CursorShape.PointingHandCursor)
        btn_delete.setToolTip("Delete mod")
        btn_delete.clicked.connect(self._confirm_delete)
        layout.addWidget(btn_delete)

        # Toggle
        self.toggle = AnimatedToggle(self)
        self.toggle.setChecked(mod.is_enabled)
        layout.addWidget(self.toggle)

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
        import shutil
        try:
            folder_name = self.mod.folder_name
            candidate = self.manager.mods_dir / folder_name
            if not candidate.exists():
                bare = Path(folder_name).name
                candidate = self.manager.mods_dir / bare
            if not candidate.exists():
                return
            if candidate.is_dir():
                shutil.rmtree(candidate)
            else:
                candidate.unlink()
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

    def handle_toggle(self):
        new_state = self.toggle.isChecked()

        new_folder_name = self.manager.toggle_mod(self.mod)
        if new_folder_name:
            self.mod.folder_name = new_folder_name
            self.mod.is_enabled = new_state

        self.parent_overlay._update_mod_count()

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

# OVERLAY WINDOW
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

        # Icon
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