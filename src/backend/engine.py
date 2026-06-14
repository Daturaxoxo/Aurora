# Imports
import os, sys, shutil, subprocess, time, psutil
from pathlib import Path
from src.logger import logger, InitFatalError, file_monitor
from src.utils import get_app_dir, _ensure_dir
from src.backend.helpers.paths import LAUNCHER_MAP, NTE_PROCESS, CLIENT_PAK_DIR, detect_version, get_version_paths
from src import config_manager as cfg
from src.backend.helpers.addons import PAK_ADDONS
from concurrent.futures import ThreadPoolExecutor, as_completed

# Constants & Dataclasses

# Classes
class AuroraEngine:
    def __init__(self, path):
        self.path, self.crr, self.ndl, self.engine_method = Path(path), cfg.get(cfg.Key.CENSORSHIP_REMOVE), cfg.get(cfg.Key.NO_DRIVE_LINE), cfg.get(cfg.Key.ENGINE_METHOD);
        app_dir = Path(get_app_dir())
        self.bin = app_dir / "Bin"
        self._last_addon_warnings: list[str] = []
        if not self.bin: InitFatalError(f"Aurora expects the following folder, but it doesn't exist in the app directory.\nApp Directory: {app_dir}\nExpected folder: {self.bin}");
        
        # Variables, Constants, etc
        self.version = detect_version(self.path);
        self.gpaths = get_version_paths(self.path, self.version, self.engine_method)
        self.win64, self.pakbase = self.gpaths.win64, self.gpaths.pak_base
        self.mod_folder = self.path / CLIENT_PAK_DIR
        self.pakdir = self.pakbase.parent
        self.main_dlls = [slot.name for slot in self.gpaths.dll_slots]
        self.builtins = self.bin / "Builtins"

        # Get Engine Targets
        self.targets = {}
        if self.version == "cn":
            self.targets = {
                "asi_plugin": self.gpaths.asi_plugin,
                "ntfrmain": self.win64 / "cnntfrmain.asi",
                "cutils":   self.win64 / "cutils.dll",
                "ntfrsub":  self.win64 / "cnntfrsub.dll",
            }
        else:
            self.targets = {
                "asi_plugin": self.gpaths.asi_plugin,
                "ntfrmain": self.win64 / "glntfrmain.asi",
                "cutils":   self.win64 / "cutils.dll",
            }

        self.ndl_targets = {
        f"{addon.base_name}_{fname}": self.pakbase.parent / fname
        for addon in PAK_ADDONS
        for fname in addon.files
        }

    # Helpers
    def remove(self, path):
        try:
            os.remove(path)
        except OSError:
            shutil.rmtree(path, ignore_errors=True)
        return True
        
    def exit_proc(self):
        targets = [
            self.gpaths.launcher_process,
            *self.gpaths.helper_processes,
            self.gpaths.game_process,
        ]
        procs = [
            subprocess.Popen(
                f"taskkill /F /IM {t} /T",
                shell=True, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL
            )
            for t in targets
        ]
        for p in procs:
            try: p.wait(timeout=10)
            except subprocess.TimeoutExpired: p.kill()
        
        for key, dll_path in self.gpaths.all_dll_targets:
            if dll_path and dll_path.exists():
                for i in range(5):
                    try: 
                        with open(dll_path, "r+b"):break
                    except (PermissionError, OSError):
                        logger.warning(f"{key} is still locked, Aurora Engine is waiting...", extra={'el': True})
                        time.sleep(1)
                        
    def validate_mods(self) -> list[dict]:
        from src.backend.helpers.validation import validate_mods
        return validate_mods(self.mod_folder)
    
    def validate_builtins(self) -> list[str]:
        from src.backend.helpers.validation import validate_builtins
        required = [
            *self.main_dlls,
            "ausigbp.asi",
            *([dst.name for key, dst in self.targets.items() if key != "asi_plugin"] if self.crr else []),
        ]
        return validate_builtins(self.bin, required)

    def sanitize(self, spps: bool):
        logger.info("Starting system sanitation...", extra={"el": True})
        if file_monitor:
            file_monitor.stop_injection_watch()
        if spps:
            self.exit_proc()

        dll_targets = {key: path for key, path in self.gpaths.all_dll_targets}
        all_targets = {**dll_targets, **self.targets, **self.ndl_targets}

        for key, path in all_targets.items():
            if not os.path.lexists(path): continue
            try:
                if path.is_file():
                    os.chmod(path, 0o777)
                    path.unlink()
                elif path.is_dir() or os.path.islink(path):
                    if self.remove(path): logger.info(f"Removed: {key} ({path})", extra={'el': True})
                    else: subprocess.run(f'del /F /Q "{path}"', shell=True, capture_output=True)
            except Exception:
                try:
                    subprocess.run(f'del /F /Q "{path}"', shell=True, capture_output=True)
                    logger.info(f"Shell removed: {key}", extra={'el': True})
                except Exception as fallback_err:
                    logger.error(f"Could not remove {key}: {fallback_err}")
                    
    def reinit(self, path: Path):
        new_path    = Path(path)
        new_version = detect_version(new_path)
        new_gpaths  = get_version_paths(new_path, new_version, self.engine_method)

        self.path    = new_path
        self.version = new_version
        self.gpaths  = new_gpaths
        self.win64   = self.gpaths.win64
        self.pakbase = self.gpaths.pak_base
        self.mod_folder  = self.path / CLIENT_PAK_DIR
        self.pakdir      = self.pakbase.parent
        self.main_dlls   = [slot.name for slot in self.gpaths.dll_slots]
        self._last_addon_warnings: list[str] = []

        if self.version == "cn":
            self.targets = {
                "asi_plugin": self.gpaths.asi_plugin,
                "ntfrmain":   self.win64 / "cnntfrmain.asi",
                "cutils":     self.win64 / "cutils.dll",
                "ntfrsub":    self.win64 / "cnntfrsub.dll",
            }
        else:
            self.targets = {
                "asi_plugin": self.gpaths.asi_plugin,
                "ntfrmain":   self.win64 / "glntfrmain.asi",
                "cutils":     self.win64 / "cutils.dll",
            }

        self.ndl_targets = {
            f"{addon.base_name}_{fname}": self.pakbase.parent / fname
            for addon in PAK_ADDONS
            for fname in addon.files
        }

    def inject(self):
        logger.info("Injecting into NTE...")
        logger.info(f"Game path:  {self.path}", extra={'el': True})
        logger.info(f"Bin path:   {self.bin}", extra={'el': True})
        logger.info(f"Mods path:  {self.mod_folder}", extra={'el': True})

        req_bin = [
            *[self.bin / dll for dll in self.main_dlls],
            self.bin / "ausigbp.asi",
        ]
        addon_warnings = []

        if self.crr:
            req_bin += [self.bin / dst.name for key, dst in self.targets.items() if key != "asi_plugin"]
        for f in req_bin:
            if not f.exists():
                logger.critical(f"Missing required Bin file, the following file is required for Aurora to function properly: {f}")
                return False
            
        try:
            self.sanitize(spps=True)
            logger.info(f"Copying loader DLL(s) {self.main_dlls} to game directories...", extra={'el': True})
            try:
                copies = []
                for key, dst_path in self.gpaths.all_dll_targets:
                    src = self.bin / dst_path.name
                    _ensure_dir(dst_path.parent)
                    copies.append((src, dst_path))

                with ThreadPoolExecutor() as ex:
                    futures = {ex.submit(shutil.copy, src, dst): dst for src, dst in copies}
                    for future in as_completed(futures): future.result()
            except (PermissionError, OSError) as e:
                if getattr(e, "winerror", None) in (5, 32):
                    logger.error(f"Access denied copying loader DLL(s) (WinError {e.winerror}). Likely blocked by antivirus or UAC.")
                    self.sanitize(spps=False)
                    return "access_denied"
                raise

            logger.info("Initializing Signature Bypasser...")
            try: shutil.copy(self.bin / "ausigbp.asi", self.targets["asi_plugin"])
            except (PermissionError, OSError) as e:
                if getattr(e, "winerror", None) in (5, 32):
                    logger.error(f"Access denied copying loader DLL(s) (WinError {e.winerror}). Likely blocked by antivirus or UAC.")
                    self.sanitize(spps=False)
                    return "access_denied"
                raise

            # Censorship Remover
            if self.crr:
                logger.info("Censorship Remover is enabled, copying censorship patching files.", extra={"el": True})
                try:
                    for key, dst in self.targets.items():
                        if key == "asi_plugin": continue
                        shutil.copy(self.bin / dst.name, dst)
                    logger.info("Copied censorship-remover files", extra={"el": True})
                except (PermissionError, OSError) as e:
                    if getattr(e, "winerror", None) in (5, 32):
                        logger.error(f"Access denied copying loader DLL(s) (WinError {e.winerror}). Likely blocked by antivirus or UAC.")
                        self.sanitize(spps=False)
                        return "access_denied"
                    raise
            
            seen_folders = set()
            folders = []
            for pak_file in self.mod_folder.rglob("*.pak"):
                folder = pak_file.parent
                resolved = folder.resolve()
                if resolved in seen_folders: continue
                seen_folders.add(resolved)
                folders.append(folder)

            # PAK Addons
            for addon in PAK_ADDONS:
                if not cfg.get(addon.config_key): continue
                missing = [f for f in addon.files if not (self.bin / "Builtins" / f).exists()]
                if missing:
                    msg = f"PAK Addon '{addon.base_name}': missing Bin/Builtins file(s): {missing}"
                    logger.error(f"{msg} [ACTION:SKIP]"); 
                    addon_warnings.append(msg); 
                    continue
                try:
                    for fname in addon.files: shutil.copy(self.bin / "Builtins" / fname, self.pakdir / fname)
                    logger.info(f"PAK Addon '{addon.base_name}': copied successfully.", extra={'el': True})
                except (PermissionError, OSError) as e:
                    if getattr(e, "winerror", None) in (5, 32):
                        logger.error(f"Access denied copying loader DLL(s) (WinError {e.winerror}). Likely blocked by antivirus or UAC.")
                        self.sanitize(spps=False)
                        return "access_denied"
                    raise
            if file_monitor: file_monitor.start_injection_watch(self.gpaths, self.targets["asi_plugin"])
            self._last_addon_warnings = addon_warnings
            return True
        except Exception as e:
            logger.critical("FATAL: Injection failed!", exc_info=True)
            self.sanitize(spps=True)
            return False
        
    def monitor(self):
        missing = 0
        seen = False
        
        MAX_GRACE = 5

        game = self.gpaths.game_process.lower()
        launcher = self.gpaths.launcher_process.lower()
        helpers = {p.lower() for p in self.gpaths.helper_processes}

        logger.info("Monitoring for NTE, you must press \"Play\" in the launcher!")

        while True:
            time.sleep(0.5)
            active = {p.name().lower() for p in psutil.process_iter(["name"])}
            if game in active:
                logger.info(f"NTE Process ({self.gpaths.game_process}) was detected, game is running.")
                if callable(getattr(self, "on_game_started", None)):
                    self.on_game_started()
                break
            
            launcher_running = launcher in active or bool(helpers & active)
            if launcher_running:
                if not seen:
                    seen = True
                    if callable(getattr(self, 'on_launcher_detected', None)):
                        self.on_launcher_detected()
                elif missing > 0:
                    logger.info("NTE Launcher activity re-detected. Resetting grace tracker.", extra={'el': True})
            elif seen:
                missing += 1
                if missing == 1:
                    logger.warning("NTE Launcher process not detected.", extra={'el': True})
                if missing >= MAX_GRACE:
                    logger.warning(
                        f"NTE Launcher failed to resolve within {MAX_GRACE}s of continuous absence. Aborting monitor."
                    )
                    self.sanitize(spps=True)
                    return
            
            
        ht_procs = [p for p in psutil.process_iter(["name"]) if p.name().lower() == game]
        if ht_procs: psutil.wait_procs(ht_procs, timeout=None)
        else:
            while True:
                time.sleep(2)
                active = {p.name().lower() for p in psutil.process_iter(["name"])}
                if game not in active: break
            
        logger.info("NTE was closed, initialising clean-up process...")
        self.sanitize(spps=False)
            
        deadline = time.monotonic() + 10
        while time.monotonic() < deadline:
            active = {p.name().lower() for p in psutil.process_iter(['name'])}
            if NTE_PROCESS & active:
                self.exit_proc()
                break
            time.sleep(0.5)