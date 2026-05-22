import os
import sys
import json

LANG_CODES = {
    "English":    "en",
    "Türkçe":     "tr",
    "中文":        "cn",
    "日本語":      "jp",
    "Español":    "es",
    "Deutsch":    "de",
    "Tiếng Việt": "vi",
    "Dutch":      "nl",
}
LANG_NAMES = {v: k for k, v in LANG_CODES.items()}

# Config keys — use these instead of raw strings
class Key:
    GAME_PATH         = "game_path"
    LANGUAGE          = "language"
    DEV_MODE          = "dev_mode"
    CENSORSHIP_REMOVE = "csn_rem"
    NO_DRIVE_LINE     = "drv_lin"
    DISCORD_RPC       = "discord_rpc"
    EXTENSIVE_LOGGING = "extensive_logging"
    EXPORT_CONSOLE    = "export_console"
    UI_SCALING        = "ui_scaling"
    UI_MINIMIZATION   = "ui_min"

DEFAULTS = {
    Key.GAME_PATH:         "",
    Key.LANGUAGE:          "en",
    Key.DEV_MODE:          False,
    Key.CENSORSHIP_REMOVE: True,
    Key.NO_DRIVE_LINE:     False,
    Key.DISCORD_RPC:       True,
    Key.EXTENSIVE_LOGGING: False,
    Key.UI_SCALING:        1.0,
    Key.UI_MINIMIZATION:   True,
}

def get_app_dir():
    if getattr(sys, 'frozen', False):
        return os.path.dirname(sys.executable)
    return os.path.dirname(os.path.dirname(os.path.abspath(__file__)))

CONFIG_FILE = os.path.join(get_app_dir(), "config.json")

def _load_raw() -> dict:
    if not os.path.exists(CONFIG_FILE):
        return {}
    try:
        if os.path.getsize(CONFIG_FILE) == 0:
            return {}
        with open(CONFIG_FILE, "r", encoding="utf-8") as f:
            return json.load(f)
    except (json.JSONDecodeError, OSError):
        return {}

def _save_raw(data: dict):
    try:
        with open(CONFIG_FILE, "w", encoding="utf-8") as f:
            json.dump(data, f, indent=2, ensure_ascii=False)
    except OSError:
        pass

def get(key: str):
    return _load_raw().get(key, DEFAULTS.get(key))

def set(key: str, value):
    data = _load_raw()
    data[key] = value
    _save_raw(data)
