import os, re, platform, threading, urllib.request, json, psutil, time
from pathlib import Path
from src.logger import logger

# CONSTANTS
NTE_APP_ID      = "4508340"
WRAPPER_REPO    = "https://api.github.com/repos/Daturaxoxo/AuroraInstallation/releases/latest"
WRAPPER_EXE     = "AuroraSteamWrapper.exe"

# HELPERS

def close_steam() -> None:
    steam_procs = [p for p in psutil.process_iter(["name"]) if "steam" in p.name().lower()]
    if not steam_procs:
        logger.info("close_steam: no Steam processes found, skipping.", extra={"el": True})
    else:
        logger.info(f"close_steam: terminating {len(steam_procs)} Steam process(es).", extra={"el": True})
    for p in steam_procs:
        try: p.terminate()
        except psutil.NoSuchProcess: pass
    _, alive = psutil.wait_procs(steam_procs, timeout=5)
    for p in alive:
        try: p.kill()
        except psutil.NoSuchProcess: pass
    if alive:
        logger.info(f"close_steam: force-killed {len(alive)} process(es) that did not terminate.", extra={"el": True})
    time.sleep(1)

# this is windows-only coded but for v2 PLEASE make sure it also works with Linux
def get_steam_root() -> Path | None:
    system = platform.system()
    logger.info(f"get_steam_root: detecting Steam on platform '{system}'.", extra={"el": True})

    if system == "Windows":
        try:
            import winreg
            for hive in (winreg.HKEY_LOCAL_MACHINE, winreg.HKEY_CURRENT_USER):
                for sub in (
                    r"SOFTWARE\Valve\Steam",
                    r"SOFTWARE\WOW6432Node\Valve\Steam",
                ):
                    try:
                        with winreg.OpenKey(hive, sub) as key:
                            path, _ = winreg.QueryValueEx(key, "InstallPath")
                            p = Path(path)
                            if p.exists():
                                logger.info(f"get_steam_root: found via registry at '{p}'.", extra={"el": True})
                                return p
                            else:
                                logger.info(f"get_steam_root: registry key '{sub}' points to '{p}' which does not exist.", extra={"el": True})
                    except (FileNotFoundError, OSError): continue
        except Exception as e:
            logger.warning(f"get_steam_root: registry lookup failed: {e}", extra={"el": True})

        for candidate in (
            Path(os.environ.get("ProgramFiles(x86)", "C:\\Program Files (x86)")) / "Steam",
            Path(os.environ.get("ProgramFiles",       "C:\\Program Files"))       / "Steam",
        ):
            if candidate.exists():
                logger.info(f"get_steam_root: found via fallback path at '{candidate}'.", extra={"el": True})
                return candidate

    else:  # Linux
        for candidate in (
            Path.home() / ".steam"  / "steam",
            Path.home() / ".steam"  / "root",
            Path.home() / ".local"  / "share" / "Steam",
            Path("/usr") / "share"  / "steam",
        ):
            if candidate.exists():
                logger.info(f"get_steam_root: found at '{candidate}'.", extra={"el": True})
                return candidate

    logger.warning("get_steam_root: could not locate Steam installation.", extra={"el": True})
    return None


def get_wrapper_dir() -> Path:
    system = platform.system()
    if system == "Windows":
        base = Path(os.environ.get("LOCALAPPDATA", Path.home() / "AppData" / "Local"))
    else:
        base = Path.home() / ".local" / "share"
    d = base / "Aurora"
    d.mkdir(parents=True, exist_ok=True)
    return d


def get_library_paths(steam_root: Path) -> list[Path]:
    libraries: list[Path] = [steam_root / "steamapps"]

    vdf = steam_root / "steamapps" / "libraryfolders.vdf"
    if not vdf.exists():
        logger.info(f"get_library_paths: libraryfolders.vdf not found at '{vdf}', using default library only.", extra={"el": True})
        return libraries

    try:
        text = vdf.read_text(encoding="utf-8", errors="replace")
        for m in re.finditer(r'"path"\s+"([^"]+)"', text):
            p = Path(m.group(1)) / "steamapps"
            if p.exists() and p not in libraries:
                libraries.append(p)
        logger.info(f"get_library_paths: found {len(libraries)} library path(s): {[str(l) for l in libraries]}", extra={"el": True})
    except OSError as e:
        logger.warning(f"get_library_paths: failed to read libraryfolders.vdf: {e}", extra={"el": True})

    return libraries


