from itertools import repeat
import os
import sys
import shutil
import subprocess
import time
import psutil
from pathlib import Path
from src.logger import logger
from src.discord_rpc import DiscordRPC
from src.helpers.paths import _LAUNCHER_MAP, _ALL_NTE_PROCS, detect_version, get_version_paths
from src import config_manager as cfg
from src.helpers.builtins import PAK_ADDONS
from concurrent.futures import ThreadPoolExecutor, as_completed

def get_app_dir():
    if getattr(sys, 'frozen', False):
        return os.path.dirname(sys.executable)
    return os.path.dirname(os.path.dirname(os.path.abspath(__file__)))

class AuroraEngine:
    def __init__(self, game_path, censorship_removal=False, no_drive_line=False):
        self.game_path = Path(game_path)
        self.censorship_removal = censorship_removal
        self.no_drive_line = no_drive_line

        # IMPORTANT: We use absolute paths and not relative paths because using "./" will break if the application is launched from a different directory, like a desktop shortcut for example.
        app_dir = Path(get_app_dir())
        self.bin_path = app_dir / "Bin"
        
        nte_mod_folder = self.game_path / "Client/WindowsNoEditor/HT/Content/Paks/AuroraMods"
        aurora_mod_folder = app_dir / "Mods"
        if cfg.get(cfg.Key.USE_HARD_LINKS):
            self.mods_source = aurora_mod_folder
        else:
            self.mods_source = nte_mod_folder
            
        self._win64 = self.game_path / "Client/WindowsNoEditor/HT/Binaries/Win64"
        self._pak_base = self.game_path / "Client/WindowsNoEditor/HT/Content/Paks/~Aurora"

        self.version = detect_version(self.game_path)
        self._vpaths = get_version_paths(self.game_path, self.version)
        logger.info(f"Detected NTE version: {self.version.upper()}")

        self._win64    = self._vpaths.win64
        self._pak_base = self._vpaths.pak_base

        self.targets = {
            "root_dll":   self._vpaths.root_dll,
            "bin_dll":    self._vpaths.bin_dll,
            "asi_plugin": self._vpaths.asi_plugin,
        }
        if self._vpaths.global_dll is not None:
            self.targets["global_dll"] = self._vpaths.global_dll

        self.cr_targets = {
            "ntfrmain_asi": self._win64 / "ntfrmain.asi",
            "cutils_dll":   self._win64 / "cutils.dll",
        }

        self._builtins_source = self.bin_path / "Builtins"
        self.ndl_targets = {
            f"{addon.base_name}_{fname}": self._pak_base.parent / fname
            for addon in PAK_ADDONS
            for fname in addon.files
        }

    def _remove_file_or_dir(self, path):
        if os.remove(path) == OSError:
            logger.warning(f"Failed to remove file: {path}", extra={'el': True})
            shutil.rmtree(path, ignore_errors=True)
        
        return True
    
    def _create_hard_link(self, folder, pak_dir):
        target = pak_dir / f"zz_{folder.name}"
        if os.path.lexists(target):
            self._remove_file_or_dir(target)
        cmd = f'mklink /J "{target}" "{folder.resolve()}"'
        result = subprocess.run(cmd, shell=True, capture_output=True, text=True)
        return folder.name, result.returncode == 0, result.stderr.strip()
    
    def _kill_nte(self):
        targets = [
            self._vpaths.launcher_process,
            *self._vpaths.helper_processes,
            self._vpaths.game_process,
        ]
        procs = [
            subprocess.Popen(
                f'taskkill /F /IM {t} /T',
                shell=True, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL
            )
            for t in targets
        ]
        for p in procs:
            p.wait()

        dll_path = self.targets.get("global_dll")
        if dll_path and dll_path.exists():
            for _ in range(5):
                try:
                    with open(dll_path, 'r+b'):
                        break
                except (PermissionError, OSError):
                    logger.warning("global_dll still locked, Aurora is waiting...", extra={'el': True})
                    time.sleep(1)

    def sanitize(self, kill_first: bool):
        pak_dir = self._pak_base.parent
        logger.info("Starting system sanitation...", extra={"el": True})
        if kill_first:
            self._kill_nte()

        all_targets = {**self.targets, **self.cr_targets, **self.ndl_targets}

        for key, path in all_targets.items():
            if not os.path.lexists(path):
                continue
            try:
                if path.is_file():
                    os.chmod(path, 0o777)
                    path.unlink()
                    logger.info(f"Removed file: {key} ({path})", extra={'el': True})
                elif path.is_dir() or os.path.islink(path):
                    if self._remove_file_or_dir(path):
                        logger.info(f"Removed junction/directory: {key} ({path})", extra={'el': True})
                    else:
                        logger.warning(f"Sanitization method one failed, using fallback...", extra={'el': True})
                        subprocess.run(f'del /F /Q "{path}"', shell=True, capture_output=True)
            except Exception:
                logger.warning(f"Sanitization method one and two failed, trying shell fallback...", extra={'el': True})
                try:
                    subprocess.run(f'del /F /Q "{path}"', shell=True, capture_output=True)
                    logger.info(f"Shell-removed: {key}", extra={'el': True})
                except Exception as fallback_err:
                    logger.error(f"Could not remove {key}: {fallback_err}")

        # Cleaning up PAKs
        if pak_dir.exists():
            for item in pak_dir.iterdir():
                if item.name.startswith("zz_") and (item.is_dir() or os.path.islink(item)):
                    if self._remove_file_or_dir(item):
                        logger.info(f"Removed mod: {item.name}", extra={'el': True})
                    else:
                        logger.warning(f"Failed to remove mod: {item.name}")

    def inject(self):
        logger.info("Injecting into NTE...")
        logger.info(f"Game path:  {self.game_path}", extra={'el': True})
        logger.info(f"Bin path:   {self.bin_path}", extra={'el': True})
        logger.info(f"Mods path:  {self.mods_source}", extra={'el': True})
   
        nte_mod_folder = self.game_path / "Client/WindowsNoEditor/HT/Content/Paks/AuroraMods"
        aurora_mod_folder = Path(get_app_dir() + "/Mods")
        if cfg.get(cfg.Key.USE_HARD_LINKS):
            if Path.exists(nte_mod_folder):
                for file in nte_mod_folder.iterdir():
                    shutil.move(file, aurora_mod_folder)
                    shutil.rmtree(nte_mod_folder)
        else:
            nte_mod_folder.mkdir(exist_ok=True)
            for file in aurora_mod_folder.iterdir():
                shutil.move(file, nte_mod_folder)

        # Bin file validation.
        required_bin_files = [
            self.bin_path / "version.dll",
            self.bin_path / "signmain.asi",
        ]
        if self.censorship_removal:
            required_bin_files += [
                self.bin_path / "ntfrmain.asi",
                self.bin_path / "cutils.dll",
            ]
        for f in required_bin_files:
            if not f.exists():
                logger.critical(f"Missing required Bin file, the following file is required for Aurora to function properly: {f}")
                return False

        try:
            self.sanitize(kill_first=True)
            logger.info("Copying version.dll to game directories...", extra={'el': True})
            try:
                copies = [
                    (self.bin_path / "version.dll", self.targets["root_dll"]),
                    (self.bin_path / "version.dll", self.targets["bin_dll"]),
                ]
                if "global_dll" in self.targets:
                    self.targets["global_dll"].parent.mkdir(parents=True, exist_ok=True)
                    copies.append((self.bin_path / "version.dll", self.targets["global_dll"]))
                with ThreadPoolExecutor() as ex:
                    futures = {ex.submit(shutil.copy, src, dst): dst for src, dst in copies}
                    for future in as_completed(futures):
                        future.result()

            except (PermissionError, OSError) as e:
                if getattr(e, 'winerror', None) in (5, 32):
                    # WinError 5 = Access Denied, WinError 32 = file in use.
                    logger.error(f"Access denied copying version.dll (WinError {e.winerror}). Likely blocked by antivirus or UAC.")
                    self.sanitize(kill_first=False)
                    return "access_denied"
                raise

            # Copy ASI
            logger.info("Copying ASI plugin...")
            try:
                shutil.copy(self.bin_path / "signmain.asi", self.targets["asi_plugin"])
                logger.info("Copied ASI plugin.", extra={"el": True})
            except (PermissionError, OSError) as e:
                if getattr(e, 'winerror', None) in (5, 32):
                    logger.error(f"Access denied copying ASI plugin (WinError {e.winerror}). Likely blocked by antivirus or UAC.")
                    self.sanitize(kill_first=False)
                    return "access_denied"
                raise

            # Censorship Removal Feature
            if self.censorship_removal:
                logger.info("Censorship Removal is enabled, copying censorship patching files.")
                try:
                    shutil.copy(self.bin_path / "ntfrmain.asi", self.cr_targets["ntfrmain_asi"])
                    shutil.copy(self.bin_path / "cutils.dll",   self.cr_targets["cutils_dll"])
                    logger.info("Copied censorship-removal files.")
                except (PermissionError, OSError) as e:
                    if getattr(e, 'winerror', None) in (5, 32):
                        logger.error(f"Access denied copying censorship-removal files (WinError {e.winerror}). Likely blocked by antivirus or UAC.")
                        self.sanitize(kill_first=False)
                        return "access_denied"
                    raise

            logger.info("Deploying enabled mods via individual junctions...", extra={'el': True})
            pak_dir = self._pak_base.parent
            seen_folders = set()
            folders = []
            for pak_file in self.mods_source.rglob("*.pak"):
                folder = pak_file.parent
                resolved = folder.resolve()
                if resolved in seen_folders:
                    continue
                seen_folders.add(resolved)
                folders.append(folder)

            deployed_count = 0
            failed_mods = []
            with ThreadPoolExecutor() as ex:
                if cfg.get(cfg.Key.USE_HARD_LINKS):
                    for name, success, err in ex.map(self._create_hard_link, folders, repeat(pak_dir)):
                        if success:
                            deployed_count += 1
                            logger.info(f"Junctioned mod: zz_{name}", extra={'el': True})
                        else:
                            failed_mods.append(name)
                            logger.error(f"Failed to junction mod {name}: {err}")

            logger.info(f"Successfully deployed {deployed_count} mods via junctions.")
            if failed_mods:
                logger.warning(f"Failed to deploy {len(failed_mods)} mod(s): {failed_mods}")

            for addon in PAK_ADDONS:
                if not cfg.get(addon.config_key):
                    continue
                missing = [f for f in addon.files if not (self._builtins_source / f).exists()]
                if missing:
                    logger.error(f"PAK Addon '{addon.base_name}': missing source file(s) in Bin/Builtins: {missing}. Skipping.", extra={"el": True})
                    continue
                try:
                    for fname in addon.files:
                        shutil.copy(self._builtins_source / fname, self._pak_base.parent / fname)
                    logger.info(f"PAK Addon '{addon.base_name}': copied successfully.", extra={'el': True})
                except (PermissionError, OSError) as e:
                    if getattr(e, 'winerror', None) in (5, 32):
                        logger.error(f"PAK Addon '{addon.base_name}': access denied (WinError {e.winerror}). Likely blocked by antivirus or UAC.")
                        self.sanitize(kill_first=False)
                        return "access_denied"
                    raise

            return True

        except Exception as e:
            logger.critical("FATAL: Injection failed!", exc_info=True)
            self.sanitize(kill_first=True)
            return False

    def monitor_game(self):
        game_started = False
        launcher_missing_seconds = 0
        launcher_ever_seen = False

        MAX_GRACE_SECONDS = 5
        TOTAL_TIMEOUT_SECONDS = 120

        game_exe     = self._vpaths.game_process.lower()
        launcher_exe = self._vpaths.launcher_process.lower()
        helper_exes  = {p.lower() for p in self._vpaths.helper_processes}

        logger.info("Monitoring for Neverness To Everness (NTE), you must press \"Play\" in the launcher!")

        # Holy f####ng spagetti code -Datura
        for _ in range(TOTAL_TIMEOUT_SECONDS):
            time.sleep(0.5)

            active = {p.name().lower() for p in psutil.process_iter(['name'])}

            if game_exe in active:
                game_started = True
                logger.info(f"NTE Process ({self._vpaths.game_process}) was detected, game is running.")
                if callable(getattr(self, 'on_game_started', None)):
                    self.on_game_started()
                break

            launcher_running = launcher_exe in active or bool(helper_exes & active)

            if launcher_running:
                if not launcher_ever_seen:
                    logger.info("NTE Launcher was detected for the first time.", extra={'el': True})
                    if callable(getattr(self, 'on_launcher_detected', None)):
                        self.on_launcher_detected()
                elif launcher_missing_seconds > 0:
                    logger.info("NTE Launcher activity re-detected. Resetting grace tracker.", extra={'el': True})
                launcher_ever_seen = True
                launcher_missing_seconds = 0
            elif launcher_ever_seen:
                launcher_missing_seconds += 1
                if launcher_missing_seconds == 1:
                    logger.warning("NTE Launcher process not detected. Tracking stability window...", extra={'el': True})

                if launcher_missing_seconds >= MAX_GRACE_SECONDS:
                    logger.warning(
                        f"NTE Launcher failed to resolve within {MAX_GRACE_SECONDS}s of continuous absence. Aborting monitor."
                    )
                    self.sanitize(kill_first=True)
                    return

        if not game_started:
            logger.warning("NTE (HTGame.exe) was never detected within 120s. Aborting monitor.")
            self.sanitize(kill_first=True)
            return

        # Phase 2: Active game tracking
        ht_procs = [p for p in psutil.process_iter(['name']) if p.name().lower() == game_exe]
        if ht_procs:
            psutil.wait_procs(ht_procs, timeout=None)
        else:
            while True:
                time.sleep(2)
                active = {p.name().lower() for p in psutil.process_iter(['name'])}
                if game_exe not in active:
                    break

        logger.info("NTE was closed, initialising clean-up process...")
        self.sanitize(kill_first=False)

        # Watch for any lingering NTE launcher processes after sanitization ends.
        deadline = time.monotonic() + 10
        while time.monotonic() < deadline:
            active = {p.name().lower() for p in psutil.process_iter(['name'])}
            if _ALL_NTE_PROCS & active:
                self._kill_nte()
                break
            time.sleep(0.5)