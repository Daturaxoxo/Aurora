use crate::classes::pages::sanitize_download_filename;
use crate::classes::toast::ToastHandler;
use crate::{AddonItem, MainWindow};
use backend::classes::addons::payload_files;
use shared::archive::{ARCHIVE_EXTENSIONS, extract_archive};
use shared::classes::gamebanana::api::GameBananaApi;
use shared::classes::info::addons;
use shared::classes::info::version::{Version, detect_version};
use shared::utils::get_cache_dir;
use shared::{config, pathfind, utils};

use anyhow::{Context, Result};
use log::*;
use slint::{Model, VecModel};

use std::fs;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::Mutex;

static PENDING_DELETE: Mutex<Option<usize>> = Mutex::new(None);

const ADDON_CONFIG_KEYS: [(&str, &str); 6] = [
    ("No 3D Driving Waypoint", "drv_lin"),
    ("Hide UID", "uid_rem"),
    ("Hide Notification Dots", "nor_rem"),
    ("Censorship Remover", "csn_rem"),
    ("Cooldown Timers", "col_tim"),
    ("Collectible Highlighter", "collectibles"),
];

fn config_key(name: &str) -> Option<&'static str> {
    ADDON_CONFIG_KEYS
        .iter()
        .find(|(addon_name, _)| *addon_name == name)
        .map(|(_, config_key)| *config_key)
}

#[derive(Debug, Clone, Default)]
struct AddonData {
    file_name: String,
    url: String,
    md5: String,
}

impl AddonData {
    const fn new(file_name: String, url: String, md5: String) -> Self {
        Self {
            file_name,
            url,
            md5,
        }
    }
}

#[derive(Debug, Clone, Default)]
struct Addon {
    folder: PathBuf,
    name: String,
    author: String,
    version: String,
    description: String,
    install_data: Vec<AddonData>,
    link: String,
    image_url: String,
    installed: bool,
    enabled: bool,
    update_available: bool,
}

pub struct AddonsHandler;

impl AddonsHandler {
    pub fn setup(window: &slint::Weak<MainWindow>) {
        info!("Addon Manager setup() called");
        Self::load(window);
        Self::bind(window);
        info!("Addon Manager setup() complete");
    }

    fn load(window: &slint::Weak<MainWindow>) {
        let ww = window.clone();
        std::thread::spawn(move || {
            let addons = Self::scan();
            let version = Self::detected_version();

            let _ = slint::invoke_from_event_loop(move || {
                let Some(w) = ww.upgrade() else {
                    error!("Addons manager could not load: window handle is dead");
                    return;
                };

                let slint_items: Vec<AddonItem> = addons
                    .iter()
                    .map(|addon| Self::to_slint_item(addon, version))
                    .collect();

                w.set_addons(Rc::new(VecModel::from(slint_items)).into());
                Self::fetch_images_async(&ww, addons);
            });
        });
    }

    fn image_cache_dir() -> PathBuf {
        get_cache_dir().join("Addons")
    }

    fn cache_filename(url: &str) -> String {
        use std::hash::{Hash, Hasher};
        let mut hasher = std::collections::hash_map::DefaultHasher::new();
        url.hash(&mut hasher);
        let hash = hasher.finish();
        let ext = url
            .rsplit('.')
            .next()
            .filter(|e| e.len() <= 5 && e.chars().all(char::is_alphanumeric))
            .unwrap_or("img");
        format!("{hash:016x}.{ext}")
    }

    fn load_image_cached(url: &str) -> anyhow::Result<(Vec<u8>, u32, u32)> {
        let cache_dir = Self::image_cache_dir();
        let cache_path = cache_dir.join(Self::cache_filename(url));

        if cache_path.exists() {
            let bytes = std::fs::read(&cache_path)?;
            let img = image::load_from_memory(&bytes)?.into_rgba8();
            let (w, h) = img.dimensions();
            return Ok((img.into_raw(), w, h));
        }

        let bytes = reqwest::blocking::Client::builder()
            .timeout(std::time::Duration::from_secs(10))
            .build()?
            .get(url)
            .send()?
            .bytes()?;

        if let Err(e) = std::fs::create_dir_all(&cache_dir) {
            warn!(
                "Addons manager could not create cache dir '{}': {e}",
                cache_dir.display()
            );
        } else if let Err(e) = std::fs::write(&cache_path, &bytes) {
            warn!(
                "Addons manager could not write cache file '{}': {e}",
                cache_path.display()
            );
        } else {
            debug!("[Addons] cached image to '{}'", cache_path.display());
        }

        let img = image::load_from_memory(&bytes)?.into_rgba8();
        let (w, h) = img.dimensions();
        Ok((img.into_raw(), w, h))
    }

