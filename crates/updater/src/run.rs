use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use ipc::lock::SingletonLock;
use ipc::manifest::{hash_file, FileEntry, LocalManifest, Manifest};
use ipc::protocol::{self, Message};

use crate::logfile;
use crate::logfile::log;
use crate::net;

type Conn = Arc<Mutex<protocol::IpcStream>>;

const PROGRESS_INTERVAL: Duration = Duration::from_millis(100);
const CONNECT_ATTEMPTS: u32 = 30;
const CONNECT_RETRY_DELAY: Duration = Duration::from_millis(300);
const ROLLBACK_ATTEMPTS: u32 = 8;
const ROLLBACK_DELAY: Duration = Duration::from_millis(250);
const READ_GRACE: Duration = Duration::from_millis(500);

enum RollbackStep {
    Restore { dst: PathBuf, bak: PathBuf },
    Remove { dst: PathBuf },
}

pub fn main() {
    let root = ipc::install_root();
    logfile::init(&root);
    log("updater started");

    let _lock = match SingletonLock::acquire(&root.join(ipc::UPDATER_LOCK_FILE)) {
        Ok(Some(lock)) => lock,
        Ok(None) => {
            log(&format!(
                "another updater instance already holds {}; exiting without contacting Aurora",
                ipc::UPDATER_LOCK_FILE
            ));
            std::process::exit(1);
        }
        Err(e) => {
            log(&format!(
                "failed to acquire {}: {e}; exiting without contacting Aurora",
                ipc::UPDATER_LOCK_FILE
            ));
            std::process::exit(1);
        }
    };

    let connected = connect_with_retry(ipc::MAIN_PIPE_NAME, CONNECT_ATTEMPTS, CONNECT_RETRY_DELAY);
    let Some(stream) = connected else {
        log("could not connect to Aurora over IPC; exiting before Aurora can be locked");
        std::process::exit(1);
    };
    let conn: Conn = Arc::new(Mutex::new(stream));
    send(&conn, &Message::Hello);
    log("connected to Aurora; hello sent");
    let heartbeat = Heartbeat::start(conn.clone());

    match run(&root, &conn, heartbeat) {
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
            Err(e) => log(&format!(
                "IPC connect failed after {attempts} attempt(s): {e}"
            )),
        }
    }
    None
}

fn send(conn: &Conn, msg: &Message) {
    if let Ok(mut stream) = conn.lock() {
        let _ = protocol::write_message(&mut *stream, msg);
    }
}

