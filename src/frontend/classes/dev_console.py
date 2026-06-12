from PyQt6.QtWidgets import (
    QVBoxLayout, QHBoxLayout,
    QPushButton, QLabel, QFrame, QTextEdit, 
)
from src.logger import dev_console_handler
# DEV CONSOLE PANEL
class DevConsolePanel(QFrame):
    def __init__(self, parent=None):
        super().__init__(parent)
        self.setObjectName("DevConsole")
        s = getattr(parent.window() if parent else None, '_s', 1.0) if parent else 1.0
        self.setFixedHeight(int(180 * s))
        self.setStyleSheet("""
            #DevConsole {
                background-color: rgba(10, 8, 18, 220);
                border-top: 1px solid #333333;
            }
            QTextEdit {
                background-color: transparent;
                color: #D7D7D7;
                font-family: 'Consolas', 'Courier New', monospace;
                font-size: 12px;
                border: none;
                padding: 6px 12px;
            }
        """)

        layout = QVBoxLayout(self)
        layout.setContentsMargins(0, 0, 0, 0)
        layout.setSpacing(0)

        header = QHBoxLayout()
        header.setContentsMargins(12, 6, 12, 0)
        lbl = QLabel("Developer Console")
        lbl.setStyleSheet("color: #969696; font-size: 11px; font-weight: bold;")
        btn_clear = QPushButton("Clear")
        btn_clear.setFixedSize(50, 22)
        btn_clear.setStyleSheet(
            "color: #969696; font-size: 11px; border: 1px solid #333; border-radius: 4px; padding: 0;"
        )

        header.addWidget(lbl)
        header.addStretch()
        header.addWidget(btn_clear)

        self.log_view = QTextEdit()
        self.log_view.setReadOnly(True)
        self.log_view.document().setMaximumBlockCount(500)  # cap at 500 lines to prevent performance drops

        btn_clear.clicked.connect(self.log_view.clear)

        layout.addLayout(header)
        layout.addWidget(self.log_view)

        if dev_console_handler:
            dev_console_handler.attach(self.log_view)

    def closeEvent(self, event):
        if dev_console_handler:
            dev_console_handler.detach()
        super().closeEvent(event)

    def repopulate(self, show_el: bool = False):
        if not dev_console_handler:
            return
        self.log_view.clear()
        for record in dev_console_handler.session_buffer:
            is_el = getattr(record, 'el', False)
            if is_el and not show_el:
                continue
            self.log_view.append(dev_console_handler._format_html(record))