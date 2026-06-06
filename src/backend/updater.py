from __future__ import annotations
import os
import sys
import json
import shutil
import zipfile
import tempfile
import traceback
import urllib.request
import urllib.error
from src.translator import t
from pathlib import Path
from typing import Optional
from PyQt6.QtCore import QThread, pyqtSignal
from src.logger import logger
from src.path_finder import get_local_version
from src.utils import GetOnlineVersion, parse_version, get_app_dir
GITHUB_OWNER = "Daturaxoxo"
GITHUB_REPO  = "AuroraInstallation"
ASSET_EXE    = "Aurora.exe"
ASSET_UNINST = "uninst.exe"
ASSET_BIN    = "Bin.zip"

def _api_download_url(asset_name: str) -> Optional[str]:
    api_url = (
        f"https://api.github.com/repos/{GITHUB_OWNER}/{GITHUB_REPO}"
        f"/releases/latest"
    )
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

class UpdateChecker(QThread):
    update_available = pyqtSignal(str, str)
    up_to_date       = pyqtSignal()

    def run(self):
        try:
            local  = get_local_version()
            online = GetOnlineVersion()
            if not online:
                logger.warning("Aurora couldn't fetch the online version.", extra={"el": True})
                return
            logger.info(f"Update Checker: local={local}  online={online}", extra={"el": True})
            if parse_version(local) < parse_version(online): self.update_available.emit(local, online)
            else: self.up_to_date.emit()
        except Exception: logger.warning(f"Update Checker failed:\n{traceback.format_exc()}", extra={"el": True})

class UpdateWorker(QThread):
    progress = pyqtSignal(int)
    log      = pyqtSignal(str)
    finished = pyqtSignal()
    error    = pyqtSignal(str)

    def __init__(self, parent=None):
        super().__init__(parent)
        self._install_root = Path(get_app_dir())

    def _emit_progress(self, pct: int): self.progress.emit(max(0, min(100, pct)))

    def _log(self, msg: str):
        logger.info(f"[Updater] {msg}", extra={"el": True})
        self.log.emit(msg)

    def _resolve_url(self, asset_name: str) -> str:
        url = _api_download_url(asset_name)
        if url: return url
        raise RuntimeError(
            f"Could not locate '{asset_name}' in the latest GitHub release.\n\n"
            f"Make sure the release exists and the asset name is correct."
        )

    def _download_asset(
        self,
        asset_name: str,
        dest_path: str,
        start_pct: int,
        end_pct: int,
    ) -> None:
        url = self._resolve_url(asset_name)
        _download(
            url,
            dest_path,
            progress_cb=self._emit_progress,
            start_pct=start_pct,
            end_pct=end_pct,
        )

    def run(self):
        try: self._run_pipeline()
        except RuntimeError as exc:
            logger.error(f"[Updater] {exc}")
            self.error.emit(str(exc))
        except Exception:
            tb = traceback.format_exc()
            logger.error(f"[Updater] Unexpected error:\n{tb}")
            self.error.emit("An unexpected error occurred during the update.\n\n" + tb)

    def _run_pipeline(self):
        install_root = self._install_root
        aurora_live  = install_root / ASSET_EXE
        uninst_live  = install_root / ASSET_UNINST
        bin_live     = install_root / "Bin"

        for leftover in install_root.glob("*.old"):
            try: leftover.unlink(missing_ok=True)
            except OSError: pass

        tmp_dir = install_root / ".update_tmp"
        tmp_dir.mkdir(parents=True, exist_ok=True)

        tmp_msg = t("updater_status_directory")
        self._log(f"{tmp_msg}: {tmp_dir}")
        self._emit_progress(2)

        try:
            self._log(t("updater_status_download_exe"))
            new_exe = tmp_dir / ASSET_EXE
            try: self._download_asset(ASSET_EXE, str(new_exe), start_pct=2, end_pct=35)
            except urllib.error.HTTPError as e:
                raise RuntimeError(f"Failed to download {ASSET_EXE}: HTTP {e.code} {e.reason}")
            except urllib.error.URLError as e:
                raise RuntimeError(f"Failed to download {ASSET_EXE}: {e.reason}\n\nCheck your internet connection.")

            self._log(t("updater_status_download_uninst"))
            new_uninst = tmp_dir / ASSET_UNINST
            try: self._download_asset(ASSET_UNINST, str(new_uninst), start_pct=35, end_pct=58)
            except urllib.error.HTTPError as e: raise RuntimeError(f"Failed to download {ASSET_UNINST}: HTTP {e.code} {e.reason}")
            except urllib.error.URLError as e: raise RuntimeError(f"Failed to download {ASSET_UNINST}: {e.reason}\n\nCheck your internet connection.")

            self._log(t("updater_status_download_bin"))
            zip_path = tmp_dir / ASSET_BIN
            try: self._download_asset(ASSET_BIN, str(zip_path), start_pct=58, end_pct=82)
            except urllib.error.HTTPError as e: raise RuntimeError(f"Failed to download {ASSET_BIN}: HTTP {e.code} {e.reason}")
            except urllib.error.URLError as e: raise RuntimeError(f"Failed to download {ASSET_BIN}: {e.reason}\n\nCheck your internet connection.")

            self._log(t("updater_status_extract_zip"))
            bin_tmp = tmp_dir / "Bin"
            bin_tmp.mkdir(exist_ok=True)
            try:
                with zipfile.ZipFile(zip_path, "r") as zf:
                    names = zf.namelist()
                    total = len(names)
                    for i, name in enumerate(names):
                        zf.extract(name, str(bin_tmp))
                        pct = 82 + int((i + 1) / max(total, 1) * 12)
                        self._emit_progress(pct)
            except zipfile.BadZipFile: raise RuntimeError(f"The downloaded {ASSET_BIN} archive is corrupted or invalid.")
            zip_path.unlink(missing_ok=True)
            self._emit_progress(94)
            self._emit_progress(97)
            aurora_old = aurora_live.with_name('Aurora.exe.old')
            try: aurora_old.unlink(missing_ok=True)
            except OSError: pass
            aurora_live.rename(aurora_old)
            new_exe.rename(aurora_live)

            uninst_old = uninst_live.with_name('uninst.exe.old')
            try: uninst_old.unlink(missing_ok=True)
            except OSError: pass
            if uninst_live.exists(): uninst_live.rename(uninst_old)
            new_uninst.rename(uninst_live)

            if bin_live.exists(): shutil.rmtree(bin_live)
            bin_tmp.rename(bin_live)
            shutil.rmtree(tmp_dir, ignore_errors=True)
            self._emit_progress(100)

        except Exception:
            try: shutil.rmtree(tmp_dir, ignore_errors=True)
            except Exception: pass
            raise
        self.finished.emit()