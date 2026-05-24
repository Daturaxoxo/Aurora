from PyQt6.QtWidgets import (
    QFrame, QVBoxLayout, QHBoxLayout, QLabel,
    QScrollArea, QWidget, QPushButton
)
from PyQt6.QtCore import Qt, QPropertyAnimation, QEasingCurve, QSize
from PyQt6.QtGui import QIcon
from src.utils import resource_path
from src.engine import get_app_dir
from src.logger import logger
from src import config_manager as cfg
from src.translator import Translator
from pathlib import Path
import json
import re

_FALLBACK_LANG = "en"

def _resolve(field, lang: str) -> str | None:
    if field is None:
        return None

    if isinstance(field, str):
        return field
    if isinstance(field, list):
        return "\n".join(str(l) for l in field)
    if isinstance(field, dict):
        value = field.get(lang) or field.get(_FALLBACK_LANG)
        if value is None:
            return None
        if isinstance(value, list):
            return "\n".join(str(l) for l in value)
        return str(value)

    return None


def _load_faq_items(lang: str = "en") -> list[dict]:
    questions_dir = Path(get_app_dir()) / "src" / "ui" / "questions"
    items = []

    if not questions_dir.exists():
        logger.warning(f"FAQ: questions directory not found: {questions_dir}", extra={"el": True})
        return items

    for path in sorted(questions_dir.glob("*.json")):
        try:
            data = json.loads(path.read_text(encoding="utf-8"))
        except (json.JSONDecodeError, OSError) as e:
            logger.warning(f"FAQ: could not load {path.name}: {e}", extra={"el": True})
            continue

        question = _resolve(data.get("question"), lang)
        answer   = _resolve(data.get("answer"),   lang)

        if not question or not answer:
            logger.warning(f"FAQ: skipping {path.name}, missing 'question' or 'answer'", extra={"el": True})
            continue

        items.append({"question": question, "answer": answer})

    return items

def _md_to_html(text: str) -> str:
    lines = text.split("\n")
    out = []
    in_list = False

    for line in lines:
        stripped = line.rstrip()

        if stripped.startswith("### "):
            if in_list: out.append("</ul>"); in_list = False
            out.append(f'<p style="color:#C8A8FF;font-size:12px;font-weight:600;margin:8px 0 2px 0;">{stripped[4:]}</p>')
            continue
        if stripped.startswith("## "):
            if in_list: out.append("</ul>"); in_list = False
            out.append(f'<p style="color:#E8E8E8;font-size:13px;font-weight:600;margin:10px 0 4px 0;">{stripped[3:]}</p>')
            continue

        if stripped.startswith("- ") or stripped.startswith("* "):
            if not in_list:
                out.append('<ul style="margin:4px 0 4px 16px;padding:0;">')
                in_list = True
            out.append(f'<li style="color:#707070;font-size:12px;margin:2px 0;">{_inline(stripped[2:])}</li>')
            continue

        if in_list:
            out.append("</ul>")
            in_list = False

        if not stripped:
            out.append('<p style="margin:3px 0;"></p>')
            continue

        out.append(f'<p style="color:#707070;font-size:12px;margin:3px 0;">{_inline(stripped)}</p>')

    if in_list:
        out.append("</ul>")

    return "".join(out)


def _inline(text: str) -> str:
    text = re.sub(
        r'\[([^\]]+)\]\((https?://[^\)]+)\)',
        r'<a href="\2" style="color:#C8A8FF;text-decoration:underline;">\1</a>',
        text,
    )
    text = re.sub(
        r'`([^`]+)`',
        r'<code style="background:rgba(200,168,255,0.10);color:#C8A8FF;'
        r'padding:1px 4px;border-radius:3px;font-family:Consolas,monospace;font-size:11px;">\1</code>',
        text,
    )
    text = re.sub(r'\*\*(.+?)\*\*', r'<b style="color:#C8C8C8;">\1</b>', text)
    text = re.sub(r'\*(.+?)\*',     r'<i style="color:#A8A8A8;">\1</i>', text)
    return text

_ENTRY_STYLE = """
    QFrame#FaqEntry {{
        background-color: rgba(255, 255, 255, {bg});
        border: 1px solid rgba(255, 255, 255, {border});
        border-radius: 10px;
    }}
    QFrame#FaqEntry:hover {{
        background-color: rgba(255, 255, 255, 7);
        border: 1px solid rgba(255, 255, 255, 12);
    }}
"""


