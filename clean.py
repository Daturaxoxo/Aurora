from pathlib import Path
import platform
import shutil
import subprocess
import re

APPIMAGE_NAME = "Aurora-x86_64.AppImage"
APPIMAGE_TARGET_DIR = "target-appimage"
APPIMAGE_IMAGE = "aurora-appimage-build"

def get_os():
    os_name = platform.system().lower()

    if os_name == "windows":
        return "windows"
    elif os_name == "linux":
        return "linux"
    else:
        return os_name

def clean_addons(target_path: Path):
    if not target_path.exists():
        print(f"Error: The target directory '{target_path.resolve()}' does not exist.")
        return

    if not target_path.is_dir():
        print(f"Error: '{target_path.resolve()}' is not a directory.")
        return

    deleted_count = 0
    kept_count = 0

    for path in target_path.rglob("*"):
        if path.is_file():
            if path.suffix.lower() == ".auadd":
                kept_count += 1
                continue

            try:
                path.unlink()
                deleted_count += 1
            except Exception as e:
                print(f"Failed to delete {path}: {e}")

    print("\n--- Cleanup Complete ---")
    print(f"Files deleted: {deleted_count}")
    print(f"Files preserved (.auadd): {kept_count}")

def remove_aurora_zip_files(directory: str = ".") -> list[str]:
    pattern = re.compile(
        r"^aurora-(?:host-)?\d+\.\d+\.\d+(?:-.+?)?-(?:WINDOWS|LINUX)\.zip$",
        re.IGNORECASE,
    )

    dir_path = Path(directory)
    deleted_files = []

    for item in dir_path.iterdir():
        if item.is_file() and pattern.match(item.name):
            try:
                item.unlink()
                deleted_files.append(item.name)
                print(f"Deleted: {item.name}")
            except OSError as e:
                print(f"Error deleting {item.name}: {e}")

    return deleted_files

def remove_appimage_artifacts(directory: str = ".") -> list[str]:
    dir_path = Path(directory)
    deleted: list[str] = []

    targets = [
        dir_path / APPIMAGE_NAME,
        dir_path / "release" / APPIMAGE_NAME,
        dir_path / "release-host" / APPIMAGE_NAME,
        dir_path / "release-github" / APPIMAGE_NAME,
        dir_path / APPIMAGE_TARGET_DIR,
    ]

    for item in targets:
        if not item.exists():
            continue

        try:
            if item.is_dir():
                shutil.rmtree(item)
            else:
                item.unlink()
            deleted.append(str(item))
            print(f"Deleted: {item}")
        except OSError as e:
            print(f"Error deleting {item}: {e}")

    return deleted

def remove_appimage_build_image() -> bool:
    if shutil.which("podman") is None:
        print("podman is not installed, skipping the build image.")
        return False

    result = subprocess.run(
        ["podman", "rmi", "-f", APPIMAGE_IMAGE],
        capture_output=True,
        text=True,
        check=False,
    )

    if result.returncode != 0:
        print(f"Error removing the {APPIMAGE_IMAGE} image: {result.stderr.strip()}")
        return False

    print(f"Deleted image: {APPIMAGE_IMAGE}")
    return True

def remove_release_folders():
    for entry in Path(".").glob("release*"):
        if entry.is_dir():
            shutil.rmtree(entry)
            print(f"Deleted folder: {entry}")

if __name__ == "__main__":
    target_folder = Path("./Bin/Addons")
    absolute_target = target_folder.resolve()

    confirm = input(f"Are you sure you want to delete all files (except .auadd) in:\n'{absolute_target}'? (y/N): ")
    if confirm.lower() in ['y', 'yes']:
        clean_addons(target_folder)
    else:
        print("Operation cancelled.")

    confirm = input("Do you want to remove Aurora's release zip files? (y/N): ")
    if confirm.lower() in ['y', 'yes']:
        remove_aurora_zip_files()
        print("Aurora zip files removed.")
    else:
        print("Operation cancelled.")

    confirm = input("Do you want to remove the release-* folders? (y/n): ")
    if confirm.lower() in ['y', 'yes']:
        remove_release_folders()
        print("Release folders removed.")
    else:
        print("Operation cancelled.")

    if get_os() == "linux":
        confirm = input(f"Do you want to remove the AppImage build artifacts ({APPIMAGE_NAME}, {APPIMAGE_TARGET_DIR}/)? (y/n): ")
        if confirm.lower() in ['y', 'yes']:
            remove_appimage_artifacts()
            print("AppImage artifacts removed.")
        else:
            print("Operation cancelled.")

        confirm = input(f"Do you want to remove the '{APPIMAGE_IMAGE}' podman image too? (y/n): ")
        if confirm.lower() in ['y', 'yes']:
            remove_appimage_build_image()
        else:
            print("Operation cancelled.")
