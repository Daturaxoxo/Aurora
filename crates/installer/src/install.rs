use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::Mutex;
use std::time::{Duration, Instant};

use ipc::manifest::{LocalManifest, Manifest, hash_file};
use shared::utils::format_bytes;
use slint::{Model, ModelRc, SharedString, VecModel, Weak};

use crate::{ComponentItem, InstallerWindow, backend, net};

const PROGRESS_INTERVAL: Duration = Duration::from_millis(100);

#[derive(Clone)]
struct Plan {
    manifest: Manifest,
    sizes: BTreeMap<String, u64>,
}

impl Plan {
    fn total_bytes(&self) -> u64 {
        self.sizes.values().sum()
    }
}

static PLAN: Mutex<Option<Plan>> = Mutex::new(None);

fn group_name(path: &str) -> &'static str {
    let lower = path.to_lowercase();
    if lower.contains("engine") {
        "Aurora Engine"
    } else if lower.contains("updater") {
        "Aurora Updater"
    } else {
        "Aurora Core"
    }
}

fn build_plan() -> Result<Plan, String> {
    let manifest = net::fetch_manifest()?;
    let sizes = manifest
        .files
        .iter()
        .map(|f| (f.path.clone(), net::file_size(&f.url).unwrap_or(0)))
        .collect();
    Ok(Plan { manifest, sizes })
}

fn get_or_build_plan() -> Result<Plan, String> {
    if let Some(plan) = PLAN.lock().unwrap().clone() {
        return Ok(plan);
    }
    let plan = build_plan()?;
    *PLAN.lock().unwrap() = Some(plan.clone());
    Ok(plan)
}

