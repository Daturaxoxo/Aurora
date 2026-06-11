import json, time, requests, re
from pathlib import Path
from typing import List, Callable, Optional
from concurrent.futures import ThreadPoolExecutor, as_completed

from src.logger import logger
from src.path_finder import get_app_dir, get_local_version

CACHE_DIR = Path(get_app_dir()) / "Cache" / "GameBanana"
CACHE_TTL = 3600

class NTEMod:
    def __init__(
        self,
        id,
        name,
        thumbnail,
        author: str = "Unknown",
        view_count: int = 0,
        download_count: int = 0,
        like_count: int = 0,
        is_nsfw: bool = False,
        root_category: str = "",
        sub_category: str = "",
        mod_url: str = "",
    ):
        self.id = id
        self.name = name
        self.thumbnail = thumbnail
        self.author = author
        self.view_count = view_count
        self.download_count = download_count
        self.like_count = like_count
        self.is_nsfw = is_nsfw
        self.root_category = root_category
        self.sub_category = sub_category
        self.mod_url = mod_url

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


def _load_page_from_cache(page: int) -> Optional[List[NTEMod]]:
    try:
        data = json.loads(_page_cache_path(page).read_text(encoding="utf-8"))
        td = _thumb_dir()
        return [
            NTEMod(
                id=entry["id"],
                name=entry["name"],
                thumbnail=(
                    (td / f"{entry['id']}.png").read_bytes()
                    if (td / f"{entry['id']}.png").exists()
                    else b""
                ),
                author=entry.get("author", "Unknown"),
                view_count=entry.get("view_count", 0),
                download_count=entry.get("download_count", 0),
                like_count=entry.get("like_count", 0),
                is_nsfw=entry.get("is_nsfw", False),
                root_category=entry.get("root_category", ""),
                sub_category=entry.get("sub_category", ""),
                mod_url=entry.get("mod_url", ""),
            )
            for entry in data.get("mods", [])
        ]
    except Exception:
        return None


def _save_page_to_cache(page: int, mods: List[NTEMod]):
    td = _thumb_dir()
    for m in mods:
        if m.thumbnail:
            (td / f"{m.id}.png").write_bytes(m.thumbnail)
    data = {
        "cached_at": time.time(),
        "page": page,
        "mods": [
            {
                "id": m.id,
                "name": m.name,
                "author": m.author,
                "view_count": m.view_count,
                "download_count": m.download_count,
                "like_count": m.like_count,
                "is_nsfw": m.is_nsfw,
                "root_category": m.root_category,
                "sub_category": m.sub_category,
                "mod_url": m.mod_url,
            }
            for m in mods
        ],
    }
    _page_cache_path(page).write_text(json.dumps(data, ensure_ascii=False), encoding="utf-8")


def clear_cache():
    import shutil
    if CACHE_DIR.exists():
        shutil.rmtree(CACHE_DIR)

def _detect_nsfw(mod: dict) -> bool:
    visibility = mod.get("_sInitialVisibility", "")
    if visibility in ("warn", "hide"):
        return True
    if visibility == "show":
        return False

    if mod.get("_bHasNsfwContent") or mod.get("_bIsNsfw"):
        return True
    for key in ("_aRootCategory", "_aSubCategory"):
        cat = mod.get(key)
        if isinstance(cat, dict):
            if "nsfw" in cat.get("_sName", "").lower():
                return True
    return False

def _fetch_one(mod: dict, headers: dict) -> NTEMod:
    mod_id = mod["_idRow"]
    img_files: dict = mod["_aPreviewMedia"]["_aImages"][0]
    n = max(k.split("e")[1] for k in img_files if "_sFile" in k and k != "_sFile")
    thumb_url = (
        f"https://images.gamebanana.com/img/ss/mods/{n}-90_"
        f"{img_files['_sFile']}"
    )
    thumb_resp = requests.get(thumb_url, headers=headers, timeout=15)
    thumb_resp.raise_for_status()

    download_count = 0
    try:
        detail_url = (
            f"https://api.gamebanana.com/Core/Item/Data"
            f"?itemtype=Mod&itemid={mod_id}"
            f"&fields=downloads&return_keys=1&format=json_min"
        )
        detail_resp = requests.get(detail_url, headers=headers, timeout=15)
        if detail_resp.ok:
            detail = detail_resp.json()
            download_count = int(detail.get("downloads", 0))
    except Exception as e:
        logger.error(f"Download count fetch failed for mod {mod_id}: {e}")

    author = "Unknown"
    if isinstance(mod.get("_aSubmitter"), dict):
        author = mod["_aSubmitter"].get("_sName", "Unknown")

    root_cat = ""
    if isinstance(mod.get("_aRootCategory"), dict):
        root_cat = mod["_aRootCategory"].get("_sName", "")
    sub_cat = ""
    if isinstance(mod.get("_aSubCategory"), dict):
        sub_cat = mod["_aSubCategory"].get("_sName", "")

    mod_url = mod.get("_sProfileUrl", "") or f"https://gamebanana.com/mods/{mod_id}"
    
    return NTEMod(
        id=mod_id,
        name=mod["_sName"],
        thumbnail=thumb_resp.content,
        author=author,
        view_count=mod.get("_nViewCount", 0),
        download_count=download_count,
        like_count=mod.get("_nLikeCount", 0),
        is_nsfw=_detect_nsfw(mod),
        root_category=root_cat,
        sub_category=sub_cat,
        mod_url=mod_url,
    )

