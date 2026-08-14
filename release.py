import hashlib
import json
import os
import platform
import shutil
import subprocess
import sys
from urllib.parse import quote

APPIMAGE_NAME = "Aurora-x86_64.AppImage"
PATH_SEPARATOR_ENCODING = "__"
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
BLACKLISTED_PATHS = ("Bin/Addons/Censorship/AuroraTF.asi","Bin/Addons/Censorship/CNAuroraTF.asi")


def is_blacklisted(path):
    if path.endswith(BLACKLISTED_EXTENSIONS):
        return True

    normalized = path.replace("\\", "/")
    return any(
        normalized == blacklisted or normalized.endswith("/" + blacklisted)
        for blacklisted in BLACKLISTED_PATHS
    )


def encode_path(rel_path):
    return rel_path.replace("\\", "/").replace("/", PATH_SEPARATOR_ENCODING)

def check_encodable(rel_path):
    for component in rel_path.replace("\\", "/").split("/"):
        if PATH_SEPARATOR_ENCODING in component:
            print(
                f"RELEASE: path component '{component}' in '{rel_path}' contains "
                f"'{PATH_SEPARATOR_ENCODING}', which is reserved for encoding "
                "path separators. Rename the file."
            )
            sys.exit(1)


def get_os():
    os_name = platform.system().lower()

    if os_name == "windows":
        return "windows"
    elif os_name == "linux":
        return "linux"
    else:
        return os_name


def calculate_sha256(filepath, chunk_size=8192):
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
    src_path = os.path.abspath(src)
    dst_path = os.path.abspath(dst)

    if not os.path.exists(src_path):
        print(f"File not found: {src}")
        return

    # Ensure parent folder for destination exists
    dst_dir = os.path.dirname(dst_path)
    if dst_dir:
        os.makedirs(dst_dir, exist_ok=True)

    shutil.copy(src_path, dst_path)
    print(f"Copied {path_to_filename(src_path)} to {dst_path}")


def copy_folder(src, dst):
    src_path = os.path.abspath(src)
    dst_path = os.path.abspath(dst)

    if not os.path.exists(src_path):
        print(f"Source folder not found: {src}")
        return

    if not os.path.isdir(src_path):
        print(f"Source path is not a directory: {src}")
        return

    shutil.copytree(src_path, dst_path, dirs_exist_ok=True)
    print(f"Copied {src_path} to {dst_path}")


def folder_exists(path):
    return os.path.isdir(path)


def path_to_filename(path):
    return os.path.basename(path)


def get_all_files(folder_path, relative=False):
    file_paths = []

    if not os.path.exists(folder_path):
        print(f"Folder not found: {folder_path}")
        return

    for root, _, files in os.walk(folder_path):
        for file in files:
            full_path = os.path.join(root, file)

            if is_blacklisted(full_path):
                continue

            if relative:
                rel_path = os.path.relpath(full_path, folder_path).replace(os.sep, "/")
                file_paths.append(rel_path)
            else:
                file_paths.append(full_path)

    return file_paths


def check_flat_names(files_list):
    seen = {}
    for entry in files_list:
        name = path_to_filename(entry["path"])
        if name in seen:
            print(
                f"RELEASE: '{entry['path']}' and '{seen[name]}' are both named "
                f"'{name}', but the host serves every file from one folder. "
                "Rename one of them."
            )
            sys.exit(1)
        seen[name] = entry["path"]


def build_manifest(
    version, base_dir=".", output_filename="manifest.json", base_url=BASE_URL
):
    files_list = []

    print("Scanning directories and calculating hashes...")

    for root, _, files in os.walk(base_dir):
        for file in files:
            filepath = os.path.join(root, file)

            if is_blacklisted(filepath):
                continue
            rel_path = os.path.relpath(filepath, base_dir).replace(os.sep, "/")

            if rel_path == output_filename: continue

            file_hash = calculate_sha256(filepath)

            if file_hash:
                file_url = base_url + quote(path_to_filename(rel_path))

                files_list.append(
                    {"path": rel_path, "sha256": file_hash, "url": file_url}
                )

    check_flat_names(files_list)

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

    print(f"Release script completely successfully! Processed {len(files_list)} files.")
    return output_data


def build_linux_manifest(version, appimage_path, output_path, base_url=BASE_URL):
    file_hash = calculate_sha256(appimage_path)
    if file_hash is None:
        print(f"LINUX: could not hash {appimage_path}")
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

    return output_data


