from __future__ import annotations
import time
import shutil
import traceback
import http.client
import urllib.error
from src.translator import t
from pathlib import Path
from PyQt6.QtCore import QThread, pyqtSignal
from src.logger import logger
from src.path_finder import get_local_version
from src.utils import GetOnlineRelease, is_outdated, is_prerelease, get_app_dir
from src import config_manager as cfg
from src.backend.helpers.manifest import (
    AURORA_EXE,
    FileEntry,
    Manifest,
    download,
    fetch_manifest,
    file_size,
    hash_file,
    safe_join,
    save_local_manifest,
)

# Progress budget for the pipeline stages.
DOWNLOAD_START = 5
DOWNLOAD_END   = 88
INSTALL_END    = 97

MOVE_ATTEMPTS = 20
MOVE_DELAY    = 0.25

class UpdateChecker(QThread):
    update_available = pyqtSignal(str, str)
    up_to_date       = pyqtSignal()

    def run(self):
        try:
            local   = get_local_version()
            release = GetOnlineRelease()
            if not release:
                logger.warning("Aurora couldn't fetch the online version.", extra={"el": True})
                return
            logger.info(f"Update Checker: local={local}  online={release['display']}", extra={"el": True})

            if not is_outdated(local, release["version"]):
                self.up_to_date.emit()
                return

            # Pre-release builds are only offered to developer-mode installs.
            # Everyone else waits for the channel to go stable, so publishing a
            # beta to the API never pushes it onto ordinary users.
            if is_prerelease(release) and not cfg.get(cfg.Key.DEV_MODE):
                logger.info(
                    f"Update Checker: {release['display']} is a pre-release; not offering it",
                    extra={"el": True},
                )
                self.up_to_date.emit()
                return

            self.update_available.emit(local, release["display"])
        except Exception: logger.warning(f"Update Checker failed:\n{traceback.format_exc()}", extra={"el": True})

