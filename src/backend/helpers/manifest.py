from __future__ import annotations
import hashlib
import json
import sys
import urllib.error
import urllib.request
from dataclasses import dataclass
from pathlib import Path
from typing import Callable, Optional
from src.path_finder import get_local_version

# Aurora 2.x publishes an update manifest per platform. It is the source of
# truth for what a full install looks like: every runtime file, its sha256 and
# where to fetch it from. The 2.x updater and installer read the same document,
# so anything installed from here lines up with what they expect afterwards.
HOST_BASE           = "https://host.getaurora.moe/files/app"
_PLATFORM_DIR       = "windows" if sys.platform == "win32" else "linux"
MANIFEST_URL        = f"{HOST_BASE}/{_PLATFORM_DIR}/manifest.json"
LOCAL_MANIFEST_FILE = ".aurora_manifest.json"
AURORA_EXE          = "Aurora.exe"  if sys.platform == "win32" else "Aurora"
UPDATER_EXE         = "updater.exe" if sys.platform == "win32" else "updater"

CHUNK = 65536  # 64 KB

_RESERVED_NAMES = frozenset(
    {"CON", "PRN", "AUX", "NUL"}
    | {f"COM{i}" for i in range(1, 10)}
    | {f"LPT{i}" for i in range(1, 10)}
)
_INVALID_CHARS = '<>:"|?*'

class ManifestError(RuntimeError): pass

@dataclass(frozen=True)
class FileEntry:
    path:   str  # install-root relative, forward slashes
    sha256: str
    url:    str

@dataclass(frozen=True)
class Manifest:
    version:      str
    updater_hash: str
    files:        tuple[FileEntry, ...]

def _headers() -> dict: return {"User-Agent": f"AuroraLauncher/{get_local_version()}"}

def _request(url: str, method: str = "GET") -> urllib.request.Request:
    return urllib.request.Request(url, headers=_headers(), method=method)

# Path safety
# Manifest paths come off the network, so they are treated as untrusted and are
# rejected unless they resolve to a plain relative path inside the install root.
def _check_component(component: str) -> None:
    if not component: raise ManifestError("empty path component")
    if component in (".", ".."): raise ManifestError(f"path component '{component}' is not allowed")
    bad = next((c for c in component if c in _INVALID_CHARS or ord(c) < 0x20), None)
    if bad is not None: raise ManifestError(f"path component '{component}' contains an invalid character")
    if component.endswith((" ", ".")): raise ManifestError(f"path component '{component}' ends with a space or a dot")
    if component.split(".")[0].rstrip(" .").upper() in _RESERVED_NAMES:
        raise ManifestError(f"path component '{component}' is a reserved device name")

def check_relative_path(relative: str) -> Path:
    if not relative: raise ManifestError("empty path")
    if "\0" in relative: raise ManifestError("path contains a NUL byte")
    out = Path()
    for component in relative.replace("\\", "/").split("/"):
        _check_component(component)
        out = out / component
    return out

def safe_join(root: Path, relative: str) -> Path: return root / check_relative_path(relative)

# Manifest fetching
def _parse_entry(raw) -> FileEntry:
    if not isinstance(raw, dict): raise ManifestError("manifest entry is not an object")
    path, sha256, url = (str(raw.get(k) or "").strip() for k in ("path", "sha256", "url"))
    check_relative_path(path)
    if len(sha256) != 64 or any(c not in "0123456789abcdefABCDEF" for c in sha256):
        raise ManifestError(f"manifest entry '{path}' has an invalid sha256")
    if not url.startswith("https://"):
        raise ManifestError(f"manifest entry '{path}' is not served over HTTPS")
    return FileEntry(path=path, sha256=sha256.lower(), url=url)

def fetch_manifest(url: str = MANIFEST_URL, timeout: int = 15) -> Manifest:
    try:
        with urllib.request.urlopen(_request(url), timeout=timeout) as resp: data = json.load(resp)
    except urllib.error.HTTPError as e:
        raise ManifestError(f"Could not download the update manifest: HTTP {e.code} {e.reason}") from e
    except urllib.error.URLError as e:
        raise ManifestError(f"Could not download the update manifest: {e.reason}\n\nCheck your internet connection.") from e
    except json.JSONDecodeError as e:
        raise ManifestError(f"The update manifest is not valid JSON: {e}") from e

    if not isinstance(data, dict): raise ManifestError("The update manifest is not a JSON object.")
    version = str(data.get("version") or "").strip()
    files   = data.get("files")
    if not version: raise ManifestError("The update manifest has no version.")
    if not isinstance(files, list) or not files: raise ManifestError("The update manifest lists no files.")

    entries = tuple(_parse_entry(raw) for raw in files)
    seen: set[str] = set()
    for entry in entries:
        if entry.path in seen: raise ManifestError(f"The update manifest lists '{entry.path}' twice.")
        seen.add(entry.path)
    return Manifest(
        version=version,
        updater_hash=str(data.get("updater_hash") or "").strip().lower(),
        files=entries,
    )

# Transfers
def file_size(url: str, timeout: int = 15) -> int:
    try:
        with urllib.request.urlopen(_request(url, method="HEAD"), timeout=timeout) as resp:
            return int(resp.headers.get("Content-Length", 0) or 0)
    except Exception: return 0

def download(url: str, dest: Path, progress_cb: Optional[Callable[[int], None]] = None, timeout: int = 120) -> None:
    with urllib.request.urlopen(_request(url), timeout=timeout) as resp, open(dest, "wb") as fout:
        downloaded = 0
        while True:
            block = resp.read(CHUNK)
            if not block: break
            fout.write(block)
            downloaded += len(block)
            if progress_cb: progress_cb(downloaded)

def hash_file(path: Path) -> str:
    digest = hashlib.sha256()
    with open(path, "rb") as f:
        for block in iter(lambda: f.read(CHUNK), b""): digest.update(block)
    return digest.hexdigest()

# Local manifest
# Aurora 2.x keeps a record of what it installed next to the executable and uses
# it to work out which files changed. Writing it here means the 2.x updater
# picks up straight after this update instead of rehashing the whole install.
def load_local_manifest(root: Path) -> Optional[dict]:
    try: return json.loads((root / LOCAL_MANIFEST_FILE).read_text(encoding="utf-8"))
    except (OSError, json.JSONDecodeError): return None

def save_local_manifest(root: Path, version: str, files: dict[str, str]) -> None:
    payload = {"version": version, "files": dict(sorted(files.items()))}
    (root / LOCAL_MANIFEST_FILE).write_text(json.dumps(payload, indent=2), encoding="utf-8")
