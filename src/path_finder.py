import os
import sys
import json
from pathlib import Path
from src.helpers.paths import _LAUNCHER_MAP

fallback_scandir = False
try:
    from scandir_rs import Scandir
except ImportError:
    fallback_scandir = True

def get_app_dir():
    if getattr(sys, 'frozen', False):
        return os.path.dirname(sys.executable)
    return os.path.dirname(os.path.dirname(os.path.abspath(__file__)))

CONFIG_FILE = os.path.join(get_app_dir(), "config.json")
GAME_FOLDER_NAME = "Neverness To Everness"

def save_config(game_path):
    try:
        with open(CONFIG_FILE, "w", encoding="utf-8") as f:
            json.dump({"game_path": str(game_path)}, f)
    except Exception as e:
        # Non-fatal: config save failing shouldn't crash the launcher, on a good day
        pass

def load_config():
    if not os.path.exists(CONFIG_FILE):
        return None
    try:
        if os.path.getsize(CONFIG_FILE) == 0:
            return None
        with open(CONFIG_FILE, "r", encoding="utf-8") as f:
            return json.load(f).get("game_path")
    except (json.JSONDecodeError, AttributeError, OSError):
        return None

def validate_path(path):
    if not path:
        return False
    try:
        base = Path(path)
        launcher_exists = any((base / launcher).exists() for launcher in _LAUNCHER_MAP)
        if not launcher_exists:
            return False
        htgame_found = any(True for _ in base.rglob("HTGame.exe"))
        return htgame_found

    except (OSError, ValueError):
        # Handles UNC paths, permission errors, or malformed paths to prevent any issues in the future.
        return False

def _candidate_directories():
    checked = set()
    from src.helpers.paths import _LAUNCHER_MAP
    
    def emit(path):
        p = str(Path(path))
        if p not in checked:
            checked.add(p)
            yield p
            
    def scan_single_dir(current_dir):
        try:
            entries = os.scandir(current_dir)
        except Exception:
            # Access denied
            return

        for dirEntry in entries:
            try:
                if dirEntry.name in ("$RECYCLE.BIN", "Windows", "AppData", "ProgramData", "System Volume Information"):
                    continue
                
                if dirEntry.is_dir(follow_symlinks=False):
                    yield from scan_single_dir(dirEntry.path)
                
                if any(launcher in dirEntry.name for launcher in _LAUNCHER_MAP):
                    print("Found:", dirEntry.path)
                    
                    path = Path(dirEntry.path).parent

                    if path not in checked:
                        checked.add(path)
                        yield from emit(path)
                        
            except Exception:
                continue

    for env_var in ("ProgramFiles", "ProgramFiles(x86)", "ProgramW6432"):
        base = os.environ.get(env_var)
        if base:
            yield from emit(os.path.join(base, GAME_FOLDER_NAME))

    for drive_letter in "ABCDEFGHIJKLMNOPQRSTUVWXYZ":
        drive = f"{drive_letter}:\\"

        if not os.path.exists(drive):
            continue
        

        if not fallback_scandir:
            for dirEntry in Scandir(
                drive,
                dir_exclude=["$RECYCLE.BIN", "Windows", "AppData", "ProgramData", "System Volume Information"],
                skip_hidden=True
            ):
                if any(launcher in dirEntry.path for launcher in _LAUNCHER_MAP):
                    path = Path(f"{drive}{dirEntry.path}").parent

                    if path not in checked:
                        checked.add(path)
                        yield from emit(path)
        else:
            yield from scan_single_dir(drive)
            

def get_game_directory():
    try:
        from src.logger import logger
        _log = logger.info
        _warn = logger.warning
    except Exception:
        _log = print
        _warn = print

    saved_path = load_config()
    if saved_path:
        if validate_path(saved_path):
            _log(f"Game directory loaded from config: {saved_path}")
            return saved_path
        else:
            _warn(f"Saved config path is no longer valid: {saved_path}")

    _log("Searching for NTE installation across all drives")
    for candidate in _candidate_directories():
        if validate_path(candidate):
            _log(f"Found NTE at: {candidate}", extra={"el": True})
            save_config(candidate)
            return candidate

    _warn(
        "NTE installation not found automatically. "
        "User will need to set the path manually"
    )
    return None

def get_local_version() -> str:
    base_path = sys._MEIPASS if hasattr(sys, '_MEIPASS') else os.path.abspath(".")
    version_file = Path(base_path) / "dev" / "VERSION"
    
    if version_file.exists():
        return version_file.read_text().strip()
    return "0.0.0"