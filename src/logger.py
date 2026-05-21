import logging
import os
import sys
from datetime import datetime
from PyQt6.QtCore import QObject, pyqtSignal
from src import config_manager as cfg

def get_app_dir():
    if getattr(sys, 'frozen', False):
        return os.path.dirname(sys.executable)
    return os.path.dirname(os.path.dirname(os.path.abspath(__file__)))

class ExtensiveLoggingFilter(logging.Filter):
    def filter(self, record):
        if getattr(record, 'el', False):
            return cfg.get(cfg.Key.EXTENSIVE_LOGGING)
        return True

class ErrorTriggeredFileHandler(logging.Handler):
    def __init__(self, log_dir):
        super().__init__(level=logging.DEBUG)
        self.log_dir = log_dir
        self.buffer = []
        self.file_created = False
        self.log_path = None
        self.formatter = logging.Formatter(
            '[AU] [%(asctime)s] [%(levelname)s] %(message)s',
            datefmt='%H:%M:%S'
        )

    def emit(self, record):
        self.buffer.append(record)
        if record.levelno >= logging.ERROR and not self.file_created:
            self._create_file()
        if self.file_created:
            self._write(record)

    def _create_file(self):
        try:
            os.makedirs(self.log_dir, exist_ok=True)
            timestamp = datetime.now().strftime("aurora_%Y-%m-%d_%H-%M-%S.log")
            self.log_path = os.path.join(self.log_dir, timestamp)
            self.file_created = True
            with open(self.log_path, 'w', encoding='utf-8') as f:
                f.write(f"--- Aurora Error Log — {datetime.now().strftime('%Y-%m-%d %H:%M:%S')} ---\n")
                f.write("(Full session history leading up to the error)\n\n")
                for record in self.buffer:
                    f.write(self.formatter.format(record) + '\n')
        except Exception:
            pass

    def _write(self, record):
        try:
            with open(self.log_path, 'a', encoding='utf-8') as f:
                f.write(self.formatter.format(record) + '\n')
        except Exception:
            pass


class _ConsoleSignaller(QObject):
    append_html = pyqtSignal(str)


class DevConsoleHandler(logging.Handler):
    def __init__(self):
        super().__init__(level=logging.DEBUG)
        self._widget = None
        self._signaller = None
        self.session_buffer: list[logging.LogRecord] = []
        self.formatter = logging.Formatter(
            '[%(asctime)s] [%(levelname)s] %(message)s',
            datefmt='%H:%M:%S'
        )
        self._colours = {
            logging.DEBUG:    "#969696",
            logging.INFO:     "#D7D7D7",
            logging.WARNING:  "#f6c177",
            logging.ERROR:    "#eb6f92",
            logging.CRITICAL: "#eb6f92",
        }

    def attach(self, widget, history: list = None):
        self._widget = widget
        # Create signaller here, QApplication is guaranteed to exist by now (hopefully)
        if self._signaller is None:
            self._signaller = _ConsoleSignaller()
        self._signaller.append_html.connect(widget.append)
        # Replay session history immediately if it exists
        if history:
            for record in history:
                widget.append(self._format_html(record))

    def detach(self):
        if self._widget is not None:
            try:
                self._signaller.append_html.disconnect(self._widget.append)
            except Exception:
                pass
        self._widget = None

    def emit(self, record):
        self.session_buffer.append(record)
        if self._widget is None or self._signaller is None:
            return
        try:
            self._signaller.append_html.emit(self._format_html(record))
        except Exception:
            pass

    def _format_html(self, record) -> str:
        colour = self._colours.get(record.levelno, "#D7D7D7")
        msg = self.formatter.format(record).replace("<", "&lt;").replace(">", "&gt;")
        return f'<span style="color:{colour}">{msg}</span>'


def setup_logger():
    app_dir = get_app_dir()
    log_dir = os.path.join(app_dir, "Logs")

    root_logger = logging.getLogger()
    if root_logger.handlers:
        return logging.getLogger("Aurora")

    console_handler = logging.StreamHandler()
    console_handler.setFormatter(logging.Formatter(
        '[AU] [%(asctime)s] [%(levelname)s] %(message)s',
        datefmt='%H:%M:%S'
    ))

    global file_handler
    file_handler = ErrorTriggeredFileHandler(log_dir)
    global dev_console_handler
    dev_console_handler = DevConsoleHandler()

    el_filter = ExtensiveLoggingFilter()
    root_logger.setLevel(logging.DEBUG)
    root_logger.addHandler(console_handler)
    root_logger.addHandler(file_handler)
    root_logger.addHandler(dev_console_handler)

    logger = logging.getLogger("Aurora")
    logger.addFilter(el_filter)
    logger.info("————— Aurora Launcher —————")
    logger.info(f"App directory: {app_dir}", extra={'el': True})
    return logger


dev_console_handler: DevConsoleHandler = None
file_handler: ErrorTriggeredFileHandler = None
logger = setup_logger()
