import os
import sys
import json
import platform

LANG_CODES = {
    "English":                "en",
    "Türkçe":                 "tr",
    "简体中文":               "cn",
    "繁體中文":               "zh-TW",
    "日本語":                 "jp",
    "Español":                "es",
    "Português (Brasil)":     "pt-br",
    "Deutsch":                "de",
    "Tiếng Việt":             "vi",
    "Nederlands":             "nl",
    "Pусский":                "ru",
    "Bahasa Indonesia":       "id",
    "Italiano":               "it",
    "French":                 "fr",
    "한국어":                 "ko"
}
LANG_NAMES = {v: k for k, v in LANG_CODES.items()}

class Key:
    GAME_PATH         = "game_path"
    ENGINE_METHOD     = "engine_method"
    LANGUAGE          = "language"
    DEV_MODE          = "dev_mode"
    CENSORSHIP_REMOVE = "csn_rem"
    HIDE_UID          = "uid_rem",
    NO_DRIVE_LINE     = "ndl_add"
    HIDE_NOTIF_DOTS   = "nor_rem"
    DISCORD_RPC       = "discord_rpc"
    EXTENSIVE_LOGGING = "extensive_logging"
    EXPORT_CONSOLE    = "export_console"
    UI_SCALING        = "ui_scaling"
    UI_MINIMIZATION   = "ui_min"
    SHOW_NSFW_MODS    = "show_nsfw_mods"
    APP_LOCATION      = "app_location"

DEFAULTS = {
    Key.GAME_PATH:         "",
    Key.LANGUAGE:          "en",
    Key.DEV_MODE:          False,
    Key.CENSORSHIP_REMOVE: True,
    Key.HIDE_UID:          True,
    Key.NO_DRIVE_LINE:     False,
    Key.HIDE_NOTIF_DOTS:   False,
    Key.DISCORD_RPC:       True,
    Key.EXTENSIVE_LOGGING: False,
    Key.UI_SCALING:        1.0,
    Key.UI_MINIMIZATION:   True,
    Key.SHOW_NSFW_MODS:    False,
    Key.ENGINE_METHOD:     "0",
    Key.APP_LOCATION:      "",
}

def get_app_dir():
    if getattr(sys, 'frozen', False): return os.path.dirname(sys.executable)
    return os.path.dirname(os.path.dirname(os.path.abspath(__file__)))

def get_cache_dir() -> str:
    if platform.system() == "Windows": base = os.environ.get("APPDATA", os.path.expanduser("~")) # Windows
    else: base = os.path.join(os.path.expanduser("~"), ".config") # Linux
    cache_dir = os.path.join(base, "Aurora", "Cache")
    os.makedirs(cache_dir, exist_ok=True)
    return cache_dir

def get_config_dir() -> str:
    if platform.system() == "Windows": base = os.environ.get("APPDATA", os.path.expanduser("~")) # Windows
    else: base = os.path.join(os.path.expanduser("~"), ".config") # Linux
    config_dir = os.path.join(base, "Aurora", "UserData")
    os.makedirs(config_dir, exist_ok=True)
    return config_dir

CONFIG_FILE = os.path.join(get_config_dir(), "config.json")
CACHE_FILE = os.path.join(get_cache_dir(), "storage.json")

def _load_raw() -> dict:
    if not os.path.exists(CONFIG_FILE): return {}
    try:
        if os.path.getsize(CONFIG_FILE) == 0: return {}
        with open(CONFIG_FILE, "r", encoding="utf-8") as f: return json.load(f)
    except (json.JSONDecodeError, OSError): return {}

def _save_raw(data: dict):
    try:
        with open(CONFIG_FILE, "w", encoding="utf-8") as f: 
            json.dump(data, f, indent=2, ensure_ascii=False)
    except OSError: pass

def get(key: str):
    return _load_raw().get(key, DEFAULTS.get(key))

def set(key: str, value):
    data = _load_raw()
    data[key] = value
    _save_raw(data)
    
def _load_cache() -> dict:
    if not os.path.exists(CACHE_FILE): return {}
    try:
        if os.path.getsize(CACHE_FILE) == 0: return {}
        with open(CACHE_FILE, "r", encoding="utf-8") as f: return json.load(f)
    except (json.JSONDecodeError, OSError): return {}
    
def _save_cache(data: dict):
    try:
        with open(CACHE_FILE, "w", encoding="utf-8") as f:
            json.dump(data, f, indent=2, ensure_ascii=False)
    except OSError:
        pass
    
def _migrate_old_config():
    cache = _load_cache()
    if cache.get("config_migrated"): return

    old_config = os.path.join(get_app_dir(), "config.json")
    if os.path.exists(old_config):
        try:
            with open(old_config, "r", encoding="utf-8") as f: old_data = json.load(f)
            existing = _load_raw()
            merged = {**old_data, **existing}
            _save_raw(merged)
            os.remove(old_config)
        except (json.JSONDecodeError, OSError): pass

    cache["config_migrated"] = True
    _save_cache(cache)

def update_app_location():
    if getattr(sys, 'frozen', False):
        exe_path = os.path.abspath(sys.executable)
    else:
        exe_path = os.path.abspath(sys.argv[0])
    set(Key.APP_LOCATION, exe_path)
    
_migrate_old_config()