    fn fetch_images_async(window: &slint::Weak<MainWindow>, addons: Vec<Addon>) {
        let image_jobs: Vec<(usize, String)> = addons
            .into_iter()
            .enumerate()
            .filter(|(_, a)| !a.image_url.is_empty())
            .map(|(i, a)| (i, a.image_url))
            .collect();

        for (index, url) in image_jobs {
            let ww = window.clone();
            std::thread::spawn(move || {
                let rgba_data = Self::load_image_cached(&url)
                    .map_err(|e| {
                        warn!(
                            "Addons manager could not load image {index}: failed for '{url}': {e}"
                        );
                    })
                    .ok();

                let Some((raw, w, h)) = rgba_data else { return };

                let _ = slint::invoke_from_event_loop(move || {
                    let buffer =
                        slint::SharedPixelBuffer::<slint::Rgba8Pixel>::clone_from_slice(&raw, w, h);
                    let image = slint::Image::from_rgba8(buffer);

                    if let Some(ui) = ww.upgrade() {
                        let model = ui.get_addons();
                        if let Some(row) = model.row_data(index) {
                            let mut updated = row;
                            updated.image = image;
                            model.set_row_data(index, updated);
                        }
                    }
                });
            });
        }
    }

    // [CALLBACKS]

    fn bind(window: &slint::Weak<MainWindow>) {
        let w = window.unwrap();

        let ww = window.clone();
        w.on_addon_action(move |index| {
            let Ok(i) = usize::try_from(index) else {
                return;
            };

            let Some(win) = ww.upgrade() else { return };
            let model = win.get_addons();
            let Some(mut row) = model.row_data(i) else {
                return;
            };
            if row.installing {
                return;
            }

            let is_toggle = row.installed && !row.update_available;
            row.installing = true;
            if is_toggle {
                row.enabled = !row.enabled;
            }
            model.set_row_data(i, row);

            let ww = ww.clone();
            std::thread::spawn(move || {
                let updated = if is_toggle {
                    let mut addons = Self::scan_local();
                    if let Some(addon) = addons.get_mut(i) {
                        Self::set_enabled(addon, !addon.enabled);
                    }
                    Self::scan_local().into_iter().nth(i)
                } else {
                    let mut addons = Self::scan();
                    if let Some(addon) = addons.get_mut(i) {
                        match Self::install(addon) {
                            Err(e) => {
                                error!(
                                    "Addons manager could not install addon '{}': {e}",
                                    addon.name
                                );
                                ToastHandler::show(
                                    &ww,
                                    format!("Failed to install {}: {e}", addon.name),
                                    "error",
                                );
                            }
                            Ok(()) => {
                                ToastHandler::show(
                                    &ww,
                                    format!("{} installed successfully.", addon.name),
                                    "success",
                                );
                            }
                        }
                    }
                    Self::scan().into_iter().nth(i)
                };

                let _ = slint::invoke_from_event_loop(move || {
                    let Some(win) = ww.upgrade() else {
                        error!("Could not reload addons: window handle is dead");
                        return;
                    };

                    let model = win.get_addons();
                    if let Some(mut row) = model.row_data(i) {
                        if let Some(addon) = updated {
                            row.installed = addon.installed;
                            row.enabled = addon.enabled;
                            if !is_toggle {
                                row.update_available = addon.update_available;
                            }
                        }
                        row.installing = false;
                        model.set_row_data(i, row);
                    }
                });
            });
        });

        let ww = window.clone();
        w.on_addon_open_link(move |index| {
            let Ok(i) = usize::try_from(index) else {
                return;
            };

            let Some(win) = ww.upgrade() else { return };
            let Some(row) = win.get_addons().row_data(i) else {
                return;
            };

            if row.link.is_empty() {
                return;
            }

            if let Err(e) = open::that(row.link.as_str()) {
                error!("Addons manager could not open link '{}': {e}", row.link);
            }
        });

        let ww = window.clone();
        w.on_addon_delete(move |index| {
            let Ok(i) = usize::try_from(index) else {
                return;
            };

            let Some(win) = ww.upgrade() else { return };
            let Some(row) = win.get_addons().row_data(i) else {
                return;
            };

            if !row.installed || row.installing {
                return;
            }

            *PENDING_DELETE.lock().unwrap() = Some(i);

            win.set_popup_id("addon-delete".into());
            win.set_popup_title("Delete Addon?".into());
            win.set_popup_message(
                format!(
                    "\"{}\" will be uninstalled and its files permanently deleted. You can install it again later.",
                    row.name
                )
                .into(),
            );
            win.set_popup_active(true);
        });

        info!("Addons bind() complete");
    }

