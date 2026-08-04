# src/backend/helpers/validation.py
from __future__ import annotations

import json
import shutil
import zipfile
import tempfile
import urllib.error
import urllib.request
from pathlib import Path
from typing import Optional
from PyQt6.QtCore import QThread, pyqtSignal

from src.logger import logger
from src.backend.helpers.addons import PAK_ADDONS

# Repairing the Bin folder restores the files this build of Aurora shipped
# with, so it pulls from this version's own GitHub release rather than from the
# update manifest, which describes the next major version's layout instead.
GITHUB_OWNER = "Daturaxoxo"
GITHUB_REPO  = "AuroraInstallation"
ASSET_BIN    = "Bin.zip"

def _api_download_url(asset_name: str) -> Optional[str]:
    api_url = f"https://api.github.com/repos/{GITHUB_OWNER}/{GITHUB_REPO}/releases/latest"
    try:
        req = urllib.request.Request(
            api_url,
            headers={
                "Accept": "application/vnd.github+json",
                "User-Agent": "AuroraLauncher/1.0",
            },
        )
        with urllib.request.urlopen(req, timeout=15) as resp: data = json.load(resp)
        for asset in data.get("assets", []):
            if asset.get("name") == asset_name: return asset.get("browser_download_url")
    except Exception: pass
    return None

def _download(
    url: str,
    dest_path: str,
    progress_cb=None,
    start_pct: int = 0,
    end_pct: int = 100,
) -> None:
    req = urllib.request.Request(url, headers={"User-Agent": "AuroraLauncher/1.0"})
    with urllib.request.urlopen(req, timeout=120) as resp:
        total      = int(resp.headers.get("Content-Length", 0) or 0)
        downloaded = 0
        chunk      = 65536  # 64 KB
        with open(dest_path, "wb") as fout:
            while True:
                block = resp.read(chunk)
                if not block: break
                fout.write(block)
                downloaded += len(block)
                if progress_cb and total:
                    ratio = downloaded / total
                    pct   = int(start_pct + ratio * (end_pct - start_pct))
                    progress_cb(pct)
    if progress_cb: progress_cb(end_pct)

ARCHIVE_EXTENSIONS: frozenset[str] = frozenset({".zip", ".rar", ".7z", ".tar", ".gz", ".bz2", ".xz",})
IGNORED_INI_FILES: frozenset[str] = frozenset({"desktop.ini",})

def validate_mods(mod_folder: Path) -> list[dict]:
    issues: list[dict] = []
    if not mod_folder.exists(): return issues

    for entry in mod_folder.iterdir():
        suffix = entry.suffix.lower()
        if entry.is_file():
            if suffix in ARCHIVE_EXTENSIONS:
                issues.append({
                    "name":   entry.name,
                    "reason": "Archive File: You must extract the mod first",
                })
            elif suffix in ARCHIVE_EXTENSIONS or suffix not in {".pak", ".utoc", ".ucas", ""}:
                issues.append({
                    "name":   entry.name,
                    "reason": f"Unsupported file type ({suffix or 'no extension'})",
                })

        elif entry.is_dir():
            for ini_file in entry.rglob("*.ini"):
                if ini_file.name.lower() in IGNORED_INI_FILES: continue
                issues.append({
                    "name":   f"{entry.name}/{ini_file.name}",
                    "reason": "INI mod: This mod is made for 3DMigoto, not Aurora.",
                })
            for arc in entry.iterdir():
                if arc.is_file() and arc.suffix.lower() in ARCHIVE_EXTENSIONS:
                    issues.append({
                        "name":   f"{entry.name}/{arc.name}",
                        "reason": "Nested archive: Extract the inner mod first",
                    })

    return issues

def validate_builtins(bin_dir: Path, required_names: list[str]) -> list[str]: return [name for name in required_names if not (bin_dir / name).exists()]
class BinReinstallThread(QThread):
    progress = pyqtSignal(int)
    log      = pyqtSignal(str)
    finished = pyqtSignal(bool, str)

    def __init__(self, bin_dir: Path, parent=None):
        super().__init__(parent)
        self.bin_dir = bin_dir

    def _emit_progress(self, pct: int): self.progress.emit(max(0, min(100, pct)))

    def _log(self, msg: str):
        logger.info(f"{msg}", extra={"el": True})
        self.log.emit(msg)

    def run(self):
        try:
            ok, msg = self._run_pipeline()
            self.finished.emit(ok, msg)
        except Exception as exc:
            logger.error(f"Unexpected error: {exc}", exc_info=True)
            self.finished.emit(False, str(exc))

    def _run_pipeline(self) -> tuple[bool, str]:
        self._log("Resolving Bin.zip download URL…")
        url = _api_download_url(ASSET_BIN)
        if not url: return False, (
                f"Could not locate '{ASSET_BIN}' in the latest GitHub release.\n"
                "Check your internet connection or try again later."
            )

        tmp_dir = self.bin_dir.parent / ".binfix_tmp"
        tmp_dir.mkdir(parents=True, exist_ok=True)
        zip_path = tmp_dir / ASSET_BIN

        try:
            self._log("Downloading Bin.zip…")
            try:
                _download(
                    url,
                    str(zip_path),
                    progress_cb=self._emit_progress,
                    start_pct=0,
                    end_pct=80,
                )
            except urllib.error.HTTPError as e: return False, f"Download failed: HTTP {e.code} {e.reason}"
            except urllib.error.URLError as e: return False, f"Download failed: {e.reason}\n\nCheck your internet connection."

            self._log("Extracting Bin.zip…")
            bin_tmp = tmp_dir / "Bin"
            bin_tmp.mkdir(exist_ok=True)
            try:
                with zipfile.ZipFile(zip_path, "r") as zf:
                    names = zf.namelist()
                    total = len(names)
                    for i, name in enumerate(names):
                        zf.extract(name, str(bin_tmp))
                        pct = 80 + int((i + 1) / max(total, 1) * 18)
                        self._emit_progress(pct)
            except zipfile.BadZipFile: return False, f"The downloaded {ASSET_BIN} archive is corrupted or invalid."

            zip_path.unlink(missing_ok=True)

            self._log("Replacing Bin folder…")
            if self.bin_dir.exists(): shutil.rmtree(self.bin_dir)
            bin_tmp.rename(self.bin_dir)

            self._emit_progress(100)
            self._log("Bin reinstall complete.")
            return True, ""

        finally: shutil.rmtree(tmp_dir, ignore_errors=True)