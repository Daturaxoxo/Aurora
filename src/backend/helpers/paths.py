from __future__ import annotations
from dataclasses import dataclass, field
from pathlib import Path
from typing import Optional

# Constants & Dataclasses
VERSION_GLOBAL = "global"
VERSION_CN     = "cn"
VERSION_TW     = "tw"
CLIENT_WIN64   = Path("Client/WindowsNoEditor/HT/Binaries/Win64")
CLIENT_PAK_DIR = Path("Client/WindowsNoEditor/HT/Content/Paks/AuroraMods")
LAUNCHER_MAP: dict[str, str] = {
    "NTEGlobalLauncher.exe": VERSION_GLOBAL,
    "NTELauncher.exe":       VERSION_CN,
    "NTETWLauncher.exe":     VERSION_TW,
}
NTE_PROCESS: frozenset[str] = frozenset({
    "ntegloballauncher.exe", "nteglobal.exe", "nteglobalgame.exe",  # GL
    "ntelauncher.exe",       "ntegame.exe",                         # CN
    "ntetwlauncher.exe",     "ntetwgame.exe",                       # TW
    "htgame.exe",                                                   # ALL
})
@dataclass(frozen=True)
class DllSlot:
    name:     str
    root:     Path
    bin:      Path
    launcher: Optional[Path]

    @property
    def all_targets(self) -> list[tuple[str, Path]]:
        targets = [("root", self.root), ("bin", self.bin)]
        if self.launcher is not None:
            targets.append(("launcher", self.launcher))
        return targets
@dataclass(frozen=True)
class VersionPaths:
    version:          str
    win64:            Path
    pak_base:         Path
    dll_slots:        tuple[DllSlot, ...]
    asi_plugin:       Path
    launcher_process: str
    helper_processes: list[str]
    game_process:     str = "HTGame.exe"
    @property
    def all_dll_targets(self) -> list[tuple[str, Path]]:
        return [
            (f"{slot.name}:{label}", path)
            for slot in self.dll_slots
            for label, path in slot.all_targets
        ]
@dataclass(frozen=True)
class VersionSpec:
    launcher_subfolder: str
    launcher_process:   str
    helper_processes:   list[str]
    dll_names:          tuple[str, ...]
VERSION_SPECS: dict[str, VersionSpec] = {
    VERSION_GLOBAL: VersionSpec(
        launcher_subfolder = "NTEGlobal",
        launcher_process   = "NTEGlobalLauncher.exe",
        helper_processes   = ["NTEGlobal.exe", "NTEGlobalGame.exe"],
        dll_names          = ("version.dll",),
    ),
    VERSION_CN: VersionSpec(
        launcher_subfolder = "NTELauncher",
        launcher_process   = "NTELauncher.exe",
        helper_processes   = ["NTEGame.exe"],
        dll_names          = ("dsound.dll",),
    ),
    VERSION_TW: VersionSpec(
        launcher_subfolder = "NTETW",
        launcher_process   = "NTETWLauncher.exe",
        helper_processes   = ["NTETWGame.exe"],
        dll_names          = ("version.dll",),
    ),
}
BYPASS_METHODS: dict[str, dict[str, tuple[tuple[str, ...], str]]] = {
    VERSION_GLOBAL: {
        "0": (("version.dll",),          "engine_method_opt_1"),
        "1": (("dsound.dll",),            "engine_method_opt_2"),
    },
    VERSION_TW: {
        "0": (("version.dll",),          "engine_method_opt_1"),
        "1": (("dsound.dll",),            "engine_method_opt_2"),
    },
    VERSION_CN: {
        "0": (("dsound.dll",),            "engine_method_opt_1"),
        "1": (("dsound.dll", "ddraw.dll"),    "engine_method_opt_3"),
    },
}

def get_bypass_dlls(version: str, method: str) -> tuple[str, ...]:
    methods = BYPASS_METHODS.get(version, BYPASS_METHODS[VERSION_GLOBAL])
    entry = methods.get(method, next(iter(methods.values())))
    return entry[0]

# Public API
def get_version_paths(game_path: Path, version: str, engine_method: str = "0") -> VersionPaths:
    spec = VERSION_SPECS.get(version)
    if spec is None:
        raise ValueError(
            f"Unknown NTE version {version!r}. "
            f"Expected one of: {', '.join(VERSION_SPECS)}"
        )
    win64    = game_path / CLIENT_WIN64
    pak_base = game_path / CLIENT_PAK_DIR
    launcher_dir = game_path / spec.launcher_subfolder
    dll_names = get_bypass_dlls(version, engine_method) if version in BYPASS_METHODS else spec.dll_names
    dll_slots = tuple(
        DllSlot(
            name     = dll_name,
            root     = game_path   / dll_name,
            bin      = win64       / dll_name,
            launcher = launcher_dir / dll_name,
        )
        for dll_name in dll_names
    )
    return VersionPaths(
        version          = version,
        win64            = win64,
        pak_base         = pak_base,
        dll_slots        = dll_slots,
        asi_plugin       = win64 / "USB.asi",
        launcher_process = spec.launcher_process,
        helper_processes = spec.helper_processes,
    )


def detect_version(game_path: Path) -> str:
    if not game_path.exists():
        raise FileNotFoundError(f"Aurora couldn't find the game path: {game_path}")

    for launcher_exe, version in LAUNCHER_MAP.items():
        if (game_path / launcher_exe).exists():
            return version

    checked = ", ".join(LAUNCHER_MAP)
    raise ValueError(
        f"Could not detect NTE version in '{game_path}'. "
        f"None of the expected launchers were found: {checked}"
    )