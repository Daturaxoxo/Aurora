import os
import shutil
import subprocess
import zipfile
from datetime import datetime

# CONFIGURATION 
APP_NAME = "Aurora"          # No spaces, avoids ShellExecuteW and PyInstaller errors
DISPLAY_NAME = "Aurora Launcher"
VERSION_FILE = os.path.join("dev", "VERSION")
if not os.path.exists(VERSION_FILE):
    print(f"FATAL: Version file not found at {VERSION_FILE}. Make sure it exists before building.")
    raise SystemExit(1)

with open(VERSION_FILE, "r", encoding="utf-8") as f: VERSION = f.read().strip()
ICON_PATH = "Bin/Assets/logo.ico"
MAIN_SCRIPT = "main.py"
DIST_DIR = f"./dist/{APP_NAME}_v{VERSION}"
SEP = ";" if os.name == "nt" else ":" # Should work both on Linux and Windows

def run_build():
    print(f"--- Starting Production Build for {DISPLAY_NAME} v{VERSION} ---")
    for folder in ("./dist", "./build"):
        if os.path.exists(folder):
            shutil.rmtree(folder)
            print(f"Cleared {folder}")

    build_cmd = [
        "python", "-m", "PyInstaller",
        "--noconfirm",
        "--onefile",
        "--windowed",
        f"--name={APP_NAME}",
        "--noupx",
        f"--add-data=Bin/Assets{SEP}Bin/Assets",
        f"--add-data=Lang{SEP}Lang",
        f"--add-data=dev/VERSION{SEP}dev",
        "--hidden-import=psutil",
        "--python-option=u",
        MAIN_SCRIPT
    ]
    if os.name == "nt":
        build_cmd.extend([
            f"--icon={ICON_PATH}",
            "--uac-admin",
            "--upx-exclude=vcruntime140.dll",
            "--hidden-import=psutil._pswindows",
        ])
    else:
        build_cmd.extend([
            "--hidden-import=psutil._pslinux",
        ])

    print("Compiling executable...")
    try: subprocess.run(build_cmd, check=True)
    except subprocess.CalledProcessError as e:
        print(f"\nFATAL: PyInstaller failed with exit code {e.returncode}.")
        print("Check the output above for the real error before proceeding.")
        raise SystemExit(1)

    print("Organizing output folder...")
    os.makedirs(DIST_DIR, exist_ok=True)
    os.makedirs(os.path.join(DIST_DIR, "Mods"), exist_ok=True)
    os.makedirs(os.path.join(DIST_DIR, "Logs"), exist_ok=True)

    ext = ".exe" if os.name == "nt" else ""
    src_exe = f"./dist/{APP_NAME}{ext}"
    dst_exe = os.path.join(DIST_DIR, f"{APP_NAME}{ext}")
    if not os.path.exists(src_exe):
        print(f"FATAL: Expected executable not found at {src_exe}. Build may have silently failed.")
        raise SystemExit(1)
    shutil.move(src_exe, dst_exe)

    if os.path.exists("./Bin"):
        bin_ignore = ["Assets"]
        if os.name != "nt":
            bin_ignore.extend(["7z.exe", "7z.dll"])
        shutil.copytree(
            "./Bin",
            os.path.join(DIST_DIR, "Bin"),
            dirs_exist_ok=True,
            ignore=shutil.ignore_patterns(*bin_ignore),
        )

    zip_name = f"{APP_NAME}_v{VERSION}.zip"
    print(f"Creating archive: {zip_name}")

    zip_anchor = "./dist"
    with zipfile.ZipFile(zip_name, 'w', zipfile.ZIP_DEFLATED) as zipf:
        for root, dirs, files in os.walk(DIST_DIR):
            for file in files:
                full_path = os.path.join(root, file)
                arcname = os.path.relpath(full_path, zip_anchor)
                zipf.write(full_path, arcname)

    print(f"\n✓ Build complete: {zip_name}")
    print(f"  EXE: {dst_exe}")

if __name__ == "__main__": run_build()