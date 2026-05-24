from dataclasses import dataclass

# Public

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

# REGISTRY
# Adding new builtins might be confusing so just listen to this explanation
# config_key: The config key that is present in config_manager
# base_name: Mod name inside of Bin\Builtins
# Example PAK addon addition:
    # PakAddon(
    #     config_key  = Key.EXAMPLE_ADDON,
    #     base_name   = "your_pak_stem",
    # ),

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