fn run(root: &Path, conn: &Conn, heartbeat: Heartbeat) -> Result<(), String> {
    let manifest = match net::fetch_manifest() {
        Ok(manifest) => manifest,
        Err(e) => {
            heartbeat.stop();
            return Err(e);
        }
    };
    if let Err(e) = manifest.validate() {
        heartbeat.stop();
        log(&format!("manifest rejected: {e}"));
        return Err(e);
    }
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
        heartbeat.stop();
        local
            .save(root)
            .map_err(|e| format!("failed to save local manifest: {e}"))?;
        send(conn, &Message::NoUpdate);
        return Ok(());
    }
    log(&format!("{} file(s) changed", changed.len()));

    send(conn, &Message::Lock);
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

    let file_count = u32::try_from(changed.len()).unwrap_or(u32::MAX);
    let mut tmps: Vec<PathBuf> = Vec::new();
    for (i, entry) in changed.iter().enumerate() {
        let file_index = u32::try_from(i).unwrap_or(u32::MAX);
        let Some(dst) = entry.resolve(root) else {
            cleanup(&tmps);
            return Err(format!("rejected manifest entry `{}`", entry.path));
        };
        let tmp = tmp_path(&dst);
        if let Some(parent) = tmp.parent() {
            fs::create_dir_all(parent)
                .map_err(|e| format!("failed to create {}: {e}", parent.display()))?;
        }
        log(&format!("downloading {}", entry.path));
        let mut last_progress = Instant::now();
        if let Err(e) = net::download(&entry.url, &tmp, |done, total| {
            if last_progress.elapsed() >= PROGRESS_INTERVAL {
                last_progress = Instant::now();
                send(
                    conn,
                    &Message::Progress {
                        file_index,
                        file_count,
                        bytes_done: done,
                        bytes_total: total,
                    },
                );
            }
        }) {
            cleanup(&tmps);
            let _ = fs::remove_file(&tmp);
            return Err(e);
        }
        send(
            conn,
            &Message::Progress {
                file_index: file_index.saturating_add(1),
                file_count,
                bytes_done: 0,
                bytes_total: 0,
            },
        );
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

    let mut plan: Vec<RollbackStep> = Vec::new();
    for entry in &others {
        let Some(dst) = entry.resolve(root) else {
            return Err(fail(
                root,
                manifest,
                &original_local,
                &plan,
                &tmps,
                &format!("rejected manifest entry `{}`", entry.path),
            ));
        };
        let tmp = tmp_path(&dst);
        if let Err(e) = swap_in(&dst, &tmp, &mut plan) {
            return Err(fail(
                root,
                manifest,
                &original_local,
                &plan,
                &tmps,
                &format!("failed to replace {}: {e}", entry.path),
            ));
        }
        local.files.insert(entry.path.clone(), entry.sha256.clone());
    }

    if exe_entries.is_empty() {
        local.version.clone_from(&manifest.version);
        local
            .save(root)
            .map_err(|e| format!("failed to save local manifest: {e}"))?;
        delete_backups(&plan);
        cleanup(&tmps);
        send(conn, &Message::Unlock);
        log("update applied (no exe change)");
        return Ok(());
    }

    let exe_entry = exe_entries[0];
    let Some(exe) = exe_entry.resolve(root) else {
        return Err(fail(
            root,
            manifest,
            &original_local,
            &plan,
            &tmps,
            &format!("rejected manifest entry `{}`", exe_entry.path),
        ));
    };
    let exe_tmp = tmp_path(&exe);
    let exe_bak = bak_path(&exe);

    let listener = match protocol::listen(ipc::INIT_PIPE_NAME) {
        Ok(listener) => listener,
        Err(e) => {
            return Err(fail(
                root,
                manifest,
                &original_local,
                &plan,
                &tmps,
                &format!("could not open init pipe: {e}"),
            ));
        }
    };

    log("Aurora.exe changed; sending close_now");
    send(conn, &Message::CloseNow);

    if let Err(e) = rename_with_retry(&exe, &exe_bak, 40, Duration::from_millis(500)) {
        let message = fail(
            root,
            manifest,
            &original_local,
            &plan,
            &tmps,
            &format!("could not move old Aurora.exe aside: {e}"),
        );
        relaunch_previous(root, &exe);
        return Err(message);
    }
    plan.push(RollbackStep::Restore {
        dst: exe.clone(),
        bak: exe_bak,
    });

    if let Err(e) = fs::rename(&exe_tmp, &exe) {
        let message = fail(
            root,
            manifest,
            &original_local,
            &plan,
            &tmps,
            &format!("could not move new Aurora.exe into place: {e}"),
        );
        relaunch_previous(root, &exe);
        return Err(message);
    }
    local
        .files
        .insert(exe_entry.path.clone(), exe_entry.sha256.clone());
    local.version.clone_from(&manifest.version);

    log("relaunching Aurora");
    let child = match Command::new(&exe)
        .arg(ipc::POST_UPDATE_ARG)
        .current_dir(root)
        .spawn()
    {
        Ok(child) => Some(child),
        Err(e) => {
            log(&format!("failed to relaunch Aurora: {e}"));
            None
        }
    };

    let confirmed = if child.is_some() {
        wait_for_init_confirmed(&listener, ipc::INIT_CONFIRM_TIMEOUT)
    } else {
        drop(listener);
        false
    };

    if confirmed {
        log("init_confirmed received; update complete");
        if let Err(e) = local.save(root) {
            log(&format!("warning: failed to save local manifest: {e}"));
        }
        delete_backups(&plan);
        cleanup(&tmps);
        return Ok(());
    }

    if let Some(mut child) = child {
        log("no init_confirmed within timeout; terminating the new Aurora");
        if let Err(e) = child.kill() {
            log(&format!("failed to terminate the new Aurora: {e}"));
        }
        if let Err(e) = child.wait() {
            log(&format!("failed to reap the new Aurora: {e}"));
        }
    }

    let message = fail(
        root,
        manifest,
        &original_local,
        &plan,
        &tmps,
        "new Aurora did not confirm init",
    );
    relaunch_previous(root, &exe);
    Err(message)
}

fn fail(
    root: &Path,
    manifest: &Manifest,
    original_local: &LocalManifest,
    plan: &[RollbackStep],
    tmps: &[PathBuf],
    context: &str,
) -> String {
    let rollback = roll_back(root, manifest, original_local, plan);
    cleanup(tmps);
    rollback.map_or_else(
        || format!("{context}; update rolled back"),
        |failures| format!("{context}; rollback also failed: {failures}"),
    )
}