    pub fn confirm_delete(window: &slint::Weak<MainWindow>) {
        let Some(i) = PENDING_DELETE.lock().unwrap().take() else {
            return;
        };

        let Some(win) = window.upgrade() else { return };
        let model = win.get_addons();
        let Some(mut row) = model.row_data(i) else {
            return;
        };
        row.installing = true;
        model.set_row_data(i, row);

        let ww = window.clone();
        std::thread::spawn(move || {
            let addons = Self::scan_local();
            let result = addons.get(i).map_or_else(
                || Err(anyhow::anyhow!("addon no longer exists")),
                |addon| Self::delete(addon).map(|()| addon.name.clone()),
            );

            match &result {
                Ok(name) => ToastHandler::show(&ww, format!("{name} deleted."), "success"),
                Err(e) => ToastHandler::show(&ww, format!("Failed to delete addon: {e}"), "error"),
            }

            // Re-scan so the config keys line up with what is left on disk
            let updated = Self::scan_local().into_iter().nth(i);

            let _ = slint::invoke_from_event_loop(move || {
                let Some(win) = ww.upgrade() else {
                    error!("Could not reload addons: window handle is dead");
                    return;
                };

                let model = win.get_addons();
                if let Some(mut row) = model.row_data(i) {
                    if let Some(addon) = updated {
                        row.installed = addon.installed;
                        row.enabled = addon.enabled;
                        row.update_available = addon.update_available;
                    }
                    row.installing = false;
                    model.set_row_data(i, row);
                }
            });
        });
    }

    fn delete(addon: &Addon) -> Result<()> {
        let entries = fs::read_dir(&addon.folder)
            .with_context(|| format!("reading '{}'", addon.folder.display()))?;

        let mut failures: Vec<String> = Vec::new();
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_file()
                && path
                    .extension()
                    .is_some_and(|ext| ext.eq_ignore_ascii_case("auadd"))
            {
                continue;
            }

            let removed = if path.is_dir() {
                fs::remove_dir_all(&path)
            } else {
                fs::remove_file(&path)
            };

            if let Err(e) = removed {
                error!("Could not delete '{}': {e}", path.display());
                failures.push(format!("{}: {e}", path.display()));
            } else {
                debug!("Deleted addon file '{}'", path.display());
            }
        }

