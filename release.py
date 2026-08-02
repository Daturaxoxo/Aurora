import hashlib
import json
import os
import shutil
import subprocess
import sys
import platform
from urllib.parse import quote

APPIMAGE_NAME = "Aurora-x86_64.AppImage"

BASE_URL = "https://host.getaurora.moe/files/app/"
# BASE_URL = "https://github.com/Alawapr/aurora-test/releases/latest/download/"
BLACKLISTED_EXTENSIONS = (
    "zip",
    "rar",
    "7z",
    "tar",
    "gz",
    "bz2",
    "xz",
    "zst",
    "lz4",
    "md5",
    "json",
    "py",
    "log",
    "ucas",
    "utoc",
    "pak",
    "disabled",
)

def get_os():
    os_name = platform.system().lower()

    if os_name == "windows":
        return "windows"
    elif os_name == "linux":
        return "linux"
    else:
        return os_name

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
        print(f"[COPY FILE]: File not found: {src}")
        return

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
        print(f"[COPY FOLDER]: Source folder not found: {src}")
        return

    if not os.path.isdir(src_path):
        print(f"[COPY FOLDER]: Source path is not a directory: {src}")
        return

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
        print(f"[GET ALL FILES]: Folder not found: {folder_path}")
        return

    for root, _, files in os.walk(folder_path):
        for file in files:
            if file.endswith(BLACKLISTED_EXTENSIONS):
                continue

            full_path = os.path.join(root, file)

            if relative:
                # Get path relative to the input folder and normalize slashes
                rel_path = os.path.relpath(full_path, folder_path).replace(os.sep, "/")
                file_paths.append(rel_path)
            else:
                file_paths.append(full_path)

    return file_paths


def build_manifest(
    version, base_dir=".", output_filename="manifest.json", base_url=BASE_URL
):
    """Scans base_dir and generates a manifest JSON file with file hashes."""
    files_list = []

    print("Scanning directories and calculating hashes...")

    for root, _, files in os.walk(base_dir):
        for file in files:
            if file.endswith(BLACKLISTED_EXTENSIONS):
                continue

            filepath = os.path.join(root, file)

            # Relative path with forward slashes
            rel_path = os.path.relpath(filepath, base_dir).replace(os.sep, "/")

            # Skip manifest output file if scanning current dir
            if rel_path == output_filename:
                continue

            file_hash = calculate_sha256(filepath)

            if file_hash:
                file_name = os.path.basename(rel_path)
                file_url = base_url + quote(file_name)

                files_list.append(
                    {"path": rel_path, "sha256": file_hash, "url": file_url}
                )

    os_name = get_os()
    updater_name = "updater.exe" if os_name == "windows" else "updater"
    updater_path = os.path.join(base_dir, updater_name)
    updater_hash = (
        calculate_sha256(updater_path) if os.path.exists(updater_path) else None
    )

    output_data = {
        "version": version,
        "updater_hash": updater_hash,
        "files": files_list,
    }

    manifest_path = os.path.join(base_dir, output_filename)
    with open(manifest_path, "w", encoding="utf-8") as json_file:
        json.dump(output_data, json_file, indent=2)

    print(f"Done! Processed {len(files_list)} files. Results saved to {manifest_path}")
    return output_data


def build_linux_manifest(version, appimage_path, output_path, base_url=BASE_URL):
    """
    Writes the Linux manifest.

    Linux ships one artifact rather than a file list: the AppImage is a single
    immutable file, so there is nothing to update piecewise. Aurora compares the
    hash below against the .AppImage it is running from.
    """
    file_hash = calculate_sha256(appimage_path)
    if file_hash is None:
        print(f"[LINUX MANIFEST]: could not hash {appimage_path}")
        sys.exit(1)

    output_data = {
        "version": version,
        "appimage": {
            "sha256": file_hash,
            "url": base_url + quote(APPIMAGE_NAME),
        },
    }

    output_dir = os.path.dirname(os.path.abspath(output_path))
    if output_dir:
        os.makedirs(output_dir, exist_ok=True)
    with open(output_path, "w", encoding="utf-8") as json_file:
        json.dump(output_data, json_file, indent=2)

    print(f"Wrote {output_path}")
    return output_data


def release_windows(version):
    if folder_exists("./release"):
        shutil.rmtree("./release")
    os.mkdir("./release")

    copy_file("./target/release/Aurora.exe", "./release/Aurora.exe")
    copy_file("./target/release/updater.exe", "./release/updater.exe")

    copy_folder("./Bin", "./release/Bin")
    build_manifest(version, "./release", "manifest.json", BASE_URL)
    copy_file("./steam_appid.txt", "./release/steam_appid.txt")
    shutil.make_archive(
        base_name=f"aurora-{version}-WINDOWS", format="zip", base_dir="./release"
    )

    if folder_exists("./release-host"):
        shutil.rmtree("./release-host")
    os.mkdir("./release-host")
    copy_file("./target/release/Aurora.exe", "./release-host/Aurora.exe")
    copy_file("./target/release/updater.exe", "./release-host/updater.exe")

    copy_file("./release/manifest.json", "./release-host/windows/manifest.json")

    for file in get_all_files("./release/Bin", relative=True) or []:
        file_name = path_to_filename(file)
        copy_file(f"./release/Bin/{file}", f"./release-host/{file_name}")

    shutil.make_archive(
        base_name=f"aurora-host-{version}-WINDOWS",
        format="zip",
        base_dir="./release-host",
    )


def release_linux(version):
    print("Building the AppImage...")
    subprocess.run(["./packaging/build-appimage.sh"], check=True)

    if not os.path.exists(APPIMAGE_NAME):
        print(f"[LINUX RELEASE]: {APPIMAGE_NAME} was not produced")
        sys.exit(1)

    if folder_exists("./release"):
        shutil.rmtree("./release")
    os.mkdir("./release")
    copy_file(APPIMAGE_NAME, f"./release/{APPIMAGE_NAME}")
    build_linux_manifest(version, APPIMAGE_NAME, "./release/manifest.json")

    if folder_exists("./release-host"):
        shutil.rmtree("./release-host")
    os.mkdir("./release-host")
    copy_file(APPIMAGE_NAME, f"./release-host/{APPIMAGE_NAME}")
    build_linux_manifest(version, APPIMAGE_NAME, "./release-host/linux/manifest.json")


def main():
    if len(sys.argv) < 2:
        print(f"Usage: python {sys.argv[0]} <version>")
        sys.exit(1)
    version = sys.argv[1]
    os_name = get_os()

    if os_name == "windows":
        release_windows(version)
    elif os_name == "linux":
        release_linux(version)
    else:
        print(f"[RELEASE]: unsupported platform: {os_name}")
        sys.exit(1)


if __name__ == "__main__":
    main()