def encode_manifest(manifest, base_url=BASE_URL):
    if "appimage" in manifest:
        return manifest

    encoded = dict(manifest)
    encoded["files"] = [
        {**entry, "url": base_url + quote(encode_path(entry["path"]))}
        for entry in manifest["files"]
    ]
    return encoded


def build_github_release(os_name, source_dir, manifest, version):
    out_dir = "./release-github"
    if folder_exists(out_dir):
        shutil.rmtree(out_dir)
    os.makedirs(out_dir, exist_ok=True)

    if "appimage" in manifest:
        rel_paths = [APPIMAGE_NAME]
    else:
        rel_paths = [entry["path"] for entry in manifest["files"]]

    for rel_path in rel_paths:
        check_encodable(rel_path)
        copy_file(
            os.path.join(source_dir, rel_path),
            os.path.join(out_dir, encode_path(rel_path)),
        )

    manifest_name = encode_path(f"{os_name}/manifest.json")
    manifest_path = os.path.join(out_dir, manifest_name)
    with open(manifest_path, "w", encoding="utf-8") as json_file:
        json.dump(encode_manifest(manifest), json_file, indent=2)
    print(f"Wrote {manifest_path}")

    archive_name = f"aurora-github-{version}-{os_name}"
    archive_path = os.path.join(out_dir, archive_name)
    shutil.make_archive(base_name=archive_name, format="zip", base_dir=out_dir)
    print(f"Wrote {archive_path}")

    print(
        f"GITHUB: {len(rel_paths)} asset(s) + {manifest_name} ready in {out_dir}; "
        f"upload them to the {version} release."
    )


def uninstaller_path():
    slim = "./target/x86_64-pc-windows-msvc/release/AuroraUninstaller.exe"
    plain = "./target/release/AuroraUninstaller.exe"

    if os.path.exists(slim):
        return slim
    if os.path.exists(plain):
        print(
            "RELEASE: warning: using {} -- run ./build-installer.ps1 for the "
            "smaller build".format(plain)
        )
        return plain

    print("RELEASE: AuroraUninstaller.exe not found; run ./build-installer.ps1")
    sys.exit(1)


def release_windows(version):
    if folder_exists("./release"):
        shutil.rmtree("./release")
    os.mkdir("./release")

    uninstaller = uninstaller_path()

    copy_file("./target/release/Aurora.exe", "./release/Aurora.exe")
    copy_file("./target/release/updater.exe", "./release/updater.exe")
    copy_file(uninstaller, "./release/AuroraUninstaller.exe")

    copy_folder("./Bin", "./release/Bin")
    manifest = build_manifest(version, "./release", "manifest.json", BASE_URL)
    copy_file("./steam_appid.txt", "./release/steam_appid.txt")
    shutil.make_archive(
        base_name=f"aurora-{version}-WINDOWS", format="zip", root_dir="./release"
    )

    if folder_exists("./release-host"):
        shutil.rmtree("./release-host")
    os.mkdir("./release-host")
    copy_file("./target/release/Aurora.exe", "./release-host/Aurora.exe")
    copy_file("./target/release/updater.exe", "./release-host/updater.exe")
    copy_file(uninstaller, "./release-host/AuroraUninstaller.exe")

    copy_file("./release/manifest.json", "./release-host/windows/manifest.json")

    for file in get_all_files("./release/Bin", relative=True) or []:
        copy_file(f"./release/Bin/{file}", f"./release-host/{path_to_filename(file)}")

    shutil.make_archive(
        base_name=f"aurora-host-{version}-WINDOWS",
        format="zip",
        root_dir="./release-host",
    )

    build_github_release("windows", "./release", manifest, version)


def release_linux(version):
    print("Building the AppImage...")
    subprocess.run(["./packaging/build-appimage.sh"], check=True)

    if not os.path.exists(APPIMAGE_NAME):
        print(f"LINUX: {APPIMAGE_NAME} was not produced")
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
    manifest = build_linux_manifest(
        version, APPIMAGE_NAME, "./release-host/linux/manifest.json"
    )
    shutil.make_archive(
        base_name=f"aurora-host-{version}-LINUX",
        format="zip",
        root_dir="./release-host",
    )

    build_github_release("linux", "./release", manifest, version)


def main():
    if len(sys.argv) < 2:
        print(f"Actual Usage: python {sys.argv[0]} <version>")
        sys.exit(1)
    version = sys.argv[1]
    os_name = get_os()

    if os_name == "windows":
        release_windows(version)
    elif os_name == "linux":
        release_linux(version)
    else:
        print(f"RELEASE: unsupported platform: {os_name}")
        sys.exit(1)


if __name__ == "__main__":
    main()