pub fn prepare(ui: Weak<InstallerWindow>) {
    std::thread::spawn(move || {
        let result = get_or_build_plan();
        let _ = slint::invoke_from_event_loop(move || {
            let Some(w) = ui.upgrade() else {
                return;
            };
            match result {
                Ok(plan) => {
                    let mut groups: Vec<(&'static str, u64)> = Vec::new();
                    for file in &plan.manifest.files {
                        let name = group_name(&file.path);
                        let bytes = plan.sizes.get(&file.path).copied().unwrap_or(0);
                        match groups.iter_mut().find(|(n, _)| *n == name) {
                            Some((_, total)) => *total += bytes,
                            None => groups.push((name, bytes)),
                        }
                    }

                    let components: Vec<ComponentItem> = groups
                        .iter()
                        .map(|(name, bytes)| ComponentItem {
                            name: (*name).into(),
                            size: format_bytes(*bytes).into(),
                        })
                        .collect();
                    w.set_components(ModelRc::from(Rc::new(VecModel::from(components))));

                    let total = format_bytes(plan.total_bytes());
                    w.set_total_size(total.clone().into());
                    w.set_space_required(total.into());
                }
                Err(e) => {
                    eprintln!("could not fetch install manifest: {e}");
                    w.set_total_size("Unknown".into());
                    w.set_space_required("Unknown".into());
                }
            }
        });
    });
}

fn log_line(ui: &Weak<InstallerWindow>, message: String) {
    let ui = ui.clone();
    let _ = slint::invoke_from_event_loop(move || {
        if let Some(w) = ui.upgrade() {
            let log = w.get_install_log();
            if let Some(model) = log.as_any().downcast_ref::<VecModel<SharedString>>() {
                model.push(message.into());
                #[allow(clippy::cast_possible_truncation, clippy::cast_possible_wrap)]
                w.set_install_log_count(model.row_count() as i32);
            }
        }
    });
}

fn set_progress(ui: &Weak<InstallerWindow>, progress: f32) {
    let ui = ui.clone();
    let _ = slint::invoke_from_event_loop(move || {
        if let Some(w) = ui.upgrade() {
            w.set_install_progress(progress);
        }
    });
}

pub fn run(
    ui: Weak<InstallerWindow>,
    install_dir: PathBuf,
    desktop_shortcut: bool,
    start_menu: bool,
) {
    std::thread::spawn(move || {
        let result = run_inner(&ui, &install_dir, desktop_shortcut, start_menu);
        let error = match result {
            Ok(()) => String::new(),
            Err(e) => {
                log_line(&ui, format!("ERROR: {e}"));
                e
            }
        };

        let _ = slint::invoke_from_event_loop(move || {
            if let Some(w) = ui.upgrade() {
                w.set_install_success(error.is_empty());
                w.set_install_error(error.into());
                w.set_install_done(true);
            }
        });
    });
}

#[allow(clippy::cast_precision_loss, clippy::cast_possible_truncation)]
fn run_inner(
    ui: &Weak<InstallerWindow>,
    install_dir: &Path,
    desktop_shortcut: bool,
    start_menu: bool,
) -> Result<(), String> {
    log_line(ui, "Preparing installation...".into());
    let plan = get_or_build_plan()?;

    fs::create_dir_all(install_dir)
        .map_err(|e| format!("failed to create {}: {e}", install_dir.display()))?;

    let total_bytes = plan.total_bytes();
    let file_count = plan.manifest.files.len();
    let mut done_bytes: u64 = 0;
    let mut tmps: Vec<PathBuf> = Vec::new();

    let result = (|| -> Result<(), String> {
        for (i, entry) in plan.manifest.files.iter().enumerate() {
            log_line(ui, format!("Downloading {}...", entry.path));

            let tmp = tmp_path(install_dir, &entry.path);
            if let Some(parent) = tmp.parent() {
                fs::create_dir_all(parent)
                    .map_err(|e| format!("failed to create {}: {e}", parent.display()))?;
            }
            tmps.push(tmp.clone());

            let mut last_progress = Instant::now();
            net::download(&entry.url, &tmp, |done, total| {
                if last_progress.elapsed() >= PROGRESS_INTERVAL {
                    last_progress = Instant::now();
                    let overall = if total_bytes > 0 {
                        (done_bytes + done) as f64 / total_bytes as f64
                    } else {
                        let file_frac = if total > 0 {
                            (done as f64 / total as f64).min(1.0)
                        } else {
                            0.0
                        };
                        (i as f64 + file_frac) / file_count.max(1) as f64
                    };
                    set_progress(ui, overall.clamp(0.0, 1.0) as f32);
                }
            })?;

            log_line(ui, format!("Verifying {}...", entry.path));
            let actual =
                hash_file(&tmp).map_err(|e| format!("failed to hash {}: {e}", entry.path))?;
            if actual != entry.sha256 {
                return Err(format!(
                    "hash mismatch for {}: expected {}, got {actual}",
                    entry.path, entry.sha256
                ));
            }

            let dst = install_dir.join(&entry.path);
            if dst.exists() {
                fs::remove_file(&dst)
                    .map_err(|e| format!("failed to replace {}: {e}", entry.path))?;
            }
            fs::rename(&tmp, &dst)
                .map_err(|e| format!("failed to move {} into place: {e}", entry.path))?;

            done_bytes += plan.sizes.get(&entry.path).copied().unwrap_or(0);
        }
        Ok(())
    })();

    if let Err(e) = result {
        cleanup(&tmps);
        return Err(e);
    }

    log_line(ui, "Writing manifest...".into());
    let local = LocalManifest {
        version: plan.manifest.version.clone(),
        files: plan
            .manifest
            .files
            .iter()
            .map(|f| (f.path.clone(), f.sha256.clone()))
            .collect(),
    };
    local
        .save(install_dir)
        .map_err(|e| format!("failed to save local manifest: {e}"))?;

    let aurora_exe = install_dir.join(ipc::AURORA_EXE);
    if desktop_shortcut {
        log_line(ui, "Creating desktop shortcut...".into());
        if let Err(e) = backend::create_desktop_shortcut(&aurora_exe) {
            log_line(
                ui,
                format!("WARNING: could not create desktop shortcut: {e}"),
            );
        }
    }
    if start_menu {
        log_line(ui, "Adding Aurora to the Start Menu...".into());
        if let Err(e) = backend::add_start_menu(&aurora_exe) {
            log_line(
                ui,
                format!("WARNING: could not add Aurora to the Start Menu: {e}"),
            );
        }
    }

    set_progress(ui, 1.0);
    log_line(ui, "Installation complete.".into());
    Ok(())
}

fn cleanup(tmps: &[PathBuf]) {
    for tmp in tmps {
        let _ = fs::remove_file(tmp);
    }
}

fn tmp_path(root: &Path, rel: &str) -> PathBuf {
    let path = root.join(rel);
    let mut name = path
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    name.push_str(".tmp");
    path.with_file_name(name)
}
