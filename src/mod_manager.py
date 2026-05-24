import os
import json
from pathlib import Path
from dataclasses import dataclass
from src.logger import logger
from src.translator import Translator, t

@dataclass
class ModEntry:
    folder_name: str      # Actual name on disk ("disabled_MintMod")
    display_name: str     # Name shown in UI ("MintMod")
    version: str = t("mod_manager_unknown")
    author: str = t("mod_manager_unknown")
    support_link: str = ""
    icon: str = ""
    is_enabled: bool = True
    has_json: bool = False

class ModManager:
    def __init__(self, mods_dir: Path, state_file: Path):
        self.mods_dir = mods_dir
        self.state_file = state_file
        self.mods_dir.mkdir(parents=True, exist_ok=True)

    def scan_mods(self) -> list[ModEntry]:
        mods = []
        if not self.mods_dir.exists():
            return mods

        for folder in self.mods_dir.iterdir():
            if not folder.is_dir():
                continue
            
            # Mod Folder Check: Must have at least one .pak file
            has_pak = any(f.suffix.lower() == ".pak" for f in folder.iterdir())
            if not has_pak:
                continue

            raw_name = folder.name
            is_enabled = not raw_name.startswith("disabled_")
            clean_name = raw_name.replace("disabled_", "").replace("_P", "")

            mod_data = {
                "folder_name": raw_name,
                "display_name": clean_name,
                "is_enabled": is_enabled,
                "version": t("mod_manager_unknown"),
                "author": t("mod_manager_unknown"),
                "support_link": "",
                "has_json": False
            }

            json_path = folder / "mod.json"
            if json_path.exists():
                try:
                    with open(json_path, 'r', encoding='utf-8') as f:
                        data = json.load(f)
                        mod_data.update({
                            "display_name": data.get("Name", mod_data["display_name"]),
                            "version": data.get("Version", "1.0.0"),
                            "author": data.get("Author", t("mod_manager_unknown")),
                            "support_link": data.get("Optionals", {}).get("Support Link", ""),
                            "icon": data.get("Icon", ""),
                            "has_json": True
                        })
                except Exception as e:
                    logger.warning(f"Failed to parse mod.json in {folder.name}: {e}")

            mods.append(ModEntry(**mod_data))
        
        # Alphabetical sorting based on folder name
        return sorted(mods, key=lambda x: x.display_name.lower())

    def toggle_mod(self, mod: ModEntry) -> str:
        old_path = self.mods_dir / mod.folder_name
        
        if mod.is_enabled: # Disabled Clause
            new_name = f"disabled_{mod.folder_name}"
        else: # Enabled Clause
            new_name = mod.folder_name.replace("disabled_", "")

        new_path = self.mods_dir / new_name
        
        try:
            if new_path.exists():
                logger.error(f"Cannot toggle mod: {new_name} already exists!")
                return None

            try:
                old_path.rename(new_path)
            except PermissionError:
                import subprocess
                result = subprocess.run(
                    f'rename "{old_path}" "{new_path.name}"',
                    shell=True, capture_output=True, text=True
                )
                if result.returncode != 0:
                    logger.error(f"Shell rename also failed: {result.stderr.strip()}")
                    return None

            logger.info(f"Mod toggled: {mod.folder_name} -> {new_name}", extra={"el": True})
            return new_name
        except Exception as e:
            logger.error(f"FileSystem error toggling mod: {e}")
            return None
