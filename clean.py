from pathlib import Path
import re

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
    """Removes files matching any of the following patterns:

      - aurora-{STAPLE.MAJOR.MINOR}-{WINDOWS,LINUX}.zip
      - aurora-{STAPLE.MAJOR.MINOR}-{JUNK}-{WINDOWS,LINUX}.zip
      - aurora-host-{STAPLE.MAJOR.MINOR}-{WINDOWS,LINUX}.zip
      - aurora-host-{STAPLE.MAJOR.MINOR}-{JUNK}-{WINDOWS,LINUX}.zip

    Returns a list of deleted filenames.
    """
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