        if failures.is_empty() {
            info!("Deleted addon '{}'", addon.name);
            Ok(())
        } else {
            Err(anyhow::anyhow!(
                "{} file(s) could not be removed: {}",
                failures.len(),
                failures.join("; ")
            ))
        }
    }

    fn scan() -> Vec<Addon> {
        Self::scan_impl(true)
    }

    fn scan_local() -> Vec<Addon> {
        Self::scan_impl(false)
    }

    fn scan_impl(fetch_remote: bool) -> Vec<Addon> {
        let addon_dir = utils::get_bin_path()
            .unwrap_or_else(|| PathBuf::from("."))
            .join("Addons");
        let mut addons = Vec::new();

        let mut unseen_keys: Vec<&str> = ADDON_CONFIG_KEYS.iter().map(|(_, k)| *k).collect();
        let mut seen_keys: Vec<(&str, bool)> = Vec::new();

        let entries = match std::fs::read_dir(&addon_dir) {
            Ok(e) => e,
            Err(e) => {
                warn!(
                    "Addons scan: could not read '{}', keeping the existing config: {e}",
                    addon_dir.display()
                );
                return addons;
            }
        };

        for entry in entries.flatten() {
            let folder = entry.path();
            if !folder.is_dir() {
                continue;
            }

            let auadd_path = std::fs::read_dir(&folder).ok().and_then(|entries| {
                entries.flatten().map(|e| e.path()).find(|p| {
                    p.is_file()
                        && p.extension()
                            .is_some_and(|ext| ext.eq_ignore_ascii_case("auadd"))
                })
            });
            let Some(auadd_path) = auadd_path else {
                warn!(
                    "Addons scan: skipping '{}' - no .auadd file",
                    folder.display()
                );
                continue;
            };
            let Ok(contents) = std::fs::read_to_string(&auadd_path) else {
                warn!(
                    "Addons scan: skipping '{}' - could not read '{}'",
                    folder.display(),
                    auadd_path.display()
                );
                continue;
            };

            let mut addon = Addon {
                folder: folder.clone(),
                ..Default::default()
            };
            // Direct download URLs (FILE), used instead of the GameBanana API
            let mut file_urls: Vec<String> = Vec::new();

            for line in contents.lines() {
                let Some((key, value)) = line.split_once('|') else {
                    continue;
                };
                match key.trim() {
                    "NAME" => addon.name = value.trim().to_string(),
                    "AUTHOR" => addon.author = value.trim().to_string(),
                    // TODO: get version from gamebanana api too
                    "VERSION" => addon.version = value.trim().to_string(),
                    "DESCRIPTION" => addon.description = value.trim().to_string(),
                    "LINK" => addon.link = value.trim().to_string(),
                    "FILE" => file_urls.push(value.trim().to_string()),
                    "IMAGE" => addon.image_url = value.trim().to_string(),
                    other => warn!(
                        "Addons scan: unknown field '{other}' in '{}'",
                        auadd_path.display()
                    ),
                }
            }

            if fetch_remote {
                addon.install_data = if file_urls.is_empty() {
                    Self::fetch_gamebanana_files(&addon)
                } else {
                    file_urls.iter().map(|url| Self::direct_file(url)).collect()
                };
            }

            let payload_files = payload_files(&folder);
            addon.installed = !payload_files.is_empty();

            addon.enabled = addon.installed
                && payload_files
                    .iter()
                    .all(|f| !f.to_string_lossy().ends_with(".disabled"));

            if addon.installed {
                let local_hash = fs::read_to_string(folder.join("addon.md5")).unwrap_or_default();
                let remote_hash = Self::combined_md5(&addon.install_data);
                addon.update_available =
                    !remote_hash.is_empty() && local_hash.trim() != remote_hash;
            }

            let Some((_, k)) = ADDON_CONFIG_KEYS.iter().find(|(n, _)| *n == addon.name) else {
                error!("Unknown addon name: {}", addon.name);
                continue;
            };
            seen_keys.push((k, addon.enabled));
            unseen_keys.retain(|key| key != k);

            addons.push(addon);
        }

        let persisted = config::modify(|data| {
            for (key, enabled) in &seen_keys {
                data.insert((*key).to_string(), (*enabled).into());
            }
            for key in &unseen_keys {
                data.insert((*key).to_string(), false.into());
            }
        });
        if !persisted {
            warn!("Addons scan: could not persist the addon config keys");
        }

        addons
    }

    fn combined_md5(install_data: &[AddonData]) -> String {
        if install_data.is_empty() || install_data.iter().any(|d| d.md5.trim().is_empty()) {
            return String::new();
        }

        install_data
            .iter()
            .map(|d| d.md5.trim())
            .collect::<Vec<_>>()
            .join(",")
    }

    fn fetch_gamebanana_files(addon: &Addon) -> Vec<AddonData> {
        if addon.link.is_empty() {
            warn!(
                "Addons scan: '{}' has neither a FILE nor a LINK field",
                addon.name
            );
            return Vec::new();
        }

        let gb = GameBananaApi::new();
        let rt = match tokio::runtime::Runtime::new() {
            Ok(rt) => rt,
            Err(e) => {
                error!("Addons scan: could not create tokio runtime: {e}");
                return Vec::new();
            }
        };
        let mod_files = rt.block_on(async {
            gb.get_mod_files(
                addon
                    .link
                    .split('/')
                    .next_back()
                    .unwrap_or("0")
                    .parse()
                    .unwrap_or(0),
            )
            .await
        });

        mod_files.map_or_else(
            || {
                warn!(
                    "Addons scan: could not fetch mod files for '{}'",
                    addon.name
                );
                Vec::new()
            },
            |files| {
                files
                    .into_iter()
                    .map(|f| AddonData::new(f.name, f.url, f.md5))
                    .collect()
            },
        )
    }

    fn direct_file(url: &str) -> AddonData {
        let file_name = url
            .split('/')
            .next_back()
            .and_then(|s| s.split(['?', '#']).next())
            .unwrap_or_default()
            .to_string();

        AddonData::new(file_name, url.to_string(), Self::remote_etag(url))
    }

    fn remote_etag(url: &str) -> String {
        let etag = reqwest::blocking::Client::builder()
            .user_agent(format!("AuroraLauncher/{}", utils::get_local_version()))
            .timeout(std::time::Duration::from_secs(10))
            .build()
            .and_then(|c| c.head(url).send())
            .map(|r| {
                r.headers()
                    .get(reqwest::header::ETAG)
                    .and_then(|v| v.to_str().ok())
                    .unwrap_or_default()
                    .trim_start_matches("W/")
                    .trim_matches('"')
                    .to_string()
            });

        match etag {
            Ok(tag) => tag,
            Err(e) => {
                warn!("Addons scan: could not read ETag for '{url}': {e}");
                String::new()
            }
        }
    }

    fn install(addon: &Addon) -> Result<()> {
        let mut failures: Vec<String> = Vec::new();
        for data in &addon.install_data {
            debug!(
                "Installing addon: downloading '{}' → '{}'",
                data.file_name,
                addon.folder.display()
            );
            match Self::download_file(&data.file_name, &data.url, &addon.folder) {
                Ok(dest) => {
                    if dest
                        .extension()
                        .is_some_and(|e| ARCHIVE_EXTENSIONS.contains(&e.to_str().unwrap_or("")))
                    {
                        if let Err(e) = Self::unpack(&dest, &addon.folder) {
                            error!(
                                "Could not install addon: failed to extract '{}': {e}",
                                dest.display()
                            );
                            failures.push(format!("{}: {e}", data.file_name));
                            continue;
                        }
                        info!("Installed addon: extracted '{}'", dest.display());
                    }
                    info!("Installed addon: saved '{}'", dest.display());
                }
                Err(e) => {
                    error!(
                        "Could not install addon: failed to download '{}': {e}",
                        data.file_name
                    );
                    failures.push(format!("{}: {e}", data.file_name));
                }
            }
        }

        if failures.is_empty() {
            let md5 = Self::combined_md5(&addon.install_data);
            if !md5.is_empty() {
                if let Err(e) = fs::write(addon.folder.join("addon.md5"), &md5) {
                    warn!(
                        "Installed addon: could not record the hash for '{}': {e}",
                        addon.name
                    );
                }
            }
            Ok(())
        } else {
            Err(anyhow::anyhow!(
                "{} of {} file(s) failed to install: {}",
                failures.len(),
                addon.install_data.len(),
                failures.join("; ")
            ))
        }
    }

    fn unpack(dest: &Path, folder: &Path) -> Result<()> {
        extract_archive(dest, folder)?;

        let files = fs::read_dir(folder)?.collect::<Vec<_>>();
        for file in files {
            let path = file?.path();
            let extension = path.extension().unwrap_or_default();
            let name = path
                .file_name()
                .with_context(|| "install download file: couldn't get path file name")?
                .to_str()
                .with_context(|| "install download file: couldn't get path file name as str")?;
            if extension == "txt" {
                fs::remove_file(&path)?;
            }

            // HACK: Red dots has 2 folders, need to get files from the "Disable" folder
            if name == "Muted" {
                fs::remove_dir_all(&path)?;
            }
            if name == "Disable" {
                let files = fs::read_dir(&path)?.collect::<Vec<_>>();
                for file in files {
                    let path = file?.path();
                    // put them in the parent
                    fs::rename(&path, folder.join(path.file_name().unwrap()))?;
                }

                fs::remove_dir_all(&path)?;
            }

            // HACK: Hide UID also has 2 other mods inside, remove those
            // wow what a hack -daturas
            if name.contains("PingStatus") || name.contains("PhoneFunctions") {
                fs::remove_file(&path)?;
            }
        }

        Ok(())
    }

    const DOWNLOAD_MAX_ATTEMPTS: u32 = 4;

    fn download_file(file_name: &str, url: &str, dest_folder: &Path) -> anyhow::Result<PathBuf> {
        let file_name = sanitize_download_filename(file_name)
            .with_context(|| format!("refusing to download to unsafe file name '{file_name}'"))?;

        let client = reqwest::blocking::Client::builder()
            .user_agent(format!("AuroraLauncher/{}", utils::get_local_version()))
            .timeout(std::time::Duration::from_secs(30))
            .build()?;

        let temp_dir = std::env::temp_dir().join("Aurora/Addons");
        std::fs::create_dir_all(&temp_dir)
            .with_context(|| format!("creating '{}'", temp_dir.display()))?;
        let temp_path = temp_dir.join(&file_name);

        let mut last_err = None;
        for attempt in 1..=Self::DOWNLOAD_MAX_ATTEMPTS {
            match client
                .get(url)
                .send()
                .and_then(reqwest::blocking::Response::error_for_status)
                .and_then(reqwest::blocking::Response::bytes)
            {
                Ok(bytes) => {
                    let dest = dest_folder.join(&file_name);
                    let written = std::fs::write(&temp_path, &bytes)
                        .with_context(|| format!("writing '{}' to disk", temp_path.display()))
                        .and_then(|()| Self::move_into_place(&temp_path, &dest));

                    return match written {
                        Ok(()) => Ok(dest),
                        Err(e) => {
                            if temp_path.exists() {
                                if let Err(e) = std::fs::remove_file(&temp_path) {
                                    warn!(
                                        "Addon download: could not remove '{}': {e}",
                                        temp_path.display()
                                    );
                                }
                            }
                            Err(e)
                        }
                    };
                }
                Err(e) => {
                    warn!(
                        "Addon download attempt {attempt}/{} failed for '{file_name}': {e}",
                        Self::DOWNLOAD_MAX_ATTEMPTS
                    );
                    last_err = Some(e);
                    if attempt < Self::DOWNLOAD_MAX_ATTEMPTS {
                        std::thread::sleep(std::time::Duration::from_millis(
                            500 * u64::from(attempt),
                        ));
                    }
                }
            }
        }

        Err(anyhow::anyhow!(
            "failed to download '{file_name}' after {} attempts: {}",
            Self::DOWNLOAD_MAX_ATTEMPTS,
            last_err.map(|e| e.to_string()).unwrap_or_default()
        ))
    }

    fn move_into_place(from: &Path, to: &Path) -> Result<()> {
        if fs::rename(from, to).is_ok() {
            return Ok(());
        }

        fs::copy(from, to)
            .with_context(|| format!("moving '{}' to '{}'", from.display(), to.display()))?;

        if let Err(e) = fs::remove_file(from) {
            warn!(
                "Addon download: could not remove '{}' after moving it: {e}",
                from.display()
            );
        }

        Ok(())
    }

    fn set_enabled(addon: &Addon, enable: bool) {
        for file in payload_files(&addon.folder) {
            let path_str = file.to_string_lossy().into_owned();

            if enable {
                if let Some(stripped) = path_str.strip_suffix(".disabled") {
                    if let Err(e) = std::fs::rename(&file, stripped) {
                        error!(
                            "Could not enable addon: failed to rename '{}': {e}",
                            file.display()
                        );
                    } else {
                        debug!("Enabled addon: renamed '{}' → '{stripped}'", file.display());
                    }
                }
            } else if !path_str.ends_with(".disabled") {
                let new_path = format!("{path_str}.disabled");
                if let Err(e) = std::fs::rename(&file, &new_path) {
                    error!(
                        "Could not disable addon: failed to rename '{}': {e}",
                        file.display()
                    );
                } else {
                    debug!(
                        "Disabled addon: renamed '{}' → '{new_path}'",
                        file.display()
                    );
                }
            }
        }
    }

    fn detected_version() -> Version {
        let version = pathfind::get_game_directory()
            .ok()
            .and_then(|path| detect_version(&path).ok())
            .unwrap_or_default();

        debug!("Resolved addon against game region: {version}");
        version
    }

    fn to_slint_item(addon: &Addon, version: Version) -> AddonItem {
        let unavailable = config_key(&addon.name)
            .is_some_and(|config_key| addons::is_unavailable(config_key, version));

        AddonItem {
            name: addon.name.clone().into(),
            author: addon.author.clone().into(),
            version: addon.version.clone().into(),
            description: addon.description.clone().into(),
            link: addon.link.clone().into(),
            image: slint::Image::default(),
            installed: addon.installed,
            enabled: addon.enabled,
            update_available: addon.update_available,
            installing: false,
            unavailable,
        }
    }
}
