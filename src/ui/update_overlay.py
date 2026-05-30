from __future__ import annotations
import subprocess
from PyQt6.QtCore import Qt, QTimer, QPropertyAnimation, QRect, pyqtSignal, QCoreApplication
from PyQt6.QtGui import (
    QPainter, QColor, QPainterPath,
)
from PyQt6.QtWidgets import (
    QWidget, QFrame, QVBoxLayout, QHBoxLayout, QLabel,
    QPushButton, QGraphicsOpacityEffect, QSizePolicy,
)
from src.styles import POPUP_STYLE
from src.updater import UpdateWorker
from src.logger import logger
from src.translator import t

_EXTRA_STYLE = """
    QLabel#VerArrow   { color: #a1a1aa; font-weight: 700; font-size: 14px; }
    QLabel#VerBadge {
        font-size: 12px; font-weight: 600; color: #a1a1aa;
        background: rgba(255,255,255,0.06);
        border: 1px solid rgba(255,255,255,0.12);
        border-radius: 10px;
        padding: 2px 8px;
        font-family: "Consolas","Courier New",monospace;
    }
    QLabel#VerBadgeNew {
        font-size: 12px; font-weight: 600; color: #ffffff;
        background: rgba(255,255,255,0.12);
        border: 1px solid rgba(255,255,255,0.25);
        border-radius: 10px;
        padding: 2px 8px;
        font-family: "Consolas","Courier New",monospace;
    }
    QLabel#StatusLabel {
        font-size: 12px; color: #71717a;
        font-family: "Consolas","Courier New",monospace;
    }
"""

class _ProgressBar(QWidget):
    def __init__(self, parent=None):
        super().__init__(parent)
        self._pct = 0
        self.setFixedHeight(32)
        self.setSizePolicy(QSizePolicy.Policy.Expanding, QSizePolicy.Policy.Fixed)

    def set_value(self, pct: int):
        self._pct = max(0, min(100, pct))
        self.update()

    def paintEvent(self, event):
        p = QPainter(self)
        p.setRenderHint(QPainter.RenderHint.Antialiasing)
        w, h = self.width(), self.height()
        radius = h / 2
        track_path = QPainterPath()
        track_path.addRoundedRect(0, 0, w, h, radius, radius)
        p.fillPath(track_path, QColor(40, 40, 40))

        fill_w = max(h, int(w * self._pct / 100))
        if self._pct > 0:
            fill_path = QPainterPath()
            fill_path.addRoundedRect(0, 0, fill_w, h, radius, radius)
            p.fillPath(fill_path, QColor(180, 180, 180))

        p.setPen(QColor(255, 255, 255, 220))
        font = p.font()
        font.setPointSize(11)
        font.setBold(True)
        font.setFamily("Segoe UI")
        p.setFont(font)
        p.drawText(QRect(0, 0, w, h), Qt.AlignmentFlag.AlignCenter, f"{self._pct}%")

        p.end()