def get_nte_mods(
    force_refresh: bool = False,
    page: int = 1,
    on_mod_ready: Optional[Callable[[NTEMod], None]] = None,
    nsfw_mods: bool = False
) -> Optional[List[NTEMod]]:

    # Cache load first
    if not force_refresh and _is_page_cached(page):
        cached = _load_page_from_cache(page)
        if cached is not None:
            if on_mod_ready:
                for m in cached:
                    on_mod_ready(m)
            return cached

    headers = {
        "User-Agent": f"AuroraLauncher/{get_local_version()}",
    }
    list_url = "https://gamebanana.com/apiv12/Game/23012/Subfeed"
    list_params = {"_nPage": page}

    try:
        logger.info(f"Requesting GameBanana page {page}...", extra={"el": True})
        list_response = requests.get(list_url, params=list_params, headers=headers, timeout=15)
        list_response.raise_for_status()
        submissions = list_response.json().get("_aRecords", [])

        if not submissions or not isinstance(submissions, list):
            logger.error("No data or invalid game ID returned.")
            return None

        only_mods = [item for item in submissions if item.get("_sModelName") == "Mod"]
        if not only_mods:
            logger.info("No actual mods found on this page.")
            return None

        nte_mods: List[NTEMod] = []
        with ThreadPoolExecutor(max_workers=8) as pool:
            futures = {pool.submit(_fetch_one, m, headers): m for m in only_mods}
            for future in as_completed(futures):
                try:
                    mod = future.result()
                    nte_mods.append(mod)
                    if on_mod_ready: on_mod_ready(mod)
                except Exception as e:
                    if getattr(e, "status_code", None) == 404 or getattr(getattr(e, "response", None), 'status_code', None) == 404: pass
                    else: logger.error(f"Mod fetch failed: {e}")

        if nte_mods:
            _save_page_to_cache(page, nte_mods)

        return nte_mods

    except requests.exceptions.RequestException as e:
        logger.error(f"Error communicating with the GameBanana API: {e}")
        return None
    
def _search_cache_path(query: str, page: int) -> Path:
    safe = re.sub(r"[^\w\-]", "_", query.strip().lower())[:60]
    p = CACHE_DIR / "search" / f"{safe}_p{page}.json"
    p.parent.mkdir(parents=True, exist_ok=True)
    return p
 
def _is_search_cached(query: str, page: int) -> bool:
    path = _search_cache_path(query, page)
    if not path.exists(): return False
    try:
        data = json.loads(path.read_text(encoding="utf-8"))
        return time.time() - data.get("cached_at", 0) < CACHE_TTL
    except Exception: return False
    
def _load_search_from_cache(query: str, page: int) -> Optional[List[NTEMod]]:
    try:
        data = json.loads(_search_cache_path(query, page).read_text(encoding="utf-8"))
        td = _thumb_dir()
        return [
            NTEMod(
                id=entry["id"],
                name=entry["name"],
                thumbnail=(
                    (td / f"{entry['id']}.png").read_bytes()
                    if (td / f"{entry['id']}.png").exists()
                    else b""
                ),
                author=entry.get("author", "Unknown"),
                view_count=entry.get("view_count", 0),
                download_count=entry.get("download_count", 0),
                like_count=entry.get("like_count", 0),
                is_nsfw=entry.get("is_nsfw", False),
                root_category=entry.get("root_category", ""),
                sub_category=entry.get("sub_category", ""),
                mod_url=entry.get("mod_url", ""),
            )
            for entry in data.get("mods", [])
        ]
    except Exception: return None
    
