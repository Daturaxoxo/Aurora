import os, re, platform, threading, urllib.request, json, psutil, time
from pathlib import Path
from src.logger import logger

# CONSTANTS
NTE_APP_ID      = "945360"
WRAPPER_REPO    = "https://api.github.com/repos/Daturaxoxo/AuroraInstallation/releases/latest"
WRAPPER_EXE     = "AuroraSteamWrapper.exe"

# HELPERS

def close_steam() -> None:
    steam_procs = [p for p in psutil.process_iter(["name"]) if "steam" in p.name().lower()]
    for p in steam_procs:
        try: p.terminate()
        except psutil.NoSuchProcess: pass
    _, alive = psutil.wait_procs(steam_procs, timeout=5)
    for p in alive:
        try: p.kill()
        except psutil.NoSuchProcess: pass
    time.sleep(1)

# this is windows-only coded but for v2 PLEASE make sure it also works with Linux
def get_steam_root() -> Path | None:
    system = platform.system()

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
                            if p.exists(): return p
                    except (FileNotFoundError, OSError): continue
        except Exception: pass
        for candidate in (
            Path(os.environ.get("ProgramFiles(x86)", "C:\\Program Files (x86)")) / "Steam",
            Path(os.environ.get("ProgramFiles",       "C:\\Program Files"))       / "Steam",
        ): 
            if candidate.exists(): return candidate

    else:  # Linux
        for candidate in (
            Path.home() / ".steam"  / "steam",
            Path.home() / ".steam"  / "root",
            Path.home() / ".local"  / "share" / "Steam",
            Path("/usr") / "share"  / "steam",
        ):
            if candidate.exists(): return candidate

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
    if not vdf.exists(): return libraries

    try:
        text = vdf.read_text(encoding="utf-8", errors="replace")
        for m in re.finditer(r'"path"\s+"([^"]+)"', text):
            p = Path(m.group(1)) / "steamapps"
            if p.exists() and p not in libraries:
                libraries.append(p)
    except OSError: pass

    return libraries


def find_acf(libraries: list[Path], app_id: str) -> Path | None:
    target = f"appmanifest_{app_id}.acf"
    for lib in libraries:
        acf = lib / target
        if acf.exists(): return acf
    return None


def read_acf_install_dir(acf: Path) -> str | None:
    try:
        text = acf.read_text(encoding="utf-8", errors="replace")
        m = re.search(r'"installdir"\s+"([^"]+)"', text)
        return m.group(1) if m else None
    except OSError: return None

def is_steam_install(game_path: str | Path) -> bool:
    steam_root = get_steam_root()
    if not steam_root: return False

    libraries = get_library_paths(steam_root)
    acf = find_acf(libraries, NTE_APP_ID)

    if not acf:
        logger.info("ACF Manifest not found", extra={"el": True})
        return False

    install_dir_name = read_acf_install_dir(acf)
    if not install_dir_name: return False

    expected = (acf.parent / "common" / install_dir_name).resolve()
    actual   = Path(game_path).resolve()

    match = expected == actual
    return match


def check_steam_async(game_path: str | Path, callback) -> threading.Thread:
    def _worker():
        try:
            result = is_steam_install(game_path)
        except Exception:
            logger.warning("Steam detection raised an exception.", exc_info=True)
            result = False
        callback(result)

    t = threading.Thread(target=_worker, name="SteamDetect", daemon=True)
    t.start()
    return t

def fetch_wrapper_download_url() -> str | None:
    api_url = f"https://api.github.com/repos/Daturaxoxo/AuroraInstallation/releases/latest"
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
            if asset.get("name") == WRAPPER_EXE: return asset.get("browser_download_url")
        logger.warning("AuroraSteamWrapper.exe not found in latest GitHub release assets.")
    except Exception as e: logger.warning(f"Failed to fetch Steam Wrapper release info: {e}")
    return None


def install_wrapper() -> bool:
    url = fetch_wrapper_download_url()
    if not url: return False

    dest = get_wrapper_dir() / WRAPPER_EXE
    try:
        req = urllib.request.Request(url, headers={"User-Agent": "AuroraLauncher/1.0"})
        with urllib.request.urlopen(req, timeout=120) as resp:
            total, downloaded = int(resp.headers.get("Content-Length", 0) or 0), 0
            with open(dest, "wb") as f:
                while chunk := resp.read(65536):
                    f.write(chunk)
                    downloaded += len(chunk)
        return True
    except Exception as e:
        logger.warning(f"Failed to download Steam Wrapper: {e}")
        return False