def find_acf(libraries: list[Path], app_id: str) -> Path | None:
    target = f"appmanifest_{app_id}.acf"
    for lib in libraries:
        acf = lib / target
        if acf.exists():
            logger.info(f"find_acf: found manifest at '{acf}'.", extra={"el": True})
            return acf
        else:
            logger.info(f"find_acf: manifest not found in '{lib}'.", extra={"el": True})
    return None


def read_acf_install_dir(acf: Path) -> str | None:
    try:
        text = acf.read_text(encoding="utf-8", errors="replace")
        m = re.search(r'"installdir"\s+"([^"]+)"', text)
        if m:
            logger.info(f"read_acf_install_dir: installdir = '{m.group(1)}'.", extra={"el": True})
            return m.group(1)
        else:
            logger.warning(f"read_acf_install_dir: 'installdir' key not found in '{acf}'.", extra={"el": True})
            return None
    except OSError as e:
        logger.warning(f"read_acf_install_dir: failed to read '{acf}': {e}", extra={"el": True})
        return None

def is_steam_install(game_path: str | Path) -> bool:
    steam_root = get_steam_root()
    if not steam_root:
        logger.info("is_steam_install: aborting, Steam root not found.", extra={"el": True})
        return False

    libraries = get_library_paths(steam_root)
    acf = find_acf(libraries, NTE_APP_ID)

    if not acf:
        logger.info("is_steam_install: ACF manifest not found in any library.", extra={"el": True})
        return False

    install_dir_name = read_acf_install_dir(acf)
    if not install_dir_name:
        logger.info("is_steam_install: could not read installdir from ACF.", extra={"el": True})
        return False

    expected = (acf.parent / "common" / install_dir_name).resolve()
    actual   = Path(game_path).resolve()

    match = expected == actual
    logger.info(f"is_steam_install: expected='{expected}', actual='{actual}', match={match}.", extra={"el": True})
    return match


def check_steam_async(game_path: str | Path, callback) -> threading.Thread:
    def _worker():
        try:
            result = is_steam_install(game_path)
        except Exception:
            logger.warning("check_steam_async: Steam detection raised an exception.", exc_info=True)
            result = False
        callback(result)

    t = threading.Thread(target=_worker, name="SteamDetect", daemon=True)
    t.start()
    return t

def fetch_wrapper_download_url() -> str | None:
    api_url = f"https://api.github.com/repos/Daturaxoxo/AuroraInstallation/releases/latest"
    logger.info(f"fetch_wrapper_download_url: querying '{api_url}'.", extra={"el": True})
    try:
        req = urllib.request.Request(
            api_url,
            headers={
                "Accept": "application/vnd.github+json",
                "User-Agent": "AuroraLauncher/1.0",
            },
        )
        with urllib.request.urlopen(req, timeout=15) as resp:
            data = json.load(resp)
        assets = data.get("assets", [])
        logger.info(f"fetch_wrapper_download_url: release '{data.get('tag_name', '?')}' has {len(assets)} asset(s).", extra={"el": True})
        for asset in assets:
            if asset.get("name") == WRAPPER_EXE:
                url = asset.get("browser_download_url")
                logger.info(f"fetch_wrapper_download_url: found download URL '{url}'.", extra={"el": True})
                return url
        logger.warning(f"fetch_wrapper_download_url: '{WRAPPER_EXE}' not found among release assets.", extra={"el": True})
    except Exception as e:
        logger.warning(f"fetch_wrapper_download_url: request failed: {e}", extra={"el": True})
    return None


