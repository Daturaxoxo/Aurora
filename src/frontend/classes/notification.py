from src.utils import resource_path
from PyQt6.QtWidgets import (
    QHBoxLayout, QVBoxLayout,
    QLabel, QFrame, QPushButton,
    QGraphicsOpacityEffect
)
from PyQt6.QtCore import QTimer, QPropertyAnimation, QRectF, Qt, QEasingCurve, QVariantAnimation
from PyQt6.QtGui import QFont, QIcon, QPainter, QPainterPath, QColor
from src.frontend.styles import TOAST_STYLE

TOAST_DRAIN_COLORS = {
    "success": QColor("#22c55e"),
    "error": QColor("#ef4444"),
    "info": QColor("#3b82f6"),
}

# TOAST NOTIFICATION
class ToastNotification(QFrame):
    def __init__(self, parent, title, subtitle="", kind="success"):
        super().__init__(parent)
        self.setObjectName("ToastContainer")
        s = getattr(parent.window(), '_s', 1.0)
        self._s = s
        self._fading = False
        self._kind = kind
        self._drain_progress = 1.0
        self._drain_h = max(2, int(2 * s))

        self.setFixedWidth(int(300 * s))
        self.setStyleSheet(TOAST_STYLE)

        outer = QVBoxLayout(self)
        outer.setContentsMargins(0, 0, 0, 0)
        outer.setSpacing(0)

        body_row = QHBoxLayout()
        body_row.setContentsMargins(int(10 * s), int(10 * s), int(10 * s), int(12 * s))
        body_row.setSpacing(int(10 * s))

        icon_label = QLabel()
        icon_label.setFixedSize(int(36 * s), int(36 * s))
        icon_label.setAlignment(Qt.AlignmentFlag.AlignCenter)
        icon_label.setObjectName(f"ToastIcon_{kind}")

        icon_files = {"success": "success.png", "error": "error.png", "info": "info.png"}
        icon_path = resource_path(f"Bin/Assets/{icon_files.get(kind, 'checkmark.png')}")
        icon_label.setPixmap(QIcon(icon_path).pixmap(int(20 * s), int(20 * s)))

        body_row.addWidget(icon_label)

        text_col = QVBoxLayout()
        text_col.setSpacing(int(2 * s))
        text_col.setContentsMargins(0, 0, 0, 0)

        title_label = QLabel(title)
        title_label.setObjectName(f"ToastTitle_{kind}")
        title_label.setWordWrap(True)
        f = QFont()
        f.setPointSize(int(10 * s))
        f.setWeight(QFont.Weight.DemiBold)
        title_label.setFont(f)
        text_col.addWidget(title_label)

        if subtitle:
            sub_label = QLabel(subtitle)
            sub_label.setObjectName("ToastSubtitle")
            sub_label.setWordWrap(True)
            f2 = QFont()
            f2.setPointSize(int(9 * s))
            sub_label.setFont(f2)
            text_col.addWidget(sub_label)

        body_row.addLayout(text_col)

        dismiss = QPushButton("✕")
        dismiss.setObjectName("ToastDismiss")
        dismiss.setFixedSize(int(18 * s), int(18 * s))
        dismiss.setCursor(Qt.CursorShape.PointingHandCursor)
        dismiss.clicked.connect(self.fade_out)
        body_row.addWidget(dismiss, alignment=Qt.AlignmentFlag.AlignTop)

        outer.addLayout(body_row)
        outer.addSpacing(self._drain_h)

        self.adjustSize()
        self.move(parent.width() - self.width() - int(20 * s), int(30 * s))

        self._drain_anim = QVariantAnimation(self)
        self._drain_anim.setDuration(4000)
        self._drain_anim.setStartValue(1.0)
        self._drain_anim.setEndValue(0.0)
        self._drain_anim.setEasingCurve(QEasingCurve.Type.Linear)
        self._drain_anim.valueChanged.connect(self.set_drain_progress)

        self.opacity_effect = QGraphicsOpacityEffect(self)
        self.setGraphicsEffect(self.opacity_effect)
        self.anim = QPropertyAnimation(self.opacity_effect, b"opacity")
        self.anim.setDuration(300)
        self.anim.setStartValue(0.0)
        self.anim.setEndValue(1.0)

        self.show()
        self.raise_()
        self.anim.start()
        self._drain_anim.start()
        QTimer.singleShot(4000, self.fade_out)

    def set_drain_progress(self, value):
        self._drain_progress = max(0.0, min(1.0, float(value)))
        self.update()

    def paintEvent(self, event):
        super().paintEvent(event)

        painter = QPainter(self)
        painter.setRenderHint(QPainter.RenderHint.Antialiasing, True)

        radius = int(10 * self._s)

        clip_path = QPainterPath()
        clip_path.addRoundedRect(QRectF(self.rect()), radius, radius)
        painter.setClipPath(clip_path)

        y = self.height() - self._drain_h
        full_width = float(self.width())

        painter.fillRect(
            QRectF(0, y, full_width, self._drain_h),
            QColor(255, 255, 255, 13),
        )

        painter.fillRect(
            QRectF(0, y, full_width * self._drain_progress, self._drain_h),
            TOAST_DRAIN_COLORS.get(self._kind, TOAST_DRAIN_COLORS["info"]),
        )

    def fade_out(self):
        if self._fading: return
        self._fading = True
        self._drain_anim.stop()
        self.anim.stop()
        self.anim.setDirection(QPropertyAnimation.Direction.Backward)
        self.anim.finished.connect(self.deleteLater)
        self.anim.start()