class FaqEntry(QFrame):
    def __init__(self, question: str, answer_md: str, parent=None):
        super().__init__(parent)
        self.setObjectName("FaqEntry")
        self._expanded = False
        self._answer_html = _md_to_html(answer_md)
        self._anim: QPropertyAnimation | None = None
        self._apply_style(False)
        self.setCursor(Qt.CursorShape.PointingHandCursor)

        outer = QVBoxLayout(self)
        outer.setContentsMargins(0, 0, 0, 0)
        outer.setSpacing(0)

        # Header
        header = QWidget()
        header.setAttribute(Qt.WidgetAttribute.WA_TranslucentBackground)
        h_layout = QHBoxLayout(header)
        h_layout.setContentsMargins(20, 0, 20, 0)
        h_layout.setSpacing(12)

        self._q_label = QLabel(question)
        self._q_label.setFixedHeight(68)
        self._q_label.setAlignment(Qt.AlignmentFlag.AlignVCenter)
        self._q_label.setStyleSheet(
            "color: #E8E8E8; font-size: 14px; font-weight: 500; "
            "background: transparent; border: none;"
        )
        self._q_label.setWordWrap(True)

        self._arrow = QLabel("›")
        self._arrow.setFixedWidth(18)
        self._arrow.setAlignment(Qt.AlignmentFlag.AlignCenter)
        self._arrow.setStyleSheet(
            "color: #585858; font-size: 20px; font-weight: 300; "
            "background: transparent; border: none;"
        )

        h_layout.addWidget(self._q_label, 1)
        h_layout.addWidget(self._arrow)

        self._answer_container = QWidget()
        self._answer_container.setAttribute(Qt.WidgetAttribute.WA_TranslucentBackground)
        self._answer_container.setMaximumHeight(0)
        self._answer_container.setMinimumHeight(0)

        ac_layout = QVBoxLayout(self._answer_container)
        ac_layout.setContentsMargins(20, 0, 20, 16)
        ac_layout.setSpacing(0)

        divider = QFrame()
        divider.setFrameShape(QFrame.Shape.HLine)
        divider.setStyleSheet("background: rgba(255,255,255,6); border: none; max-height: 1px;")
        ac_layout.addWidget(divider)
        ac_layout.addSpacing(12)

        self._a_label = QLabel()
        self._a_label.setTextFormat(Qt.TextFormat.RichText)
        self._a_label.setText(self._answer_html)
        self._a_label.setWordWrap(True)
        self._a_label.setOpenExternalLinks(True)
        self._a_label.setStyleSheet("background: transparent; border: none;")
        ac_layout.addWidget(self._a_label)

        outer.addWidget(header)
        outer.addWidget(self._answer_container)

    def _apply_style(self, expanded: bool):
        self.setStyleSheet(_ENTRY_STYLE.format(
            bg=7 if expanded else 4,
            border=12 if expanded else 7,
        ))

    def _content_height(self) -> int:
        self._answer_container.setMaximumHeight(16_777_215)
        h = self._answer_container.sizeHint().height()
        if not self._expanded:
            self._answer_container.setMaximumHeight(0)
        return h

    def toggle(self):
        self._expanded = not self._expanded
        self._apply_style(self._expanded)
        self._arrow.setStyleSheet(
            f"color: {'#C8A8FF' if self._expanded else '#585858'}; font-size: 20px; "
            "font-weight: 300; background: transparent; border: none;"
        )
        target_h = self._content_height() if self._expanded else 0

        if self._anim and self._anim.state() == QPropertyAnimation.State.Running:
            self._anim.stop()

        self._anim = QPropertyAnimation(self._answer_container, b"maximumHeight")
        self._anim.setDuration(220)
        self._anim.setStartValue(self._answer_container.maximumHeight())
        self._anim.setEndValue(target_h)
        self._anim.setEasingCurve(
            QEasingCurve.Type.OutCubic if self._expanded else QEasingCurve.Type.InCubic
        )
        self._anim.start()

    def mousePressEvent(self, event):
        if event.button() == Qt.MouseButton.LeftButton:
            self.toggle()
        super().mousePressEvent(event)