fn roll_back(
    root: &Path,
    manifest: &Manifest,
    original_local: &LocalManifest,
    plan: &[RollbackStep],
) -> Option<String> {
    let mut failures: Vec<String> = Vec::new();
    for step in plan.iter().rev() {
        match step {
            RollbackStep::Restore { dst, bak } => {
                if let Err(e) = rename_with_retry(bak, dst, ROLLBACK_ATTEMPTS, ROLLBACK_DELAY) {
                    failures.push(format!("could not restore {}: {e}", dst.display()));
                }
            }
            RollbackStep::Remove { dst } => {
                if let Err(e) = remove_with_retry(dst, ROLLBACK_ATTEMPTS, ROLLBACK_DELAY) {
                    failures.push(format!("could not remove {}: {e}", dst.display()));
                }
            }
        }
    }

    if failures.is_empty() {
        if let Err(e) = original_local.save(root) {
            log(&format!("warning: failed to restore local manifest: {e}"));
        }
        log("rollback complete");
        return None;
    }

    for failure in &failures {
        log(&format!("rollback failure: {failure}"));
    }
    resync_local_manifest(root, manifest, &original_local.version);
    Some(failures.join("; "))
}

fn resync_local_manifest(root: &Path, manifest: &Manifest, version: &str) {
    log("rebuilding the local manifest from what is on disk");
    let mut rebuilt = LocalManifest::build_manifest_from_disk(root, manifest);
    version.clone_into(&mut rebuilt.version);
    if let Err(e) = rebuilt.save(root) {
        log(&format!("warning: failed to write local manifest: {e}"));
    }
}

fn relaunch_previous(root: &Path, exe: &Path) {
    log("relaunching the previous Aurora");
    if let Err(e) = Command::new(exe)
        .arg(ipc::SKIP_UPDATE_CHECK_ARG)
        .current_dir(root)
        .spawn()
    {
        log(&format!("failed to relaunch the previous Aurora: {e}"));
    }
}

fn swap_in(dst: &Path, tmp: &Path, plan: &mut Vec<RollbackStep>) -> std::io::Result<()> {
    if dst.exists() {
        let bak = bak_path(dst);
        let _ = fs::remove_file(&bak);
        fs::rename(dst, &bak)?;
        plan.push(RollbackStep::Restore {
            dst: dst.to_path_buf(),
            bak,
        });
    } else {
        plan.push(RollbackStep::Remove {
            dst: dst.to_path_buf(),
        });
    }
    fs::rename(tmp, dst)
}

fn delete_backups(plan: &[RollbackStep]) {
    for step in plan {
        if let RollbackStep::Restore { bak, .. } = step {
            let _ = fs::remove_file(bak);
        }
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

fn remove_with_retry(path: &Path, attempts: u32, delay: Duration) -> std::io::Result<()> {
    let mut last = None;
    for _ in 0..attempts {
        match fs::remove_file(path) {
            Ok(()) => return Ok(()),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(()),
            Err(e) => {
                last = Some(e);
                std::thread::sleep(delay);
            }
        }
    }
    Err(last.unwrap_or_else(|| std::io::Error::other("remove failed")))
}

fn wait_for_init_confirmed(listener: &protocol::IpcListener, timeout: Duration) -> bool {
    let deadline = Instant::now() + timeout;
    let stream = match protocol::accept_timeout(listener, timeout) {
        Ok(stream) => stream,
        Err(e) => {
            log(&format!("nothing connected to the init pipe: {e}"));
            return false;
        }
    };
    let remaining = deadline
        .saturating_duration_since(Instant::now())
        .max(READ_GRACE);
    let rx = protocol::spawn_reader(Arc::new(stream));
    match rx.recv_timeout(remaining) {
        Ok(Ok(Message::InitConfirmed)) => true,
        Ok(Ok(other)) => {
            log(&format!("unexpected message on the init pipe: {other:?}"));
            false
        }
        Ok(Err(e)) => {
            log(&format!("failed to read from the init pipe: {e}"));
            false
        }
        Err(e) => {
            log(&format!("no init_confirmed on the init pipe: {e}"));
            false
        }
    }
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

fn tmp_path(target: &Path) -> PathBuf {
    with_suffix(target, "tmp")
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
