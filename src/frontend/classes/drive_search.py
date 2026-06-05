import time

from PyQt6.QtWidgets import (
    QWidget, QVBoxLayout, QHBoxLayout, QLabel, QPushButton, QFrame,
)
from PyQt6.QtCore import (
    Qt, QThread, QTimer, QPointF, QRectF, pyqtSignal, QPoint, QSize,
)
from PyQt6.QtGui import (
    QColor, QPainter, QPen, QBrush, QLinearGradient, QIcon,
    QPainterPath, QPixmap,
)

from src.path_finder import get_game_directory
from src.utils import resource_path

_BG          = "#101010"
_BORDER      = "#282828"
_TEXT_PRI    = "#D7D7D7"
_TEXT_SEC    = "#707070"
_TEXT_MUTED  = "#484848"
_ACCENT_FULL = "#C8A8FF"
_ACCENT_DIM  = "rgba(180, 140, 255, 120)"

_WIN_W = 400
_WIN_H = 220

_WINDOW_STYLE = f"""
QWidget#DriveSearchWindow {{
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

class _SearchThread(QThread):
    finished = pyqtSignal(str)

    def run(self):
        result = get_game_directory()
        self.finished.emit(result or "")

class _IndeterminateBar(QWidget):
    _TRACK_H   = 3
    _GLOW_FRAC = 0.38
    _SPEED     = 0.0022

    def __init__(self, parent=None):
        super().__init__(parent)
        self.setFixedHeight(self._TRACK_H + 2)
        self._pos = 0.0
        self._last_tick = time.monotonic()

        self._timer = QTimer(self)
        self._timer.setInterval(16)
        self._timer.timeout.connect(self._tick)
        self._timer.start()

    def _tick(self):
        now  = time.monotonic()
        dt   = (now - self._last_tick) * 1000
        self._last_tick = now
        self._pos = (self._pos + self._SPEED * dt) % 1.0
        self.update()

    def paintEvent(self, _event):
        p = QPainter(self)
        p.setRenderHint(QPainter.RenderHint.Antialiasing)

        w, h = self.width(), self._TRACK_H
        y    = (self.height() - h) // 2

        # Track
        track_color = QColor(40, 40, 40)
        p.setPen(Qt.PenStyle.NoPen)
        path = QPainterPath()
        path.addRoundedRect(QRectF(0, y, w, h), h / 2, h / 2)
        p.fillPath(path, QBrush(track_color))

        frac   = self._GLOW_FRAC
        center = self._pos
        x0     = (center - frac / 2) * w
        x1     = (center + frac / 2) * w

        grad = QLinearGradient(QPointF(x0, 0), QPointF(x1, 0))
        accent = QColor(_ACCENT_FULL)
        transparent = QColor(accent)
        transparent.setAlpha(0)
        dim = QColor(accent)
        dim.setAlpha(90)

        grad.setColorAt(0.0,  transparent)
        grad.setColorAt(0.25, dim)
        grad.setColorAt(0.5,  accent)
        grad.setColorAt(0.75, dim)
        grad.setColorAt(1.0,  transparent)

        p.save()
        p.setClipPath(path)
        p.fillRect(int(x0), y, int(x1 - x0) + 1, h, QBrush(grad))
        p.restore()

        p.end()

    def stop(self):
        self._timer.stop()

class DriveSearchWindow(QWidget):
    search_finished = pyqtSignal(str)
    cancelled       = pyqtSignal()

    def __init__(self, parent=None):
        super().__init__(parent, Qt.WindowType.FramelessWindowHint |
                                 Qt.WindowType.WindowStaysOnTopHint |
                                 Qt.WindowType.Tool)
        self.setAttribute(Qt.WidgetAttribute.WA_TranslucentBackground)
        self.setFixedSize(_WIN_W, _WIN_H)
        self.setObjectName("DriveSearchWindow")
        self.setStyleSheet(_WINDOW_STYLE)

        self._thread      : _SearchThread | None = None
        self._elapsed_ms  = 0
        self._old_pos     : QPoint | None = None

        self._build_ui()
        self._elapsed_timer = QTimer(self)
        self._elapsed_timer.setInterval(500)
        self._elapsed_timer.timeout.connect(self._update_elapsed)

        from PyQt6.QtWidgets import QApplication
        screen_geo = QApplication.primaryScreen().availableGeometry()
        self.move(
            screen_geo.center().x() - _WIN_W // 2,
            screen_geo.center().y() - _WIN_H // 2,
        )

    def _build_ui(self):
        root = QVBoxLayout(self)
        root.setContentsMargins(28, 24, 28, 20)
        root.setSpacing(0)

        top_row = QHBoxLayout()
        top_row.setSpacing(10)

        self._lbl_icon = QLabel()
        _search_pix = QPixmap(resource_path("Bin/Assets/search.png"))
        if not _search_pix.isNull():
            self._lbl_icon.setPixmap(
                _search_pix.scaled(20, 20,
                    Qt.AspectRatioMode.KeepAspectRatio,
                    Qt.TransformationMode.SmoothTransformation)
            )
        self._lbl_icon.setFixedSize(20, 20)

        lbl_title = QLabel("Searching Drives")
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

        top_row.addWidget(self._lbl_icon)
        top_row.addWidget(lbl_title)
        top_row.addStretch()
        top_row.addWidget(btn_close)

        root.addLayout(top_row)
        root.addSpacing(18)

        self._lbl_status = QLabel("Scanning available drives for NTE...")
        self._lbl_status.setStyleSheet(
            f"color: {_TEXT_SEC}; font-size: 12px;"
            " font-family: 'Segoe UI', system-ui, sans-serif;"
        )
        self._lbl_status.setWordWrap(True)
        root.addWidget(self._lbl_status)
        root.addSpacing(14)

        self._bar = _IndeterminateBar(self)
        root.addWidget(self._bar)
        root.addSpacing(10)

        self._lbl_path = QLabel("")
        self._lbl_path.setStyleSheet(
            f"color: {_TEXT_MUTED}; font-size: 10px;"
            " font-family: 'Consolas', 'Cascadia Code', monospace;"
        )
        self._lbl_path.setFixedHeight(14)
        root.addWidget(self._lbl_path)

        root.addStretch()

        divider = QFrame()
        divider.setFrameShape(QFrame.Shape.HLine)
        divider.setStyleSheet(
            "background-color: rgba(255,255,255,6); border: none; max-height: 1px;"
        )
        root.addWidget(divider)
        root.addSpacing(12)
        bottom_row = QHBoxLayout()
        bottom_row.setSpacing(0)

        self._lbl_elapsed = QLabel("Elapsed: 0s")
        self._lbl_elapsed.setStyleSheet(
            f"color: {_TEXT_MUTED}; font-size: 11px;"
            " font-family: 'Segoe UI', system-ui, sans-serif;"
        )

        self._btn_cancel = QPushButton("Cancel")
        self._btn_cancel.setObjectName("CancelBtn")
        self._btn_cancel.setCursor(Qt.CursorShape.PointingHandCursor)
        self._btn_cancel.clicked.connect(self._on_cancel)

        bottom_row.addWidget(self._lbl_elapsed)
        bottom_row.addStretch()
        bottom_row.addWidget(self._btn_cancel)

        root.addLayout(bottom_row)

    def start(self):
        self._elapsed_ms = 0
        self._elapsed_timer.start()

        self._thread = _SearchThread()
        self._thread.finished.connect(self._on_thread_finished)
        self._thread.start()

    # Internals
    def _update_elapsed(self):
        self._elapsed_ms += 500
        total_s = self._elapsed_ms // 1000

        if total_s < 60:
            self._lbl_elapsed.setText(f"Elapsed: {total_s}s")
        else:
            m, s = divmod(total_s, 60)
            self._lbl_elapsed.setText(f"Elapsed: {m}m {s:02d}s")

        _HINTS = [
            "Scanning drives for NTE",
            "The all seeing eye is finding NTE",
            "Searching the one piece for NTE",
            "Using a XRay texture pack to find NTE easier",
            "This might take a bit on larger drives",
        ]
        hint_idx = (total_s // 4) % len(_HINTS)
        self._lbl_status.setText(_HINTS[hint_idx])

    def _on_thread_finished(self, found_path: str):
        self._stop_timers()
        self.search_finished.emit(found_path)

    def _on_cancel(self):
        self._stop_timers()
        if self._thread and self._thread.isRunning():
            self._thread.requestInterruption()
        self.cancelled.emit()
        self.close()

    def _stop_timers(self):
        self._elapsed_timer.stop()
        self._bar.stop()

    def mousePressEvent(self, event):
        if event.button() == Qt.MouseButton.LeftButton:
            self._old_pos = event.globalPosition().toPoint()

    def mouseMoveEvent(self, event):
        if self._old_pos is not None:
            delta = event.globalPosition().toPoint() - self._old_pos
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
        pen = QPen(QColor(_BORDER))
        pen.setWidth(1)
        p.setPen(pen)
        p.setBrush(Qt.BrushStyle.NoBrush)
        p.drawRoundedRect(
            self.rect().adjusted(1, 1, -1, -1), 15, 15
        )
        glow = QLinearGradient(QPointF(0, 0), QPointF(0, 40))
        glow.setColorAt(0.0, QColor(200, 168, 255, 18))
        glow.setColorAt(1.0, QColor(200, 168, 255, 0))
        p.setPen(Qt.PenStyle.NoPen)
        p.setBrush(QBrush(glow))
        p.drawRoundedRect(self.rect(), 16, 16)
        p.end()