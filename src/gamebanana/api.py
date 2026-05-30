import json
import time
from pathlib import Path
from typing import List
from concurrent.futures import ThreadPoolExecutor, as_completed

import requests

from src.path_finder import get_app_dir

CACHE_DIR = Path(get_app_dir()) / "Cache" / "GameBanana"
CACHE_TTL = 3600

class NTEMod:
    def __init__(self, id, name, thumbnail):
        self.id = id
        self.name = name
        self.thumbnail = thumbnail

def _page_cache_path(page: int) -> Path:
    p = CACHE_DIR / f"page_{page}.json"
    p.parent.mkdir(parents=True, exist_ok=True)
    return p

def _thumb_dir() -> Path:
    p = CACHE_DIR / "thumbnails"
    p.mkdir(parents=True, exist_ok=True)
    return p

def _is_page_cached(page: int) -> bool:
    path = _page_cache_path(page)
    if not path.exists():
        return False
    try:
        data = json.loads(path.read_text(encoding="utf-8"))
        return time.time() - data.get("cached_at", 0) < CACHE_TTL
    except Exception:
        return False

def _load_page_from_cache(page: int) -> List[NTEMod] | None:
    try:
        data = json.loads(_page_cache_path(page).read_text(encoding="utf-8"))
        td = _thumb_dir()
        return [
            NTEMod(
                entry["id"],
                entry["name"],
                (td / f"{entry['id']}.png").read_bytes()
                if (td / f"{entry['id']}.png").exists()
                else b"",
            )
            for entry in data.get("mods", [])
        ]
    except Exception:
        return None

def _save_page_to_cache(page: int, mods: List[NTEMod]):
    td = _thumb_dir()
    for m in mods:
        (td / f"{m.id}.png").write_bytes(m.thumbnail)
    data = {
        "cached_at": time.time(),
        "page": page,
        "mods": [{"id": m.id, "name": m.name} for m in mods],
    }
    _page_cache_path(page).write_text(json.dumps(data, ensure_ascii=False), encoding="utf-8")

def clear_cache():
    import shutil
    if CACHE_DIR.exists():
        shutil.rmtree(CACHE_DIR)

def _fetch_thumbnail(mod: dict, headers: dict) -> NTEMod:
    img_files: dict = mod["_aPreviewMedia"]["_aImages"][0]
    n = max(k.split("e")[1] for k in img_files if "_sFile" in k and k != "_sFile")
    thumb_url = f"https://images.gamebanana.com/img/ss/mods/{n}-90_{mod["_aPreviewMedia"]["_aImages"][0]["_sFile"]}"
    resp = requests.get(thumb_url, headers=headers, timeout=15)
    resp.raise_for_status()
    return NTEMod(mod["_idRow"], mod["_sName"], resp.content)


def get_nte_mods(force_refresh: bool = False, page: int = 1) -> List[NTEMod] | None:
    if not force_refresh and _is_page_cached(page):
        cached = _load_page_from_cache(page)
        if cached is not None:
            return cached

    headers = {
        "User-Agent": "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36"
    }

    list_url = "https://gamebanana.com/apiv12/Game/23012/Subfeed"
    list_params = {"_nPage": page}

    try:
        print(f"Requesting GameBanana page {page}...")
        list_response = requests.get(list_url, params=list_params, headers=headers, timeout=15)
        list_response.raise_for_status()
        submissions = list_response.json()["_aRecords"]

        if not submissions or not isinstance(submissions, list):
            print("No data or invalid game ID returned.")
            return None

        only_mods = [item for item in submissions if item["_sModelName"] == "Mod"]

        if not only_mods:
            print("No actual mods found on this page.")
            return None

        nte_mods = []
        with ThreadPoolExecutor(max_workers=16) as pool:
            futures = [pool.submit(_fetch_thumbnail, m, headers) for m in only_mods]
            for future in as_completed(futures):
                try:
                    nte_mods.append(future.result())
                except requests.RequestException as e:
                    print(f"Thumbnail download failed: {e}")

        if nte_mods:
            _save_page_to_cache(page, nte_mods)

        return nte_mods

    except requests.exceptions.RequestException as e:
        print(f"Error communicating with the GameBanana API: {e}")