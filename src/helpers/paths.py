from dataclasses import dataclass, field
from pathlib import Path
from typing import Optional


# Version Identifiers

VERSION_GLOBAL = "global"
VERSION_CN     = "cn"
VERSION_TW     = "tw"
_LAUNCHER_MAP = {
    "NTEGlobalLauncher.exe": VERSION_GLOBAL,
    "NTELauncher.exe":       VERSION_CN,
    "NTETWLauncher.exe":     VERSION_TW,
}

_ALL_NTE_PROCS = {
    "ntegloballauncher.exe", "nteglobal.exe", "nteglobalgame.exe", # GL
    "ntelauncher.exe", "ntegame.exe", # CN
    "ntetwlauncher.exe", "ntetwgame.exe", # TW
    "htgame.exe" # GL
}

# VERSION PATHS
@dataclass
class VersionPaths:
    version: str
    win64:    Path   # .../Binaries/Win64
    pak_base: Path
    root_dll:   Path
    bin_dll:    Path
    global_dll: Optional[Path]
    asi_plugin: Path

    launcher_process:  str
    helper_processes:  list[str]
    game_process:      str = "HTGame.exe"

# Get Version Paths

def get_version_paths(game_path: Path, version: str) -> VersionPaths:

    if version == VERSION_GLOBAL:
        win64    = game_path / "Client/WindowsNoEditor/HT/Binaries/Win64"
        pak_base = game_path / "Client/WindowsNoEditor/HT/Content/Paks/AuroraMods"
        return VersionPaths(
            version          = VERSION_GLOBAL,
            win64            = win64,
            pak_base         = pak_base,
            root_dll         = game_path / "version.dll",
            bin_dll          = win64 / "version.dll",
            global_dll       = game_path / "NTEGlobal" / "version.dll",
            asi_plugin       = win64 / "ausigbp.asi",
            launcher_process = "NTEGlobalLauncher.exe",
            helper_processes = ["NTEGlobal.exe", "NTEGlobalGame.exe"],
        )

    if version == VERSION_CN:
        win64    = game_path / "Client/WindowsNoEditor/HT/Binaries/Win64"
        pak_base = game_path / "Client/WindowsNoEditor/HT/Content/Paks/AuroraMods"
        return VersionPaths(
            version          = VERSION_CN,
            win64            = win64,
            pak_base         = pak_base,
            root_dll         = game_path / "dinput8.dll",
            bin_dll          = win64 / "dinput8.dll",
            global_dll       = game_path / "NTELauncher" / "dinput8.dll",
            asi_plugin       = win64 / "ausigbp.asi",
            launcher_process = "NTELauncher.exe",
            helper_processes = ["NTEGame.exe"],  # equivalent of NTEGlobalGame.exe, lives in NTELauncher/
        )

    if version == VERSION_TW:
        win64    = game_path / "Client/WindowsNoEditor/HT/Binaries/Win64"
        pak_base = game_path / "Client/WindowsNoEditor/HT/Content/Paks/AuroraMods"
        return VersionPaths(
            version          = VERSION_TW,
            win64            = win64,
            pak_base         = pak_base,
            root_dll         = game_path / "version.dll",
            bin_dll          = win64 / "version.dll",
            global_dll       = game_path / "NTETW" / "version.dll",
            asi_plugin       = win64 / "ausigbp.asi",
            launcher_process = "NTETWLauncher.exe",
            helper_processes = ["NTETWGame.exe"],  # equivalent of NTEGlobalGame.exe, lives in NTELauncher/
        )

    raise ValueError(f"Unknown NTE version: '{version!r}'")


# Public API

def detect_version(game_path: Path) -> str:
    if not game_path.exists():
        raise FileNotFoundError(f"Game path does not exist: {game_path}")

    for launcher_exe, version in _LAUNCHER_MAP.items():
        if (game_path / launcher_exe).exists():
            return version

    checked = ", ".join(_LAUNCHER_MAP.keys())
    raise ValueError(
        f"Could not detect NTE version in '{game_path}'. "
        f"None of the expected launchers were found: {checked}"
    )