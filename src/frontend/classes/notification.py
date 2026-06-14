from src.utils import resource_path
from PyQt6.QtWidgets import (
    QHBoxLayout, QVBoxLayout,
    QLabel, QFrame, QPushButton,
    QGraphicsOpacityEffect,
)
from PyQt6.QtCore import QTimer, QPropertyAnimation, QRect, Qt
from PyQt6.QtGui import QFont, QIcon
from src.frontend.styles import TOAST_STYLE

# TOAST NOTIFICATION
class ToastNotification(QFrame):
    def __init__(self, parent, title, subtitle="", kind="success"):
        super().__init__(parent)
        self.setObjectName("ToastContainer")
        s = getattr(parent.window(), '_s', 1.0)
        self._s = s
        self._fading = False

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

        drain_track = QFrame()
        drain_track.setFixedHeight(int(2 * s))
        drain_track.setObjectName("ToastDrainTrack")
        outer.addWidget(drain_track)

        self.adjustSize()
        self.move(parent.width() - self.width() - int(20 * s), int(30 * s))

        bar_h = int(2 * s)
        bar_y = self.height() - bar_h
        bar_w = self.width()
        self._drain = QFrame(self)
        self._drain.setObjectName(f"ToastDrain_{kind}")
        self._drain.setGeometry(0, bar_y, bar_w, bar_h)
        self._drain.raise_()

        self._drain_anim = QPropertyAnimation(self._drain, b"geometry")
        self._drain_anim.setDuration(4000)
        self._drain_anim.setStartValue(QRect(0, bar_y, bar_w, bar_h))
        self._drain_anim.setEndValue(QRect(0, bar_y, 0, bar_h))

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

    def fade_out(self):
        if self._fading: return
        self._fading = True
        self._drain_anim.stop()
        self.anim.stop()
        self.anim.setDirection(QPropertyAnimation.Direction.Backward)
        self.anim.finished.connect(self.deleteLater)
        self.anim.start()