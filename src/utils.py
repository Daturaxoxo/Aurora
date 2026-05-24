import os
from pathlib import Path
import sys
import urllib
import urllib.request
from src.engine import get_app_dir
from src.logger import logger
from src import config_manager as cfg

def resource_path(relative_path):
    try:
        base_path = sys._MEIPASS
    except Exception:
        base_path = os.path.abspath(".")

    return os.path.join(base_path, relative_path)

def parse_version(v):
    try:
        return tuple(int(x) for x in v.strip().split("."))
    except (ValueError, AttributeError):
        return (0, 0, 0)

def GetOnlineVersion():
    try:
        with urllib.request.urlopen("https://raw.githubusercontent.com/Daturaxoxo/Aurora/refs/heads/main/dev/VERSION") as response:
            version_info = response.read().decode('utf-8').strip()
        return version_info or "9.9.9"
    except Exception as _:
        logger.warning("Couldn't get version information GitHub ")

def get_mods_path():
    if cfg.get(cfg.Key.USE_HARD_LINKS):
        return Path(get_app_dir()) / "Mods"
    else:
        return Path(cfg.get(cfg.Key.GAME_PATH)) / "Client/WindowsNoEditor/HT/Content/Paks/AuroraMods"