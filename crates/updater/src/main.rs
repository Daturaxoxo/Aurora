#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod logfile;
mod net;

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use ipc::lock::SingletonLock;
use ipc::manifest::{hash_file, FileEntry, LocalManifest, Manifest};
use ipc::protocol::{self, Message};

use logfile::log;

type Conn = Arc<Mutex<protocol::IpcStream>>;

fn main() {
    let root = ipc::install_root();
    logfile::init(&root);
    log("updater started");

    let _lock = match SingletonLock::acquire(&root.join(ipc::UPDATER_LOCK_FILE)) {
        Ok(Some(lock)) => lock,
        Ok(None) => {
            log("another updater instance is already running; exiting");
            return;
        }
        Err(e) => {
            log(&format!("failed to acquire singleton lock: {e}"));
            std::process::exit(1);
        }
    };

    let Some(stream) = connect_with_retry(ipc::MAIN_PIPE_NAME, 10, Duration::from_millis(300))
    else {
        log("could not connect to Aurora over IPC; exiting");
        std::process::exit(1);
    };
    let conn: Conn = Arc::new(Mutex::new(stream));

    match run(&root, &conn) {
        Ok(()) => log("updater finished"),
        Err(e) => {
            log(&format!("update failed: {e}"));
            send(&conn, &Message::Error { message: e });
            std::process::exit(1);
        }
    }
}

fn connect_with_retry(pipe: &str, attempts: u32, delay: Duration) -> Option<protocol::IpcStream> {
    for attempt in 0..attempts {
        match protocol::connect(pipe) {
            Ok(stream) => return Some(stream),
            Err(_) if attempt + 1 < attempts => std::thread::sleep(delay),
            Err(e) => log(&format!("IPC connect failed: {e}")),
        }
    }
    None
}

fn send(conn: &Conn, msg: &Message) {
    if let Ok(mut stream) = conn.lock() {
        let _ = protocol::write_message(&mut *stream, msg);
    }
}

fn run(root: &Path, conn: &Conn) -> Result<(), String> {
    let manifest = net::fetch_manifest()?;
    log(&format!("manifest fetched: version {}", manifest.version));

    let mut local = match LocalManifest::load(root) {
        Ok(Some(local)) => local,
        Ok(None) => {
            log("no local manifest; hashing installed files");
            LocalManifest::build_manifest_from_disk(root, &manifest)
        }
        Err(e) => {
            log(&format!(
                "local manifest unreadable ({e}); rebuilding from disk"
            ));
            LocalManifest::build_manifest_from_disk(root, &manifest)
        }
    };

    let changed = manifest.changed_files(root, &local);
    if changed.is_empty() {
        log("no changes; local state matches manifest");
        local.version = manifest.version;
        local
            .save(root)
            .map_err(|e| format!("failed to save local manifest: {e}"))?;
        send(conn, &Message::NoUpdate);
        return Ok(());
    }
    log(&format!("{} file(s) changed", changed.len()));

    send(conn, &Message::Lock);
    let heartbeat = Heartbeat::start(conn.clone());
    let result = apply_update(root, conn, &manifest, &mut local, &changed);
    heartbeat.stop();
    result
}

