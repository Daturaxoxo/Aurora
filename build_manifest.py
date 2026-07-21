"""
Script to generate a manifest.json for use in updating Aurora.
Use it in a folder with all of Aurora's files, providing it the version number (X.Y.Z).
Example:
    python release.py 2.3.1
    
    In a folder that looks like:
    | Aurora.exe
    | updater.exe
    | Bin/
    |   | AuroraEngine.dll
    |   | Everlight.asi 
    | ETC
    
    will generate a manifest.json that looks like:
    {
        "version": "2.3.1",
        "update_hash": "...",
        "files": [
            { "path": "Aurora.exe", "sha256": "...", "url": "URL/Aurora.exe" },
            { "path": "updater.exe", "sha256": "...", "url": "URL/updater.exe" },
            { "path": "Bin/AuroraEngine.dll", "sha256": "...", "url": "URL/Bin/AuroraEngine.dll" },
            { "path": "Bin/Everlight.asi", "sha256": "...", "url": "URL/Bin/Everlight.asi" }
        ]
    }
"""

import os
import hashlib
import json
import sys
from urllib.parse import quote

BASE_URL = "https://github.com/Alawapr/aurora-test/releases/latest/download/"

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

def main():
    output_filename = "manifest.json"
    base_dir = "."
    files_list = []
    try:
        version = sys.argv[1]
    except IndexError:
        print("Usage: python release.py <version>")
        sys.exit(1)

    print("Scanning directories and calculating hashes...")

    for root, _, files in os.walk(base_dir):
        for file in files:
            BLACKLISTED_EXTENSIONS = ["zip", "rar", "7z", "tar", "gz", "bz2", "xz", "zst", "lz4", "md5", "json", "py", "log", "ucas", "utoc", "pak", "disabled"]
            if file.endswith(tuple(BLACKLISTED_EXTENSIONS)):
                continue
            filepath = os.path.join(root, file)
            
            # Get the relative path starting from the current directory
            rel_path = os.path.relpath(filepath, base_dir)
            
            # Normalize path separators to forward slashes
            rel_path = rel_path.replace(os.sep, '/')
            
            # Skip the output file itself
            if rel_path == output_filename:
                continue

            file_hash = calculate_sha256(filepath)
            
            if file_hash:
                file_name = os.path.basename(rel_path)
                file_url = BASE_URL + quote(file_name)

                files_list.append({
                    "path": rel_path,
                    "sha256": file_hash,
                    "url": file_url
                })

    output_data = {
        "version": version,
        "updater_hash": calculate_sha256("updater.exe"),
        "files": files_list
    }

    with open(output_filename, "w", encoding="utf-8") as json_file:
        json.dump(output_data, json_file, indent=2)
        
    print(f"Done! Processed {len(files_list)} files. Results saved to {output_filename}")

main()