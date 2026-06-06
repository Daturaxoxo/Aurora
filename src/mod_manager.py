import os
import json
from pathlib import Path
from dataclasses import dataclass
from src.logger import logger
from src.translator import Translator, t
from src.utils import get_mods_path

@dataclass
class ModEntry:
    folder_name: str
    display_name: str
    folder_path: Path = None
    version: str = t("mod_manager_unknown")
    author: str = t("mod_manager_unknown")
    support_link: str = ""
    icon: str = ""
    is_enabled: bool = True
    has_json: bool = False

class ModManager:
    def __init__(self, mods_dir: Path, state_file: Path):
        self.state_file = state_file

    @property
    def mods_dir(self) -> Path:
        return get_mods_path()

    def scan_mods(self) -> list[ModEntry]:
        mods = []
        if not self.mods_dir.exists():
            return mods

        seen_paths = set()

        # Find all folders that contain a .pak or .pak.disabled file
        for pak_file in list(self.mods_dir.rglob("*.pak")) + list(self.mods_dir.rglob("*.pak.disabled")):
            folder = pak_file.parent
            resolved = folder.resolve()
            if resolved in seen_paths:
                continue
            seen_paths.add(resolved)

            raw_name = folder.name
            # A folder is disabled only if ALL its pak files are .pak.disabled
            pak_files = list(folder.glob("*.pak"))
            disabled_files = list(folder.glob("*.pak.disabled"))
            is_enabled = len(pak_files) > 0  # has at least one active .pak
            clean_name = raw_name.replace("_P", "")

            mod_data = {
                "folder_name": raw_name,
                "display_name": clean_name,
                "folder_path": folder,
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

    def toggle_mod(self, mod: ModEntry) -> bool:
        folder = mod.folder_path
        if folder is None or not folder.exists():
            logger.error(f"Cannot toggle mod: folder not found for {mod.folder_name}")
            return False

        try:
            if mod.is_enabled:
                # Disable: rename every .pak -> .pak.disabled
                targets = list(folder.rglob("*.pak"))
                if not targets:
                    logger.error(f"Cannot disable mod: no .pak files found in {folder.name}")
                    return False
                for pak in targets:
                    new_path = pak.with_suffix(pak.suffix + ".disabled")
                    if new_path.exists():
                        logger.error(f"Cannot disable: {new_path.name} already exists!")
                        return False
                    pak.rename(new_path)
                logger.info(f"Mod disabled: renamed {len(targets)} file(s) in {folder.name}", extra={"el": True})
            else:
                # Enable: rename every .pak.disabled -> .pak
                targets = list(folder.glob("*.pak.disabled"))
                if not targets:
                    logger.error(f"Cannot enable mod: no .pak.disabled files found in {folder.name}")
                    return False
                for disabled in targets:
                    # Strip the trailing .disabled suffix
                    new_path = disabled.with_suffix("")  # e.g. Foo.pak.disabled -> Foo.pak
                    if new_path.exists():
                        logger.error(f"Cannot enable: {new_path.name} already exists!")
                        return False
                    disabled.rename(new_path)
                logger.info(f"Mod enabled: renamed {len(targets)} file(s) in {folder.name}", extra={"el": True})

            return True
        except PermissionError as e:
            logger.error(f"Permission error toggling mod {folder.name}: {e}")
            return False
        except Exception as e:
            logger.error(f"FileSystem error toggling mod: {e}")
            return False