fn apply_update(
    root: &Path,
    conn: &Conn,
    manifest: &Manifest,
    local: &mut LocalManifest,
    changed: &[&FileEntry],
) -> Result<(), String> {
    let original_local = local.clone();

    let mut tmps: Vec<PathBuf> = Vec::new();
    for entry in changed {
        let tmp = tmp_path(root, &entry.path);
        if let Some(parent) = tmp.parent() {
            fs::create_dir_all(parent)
                .map_err(|e| format!("failed to create {}: {e}", parent.display()))?;
        }
        log(&format!("downloading {}", entry.path));
        if let Err(e) = net::download(&entry.url, &tmp) {
            cleanup(&tmps);
            let _ = fs::remove_file(&tmp);
            return Err(e);
        }
        tmps.push(tmp.clone());

        let actual = hash_file(&tmp).map_err(|e| {
            cleanup(&tmps);
            format!("failed to hash {}: {e}", tmp.display())
        })?;
        if actual != entry.sha256 {
            cleanup(&tmps);
            return Err(format!(
                "hash mismatch for {}: expected {}, got {actual}",
                entry.path, entry.sha256
            ));
        }
    }

    let (exe_entries, others): (Vec<&FileEntry>, Vec<&FileEntry>) =
        changed.iter().partition(|e| e.path == ipc::AURORA_EXE);

    let mut backups: Vec<(PathBuf, PathBuf)> = Vec::new();
    for entry in &others {
        let dst = root.join(&entry.path);
        let tmp = tmp_path(root, &entry.path);
        if let Err(e) = swap_in(&dst, &tmp, &mut backups) {
            restore_backups(&backups);
            cleanup(&tmps);
            return Err(format!("failed to replace {}: {e}", entry.path));
        }
        local.files.insert(entry.path.clone(), entry.sha256.clone());
    }

    if exe_entries.is_empty() {
        local.version.clone_from(&manifest.version);
        local
            .save(root)
            .map_err(|e| format!("failed to save local manifest: {e}"))?;
        delete_backups(&backups);
        send(conn, &Message::Unlock);
        log("update applied (no exe change)");
        return Ok(());
    }

    let exe_entry = exe_entries[0];
    let exe = root.join(&exe_entry.path);
    let exe_tmp = tmp_path(root, &exe_entry.path);
    let exe_bak = bak_path(&exe);

    log("Aurora.exe changed; sending close_now");
    send(conn, &Message::CloseNow);

    if let Err(e) = rename_with_retry(&exe, &exe_bak, 40, Duration::from_millis(500)) {
        restore_backups(&backups);
        cleanup(&tmps);
        return Err(format!("could not move old Aurora.exe aside: {e}"));
    }
    backups.push((exe.clone(), exe_bak));

    if let Err(e) = fs::rename(&exe_tmp, &exe) {
        restore_backups(&backups);
        cleanup(&tmps);
        return Err(format!("could not move new Aurora.exe into place: {e}"));
    }
    local
        .files
        .insert(exe_entry.path.clone(), exe_entry.sha256.clone());
    local.version.clone_from(&manifest.version);
    if let Err(e) = local.save(root) {
        log(&format!("warning: failed to save local manifest: {e}"));
    }

    let listener = match protocol::listen(ipc::INIT_PIPE_NAME) {
        Ok(listener) => listener,
        Err(e) => {
            restore_backups(&backups);
            let _ = original_local.save(root);
            return Err(format!("could not open init pipe: {e}"));
        }
    };

    log("relaunching Aurora");
    let child = Command::new(&exe)
        .arg(ipc::POST_UPDATE_ARG)
        .current_dir(root)
        .spawn();

    let confirmed = match child {
        Ok(_) => wait_for_init_confirmed(listener, ipc::INIT_CONFIRM_TIMEOUT),
        Err(e) => {
            log(&format!("failed to relaunch Aurora: {e}"));
            false
        }
    };

    if confirmed {
        log("init_confirmed received; update complete");
        delete_backups(&backups);
        return Ok(());
    }

    log("no init_confirmed within timeout; rolling back");
    restore_backups(&backups);
    if let Err(e) = original_local.save(root) {
        log(&format!("warning: failed to restore local manifest: {e}"));
    }
    if let Err(e) = Command::new(&exe)
        .arg(ipc::SKIP_UPDATE_CHECK_ARG)
        .current_dir(root)
        .spawn()
    {
        log(&format!("failed to relaunch old Aurora: {e}"));
    }
    Err("new Aurora did not confirm init; update rolled back".into())
}

fn swap_in(dst: &Path, tmp: &Path, backups: &mut Vec<(PathBuf, PathBuf)>) -> std::io::Result<()> {
    if dst.exists() {
        let bak = bak_path(dst);
        let _ = fs::remove_file(&bak);
        fs::rename(dst, &bak)?;
        backups.push((dst.to_path_buf(), bak));
    }
    fs::rename(tmp, dst)
}

fn restore_backups(backups: &[(PathBuf, PathBuf)]) {
    for (dst, bak) in backups.iter().rev() {
        let _ = fs::remove_file(dst);
        if let Err(e) = fs::rename(bak, dst) {
            log(&format!("rollback failed for {}: {e}", dst.display()));
        }
    }
}

fn delete_backups(backups: &[(PathBuf, PathBuf)]) {
    for (_, bak) in backups {
        let _ = fs::remove_file(bak);
    }
}

fn cleanup(tmps: &[PathBuf]) {
    for tmp in tmps {
        let _ = fs::remove_file(tmp);
    }
}

fn rename_with_retry(
    from: &Path,
    to: &Path,
    attempts: u32,
    delay: Duration,
) -> std::io::Result<()> {
    let _ = fs::remove_file(to);
    let mut last = None;
    for _ in 0..attempts {
        match fs::rename(from, to) {
            Ok(()) => return Ok(()),
            Err(e) => {
                last = Some(e);
                std::thread::sleep(delay);
            }
        }
    }
    Err(last.unwrap_or_else(|| std::io::Error::other("rename failed")))
}

fn wait_for_init_confirmed(listener: protocol::IpcListener, timeout: Duration) -> bool {
    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || {
        if let Ok(mut stream) = protocol::accept(&listener) {
            if let Ok(msg) = protocol::read_message(&mut stream) {
                let _ = tx.send(msg);
            }
        }
    });
    matches!(rx.recv_timeout(timeout), Ok(Message::InitConfirmed))
}

struct Heartbeat {
    stop: Arc<AtomicBool>,
    handle: std::thread::JoinHandle<()>,
}

impl Heartbeat {
    fn start(conn: Conn) -> Self {
        let stop = Arc::new(AtomicBool::new(false));
        let stop_flag = stop.clone();
        let handle = std::thread::spawn(move || {
            while !stop_flag.load(Ordering::Relaxed) {
                send(&conn, &Message::Heartbeat);
                std::thread::sleep(ipc::HEARTBEAT_INTERVAL);
            }
        });
        Self { stop, handle }
    }

    fn stop(self) {
        self.stop.store(true, Ordering::Relaxed);
        let _ = self.handle.join();
    }
}

fn tmp_path(root: &Path, rel: &str) -> PathBuf {
    with_suffix(&root.join(rel), "tmp")
}

fn bak_path(path: &Path) -> PathBuf {
    with_suffix(path, "bak")
}

fn with_suffix(path: &Path, suffix: &str) -> PathBuf {
    let mut name = path
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    name.push('.');
    name.push_str(suffix);
    path.with_file_name(name)
}
