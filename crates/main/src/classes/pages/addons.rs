use crate::{AddonItem, MainWindow};
use log::{debug, error, info, warn};
use slint::{Model, VecModel};
use std::path::{Path, PathBuf};
use std::rc::Rc;

#[derive(Debug, Clone, Default)]
struct AddonData {
    folder:       PathBuf,
    name:         String,
    author:       String,
    version:      String,
    description:  String,
    install_urls: Vec<String>,
    link:         String,
    image_url:    String,
    installed:    bool,
    enabled:      bool,
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
        let Some(w) = window.upgrade() else {
            error!("Addons manager could not load: window handle is dead");
            return;
        };

        let root = Self::root_path();
        let addons = Self::scan(&root);

        let slint_items: Vec<AddonItem> = addons
            .iter()
            .map(Self::to_slint_item_no_image)
            .collect();

        w.set_addons(Rc::new(VecModel::from(slint_items)).into());
        Self::fetch_images_async(window, addons);
    }

    fn image_cache_dir() -> PathBuf {
        #[cfg(target_os = "windows")]
        let base = std::env::var("APPDATA")
            .map_or_else(|_| PathBuf::from("."), PathBuf::from);

        #[cfg(not(target_os = "windows"))]
        let base = std::env::var("HOME")
            .map(|h| PathBuf::from(h).join(".config"))
            .unwrap_or_else(|_| PathBuf::from("."));

        base.join("Aurora").join("Cache").join("Addons")
    }

    fn cache_filename(url: &str) -> String {
        use std::hash::{Hash, Hasher};
        let mut hasher = std::collections::hash_map::DefaultHasher::new();
        url.hash(&mut hasher);
        let hash = hasher.finish();
        let ext = url.rsplit('.').next()
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
            warn!("Addons manager could not create cache dir '{}': {e}", cache_dir.display());
        } else if let Err(e) = std::fs::write(&cache_path, &bytes) {
            warn!("Addons manager could not write cache file '{}': {e}", cache_path.display());
        } else {
            debug!("[Addons] cached image to '{}'", cache_path.display());
        }

        let img = image::load_from_memory(&bytes)?.into_rgba8();
        let (w, h) = img.dimensions();
        Ok((img.into_raw(), w, h))
    }

    fn fetch_images_async(window: &slint::Weak<MainWindow>, addons: Vec<AddonData>) {
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
                    .map_err(|e| warn!("Addons manager could not load image {index}: failed for '{url}': {e}"))
                    .ok();

                let Some((raw, w, h)) = rgba_data else { return };

                let _ = slint::invoke_from_event_loop(move || {
                    let buffer = slint::SharedPixelBuffer::<slint::Rgba8Pixel>::clone_from_slice(&raw, w, h,);
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
            let root = Self::root_path();
            let addons = Self::scan(&root);

            let Ok(i) = usize::try_from(index) else { return };
            let Some(addon) = addons.get(i) else {return};

            if !addon.installed {
                Self::install(addon);
            } else if addon.enabled {
                Self::set_enabled(addon, false);
            } else {
                Self::set_enabled(addon, true);
            }
            Self::reload(&ww, &root);
        });

        w.on_addon_open_link(move |index| {
            let root = Self::root_path();
            let addons = Self::scan(&root);

            let Ok(i) = usize::try_from(index) else { return };
            let Some(addon) = addons.get(i) else {return};

            if addon.link.is_empty() {return;}

            if let Err(e) = open::that(&addon.link) {error!("Addons manager could not open link '{}': {e}", addon.link);}
        });

        info!("Addons bind() complete");
    }

    fn root_path() -> PathBuf {
        #[cfg(debug_assertions)]
        {
            PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                .parent()
                .and_then(|p| p.parent()) 
                .map(PathBuf::from)
                .expect("Addons manager could not resolve repo root")
        }
        #[cfg(not(debug_assertions))]
        {
            std::env::current_exe()
                .expect("Addons Manager could not resolve exe path")
                .parent()
                .map(PathBuf::from)
                .expect("Addons manager could not find exe: has no parent directory")
        }
    }

    fn addons_dir(root: &Path) -> PathBuf {
        root.join("Bin").join("Addons")
    }

    fn scan(root: &Path) -> Vec<AddonData> {
        let dir = Self::addons_dir(root);
        let mut addons = Vec::new();

        let entries = match std::fs::read_dir(&dir) {
            Ok(e) => e,
            Err(_e) => {return addons;}
        };

        for entry in entries.flatten() {
            let folder = entry.path();
            if !folder.is_dir() {
                continue;
            }

            let auadd_path = folder.join("addon.auadd");
            let Ok(contents) = std::fs::read_to_string(&auadd_path) else {
                warn!("Addons scan: skipping '{}' - no addon.auadd", folder.display());
                continue;
            };

            let mut addon = AddonData {
                folder: folder.clone(),
                ..Default::default()
            };

            for line in contents.lines() {
                let Some((key, value)) = line.split_once('|') else {
                    continue;
                };
                match key.trim() {
                    "NAME"        => addon.name        = value.trim().to_string(),
                    "AUTHOR"      => addon.author       = value.trim().to_string(),
                    "VERSION"     => addon.version      = value.trim().to_string(),
                    "DESCRIPTION" => addon.description  = value.trim().to_string(),
                    "INSTALL"     => addon.install_urls = value
                                        .split(',')
                                        .map(str::trim)
                                        .filter(|s| !s.is_empty())
                                        .map(String::from)
                                        .collect(),
                    "LINK"        => addon.link         = value.trim().to_string(),
                    "IMAGE"       => addon.image_url    = value.trim().to_string(),
                    other         => warn!("Addons scan: unknown field '{other}' in '{}'", auadd_path.display()),
                }
            }

            let payload_files = Self::payload_files(&folder);
            addon.installed = !payload_files.is_empty();
            addon.enabled   = addon.installed
                && payload_files.iter().all(|f| {
                    !f.to_string_lossy().ends_with(".disabled")
                });

            addons.push(addon);
        }

        addons
    }

    /// All files inside an addon folder except addon.auadd.
    fn payload_files(folder: &Path) -> Vec<PathBuf> {
        std::fs::read_dir(folder)
            .into_iter()
            .flatten()
            .flatten()
            .map(|e| e.path())
            .filter(|p| {
                p.is_file()
                    && p.file_name()
                        .is_some_and(|n| n != "addon.auadd")
            })
            .collect()
    }

    fn install(addon: &AddonData) {
        for url in &addon.install_urls {
            debug!("Installing addon: downloading '{url}' → '{}'", addon.folder.display());
            match Self::download_file(url, &addon.folder) {
                Ok(dest) => info!("Installed addon: saved '{}'", dest.display()),
                Err(e)   => error!("Could not install addon: failed to download '{url}': {e}"),
            }
        }
    }

    fn download_file(url: &str, dest_folder: &Path) -> anyhow::Result<PathBuf> {
        let response = reqwest::blocking::get(url)?;
        let filename = url
            .rsplit('/')
            .next()
            .filter(|s| !s.is_empty())
            .unwrap_or("file.bin");

        let dest = dest_folder.join(filename);
        let bytes = response.bytes()?;
        std::fs::write(&dest, &bytes)?;
        Ok(dest)
    }

    fn set_enabled(addon: &AddonData, enable: bool) {
        for file in Self::payload_files(&addon.folder) {
            let path_str = file.to_string_lossy().into_owned();

            if enable {
                if let Some(stripped) = path_str.strip_suffix(".disabled") {
                    if let Err(e) = std::fs::rename(&file, stripped) {
                        error!("Could not enable addon: failed to rename '{}': {e}", file.display());
                    } else {
                        debug!("Enabled addon: renamed '{}' → '{stripped}'", file.display());
                    }
                }
            } else if !path_str.ends_with(".disabled") {
                let new_path = format!("{path_str}.disabled");
                if let Err(e) = std::fs::rename(&file, &new_path) {
                    error!("Could not disable addon: failed to rename '{}': {e}", file.display());
                } else {
                    debug!("Disabled addon: renamed '{}' → '{new_path}'", file.display());
                }
            }
        }
    }

    fn reload(window: &slint::Weak<MainWindow>, root: &Path) {
        let addons = Self::scan(root);

        let Some(w) = window.upgrade() else {
            error!("Could not reload addons: window handle is dead");
            return;
        };

        let model = w.get_addons();

        for (i, addon) in addons.iter().enumerate() {
            if let Some(mut row) = model.row_data(i) {
                row.installed = addon.installed;
                row.enabled   = addon.enabled;
                model.set_row_data(i, row);
            }
        }
    }

    fn to_slint_item_no_image(addon: &AddonData) -> AddonItem {
        AddonItem {
            name:        addon.name.clone().into(),
            author:      addon.author.clone().into(),
            version:     addon.version.clone().into(),
            description: addon.description.clone().into(),
            link:        addon.link.clone().into(),
            image:       slint::Image::default(),
            installed:   addon.installed,
            enabled:     addon.enabled,
        }
    }


}