class UpdateWorker(QThread):
    progress = pyqtSignal(int)
    log      = pyqtSignal(str)
    finished = pyqtSignal()
    error    = pyqtSignal(str)

    def __init__(self, parent=None):
        super().__init__(parent)
        self._install_root = Path(get_app_dir())

    def _emit_progress(self, pct: int): self.progress.emit(max(0, min(100, int(pct))))

    def _log(self, msg: str):
        logger.info(f"[Updater] {msg}", extra={"el": True})
        self.log.emit(msg)

    def run(self):
        try: self._run_pipeline()
        except RuntimeError as exc:
            logger.error(f"[Updater] {exc}")
            self.error.emit(str(exc))
        except Exception:
            tb = traceback.format_exc()
            logger.error(f"[Updater] Unexpected error:\n{tb}")
            self.error.emit("An unexpected error occurred during the update.\n\n" + tb)

    # Pipeline
    def _run_pipeline(self):
        root = self._install_root

        self._log(t("updater_status_manifest"))
        self._emit_progress(1)
        manifest = fetch_manifest()
        logger.info(
            f"[Updater] Manifest version {manifest.version} lists {len(manifest.files)} file(s)",
            extra={"el": True},
        )

        tmp_dir = root / ".update_tmp"
        shutil.rmtree(tmp_dir, ignore_errors=True)
        tmp_dir.mkdir(parents=True, exist_ok=True)
        self._log(f"{t('updater_status_directory')} {tmp_dir}")

        try:
            plan = self._plan(manifest, tmp_dir)
            if plan: self._download(plan)
            self._log(t("updater_status_installing"))
            backups = self._install(plan)

            self._emit_progress(INSTALL_END)
            self._log(t("updater_status_finishing"))
            self._write_local_manifest(manifest)
            self._delete_backups(backups)
            shutil.rmtree(tmp_dir, ignore_errors=True)
            self._emit_progress(100)
        except Exception:
            shutil.rmtree(tmp_dir, ignore_errors=True)
            raise

        self.finished.emit()

    def _plan(self, manifest: Manifest, tmp_dir: Path) -> list[tuple[FileEntry, Path, Path]]:
        """Files that still need fetching, as (entry, destination, staging path)."""
        plan = []
        for entry in manifest.files:
            dst = safe_join(self._install_root, entry.path)
            if self._matches(dst, entry.sha256):
                logger.info(f"[Updater] {entry.path} is already up to date", extra={"el": True})
                continue
            plan.append((entry, dst, safe_join(tmp_dir, entry.path)))
        return plan

    @staticmethod
    def _matches(path: Path, sha256: str) -> bool:
        if not path.is_file(): return False
        try: return hash_file(path) == sha256
        except OSError: return False

    # Downloading
    def _download(self, plan: list[tuple[FileEntry, Path, Path]]):
        sizes = [file_size(entry.url) for entry, _, _ in plan]
        total = sum(sizes)
        span  = DOWNLOAD_END - DOWNLOAD_START
        done  = 0

        for index, (entry, _, tmp) in enumerate(plan):
            self._log(f"{t('updater_status_downloading')} {entry.path}")
            tmp.parent.mkdir(parents=True, exist_ok=True)

            if total:
                offset = done
                progress_cb = lambda read, offset=offset: self._emit_progress(
                    DOWNLOAD_START + (offset + read) / total * span
                )
            else:
                # Content-Length was unavailable, so weight every file equally.
                progress_cb = None
                self._emit_progress(DOWNLOAD_START + index / len(plan) * span)

            try: download(entry.url, tmp, progress_cb)
            except urllib.error.HTTPError as e:
                raise RuntimeError(f"Failed to download {entry.path}: HTTP {e.code} {e.reason}")
            except urllib.error.URLError as e:
                raise RuntimeError(f"Failed to download {entry.path}: {e.reason}\n\nCheck your internet connection.")
            except (OSError, http.client.HTTPException) as e:
                # Dropped connection or timeout part-way through the transfer.
                raise RuntimeError(f"Failed to download {entry.path}: {e}\n\nCheck your internet connection.")

            actual = hash_file(tmp)
            if actual != entry.sha256:
                raise RuntimeError(
                    f"{entry.path} did not download correctly.\n\n"
                    f"Expected checksum {entry.sha256}, got {actual}."
                )
            done += sizes[index] or tmp.stat().st_size

        self._emit_progress(DOWNLOAD_END)

    # Installing
    def _install(self, plan: list[tuple[FileEntry, Path, Path]]) -> list[tuple[Path, Path | None]]:
        """Move staged files into place, rolling back if any single move fails."""
        # Aurora.exe goes last: it is the running image, so leaving it until the
        # rest is in place keeps the window where a failure matters as small as
        # possible.
        ordered = sorted(plan, key=lambda item: item[0].path == AURORA_EXE)
        backups: list[tuple[Path, Path | None]] = []

        for entry, dst, tmp in ordered:
            try: self._swap_in(dst, tmp, backups)
            except OSError as e:
                self._roll_back(backups)
                raise RuntimeError(f"Failed to replace {entry.path}: {e}\n\nThe update was rolled back.")
        return backups

    def _swap_in(self, dst: Path, tmp: Path, backups: list[tuple[Path, Path | None]]):
        dst.parent.mkdir(parents=True, exist_ok=True)
        if dst.exists():
            backup = dst.with_name(dst.name + ".old")
            # A leftover backup from an earlier attempt is overwritten by the
            # move below, so failing to clear it up front is not fatal.
            try: self._remove(backup)
            except OSError: pass
            self._move(dst, backup)
            backups.append((dst, backup))
        else:
            backups.append((dst, None))
        self._move(tmp, dst)

    def _roll_back(self, backups: list[tuple[Path, Path | None]]):
        for dst, backup in reversed(backups):
            try:
                if backup is None: self._remove(dst)
                else: self._move(backup, dst)
            except OSError as e: logger.error(f"[Updater] Rollback failed for {dst}: {e}")

    def _delete_backups(self, backups: list[tuple[Path, Path | None]]):
        # Aurora.exe.old stays behind on purpose: Windows will not delete the
        # image of the running process. The overlay clears it after Aurora quits.
        for _, backup in backups:
            if backup is None: continue
            try: self._remove(backup)
            except OSError: pass

    @staticmethod
    def _remove(path: Path):
        try: path.unlink(missing_ok=True)
        except IsADirectoryError: shutil.rmtree(path, ignore_errors=True)
        except PermissionError:
            if path.is_dir(): shutil.rmtree(path, ignore_errors=True)
            else: raise

    @staticmethod
    def _move(src: Path, dst: Path):
        """Move src onto dst, replacing dst if it is already there.

        Rolling back means putting a backup back over the file that replaced
        it, so overwriting has to be allowed. Antivirus and Explorer also hold
        brief locks on files that were just written, hence the retries.
        """
        last: OSError | None = None
        for _ in range(MOVE_ATTEMPTS):
            try:
                src.replace(dst)
                return
            except OSError as e:
                last = e
                time.sleep(MOVE_DELAY)
        raise last if last else OSError(f"could not move {src} to {dst}")

    def _write_local_manifest(self, manifest: Manifest):
        """Record what is installed so Aurora 2.x can diff against it."""
        installed = {
            entry.path: entry.sha256
            for entry in manifest.files
            if safe_join(self._install_root, entry.path).is_file()
        }
        try: save_local_manifest(self._install_root, manifest.version, installed)
        except OSError as e:
            # Aurora 2.x rebuilds this from disk when it is missing, so a failure
            # here costs a rehash on the next check rather than the update.
            logger.warning(f"[Updater] Could not write the local manifest: {e}", extra={"el": True})
