import stat, re, subprocess, os
from pathlib import Path
from src.utils import get_app_dir
from dataclasses import dataclass

# Constants & Dataclasses
SECTION_HEADER = "[/Script/Engine.UserInterfaceSettings]"
KEY            = "ApplicationScale"
ENGINE_INI_PATH = Path(os.environ.get("LOCALAPPDATA", ""), "HT", "Saved_Global", "Config", "Windows", "Engine.ini")
@dataclass(frozen=True)
class PakAddon:
    config_key:  str
    base_name:   str

    @property
    def files(self) -> list[str]:
        return [
            f"{self.base_name}.pak",
            f"{self.base_name}.utoc",
            f"{self.base_name}.ucas",
        ]

# [Helpers]
# UI Scaling
def get_ini_path() -> Path:
    local_app_data = os.environ.get("LOCALAPPDATA") or os.path.expandvars("%LOCALAPPDATA%")
    return Path(local_app_data) / "HT" / "Saved_Global" / "Config" / "Windows" / "Engine.ini"

def is_readonly(path: Path) -> bool:
    try:
        return not (path.stat().st_mode & stat.S_IWRITE)
    except OSError:
        return False
    
def set_readonly(path: Path, readonly: bool) -> None:
    flag = "+R" if readonly else "-R"
    subprocess.run(
        ["attrib", flag, str(path)],
        shell=False, capture_output=True
    )

def strip_section(text: str) -> str:
    pattern = re.compile(
        r"\[/Script/Engine\.UserInterfaceSettings\][^\[]*",
        re.IGNORECASE,
    )
    cleaned = pattern.sub("", text)
    cleaned = re.sub(r"\n{3,}", "\n\n", cleaned)
    return cleaned.rstrip("\n")

# [Public API]
# UI Scaling
def get_current_scale() -> float:
    path = get_ini_path()
    if not path.exists():
        return 1.0
    try:
        text = path.read_text(encoding="utf-8", errors="replace")
        m = re.search(
            r"\[/Script/Engine\.UserInterfaceSettings\].*?ApplicationScale\s*=\s*([0-9.]+)",
            text,
            re.IGNORECASE | re.DOTALL,
        )
        if m:
            return float(m.group(1))
    except (OSError, ValueError):
        pass
    return 1.0

def apply_scale(scale: float) -> bool:
    scale = round(max(0.5, min(2.0, scale)), 2)
    path  = get_ini_path()
    try:
        path.parent.mkdir(parents=True, exist_ok=True)
        if path.exists():
            set_readonly(path, False)
        existing = path.read_text(encoding="utf-8", errors="replace") if path.exists() else ""
        base = strip_section(existing)
        new_text = base + f"\n\n{SECTION_HEADER}\n{KEY}={scale}\n"
        with open(path, "w", encoding="utf-8") as f:
            f.write(new_text)
        set_readonly(path, True)
        return True

    except Exception as e:
        from src.logger import logger
        logger.error(f"engine_ini.apply_scale failed: {e}", exc_info=True)
        return False
    
def remove_scale() -> bool:
    path = get_ini_path()
    if not path.exists():
        return True
 
    try:
        if is_readonly(path):
            set_readonly(path, False)
 
        existing = path.read_text(encoding="utf-8", errors="replace")
        cleaned  = strip_section(existing)
        path.write_text(cleaned + "\n", encoding="utf-8")
        return True
 
    except (OSError, PermissionError):
        return False
    
def ini_path() -> Path:
    return get_ini_path() # Useful for logging in the UI

# PAK Addon Manager
PAK_ADDONS: list[PakAddon] = [
    PakAddon(
        config_key  = "drv_lin",       # Key.NO_DRIVE_LINE
        base_name   = "auddl_P",
    ),
    PakAddon(
        config_key  = "uid_rem",       # Key.HIDE_UID
        base_name   = "uidrm_P",
    ),
   PakAddon(
        config_key  = "nor_rem",       # Key.HIDE_NOTIF_DOTS
        base_name   = "nrdrm_P",
    ),
]