from src.utils import resource_path
from PyQt6.QtWidgets import (
    QHBoxLayout,
    QLabel, QFrame, QGraphicsOpacityEffect, 
)
from PyQt6.QtCore import QTimer, QPropertyAnimation
from PyQt6.QtGui import QIcon
from src.frontend.styles import TOAST_STYLE

# TOAST NOTIFICATION
class ToastNotification(QFrame):
    def __init__(self, parent, message, is_error=False):
        super().__init__(parent)
        self.setObjectName("ToastContainer")
        s = getattr(parent.window(), '_s', 1.0)
        self.setFixedSize(int(340 * s), int(70 * s))
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