def install_wrapper() -> bool:
    url = fetch_wrapper_download_url()
    if not url:
        logger.warning("install_wrapper: no download URL, aborting.", extra={"el": True})
        return False

    dest = get_wrapper_dir() / WRAPPER_EXE
    logger.info(f"install_wrapper: downloading to '{dest}'.", extra={"el": True})
    try:
        req = urllib.request.Request(url, headers={"User-Agent": "AuroraLauncher/1.0"})
        with urllib.request.urlopen(req, timeout=120) as resp:
            total, downloaded = int(resp.headers.get("Content-Length", 0) or 0), 0
            with open(dest, "wb") as f:
                while chunk := resp.read(65536):
                    f.write(chunk)
                    downloaded += len(chunk)
        logger.info(f"install_wrapper: downloaded {downloaded} / {total} bytes to '{dest}'.", extra={"el": True})
        return True
    except Exception as e:
        logger.warning(f"install_wrapper: download failed: {e}", extra={"el": True})
        return False

def get_localconfig_paths(steam_root: Path) -> list[Path]:
    userdata = steam_root / "userdata"
    if not userdata.exists():
        logger.warning(f"get_localconfig_paths: userdata directory not found at '{userdata}'.", extra={"el": True})
        return []
    configs = list(userdata.glob(f"*/config/localconfig.vdf"))
    logger.info(f"get_localconfig_paths: found {len(configs)} localconfig.vdf file(s).", extra={"el": True})
    return configs

def most_recent_config(configs: list[Path]) -> Path | None:
    if not configs:
        logger.warning("most_recent_config: no localconfig.vdf files to choose from.", extra={"el": True})
        return None
    result = max(configs, key=lambda p: p.stat().st_mtime)
    logger.info(f"most_recent_config: selected '{result}'.", extra={"el": True})
    return result

def build_launch_option(wrapper_path: Path) -> str:
    escaped = str(wrapper_path).replace("\\", "\\\\")
    return f'\\"{ escaped}\\" %command%'

def patch_localconfig(config_path: Path, app_id: str, launch_option: str) -> bool:
    logger.info(f"patch_localconfig: patching '{config_path}' for app {app_id}.", extra={"el": True})
    try:
        original = config_path.read_text(encoding="utf-8", errors="replace")
    except OSError as e:
        logger.warning(f"patch_localconfig: failed to read config: {e}", extra={"el": True})
        return False

    backup = config_path.with_suffix(".vdf.aurora_backup")
    try:
        backup.write_text(original, encoding="utf-8")
        logger.info(f"patch_localconfig: backup written to '{backup}'.", extra={"el": True})
    except OSError:
        logger.warning(f"patch_localconfig: could not write backup to '{backup}'.", extra={"el": True})

    def find_block_end(text: str, start: int) -> int:
        depth = 0
        for i, ch in enumerate(text[start:], start):
            if ch == "{": depth += 1
            elif ch == "}":
                depth -= 1
                if depth == 0:
                    return i
        return -1

    def find_section(text: str, *keys: str) -> tuple[int, int] | None:
        pos = 0
        for key in keys:
            pattern = re.compile(rf'"{re.escape(key)}"\s*\{{', re.DOTALL)
            m = pattern.search(text, pos)
            if not m:
                logger.warning(f"patch_localconfig: section key '{key}' not found in config.", extra={"el": True})
                return None
            brace_start = m.end() - 1
            block_end   = find_block_end(text, brace_start)
            if block_end == -1:
                logger.warning(f"patch_localconfig: could not find closing brace for section '{key}'.", extra={"el": True})
                return None
            pos = brace_start + 1
        return brace_start + 1, block_end
    
    result = find_section(original, "Software", "Valve", "Steam", "apps")
    if not result:
        logger.warning("patch_localconfig: could not find Software > Valve > Steam > apps section.", extra={"el": True})
        return False

    apps_inner_start, apps_end = result
    apps_content = original[apps_inner_start:apps_end]
    app_match = re.search(rf'"{re.escape(app_id)}"\s*\{{', apps_content)

    if app_match:
        logger.info(f"patch_localconfig: found existing app block for {app_id}, updating LaunchOptions.", extra={"el": True})
        abs_block_open = apps_inner_start + app_match.end() - 1
        abs_block_end  = find_block_end(original, abs_block_open)
        block_interior = original[abs_block_open + 1:abs_block_end]

        if '"LaunchOptions"' in block_interior:
            logger.info("patch_localconfig: replacing existing LaunchOptions entry.", extra={"el": True})
            new_interior = re.sub(
                r'"LaunchOptions"\s*"[^"]*"',
                lambda m: f'"LaunchOptions"\t\t"{launch_option}"',
                block_interior,
            )
        else:
            logger.info("patch_localconfig: no LaunchOptions entry found, inserting new one.", extra={"el": True})
            new_interior = block_interior.rstrip() + f'\n\t\t\t\t\t"LaunchOptions"\t\t"{launch_option}"\n\t\t\t\t'

        patched = (
            original[:abs_block_open + 1]
            + new_interior
            + original[abs_block_end:]
        )
    else:
        logger.info(f"patch_localconfig: no existing block for app {app_id}, creating new entry.", extra={"el": True})
        new_block = (
            f'\t\t\t\t\t"{app_id}"\n'
            f'\t\t\t\t\t{{\n'
            f'\t\t\t\t\t\t"LaunchOptions"\t\t"{launch_option}"\n'
            f'\t\t\t\t\t}}\n\t\t\t\t'
        )
        patched = original[:apps_end] + new_block + original[apps_end:]

    try:
        config_path.write_text(patched, encoding="utf-8")
        logger.info("patch_localconfig: config written successfully.", extra={"el": True})
        return True
    except OSError as e:
        logger.warning(f"patch_localconfig: failed to write patched config: {e}", extra={"el": True})
        try:
            config_path.write_text(original, encoding="utf-8")
            logger.info("patch_localconfig: restored original config after write failure.", extra={"el": True})
        except OSError as e2:
            logger.warning(f"patch_localconfig: failed to restore original config: {e2}", extra={"el": True})
        return False