def _save_search_to_cache(query: str, page: int, mods: List[NTEMod]):
    td = _thumb_dir()
    for m in mods:
        if m.thumbnail: (td / f"{m.id}.png").write_bytes(m.thumbnail)
    data = {
        "cached_at": time.time(),
        "query": query,
        "page": page,
        "mods": [
            {
                "id": m.id,
                "name": m.name,
                "author": m.author,
                "view_count": m.view_count,
                "download_count": m.download_count,
                "like_count": m.like_count,
                "is_nsfw": m.is_nsfw,
                "root_category": m.root_category,
                "sub_category": m.sub_category,
                "mod_url": m.mod_url,
            }
            for m in mods
        ],
    }
    _search_cache_path(query, page).write_text(json.dumps(data, ensure_ascii=False), encoding="utf-8")
    
def search_nte_mods(
    query: str,
    page: int = 1,
    force_refresh: bool = False,
    on_mod_ready: Optional[Callable[[NTEMod], None]] = None,
) -> Optional[List[NTEMod]]:
    query = query.strip()
    if not query: return []
    if len(query) < 3: logger.warning(f"Search query '{query}' is too short."); return []
 
    # Cache hit
    if not force_refresh and _is_search_cached(query, page):
        cached = _load_search_from_cache(query, page)
        if cached is not None:
            if on_mod_ready:
                for m in cached: on_mod_ready(m)
            return cached
 
    headers = {"User-Agent": f"AuroraLauncher/{get_local_version()}"}
    list_url = (
        f"https://gamebanana.com/apiv12/Game/23012/Subfeed"
        f"?_nPage={page}&_sName={requests.utils.quote(query, safe='')}"
    )
 
    try:
        logger.info( f"Searching GameBanana for '{query}' page {page}",extra={"el": True},)
        resp = requests.get(list_url, headers=headers, timeout=15)
        resp.raise_for_status()
        submissions = resp.json().get("_aRecords", [])
 
        if not submissions or not isinstance(submissions, list):
            logger.info(f"No search results for '{query}' page {page}.")
            return []
        only_mods = [s for s in submissions if s.get("_sModelName") == "Mod"]
        if not only_mods: return []
 
        nte_mods: List[NTEMod] = []
        with ThreadPoolExecutor(max_workers=8) as pool:
            futures = {pool.submit(_fetch_one, m, headers): m for m in only_mods}
            for future in as_completed(futures):
                try:
                    mod = future.result()
                    nte_mods.append(mod)
                    if on_mod_ready: on_mod_ready(mod)
                except Exception as e:
                    if (getattr(e, "status_code", None) == 404or getattr(getattr(e, "response", None), "status_code", None) == 404): pass
                    else: logger.error(f"Search mod fetch failed: {e}")
 
        if nte_mods:_save_search_to_cache(query, page, nte_mods)
        return nte_mods
 
    except requests.exceptions.RequestException as e:
        logger.error(f"GameBanana search request error: {e}")
        return None

class NTEModFile:
    def __init__(
        self,
        id: int,
        name: str,
        size: int,
        download_count: int,
        url: str,
        md5: str,
        is_archived: bool,
        has_contents: bool
    ):
        self.id = id
        self.name = name
        self.size = size
        self.download_count = download_count
        self.url = url
        self.md5 = md5
        self.is_archived = is_archived
        self.has_contents = has_contents
    
def get_mod_files(mod_id: int) -> Optional[List[NTEModFile]]:
    headers = {
        "User-Agent": f"AuroraLauncher/{get_local_version()}",
    }
    detail_url = (f"https://gamebanana.com/apiv12/Mod/{mod_id}/ProfilePage")
    detail_resp = requests.get(detail_url, headers=headers, timeout=15)
    files = []
    if detail_resp.ok:
        detail = detail_resp.json()
        for file in detail.get("_aFiles", []):
            files.append(NTEModFile(
                id=file.get("_idRow", 0),
                name=file.get("_sFile", ""),
                size=file.get("_nFilesize", 0),
                download_count=file.get("_nDownloadCount", 0),
                url=file.get("_sDownloadUrl", ""),
                md5=file.get("_sMd5Checksum", ""),
                is_archived=file.get("_bIsArchived", False),
                has_contents=file.get("_bHasContents", False)
            ))
        logger.info(f"Retrieved files for mod {mod_id}")
        return files
    logger.error(f"Failed to retrieve files for mod {mod_id}")
    return None