def get_localconfig_paths(steam_root: Path) -> list[Path]:
    userdata = steam_root / "userdata"
    if not userdata.exists(): return []
    return list(userdata.glob(f"*/config/localconfig.vdf"))

def most_recent_config(configs: list[Path]) -> Path | None:
    if not configs: return None
    return max(configs, key=lambda p: p.stat().st_mtime)

def build_launch_option(wrapper_path: Path) -> str:
    escaped = str(wrapper_path).replace("\\", "\\\\")
    return f'\\"{ escaped}\\" %command%'

def patch_localconfig(config_path: Path, app_id: str, launch_option: str) -> bool:
    try:
        original = config_path.read_text(encoding="utf-8", errors="replace")
    except OSError as e: return False

    backup = config_path.with_suffix(".vdf.aurora_backup")
    try:
        backup.write_text(original, encoding="utf-8")
    except OSError:
        logger.warning(f"Could not write backup for {config_path}.")

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
            if not m: return None
            brace_start = m.end() - 1
            block_end   = find_block_end(text, brace_start)
            if block_end == -1: return None
            pos = brace_start + 1
        return brace_start + 1, block_end
    
    result = find_section(original, "Software", "Valve", "Steam", "apps")
    if not result:
        logger.error("Could not find Software > Valve > Steam > apps in localconfig.vdf")
        return False

    apps_inner_start, apps_end = result
    apps_content = original[apps_inner_start:apps_end]
    app_match = re.search(rf'"{re.escape(app_id)}"\s*\{{', apps_content)

    if app_match:
        abs_block_open = apps_inner_start + app_match.end() - 1
        abs_block_end  = find_block_end(original, abs_block_open)
        block_interior = original[abs_block_open + 1:abs_block_end]

        if '"LaunchOptions"' in block_interior:
            new_interior = re.sub(
                r'"LaunchOptions"\s*"[^"]*"',
                lambda m: f'"LaunchOptions"\t\t"{launch_option}"',
                block_interior,
            )
        else:
            new_interior = block_interior.rstrip() + f'\n\t\t\t\t\t"LaunchOptions"\t\t"{launch_option}"\n\t\t\t\t'

        patched = (
            original[:abs_block_open + 1]
            + new_interior
            + original[abs_block_end:]
        )
    else:
        new_block = (
            f'\t\t\t\t\t"{app_id}"\n'
            f'\t\t\t\t\t{{\n'
            f'\t\t\t\t\t\t"LaunchOptions"\t\t"{launch_option}"\n'
            f'\t\t\t\t\t}}\n\t\t\t\t'
        )
        patched = original[:apps_end] + new_block + original[apps_end:]

    try:
        config_path.write_text(patched, encoding="utf-8")
        return True
    except OSError as e:
        try:
            config_path.write_text(original, encoding="utf-8")
        except OSError: pass
        return False


def restore_localconfig(config_path: Path, app_id: str) -> bool:
    try:
        text = config_path.read_text(encoding="utf-8", errors="replace")
    except OSError: return False

    patched = re.sub(
        rf'("LaunchOptions"\s*")[^"]*(")',
        r'\1\2',
        text,
    )
    try:
        config_path.write_text(patched, encoding="utf-8")
        return True
    except OSError: return False


def apply_steam_wrapper() -> bool:
    close_steam()

    if not install_wrapper(): return False

    steam_root = get_steam_root()
    if not steam_root: return False

    configs = get_localconfig_paths(steam_root)
    target  = most_recent_config(configs)
    if not target: return False

    wrapper_path  = get_wrapper_dir() / WRAPPER_EXE
    launch_option = build_launch_option(wrapper_path)
    return patch_localconfig(target, NTE_APP_ID, launch_option)


def remove_steam_wrapper() -> bool:
    steam_root = get_steam_root()
    if not steam_root: return False

    configs = get_localconfig_paths(steam_root)
    target  = most_recent_config(configs)
    if not target: return False

    return restore_localconfig(target, NTE_APP_ID)