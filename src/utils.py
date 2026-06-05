import os
from pathlib import Path
import sys
import urllib
import urllib.request
import requests
from src import config_manager as cfg
from src.path_finder import get_local_version

def get_app_dir():
    if getattr(sys, 'frozen', False):
        return os.path.dirname(sys.executable)
    return os.path.dirname(os.path.dirname(os.path.abspath(__file__)))

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
        print("WARN: Couldn't get version info from GitHub")

def get_mods_path():
    if cfg.get(cfg.Key.USE_HARD_LINKS):
        return Path(get_app_dir()) / "Mods"
    else:
        return Path(cfg.get(cfg.Key.GAME_PATH)) / "Client/WindowsNoEditor/HT/Content/Paks/AuroraMods"
    
def _ensure_dir(path: Path):
    if path.exists() and not path.is_dir():
        path.unlink()  # remove the file so mkdir can proceed
    path.mkdir(parents=True, exist_ok=True)

def download_file(filename: str, url: str, dest_folder: Path = get_mods_path()):
    headers = {
        "User-Agent": f"AuroraLauncher/{get_local_version()}",
    }
    
    try:
        with requests.get(url, headers=headers, stream=True) as response:
            response.raise_for_status()
            
            filepath = os.path.join(dest_folder, filename)
            
            with open(filepath, 'wb') as f:
                for chunk in response.iter_content(chunk_size=8192):
                    f.write(chunk)

        return filepath
        
    except requests.exceptions.RequestException as e:
        return None
    
def bytes_to_human_readable(num_bytes: float) -> str:
    for unit in ['B', 'KB', 'MB', 'GB']:
        if num_bytes < 1024.0:
            return f"{num_bytes:.2f} {unit}"
        num_bytes /= 1024.0
    return f"{num_bytes:.2f} GB"