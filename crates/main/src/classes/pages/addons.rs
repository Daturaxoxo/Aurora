use crate::{AddonItem, MainWindow};
use backend::classes::addons::payload_files;
use shared::classes::gamebanana::api::GameBananaApi;
use shared::{config, utils};

use anyhow::{Context, Result};
use archive::{ArchiveExtractor, ArchiveFormat};
use log::*;
use slint::{Model, VecModel};
use unrar::Archive as RarArchive;

use std::fs;
use std::path::{Path, PathBuf};
use std::rc::Rc;

pub const ARCHIVE_EXTENSIONS: [&str; 9] =
    ["zip", "rar", "7z", "tar", "gz", "bz2", "xz", "zst", "lz4"];

const ADDON_CONFIG_KEYS: [(&str, &str); 6] = [
    ("No 3D Driving Waypoint", "drv_lin"),
    ("Hide UID", "uid_rem"),
    ("Hide Notification Dots", "nor_rem"),
    ("Censorship Remover", "csn_rem"),
    ("Cooldown Timers", "col_tim"),
    ("Collectible Highlighter", "collectibles"),
];

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

            let _ = slint::invoke_from_event_loop(move || {
                let Some(w) = ww.upgrade() else {
                    error!("Addons manager could not load: window handle is dead");
                    return;
                };

                let slint_items: Vec<AddonItem> =
                    addons.iter().map(Self::to_slint_item_no_image).collect();

                w.set_addons(Rc::new(VecModel::from(slint_items)).into());
                Self::fetch_images_async(&ww, addons);
            });
        });
    }

    fn image_cache_dir() -> PathBuf {
        let base = dirs::config_dir().unwrap_or_else(|| ".".into());

        base.join("Aurora").join("Cache").join("Addons")
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
                // Flip the button immediately; the file renames happen in the
                // background and the row is reconciled once they're done
                row.enabled = !row.enabled;
            }
            model.set_row_data(i, row);

            let ww = ww.clone();
            std::thread::spawn(move || {
                let updated = if is_toggle {
                    // No network needed to rename payload files on disk
                    let mut addons = Self::scan_local();
                    if let Some(addon) = addons.get_mut(i) {
                        Self::set_enabled(addon, !addon.enabled);
                    }
                    Self::scan_local().into_iter().nth(i)
                } else {
                    let mut addons = Self::scan();
                    if let Some(addon) = addons.get_mut(i) {
                        if let Err(e) = Self::install(addon) {
                            // TODO: probably should display an error message @daturas
                            error!(
                                "Addons manager could not install addon '{}': {e}",
                                addon.name
                            );
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

        info!("Addons bind() complete");
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

        let entries = match std::fs::read_dir(&addon_dir) {
            Ok(e) => e,
            Err(_e) => {
                for key in unseen_keys {
                    config::set(key, false);
                }
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
                    "LINK" => {
                        addon.link = value.trim().to_string();
                        if !fetch_remote {
                            continue;
                        }
                        let gb = GameBananaApi::new();
                        let rt = match tokio::runtime::Runtime::new() {
                            Ok(rt) => rt,
                            Err(e) => {
                                error!("Addons scan: could not create tokio runtime: {e}");
                                continue;
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

                        addon.install_data = if let Some(files) = mod_files {
                            files
                                .into_iter()
                                .map(|f| AddonData::new(f.name, f.url, f.md5))
                                .collect()
                        } else {
                            warn!(
                                "Addons scan: could not fetch mod files for '{}'",
                                addon.name
                            );
                            Vec::new()
                        };
                    }
                    "IMAGE" => addon.image_url = value.trim().to_string(),
                    other => warn!(
                        "Addons scan: unknown field '{other}' in '{}'",
                        auadd_path.display()
                    ),
                }
            }

            let payload_files = payload_files(&folder);
            addon.installed = !payload_files.is_empty();

            addon.enabled = addon.installed
                && payload_files
                    .iter()
                    .all(|f| !f.to_string_lossy().ends_with(".disabled"));

            if addon.installed {
                let local_hash = fs::read_to_string(folder.join("addon.md5")).unwrap_or_default();
                let remote_hash = addon.install_data.first().map_or("", |d| d.md5.as_str());
                addon.update_available =
                    !remote_hash.is_empty() && local_hash.trim() != remote_hash.trim();
            }

            let Some((_, k)) = ADDON_CONFIG_KEYS.iter().find(|(n, _)| *n == addon.name) else {
                error!("Unknown addon name: {}", addon.name);
                continue;
            };
            config::set(k, addon.enabled);
            unseen_keys.retain(|key| key != k);

            addons.push(addon);
        }

        for key in unseen_keys {
            config::set(key, false);
        }

        addons
    }

    fn install(addon: &Addon) -> Result<()> {
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
                        let extension = dest.extension().unwrap_or_default();
                        if extension == "rar" {
                            let archive = RarArchive::new(&dest).open_for_processing()?;
                            archive.extract_all(&addon.folder)?;
                        } else {
                            let data = fs::read(&dest)?;
                            let extractor = ArchiveExtractor::new();
                            let files = match extension.to_str().unwrap_or("") {
                                "zip" => extractor.extract(&data, ArchiveFormat::Zip)?,
                                "7z" => extractor.extract(&data, ArchiveFormat::SevenZ)?,
                                "tar" => extractor.extract(&data, ArchiveFormat::Tar)?,
                                "gz" => extractor.extract(&data, ArchiveFormat::Gz)?,
                                "bz2" => extractor.extract(&data, ArchiveFormat::Bz2)?,
                                "xz" => extractor.extract(&data, ArchiveFormat::Xz)?,
                                "zst" => extractor.extract(&data, ArchiveFormat::Zst)?,
                                "lz4" => extractor.extract(&data, ArchiveFormat::Lz4)?,
                                _ => unreachable!(),
                            };

                            for file in files {
                                if file.is_directory {
                                    fs::create_dir_all(addon.folder.join(file.path))?;
                                } else {
                                    fs::write(addon.folder.join(file.path), file.data)?;
                                }
                            }
                        }

                        let files = fs::read_dir(&addon.folder)?.collect::<Vec<_>>();
                        for file in files {
                            let path = file?.path();
                            let extension = path.extension().unwrap_or_default();
                            let name = path
                                .file_name()
                                .with_context(|| {
                                    "install download file: couldn't get path file name"
                                })?
                                .to_str()
                                .with_context(|| {
                                    "install download file: couldn't get path file name as str"
                                })?;
                            if extension == "txt" {
                                fs::remove_file(&path)?;
                            }

                            let _ = fs::write(addon.folder.join("addon.md5"), data.md5.clone());

                            // HACK: Red dots has 2 folders, need to get files from the "Disable" folder
                            if name == "Muted" {
                                fs::remove_dir_all(&path)?;
                            }
                            if name == "Disable" {
                                let files = fs::read_dir(&path)?.collect::<Vec<_>>();
                                for file in files {
                                    let path = file?.path();
                                    // put them in the parent
                                    fs::rename(
                                        &path,
                                        addon.folder.join(path.file_name().unwrap()),
                                    )?;
                                }

                                fs::remove_dir_all(&path)?;
                            }

                            // HACK: Hide UID also has 2 other mods inside, remove those
                            if name.contains("PingStatus") || name.contains("PhoneFunctions") {
                                fs::remove_file(&path)?;
                            }
                        }
                        info!("Installed addon: extracted '{}'", dest.display());
                    }
                    info!("Installed addon: saved '{}'", dest.display());
                }
                Err(e) => error!(
                    "Could not install addon: failed to download '{}': {e}",
                    data.file_name
                ),
            }
        }

        Ok(())
    }

    fn download_file(file_name: &str, url: &str, dest_folder: &Path) -> anyhow::Result<PathBuf> {
        let response = reqwest::blocking::get(url)?;

        let dest = dest_folder.join(file_name);
        let bytes = response.bytes()?;
        std::fs::write(&dest, &bytes)?;
        Ok(dest)
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

    fn to_slint_item_no_image(addon: &Addon) -> AddonItem {
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
        }
    }
}