# Main
class UpdateOverlay(QWidget):
    skipped = pyqtSignal()

    def __init__(self, parent: QWidget, local_version: str, online_version: str):
        super().__init__(parent)
        self._local  = local_version
        self._online = online_version
        self._worker: UpdateWorker | None = None

        self.setObjectName("DimOverlay")
        self.setFixedSize(parent.size())
        self.move(0, 0)
        self.setAttribute(Qt.WidgetAttribute.WA_StyledBackground)
        self.setStyleSheet(POPUP_STYLE + _EXTRA_STYLE)

        self._build_card()

        self.opacity_effect = QGraphicsOpacityEffect(self)
        self.setGraphicsEffect(self.opacity_effect)
        self.anim = QPropertyAnimation(self.opacity_effect, b"opacity")
        self.anim.setDuration(200)
        self.anim.setStartValue(0)
        self.anim.setEndValue(1)
        self.show()
        self.raise_()
        self.anim.start()

    def _build_card(self):
        self._card = QFrame(self)
        self._card.setObjectName("PopupContainer")
        self._card.setFixedWidth(440)
        self._card.setAttribute(Qt.WidgetAttribute.WA_StyledBackground)
        self._card.setStyleSheet(POPUP_STYLE + _EXTRA_STYLE)

        self._card_layout = QVBoxLayout(self._card)
        self._card_layout.setContentsMargins(32, 28, 32, 28)
        self._card_layout.setSpacing(16)

        lbl_title = QLabel(t("updater_title"))
        lbl_title.setObjectName("PopupTitle")
        self._card_layout.addWidget(lbl_title)

        ver_row = QHBoxLayout()
        ver_row.setSpacing(10)
        ver_row.setContentsMargins(0, 0, 0, 0)

        ver_row.addWidget(QLabel(self._local,  objectName="VerBadge"))
        ver_row.addWidget(QLabel("➜",          objectName="VerArrow"))
        ver_row.addWidget(QLabel(self._online, objectName="VerBadgeNew"))
        ver_row.addStretch()
        self._card_layout.addLayout(ver_row)

        self._progress_bar = _ProgressBar()
        self._progress_bar.hide()
        self._card_layout.addWidget(self._progress_bar)

        self._status_label = QLabel("")
        self._status_label.setObjectName("StatusLabel")
        self._status_label.hide()
        self._card_layout.addWidget(self._status_label)

        self._btn_row_widget = QWidget()
        btn_row = QHBoxLayout(self._btn_row_widget)
        btn_row.setContentsMargins(0, 0, 0, 0)
        btn_row.setSpacing(12)
        btn_row.addStretch()

        self._btn_skip = QPushButton(t("updater_skip"))
        self._btn_skip.setObjectName("PopupCancelButton")
        self._btn_skip.setFixedHeight(36)
        self._btn_skip.setCursor(Qt.CursorShape.PointingHandCursor)
        self._btn_skip.clicked.connect(self._on_skip)

        self._btn_update = QPushButton(t("updater_update"))
        self._btn_update.setObjectName("PopupConfirmButton")
        self._btn_update.setFixedHeight(36)
        self._btn_update.setCursor(Qt.CursorShape.PointingHandCursor)
        self._btn_update.clicked.connect(self._on_update_now)

        btn_row.addWidget(self._btn_skip)
        btn_row.addWidget(self._btn_update)
        self._card_layout.addWidget(self._btn_row_widget)

        self._card.adjustSize()
        self._recentre()

    def _recentre(self):
        self._card.setMaximumHeight(16777215)
        QCoreApplication.processEvents() 
        
        self._card.adjustSize()
        self._card.move(
            (self.width()  - self._card.width())  // 2,
            (self.height() - self._card.height()) // 2,
        )

    def _show_progress_state(self):
        self._progress_bar.show()
        self._status_label.show()
        self._btn_row_widget.hide()
        self._recentre()

    def _on_skip(self):
        logger.info("User skipped the update.", extra={"el": True})
        self.skipped.emit()
        self._close()

    def _on_update_now(self):
        logger.info(
            f"User accepted update: {self._local} -> {self._online}",
            extra={"el": True},
        )
        self._show_progress_state()

        self._worker = UpdateWorker(parent=self)
        self._worker.progress.connect(self._on_progress)
        self._worker.log.connect(self._on_log)
        self._worker.finished.connect(self._on_worker_finished)
        self._worker.error.connect(self._on_worker_error)
        self._worker.start()

    def _on_progress(self, pct: int):
        self._progress_bar.set_value(pct)

    def _on_log(self, text: str):
        self._status_label.setText(f"> {text}")

    def _on_worker_finished(self):
        self._progress_bar.set_value(100)
        self._status_label.setText(t("updater_status_restart"))

        lbl_done = QLabel(t("updater_restart_prompt"))
        lbl_done.setObjectName("PopupMessage")
        lbl_done.setWordWrap(True)
        insert_at = self._card_layout.count() - 1
        self._card_layout.insertWidget(insert_at, lbl_done)

        self._btn_update.setEnabled(True)
        self._btn_update.setText(t("updater_close"))
        self._btn_update.clicked.disconnect()
        self._btn_update.clicked.connect(self._on_close_and_reveal)
        self._btn_skip.hide()
        self._btn_row_widget.show()
        self._recentre()

    def _on_close_and_reveal(self):
        install_dir = str(self._worker._install_root)
        
        subprocess.Popen(
            f'cmd.exe /c timeout /t 2 /nobreak >nul & del /f /q "{install_dir}\\*.old"',
            creationflags=subprocess.CREATE_NO_WINDOW,
            shell=True
        )
        
        subprocess.Popen(
            ["explorer.exe", install_dir],
            creationflags=subprocess.CREATE_NO_WINDOW,
        )
        QCoreApplication.quit()

    def _on_worker_error(self, message: str):
        logger.error(f"[Updater] {message}")
        self._btn_update.setEnabled(True)
        self._btn_skip.setEnabled(True)
        self._btn_update.setText("Retry")
        self._btn_row_widget.show()
        self._status_label.setText(t("updater_status_error"))

        err = QLabel(f"{message[:140]}{'...' if len(message) > 140 else ''}")
        err.setWordWrap(True)
        err.setObjectName("PopupMessage")
        err.setStyleSheet("color: #f87171;")
        insert_at = self._card_layout.count() - 2
        self._card_layout.insertWidget(insert_at, err)
        self._recentre()

    def _close(self):
        self.anim.setDirection(QPropertyAnimation.Direction.Backward)
        self.anim.finished.connect(self.deleteLater)
        self.anim.start()