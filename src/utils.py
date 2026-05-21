import os
import sys

import urllib
import urllib.request

from src.logger import logger

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