_OVERLAY_STYLE = """
    QFrame#FaqContainer {
        background-color: rgb(18, 18, 20);
        border: 1px solid rgba(255, 255, 255, 10);
        border-radius: 12px;
    }
    QScrollArea {
        background: transparent;
        border: none;
    }
    QScrollBar:vertical {
        background: transparent;
        width: 6px;
        margin: 0;
    }
    QScrollBar::handle:vertical {
        background: rgba(255, 255, 255, 20);
        border-radius: 3px;
        min-height: 20px;
    }
    QScrollBar::handle:vertical:hover {
        background: rgba(255, 255, 255, 35);
    }
    QScrollBar::add-line:vertical,
    QScrollBar::sub-line:vertical { height: 0; }
    QScrollBar::add-page:vertical,
    QScrollBar::sub-page:vertical { background: transparent; }
"""

_SECTION_LABEL_STYLE = "color: #484848; font-size: 11px; font-weight: 600; letter-spacing: 1px;"


class FaqOverlay(QFrame):
    def __init__(self, parent=None):
        super().__init__(parent)
        self.setObjectName("FaqContainer")
        self.setFixedSize(800, 500)
        self.move(240, 110)
        self.setAttribute(Qt.WidgetAttribute.WA_StyledBackground)
        self.setStyleSheet(_OVERLAY_STYLE)
        self.hide()

        root = QVBoxLayout(self)
        root.setContentsMargins(32, 28, 32, 28)
        root.setSpacing(0)

        # Header row
        hdr = QHBoxLayout()
        hdr.setSpacing(0)
        hdr.setContentsMargins(0, 0, 0, 0)

        title = QLabel("FAQ")
        title.setStyleSheet(
            "color: #D7D7D7; font-size: 20px; font-weight: 500; "
            "background: transparent; border: none;"
        )

        btn_close = QPushButton()
        btn_close.setIcon(QIcon(resource_path("Bin/Assets/close.png")))
        btn_close.setIconSize(QSize(24, 24))
        btn_close.setFixedSize(24, 24)
        btn_close.setCursor(Qt.CursorShape.PointingHandCursor)
        btn_close.setStyleSheet("QPushButton { background: transparent; border: none; }")
        btn_close.clicked.connect(self.hide)

        hdr.addWidget(title)
        hdr.addStretch()
        hdr.addWidget(btn_close)

        root.addLayout(hdr)
        root.addSpacing(20)

        sec = QLabel("GENERAL")
        sec.setStyleSheet(_SECTION_LABEL_STYLE)
        sec.setContentsMargins(4, 0, 0, 0)
        root.addWidget(sec)
        root.addSpacing(10)

        self._scroll = QScrollArea()
        self._scroll.setWidgetResizable(True)
        self._scroll.setHorizontalScrollBarPolicy(Qt.ScrollBarPolicy.ScrollBarAlwaysOff)
        self._scroll.setVerticalScrollBarPolicy(Qt.ScrollBarPolicy.ScrollBarAsNeeded)
        root.addWidget(self._scroll, 1)

        self._entries: list[FaqEntry] = []
        self._rebuild_list()

        Translator.language_changed.connect(self._rebuild_list)

    def _rebuild_list(self):
        lang = cfg.get(cfg.Key.LANGUAGE) or _FALLBACK_LANG

        content = QWidget()
        content.setStyleSheet("background: transparent;")
        content_layout = QVBoxLayout(content)
        content_layout.setContentsMargins(0, 0, 6, 0)
        content_layout.setSpacing(6)

        faq_items = _load_faq_items(lang)
        self._entries = []

        if not faq_items:
            placeholder = QLabel(
                "No FAQ entries found.\n"
                "This might be because of a corrupted install. Please try reinstalling or getting support."
            )
            placeholder.setAlignment(Qt.AlignmentFlag.AlignCenter)
            placeholder.setStyleSheet(
                "color: #484848; font-size: 13px; background: transparent; border: none;"
            )
            content_layout.addWidget(placeholder)
        else:
            for item in faq_items:
                entry = FaqEntry(item["question"], item["answer"])
                content_layout.addWidget(entry)
                self._entries.append(entry)

        content_layout.addStretch()

        old = self._scroll.takeWidget()
        if old:
            old.deleteLater()
        self._scroll.setWidget(content)

        logger.info(
            f"FAQ: loaded {len(self._entries)} entr{'y' if len(self._entries) == 1 else 'ies'} "
            f"for lang '{lang}'", extra={"el": True}
        )