def restore_localconfig(config_path: Path, app_id: str) -> bool:
    logger.info(f"restore_localconfig: clearing LaunchOptions for app {app_id} in '{config_path}'.", extra={"el": True})
    try:
        text = config_path.read_text(encoding="utf-8", errors="replace")
    except OSError as e:
        logger.warning(f"restore_localconfig: failed to read config: {e}", extra={"el": True})
        return False

    patched = re.sub(
        rf'("LaunchOptions"\s*")[^"]*(")',
        r'\1\2',
        text,
    )
    try:
        config_path.write_text(patched, encoding="utf-8")
        logger.info("restore_localconfig: LaunchOptions cleared successfully.", extra={"el": True})
        return True
    except OSError as e:
        logger.warning(f"restore_localconfig: failed to write restored config: {e}", extra={"el": True})
        return False


def apply_steam_wrapper() -> bool:
    logger.info("apply_steam_wrapper: starting.", extra={"el": True})
    close_steam()

    if not install_wrapper():
        logger.warning("apply_steam_wrapper: wrapper installation failed, aborting.", extra={"el": True})
        return False

    steam_root = get_steam_root()
    if not steam_root:
        logger.warning("apply_steam_wrapper: Steam root not found, aborting.", extra={"el": True})
        return False

    configs = get_localconfig_paths(steam_root)
    target  = most_recent_config(configs)
    if not target:
        logger.warning("apply_steam_wrapper: no localconfig.vdf found, aborting.", extra={"el": True})
        return False

    wrapper_path  = get_wrapper_dir() / WRAPPER_EXE
    launch_option = build_launch_option(wrapper_path)
    logger.info(f"apply_steam_wrapper: launch option = '{launch_option}'.", extra={"el": True})
    result = patch_localconfig(target, NTE_APP_ID, launch_option)
    logger.info(f"apply_steam_wrapper: finished with result={result}.", extra={"el": True})
    return result


def remove_steam_wrapper() -> bool:
    logger.info("remove_steam_wrapper: starting.", extra={"el": True})
    steam_root = get_steam_root()
    if not steam_root:
        logger.warning("remove_steam_wrapper: Steam root not found, aborting.", extra={"el": True})
        return False

    configs = get_localconfig_paths(steam_root)
    target  = most_recent_config(configs)
    if not target:
        logger.warning("remove_steam_wrapper: no localconfig.vdf found, aborting.", extra={"el": True})
        return False

    result = restore_localconfig(target, NTE_APP_ID)
    logger.info(f"remove_steam_wrapper: finished with result={result}.", extra={"el": True})
    return result