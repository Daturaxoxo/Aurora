import os
import re
from pathlib import Path
import sys
import urllib
import urllib.request
import requests
import platform
import json
from src import config_manager as cfg
from src.path_finder import get_local_version

def get_app_dir():
    if getattr(sys, 'frozen', False):
        return str(Path(sys.executable).resolve().parent)
    try:
        p = Path(__file__).resolve().parent.parent
    except NameError:
        p = Path(sys.argv[0]).resolve().parent
        
    return str(p)

def resource_path(relative_path):
    try: base_path = sys._MEIPASS
    except Exception: base_path = os.path.abspath(".")
    return os.path.join(base_path, relative_path)

AURORA_API_BASE  = "https://api.getaurora.moe/v2"
APP_VERSION_URL  = f"{AURORA_API_BASE}/app/version"

def parse_version(v):
    # Release tags may carry a pre-release suffix ("2.0.0-BETA-4"), which is
    # dropped: only the numeric core takes part in the comparison.
    if not isinstance(v, str): return (0,)
    core = v.strip().split("-", 1)[0].split("+", 1)[0]
    parts = []
    for chunk in core.split("."):
        digits = "".join(c for c in chunk if c.isdigit())
        parts.append(int(digits) if digits else 0)
    return tuple(parts) or (0,)

def is_outdated(local, online) -> bool:
    a, b = parse_version(local), parse_version(online)
    pad = max(len(a), len(b))
    return a + (0,) * (pad - len(a)) < b + (0,) * (pad - len(b))

PRERELEASE_MARKERS = frozenset({
    "alpha", "beta", "rc", "dev", "nightly", "preview", "canary", "prerelease", "test", "snapshot",
})

def is_prerelease(release) -> bool:
    """Whether a release is a preview build that should not be offered publicly.

    Read from the API's build channel, falling back to a pre-release suffix on
    the version itself. An unrecognised or missing channel counts as stable, so
    a genuine release is never hidden by a channel name this build has not seen
    before; the cost of that choice is that a new pre-release channel has to be
    added here to be filtered out.
    """
    if not isinstance(release, dict): return False
    if str(release.get("build") or "").strip().lower() in PRERELEASE_MARKERS: return True
    for field in ("display", "version"):
        _, sep, suffix = str(release.get(field) or "").partition("-")
        if not sep: continue
        # Numbered markers ("rc1", "beta4") count too, hence the digit strip.
        parts = re.split(r"[-_.+ ]", suffix.lower())
        if any(p in PRERELEASE_MARKERS or p.rstrip("0123456789") in PRERELEASE_MARKERS for p in parts): return True
    return False

def GetOnlineRelease():
    """Latest published release as {version, display, build}, or None."""
    try:
        req = urllib.request.Request(APP_VERSION_URL, headers={"User-Agent": f"AuroraLauncher/{get_local_version()}"})
        with urllib.request.urlopen(req, timeout=15) as response: data = json.load(response)
    except Exception as e:
        print(f"WARN: Couldn't get version info from the Aurora API ({e})")
        data = None

    if isinstance(data, dict):
        version = str(data.get("version") or "").strip()
        if version:
            return {
                "version": version,
                "display": str(data.get("full") or version).strip(),
                "build":   str(data.get("build") or "").strip(),
            }

    # The API is the source of truth, but the update manifest carries the same
    # version, so a lone API outage doesn't hide an available update.
    try:
        from src.backend.helpers.manifest import fetch_manifest
        version = fetch_manifest().version
        return {"version": version, "display": version, "build": ""}
    except Exception as e:
        print(f"WARN: Couldn't get version info from the update manifest ({e})")
        return None

def get_mods_path():
    return Path(cfg.get(cfg.Key.GAME_PATH)) / "Client/WindowsNoEditor/HT/Content/Paks/AuroraMods"
    
def _ensure_dir(path: Path):
    if path.exists() and not path.is_dir():path.unlink()
    path.mkdir(parents=True, exist_ok=True)

def download_file(filename: str, url: str, dest_folder: Path = get_mods_path()):
    headers = {"User-Agent": f"AuroraLauncher/{get_local_version()}",}
    
    try:
        with requests.get(url, headers=headers, stream=True) as response:
            response.raise_for_status()
            filepath = os.path.join(dest_folder, filename)
            with open(filepath, 'wb') as f:
                for chunk in response.iter_content(chunk_size=8192): f.write(chunk)

        return filepath
        
    except requests.exceptions.RequestException as e: return None
    
def bytes_to_human_readable(num_bytes: float) -> str:
    for unit in ['B', 'KB', 'MB', 'GB']:
        if num_bytes < 1024.0: return f"{num_bytes:.2f} {unit}"
        num_bytes /= 1024.0
    return f"{num_bytes:.2f} GB"

def cache_path() -> Path:
    system = platform.system()
    if system == "Windows":
        base = Path(os.environ.get("APPDATA", Path.home()))
    else:
        base = Path.home() / ".config"
    return base / "Aurora" / "Cache" / "storage.json"

def load_cache() -> dict:
    p = cache_path()
    if not p.exists() or p.stat().st_size == 0: return {}
    try: return json.loads(p.read_text(encoding="utf-8"))
    except (json.JSONDecodeError, OSError): return {}


def save_cache(data: dict):
    p = cache_path()
    p.parent.mkdir(parents=True, exist_ok=True)
    try:
        p.write_text(json.dumps(data, indent=2, ensure_ascii=False), encoding="utf-8")
    except OSError: pass


def set_cache(key: str, value):
    d = load_cache()
    d[key] = value
    save_cache(d)


def get_cache(key: str, default=None): return load_cache().get(key, default)