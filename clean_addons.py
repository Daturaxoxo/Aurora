from pathlib import Path

def clean_directory(target_path: Path):
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

if __name__ == "__main__":
    target_folder = Path("./Bin/Addons")
    
    absolute_target = target_folder.resolve()
    
    confirm = input(f"Are you sure you want to delete all files (except .auadd) in:\n'{absolute_target}'? (y/N): ")
    if confirm.lower() in ['y', 'yes']:
        clean_directory(target_folder)
    else:
        print("Operation cancelled.")