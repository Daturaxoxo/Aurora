import os
from PyQt6.QtWidgets import (
    QWidget, 
)
from PyQt6.QtCore import Qt
from PyQt6.QtGui import QPixmap, QPainter

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


class OverlayWidget(QWidget):
    BASE_TOP_BAR_HEIGHT = 60

    def __init__(self, parent=None, scale: float = 1.0):
        super().__init__(parent)
        self.TOP_BAR_HEIGHT = int(self.BASE_TOP_BAR_HEIGHT * scale)
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
