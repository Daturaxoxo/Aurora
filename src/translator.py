import os
import sys
import json
from src.logger import logger
from PyQt6.QtCore import QObject, pyqtSignal
from src.utils import resource_path

class _Translator(QObject):
    language_changed = pyqtSignal()

    def __init__(self):
        super().__init__()
        self._strings: dict = {}
        self._lang: str = "en"
        
        self._lang_dir = resource_path("Lang")

    def load(self, lang_code: str):
        path = os.path.join(self._lang_dir, "langs.json")

        try:
            with open(path, "r", encoding="utf-8") as f:
                all_strings: dict = json.load(f)
        except Exception as e:
            logger.warning(f"Error loading translations file ({path}): {e}")
            self._strings = {}
            self.language_changed.emit()
            return

        if lang_code not in all_strings:
            logger.warning(f"Language '{lang_code}' not found in langs.json, falling back to English.")
            lang_code = "en"

        self._strings = all_strings.get(lang_code, {})
        self._lang = lang_code
        self.language_changed.emit()

    def t(self, key: str) -> str:
        return self._strings.get(key, key)

Translator = _Translator()
t = Translator.t