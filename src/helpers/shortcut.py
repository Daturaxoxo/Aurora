import sys, platform, shutil
from pathlib import Path
from src.utils import get_cache, set_cache, resource_path, cache_path
from src.logger import logger

SHORTCUT_CACHE_KEY = "quick_start_shortcut_created"

def create_shortcut():
    if platform.system() != "Windows": return
    if get_cache(SHORTCUT_CACHE_KEY): return

    try:
        import winshell
        from win32com.client import Dispatch
    except ImportError: return

    try:
        desktop = Path(winshell.desktop())
        shortcut_path = desktop / "Aurora Quick Start.lnk"
        if getattr(sys, "frozen", False):
            aurora_exe = Path(sys.executable)
        else: return
        icon_path = cache_path().parent.parent / "Assets" / "startup.ico"

        shell = Dispatch("WScript.Shell")
        shortcut = shell.CreateShortcut(str(shortcut_path))
        shortcut.TargetPath = str(aurora_exe)
        shortcut.Arguments = "--quick-start"
        shortcut.WorkingDirectory = str(aurora_exe.parent)
        shortcut.IconLocation = str(icon_path)
        shortcut.Description = "Launch NTE instantly with mods, skipping Aurora's UI"
        shortcut.save()

        set_cache(SHORTCUT_CACHE_KEY, True)
    except Exception as e: logger.warning(f"Quick Start Exception: {e}")

def refresh_shortcut_icon():
    if not getattr(sys, "frozen", False): return
    if platform.system() != "Windows": return
    try:
        from src.utils import cache_path
        icon_src = Path(resource_path("Bin/Assets/startup.ico"))
        icon_dst = cache_path().parent.parent / "Assets" / "startup.ico"
        icon_dst.parent.mkdir(parents=True, exist_ok=True)
        shutil.copy2(icon_src, icon_dst)
    except Exception: pass