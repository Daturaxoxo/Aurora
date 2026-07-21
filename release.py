import hashlib
import json
import os
import shutil
import sys
from urllib.parse import quote

BASE_URL = "https://github.com/Alawapr/aurora-test/releases/latest/download/"
BLACKLISTED_EXTENSIONS = (
    "zip", "rar", "7z", "tar", "gz", "bz2", "xz", "zst", "lz4", 
    "md5", "json", "py", "log", "ucas", "utoc", "pak", "disabled"
)

def calculate_sha256(filepath, chunk_size=8192):
    """Calculate the SHA256 hash of a file."""
    sha256_hash = hashlib.sha256()
    try:
        with open(filepath, "rb") as f:
            while chunk := f.read(chunk_size):
                sha256_hash.update(chunk)
        return sha256_hash.hexdigest()
    except (OSError, PermissionError) as e:
        print(f"Skipping {filepath}: {e}")
        return None

def copy_file(src, dst):
    """
    Copies a file from `src` to `dst` relative paths.
    Creates destination directories if they don't exist.
    """
    src_path = os.path.abspath(src)
    dst_path = os.path.abspath(dst)

    if not os.path.exists(src_path):
        raise FileNotFoundError(f"Source file not found: {src}")

    # Ensure parent folder for destination exists
    dst_dir = os.path.dirname(dst_path)
    if dst_dir:
        os.makedirs(dst_dir, exist_ok=True)

    shutil.copy(src_path, dst_path)
    print(f"Copied {path_to_filename(src_path)} to {dst_path}")
    
def copy_folder(src, dst):
    """
    Recursively copies a folder and all its contents from `src` to `dst`.
    Works with relative or absolute paths.
    """
    src_path = os.path.abspath(src)
    dst_path = os.path.abspath(dst)

    if not os.path.exists(src_path):
        raise FileNotFoundError(f"Source folder not found: {src}")
    
    if not os.path.isdir(src_path):
        raise NotADirectoryError(f"Source path is not a directory: {src}")

    shutil.copytree(src_path, dst_path, dirs_exist_ok=True)
    print(f"Copied {src_path} to {dst_path}")
    
def folder_exists(path):
    return os.path.isdir(path)

def path_to_filename(path):
    return os.path.basename(path)

def get_all_files(folder_path, relative=False):
    """
    Recursively finds all files inside a directory and its subdirectories.
    
    :param folder_path: The root directory to scan.
    :param relative: If True, returns paths relative to folder_path.
                     If False, returns full/absolute paths.
    :return: A list of file path strings.
    """
    file_paths = []
    
    if not os.path.exists(folder_path):
        raise FileNotFoundError(f"Folder not found: {folder_path}")

    for root, _, files in os.walk(folder_path):
        for file in files:
            if file.endswith(BLACKLISTED_EXTENSIONS):
                continue
            full_path = os.path.join(root, file)
            
            if relative:
                # Get path relative to the input folder and normalize slashes
                rel_path = os.path.relpath(full_path, folder_path).replace(os.sep, '/')
                file_paths.append(rel_path)
            else:
                file_paths.append(full_path)

    return file_paths

def build_manifest(version, base_dir=".", output_filename="manifest.json", base_url=BASE_URL):
    """Scans base_dir and generates a manifest JSON file with file hashes."""
    files_list = []

    print("Scanning directories and calculating hashes...")

    for root, _, files in os.walk(base_dir):
        for file in files:
            if file.endswith(BLACKLISTED_EXTENSIONS):
                continue

            filepath = os.path.join(root, file)
            
            # Relative path with forward slashes
            rel_path = os.path.relpath(filepath, base_dir).replace(os.sep, '/')
            
            # Skip manifest output file if scanning current dir
            if rel_path == output_filename:
                continue

            file_hash = calculate_sha256(filepath)
            
            if file_hash:
                file_name = os.path.basename(rel_path)
                file_url = base_url + quote(file_name)

                files_list.append({
                    "path": rel_path,
                    "sha256": file_hash,
                    "url": file_url
                })

    updater_path = os.path.join(base_dir, "updater.exe")
    updater_hash = calculate_sha256(updater_path) if os.path.exists(updater_path) else None

    output_data = {
        "version": version,
        "updater_hash": updater_hash,
        "files": files_list
    }

    manifest_path = os.path.join(base_dir, output_filename)
    with open(manifest_path, "w", encoding="utf-8") as json_file:
        json.dump(output_data, json_file, indent=2)

    print(f"Done! Processed {len(files_list)} files. Results saved to {manifest_path}")
    return output_data

def main():
    if sys.argv[1] is None:
        print(f"Usage: python {sys.argv[0]} <version>")
        sys.exit(1)
    version = sys.argv[1]
    if folder_exists("./release"):
        shutil.rmtree("./release")
    os.mkdir("./release")
    copy_file("./target/release/aurora.exe", "./release/aurora.exe")
    copy_file("./target/release/updater.exe", "./release/updater.exe")
    copy_folder("./Bin", "./release/Bin")
    build_manifest(version, "./release", "manifest.json", BASE_URL)
    shutil.make_archive(base_name=f"aurora-{version}", format="zip", base_dir="./release")
    
    if folder_exists("./release-host"):
        shutil.rmtree("./release-host")
    os.mkdir("./release-host")
    copy_file("./target/release/aurora.exe", "./release-host/aurora.exe")
    copy_file("./target/release/updater.exe", "./release-host/updater.exe")
    copy_file("./release/manifest.json", "./release-host/manifest.json")
    for file in get_all_files("./release/Bin", relative=True):
        file_name = path_to_filename(file)
        copy_file(f"./release/Bin/{file}", f"./release-host/{file_name}")

if __name__ == "__main__":
    main()