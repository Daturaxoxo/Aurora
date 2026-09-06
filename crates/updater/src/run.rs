use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use ipc::lock::SingletonLock;
use ipc::manifest::{FileEntry, LocalManifest, Manifest, RemovalCandidate, UpdateDelta, hash_file};
use ipc::protocol::{self, Message};

use crate::logfile;
use crate::logfile::log;
use crate::net;

type Conn = Arc<Mutex<protocol::IpcStream>>;

const PROGRESS_INTERVAL: Duration = Duration::from_millis(100);
const ROLLBACK_ATTEMPTS: u32 = 8;
const ROLLBACK_DELAY: Duration = Duration::from_millis(250);
const READ_GRACE: Duration = Duration::from_millis(500);
const ACCEPT_SLICE: Duration = Duration::from_millis(250);

enum RollbackStep {
    RestoreReplacement { dst: PathBuf, bak: PathBuf },
    RestoreRemoval { dst: PathBuf, bak: PathBuf },
    Remove { dst: PathBuf },
}

static PEER_LOST: AtomicBool = AtomicBool::new(false);

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

    let connected = connect_with_retry(
        &pipe_candidates(),
        ipc::UPDATER_CONNECT_ATTEMPTS,
        ipc::UPDATER_CONNECT_RETRY_DELAY,
    );
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

fn pipe_candidates() -> Vec<String> {
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        if arg == ipc::PIPE_ARG
            && let Some(pipe) = args.next().filter(|p| !p.is_empty())
        {
            return vec![pipe];
        }
    }
    ipc::main_pipe_candidates()
}

fn connect_with_retry(
    pipes: &[String],
    attempts: u32,
    delay: Duration,
) -> Option<protocol::IpcStream> {
    let mut last = None;
    for attempt in 0..attempts {
        for pipe in pipes {
            match protocol::connect(pipe) {
                Ok(stream) => {
                    log(&format!("connected to Aurora on `{pipe}`"));
                    return Some(stream);
                }
                Err(e) => last = Some(e),
            }
        }
        if attempt + 1 < attempts {
            std::thread::sleep(delay);
        }
    }
    log(&format!(
        "IPC connect failed after {attempts} attempt(s) on {pipes:?}: {}",
        last.map_or_else(|| "no error recorded".to_owned(), |e| e.to_string())
    ));
    None
}

fn send(conn: &Conn, msg: &Message) {
    let delivered = conn
        .lock()
        .is_ok_and(|mut stream| protocol::write_message(&mut *stream, msg).is_ok());
    if !delivered {
        PEER_LOST.store(true, Ordering::Relaxed);
    }
}

fn peer_lost() -> bool {
    PEER_LOST.load(Ordering::Relaxed)
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

    reconcile_orphans(root, &manifest, &local);

    let delta = manifest.update_delta(root, &local);
    if delta.is_empty() {
        log("no changes; local state matches manifest");
        local.version = manifest.version;
        heartbeat.stop();
        local
            .save(root)
            .map_err(|e| format!("failed to save local manifest: {e}"))?;
        send(conn, &Message::NoUpdate);
        return Ok(());
    }
    log(&format!(
        "update delta: {} download(s), {} removal candidate(s)",
        delta.downloads.len(),
        delta.removals.len()
    ));

    if peer_lost() {
        heartbeat.stop();
        return Err(
            "Aurora stopped listening before the update began; nothing was changed".to_owned(),
        );
    }

    send(conn, &Message::Lock);
    let result = apply_update(root, conn, &manifest, &mut local, &delta);
    heartbeat.stop();
    result
}

fn apply_update(
    root: &Path,
    conn: &Conn,
    manifest: &Manifest,
    local: &mut LocalManifest,
    delta: &UpdateDelta<'_>,
) -> Result<(), String> {
    let original_local = local.clone();
    let changed = &delta.downloads;

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
        let sources = entry.download_urls();
        if let Err(e) = net::download_from_any(&sources, &tmp, &mut |done, total| {
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
        tmps.push(tmp.clone());

        let actual = hash_file(&tmp).map_err(|e| {
            cleanup(&tmps);
            if e.kind() == std::io::ErrorKind::NotFound {
                format!(
                    "{} was removed after downloading (likely antivirus); \
                     allow the Aurora folder and try again",
                    tmp.display()
                )
            } else {
                format!("failed to hash {}: {e}", tmp.display())
            }
        })?;
        if !ipc::manifest::hash_eq(&actual, &entry.sha256) {
            cleanup(&tmps);
            return Err(format!(
                "hash mismatch for {}: expected {}, got {actual}",
                entry.path, entry.sha256
            ));
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

    let removed_parents = stage_removals(root, &delta.removals, local, &mut plan);

    if exe_entries.is_empty() {
        local.version.clone_from(&manifest.version);
        if let Err(e) = local.save(root) {
            return Err(fail(
                root,
                manifest,
                &original_local,
                &plan,
                &tmps,
                &format!("failed to save local manifest: {e}"),
            ));
        }
        delete_backups(&plan);
        cleanup(&tmps);
        prune_empty_parents(root, &removed_parents);
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

    let listener = match protocol::listen(&ipc::init_pipe_name()) {
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
    if !wait_for_aurora_exit(root) {
        drop(listener);
        return Err(fail(
            root,
            manifest,
            &original_local,
            &plan,
            &tmps,
            &format!(
                "Aurora did not shut down within {}s; the update was abandoned before replacing {}",
                ipc::AURORA_EXIT_TIMEOUT.as_secs(),
                ipc::AURORA_EXE
            ),
        ));
    }

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
    plan.push(RollbackStep::RestoreReplacement {
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

    let mut child = child;
    let confirmed = if let Some(child) = child.as_mut() {
        wait_for_init_confirmed(&listener, child, ipc::INIT_CONFIRM_TIMEOUT)
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
        prune_empty_parents(root, &removed_parents);
        return Ok(());
    }

    if let Some(mut child) = child {
        if let Ok(Some(status)) = child.try_wait() {
            log(&format!(
                "the new Aurora is already gone ({status}); nothing to terminate"
            ));
        } else {
            log("no init_confirmed within timeout; terminating the new Aurora");
            if let Err(e) = child.kill() {
                log(&format!("failed to terminate the new Aurora: {e}"));
            }
            if let Err(e) = child.wait() {
                log(&format!("failed to reap the new Aurora: {e}"));
            }
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
            RollbackStep::RestoreReplacement { dst, bak }
            | RollbackStep::RestoreRemoval { dst, bak } => {
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
    resync_local_manifest(root, manifest, original_local);
    Some(failures.join("; "))
}

fn resync_local_manifest(root: &Path, manifest: &Manifest, original_local: &LocalManifest) {
    log("rebuilding the local manifest from what is on disk");
    let mut rebuilt = LocalManifest::build_manifest_from_disk(root, manifest);
    for (path, hash) in &original_local.files {
        rebuilt
            .files
            .entry(path.clone())
            .or_insert_with(|| hash.clone());
    }
    rebuilt.version.clone_from(&original_local.version);
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

fn stage_removals(
    root: &Path,
    candidates: &[RemovalCandidate],
    local: &mut LocalManifest,
    plan: &mut Vec<RollbackStep>,
) -> Vec<PathBuf> {
    let mut parents = Vec::new();

    for candidate in candidates {
        let Some(dst) = ipc::manifest::safe_join(root, &candidate.path) else {
            log(&format!(
                "preserving removal candidate `{}`: path is unsafe",
                candidate.path
            ));
            continue;
        };
        let bak = bak_path(&dst);

        let metadata = match fs::symlink_metadata(&dst) {
            Ok(metadata) => metadata,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                if backup_path_is_occupied(&bak) {
                    log(&format!(
                        "preserving removal candidate `{}`: its backup could not be reconciled",
                        candidate.path
                    ));
                    continue;
                }
                log(&format!(
                    "removal candidate `{}` is already absent",
                    candidate.path
                ));
                local.files.remove(&candidate.path);
                continue;
            }
            Err(e) => {
                log(&format!(
                    "preserving removal candidate `{}`: could not inspect it: {e}",
                    candidate.path
                ));
                continue;
            }
        };

        if metadata.file_type().is_symlink() || !metadata.is_file() {
            log(&format!(
                "preserving removal candidate `{}`: it is not a regular file",
                candidate.path
            ));
            continue;
        }
        if !parent_resolves_below_root(root, &dst) {
            log(&format!(
                "preserving removal candidate `{}`: its parent resolves outside the install root",
                candidate.path
            ));
            continue;
        }

        let actual = match hash_file(&dst) {
            Ok(actual) => actual,
            Err(e) => {
                log(&format!(
                    "preserving removal candidate `{}`: could not hash it: {e}",
                    candidate.path
                ));
                continue;
            }
        };
        if !ipc::manifest::hash_eq(&actual, &candidate.sha256) {
            log(&format!(
                "preserving modified removal candidate `{}`",
                candidate.path
            ));
            continue;
        }

        if backup_path_is_occupied(&bak) {
            log(&format!(
                "preserving removal candidate `{}`: backup path {} already exists",
                candidate.path,
                bak.display()
            ));
            continue;
        }
        if let Err(e) =
            rename_without_overwrite_with_retry(&dst, &bak, ROLLBACK_ATTEMPTS, ROLLBACK_DELAY)
        {
            log(&format!(
                "preserving removal candidate `{}`: could not stage it for removal: {e}",
                candidate.path
            ));
            continue;
        }
        let staged_matches =
            hash_file(&bak).is_ok_and(|actual| ipc::manifest::hash_eq(&actual, &candidate.sha256));
        if !staged_matches {
            log(&format!(
                "preserving removal candidate `{}`: it changed while being staged",
                candidate.path
            ));
            if backup_path_is_occupied(&dst)
                || rename_without_overwrite_with_retry(
                    &bak,
                    &dst,
                    ROLLBACK_ATTEMPTS,
                    ROLLBACK_DELAY,
                )
                .is_err()
            {
                log(&format!(
                    "could not move {} back into place; ownership and its backup were retained",
                    candidate.path
                ));
            }
            continue;
        }

        log(&format!("staged `{}` for removal", candidate.path));
        plan.push(RollbackStep::RestoreRemoval {
            dst: dst.clone(),
            bak,
        });
        local.files.remove(&candidate.path);
        if let Some(parent) = dst.parent() {
            parents.push(parent.to_path_buf());
        }
    }

    parents
}

fn parent_resolves_below_root(root: &Path, path: &Path) -> bool {
    let (Ok(root), Some(parent)) = (fs::canonicalize(root), path.parent()) else {
        return false;
    };
    fs::canonicalize(parent).is_ok_and(|parent| parent.starts_with(root))
}

fn prune_empty_parents(root: &Path, parents: &[PathBuf]) {
    let mut parents = parents.to_vec();
    parents.sort_by(|a, b| {
        b.components()
            .count()
            .cmp(&a.components().count())
            .then_with(|| a.cmp(b))
    });
    parents.dedup();

    for start in parents {
        let mut current = start;
        while current != root && current.starts_with(root) {
            let parent = current.parent().map(Path::to_path_buf);
            let safe_directory = fs::symlink_metadata(&current).is_ok_and(|metadata| {
                metadata.is_dir()
                    && !metadata.file_type().is_symlink()
                    && fs::canonicalize(&current).is_ok_and(|current| {
                        fs::canonicalize(root).is_ok_and(|root| current.starts_with(root))
                    })
            });
            if !safe_directory {
                break;
            }
            match fs::remove_dir(&current) {
                Ok(()) => log(&format!("removed empty directory {}", current.display())),
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
                Err(e)
                    if matches!(
                        e.kind(),
                        std::io::ErrorKind::DirectoryNotEmpty
                            | std::io::ErrorKind::PermissionDenied
                    ) =>
                {
                    break;
                }
                Err(e) => {
                    log(&format!(
                        "could not remove parent directory {}: {e}",
                        current.display()
                    ));
                    break;
                }
            }
            let Some(parent) = parent else { break };
            current = parent;
        }
    }
}

fn swap_in(dst: &Path, tmp: &Path, plan: &mut Vec<RollbackStep>) -> std::io::Result<()> {
    if dst.exists() {
        let bak = bak_path(dst);
        if bak.exists() {
            let previous = old_bak_path(dst);
            let _ = fs::remove_file(&previous);
            if fs::rename(&bak, &previous).is_err() {
                let _ = fs::remove_file(&bak);
            }
        }
        fs::rename(dst, &bak)?;
        plan.push(RollbackStep::RestoreReplacement {
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
        match step {
            RollbackStep::RestoreReplacement { dst, bak } => {
                let _ = fs::remove_file(bak);
                let _ = fs::remove_file(old_bak_path(dst));
            }
            RollbackStep::RestoreRemoval { bak, .. } => {
                let _ = fs::remove_file(bak);
            }
            RollbackStep::Remove { .. } => {}
        }
    }
}

fn cleanup(tmps: &[PathBuf]) {
    for tmp in tmps {
        let _ = fs::remove_file(tmp);
        net::cleanup_attempts(tmp);
    }
}

fn rename_with_retry(
    from: &Path,
    to: &Path,
    attempts: u32,
    delay: Duration,
) -> std::io::Result<()> {
    let mut last = None;
    for attempt in 0..attempts {
        match fs::rename(from, to) {
            Ok(()) => return Ok(()),
            Err(e) => {
                if to.exists() && fs::remove_file(to).is_ok() && fs::rename(from, to).is_ok() {
                    return Ok(());
                }
                last = Some(e);
                if attempt + 1 < attempts {
                    std::thread::sleep(delay);
                }
            }
        }
    }
    Err(last.unwrap_or_else(|| std::io::Error::other("rename failed")))
}

fn rename_without_overwrite_with_retry(
    from: &Path,
    to: &Path,
    attempts: u32,
    delay: Duration,
) -> std::io::Result<()> {
    let mut last = None;
    for attempt in 0..attempts {
        if backup_path_is_occupied(to) {
            return Err(std::io::Error::new(
                std::io::ErrorKind::AlreadyExists,
                format!("{} already exists", to.display()),
            ));
        }
        match fs::rename(from, to) {
            Ok(()) => return Ok(()),
            Err(e) => {
                last = Some(e);
                if attempt + 1 < attempts {
                    std::thread::sleep(delay);
                }
            }
        }
    }
    Err(last.unwrap_or_else(|| std::io::Error::other("rename failed")))
}

fn backup_path_is_occupied(path: &Path) -> bool {
    match fs::symlink_metadata(path) {
        Ok(_) => true,
        Err(e) => e.kind() != std::io::ErrorKind::NotFound,
    }
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

#[cfg(windows)]
fn wait_for_aurora_exit(root: &Path) -> bool {
    let lock = root.join(ipc::AURORA_LOCK_FILE);
    if ipc::lock::wait_until_released(&lock, ipc::AURORA_EXIT_TIMEOUT) {
        log("the running Aurora has exited");
        return true;
    }
    log(&format!(
        "the running Aurora still holds {} after {}s",
        ipc::AURORA_LOCK_FILE,
        ipc::AURORA_EXIT_TIMEOUT.as_secs()
    ));
    false
}

#[cfg(all(test, not(windows)))]
const fn wait_for_aurora_exit(_root: &Path) -> bool {
    false
}

fn accept_init(
    listener: &protocol::IpcListener,
    child: &mut std::process::Child,
    deadline: Instant,
) -> Option<protocol::IpcStream> {
    loop {
        let left = deadline.saturating_duration_since(Instant::now());
        if left.is_zero() {
            log("nothing connected to the init pipe before the timeout");
            return None;
        }
        match protocol::accept_timeout(listener, left.min(ACCEPT_SLICE)) {
            Ok(stream) => return Some(stream),
            Err(e) if e.kind() == std::io::ErrorKind::TimedOut => {
                if let Ok(Some(status)) = child.try_wait() {
                    log(&format!(
                        "the new Aurora exited ({status}) without confirming init"
                    ));
                    return None;
                }
            }
            Err(e) => {
                log(&format!("could not accept on the init pipe: {e}"));
                return None;
            }
        }
    }
}

fn wait_for_init_confirmed(
    listener: &protocol::IpcListener,
    child: &mut std::process::Child,
    timeout: Duration,
) -> bool {
    let deadline = Instant::now() + timeout;
    let Some(stream) = accept_init(listener, child, deadline) else {
        return false;
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
                if peer_lost() {
                    break;
                }
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

fn old_bak_path(path: &Path) -> PathBuf {
    with_suffix(path, "bak.old")
}

fn reconcile_orphans(root: &Path, manifest: &Manifest, local: &LocalManifest) {
    for entry in &manifest.files {
        let Some(dst) = entry.resolve(root) else {
            continue;
        };

        restore_interrupted_backup(&dst, &entry.path);

        let tmp = tmp_path(&dst);
        if tmp.exists() {
            let _ = fs::remove_file(&tmp);
        }

        net::cleanup_attempts(&tmp);
    }

    for candidate in manifest.update_delta(root, local).removals {
        let Some(dst) = ipc::manifest::safe_join(root, &candidate.path) else {
            continue;
        };
        if parent_resolves_below_root(root, &dst) {
            restore_interrupted_removal_backup(&dst, &candidate);
        }
    }
}

fn restore_interrupted_removal_backup(dst: &Path, candidate: &RemovalCandidate) {
    let bak = bak_path(dst);
    if backup_path_is_occupied(dst) || !backup_path_is_occupied(&bak) {
        return;
    }
    let safe_backup = fs::symlink_metadata(&bak).is_ok_and(|metadata| {
        metadata.is_file()
            && !metadata.file_type().is_symlink()
            && hash_file(&bak)
                .is_ok_and(|actual| ipc::manifest::hash_eq(&actual, &candidate.sha256))
    });
    if !safe_backup {
        log(&format!(
            "could not restore interrupted removal `{}`: its backup is not the owned file",
            candidate.path
        ));
        return;
    }
    restore_interrupted_backup(dst, &candidate.path);
}

fn restore_interrupted_backup(dst: &Path, relative: &str) {
    let bak = bak_path(dst);
    if !dst.exists() && bak.exists() {
        match fs::rename(&bak, dst) {
            Ok(()) => log(&format!(
                "restored {relative} from a backup left by an interrupted update"
            )),
            Err(e) => log(&format!(
                "could not restore {relative} from {}: {e}",
                bak.display()
            )),
        }
    }
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

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;
    use std::sync::atomic::{AtomicUsize, Ordering};

    use ipc::manifest::hash_bytes;

    use super::*;

    static NEXT_TEST_DIR: AtomicUsize = AtomicUsize::new(0);

    struct TestDir(PathBuf);

    impl TestDir {
        fn new() -> Self {
            let id = NEXT_TEST_DIR.fetch_add(1, Ordering::Relaxed);
            let path = std::env::temp_dir()
                .join(format!("aurora-updater-test-{}-{id}", std::process::id()));
            fs::create_dir(&path).unwrap();
            Self(path)
        }
    }

    impl Drop for TestDir {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    fn candidate(path: &str, contents: &[u8]) -> RemovalCandidate {
        RemovalCandidate {
            path: path.to_owned(),
            sha256: hash_bytes(contents),
        }
    }

    fn local(path: &str, contents: &[u8]) -> LocalManifest {
        LocalManifest {
            version: "2.0.0".to_owned(),
            files: BTreeMap::from([(path.to_owned(), hash_bytes(contents))]),
        }
    }

    fn empty_manifest() -> Manifest {
        Manifest {
            version: "2.1.0".to_owned(),
            updater_hash: String::new(),
            files: Vec::new(),
        }
    }

    #[test]
    fn removal_only_transaction_removes_owned_file_and_empty_parents() {
        let root = TestDir::new();
        let nested = root.0.join("Bin").join("Legacy");
        fs::create_dir_all(&nested).unwrap();
        fs::write(nested.join("old.dll"), b"owned").unwrap();
        let mut local = local("Bin/Legacy/old.dll", b"owned");
        let mut plan = Vec::new();

        let parents = stage_removals(
            &root.0,
            &[candidate("Bin/Legacy/old.dll", b"owned")],
            &mut local,
            &mut plan,
        );

        assert!(!nested.join("old.dll").exists());
        assert!(bak_path(&nested.join("old.dll")).exists());
        assert!(!local.files.contains_key("Bin/Legacy/old.dll"));
        delete_backups(&plan);
        prune_empty_parents(&root.0, &parents);
        assert!(!root.0.join("Bin").exists());
    }

    #[test]
    fn mixed_replacement_and_removal_commit_together() {
        let root = TestDir::new();
        let changed = root.0.join("changed.dll");
        let changed_tmp = tmp_path(&changed);
        let obsolete = root.0.join("obsolete.dll");
        fs::write(&changed, b"old").unwrap();
        fs::write(&changed_tmp, b"new").unwrap();
        fs::write(&obsolete, b"obsolete").unwrap();
        let mut local = LocalManifest {
            version: "2.0.0".to_owned(),
            files: BTreeMap::from([
                ("changed.dll".to_owned(), hash_bytes(b"old")),
                ("obsolete.dll".to_owned(), hash_bytes(b"obsolete")),
            ]),
        };
        let mut plan = Vec::new();

        swap_in(&changed, &changed_tmp, &mut plan).unwrap();
        local
            .files
            .insert("changed.dll".to_owned(), hash_bytes(b"new"));
        let parents = stage_removals(
            &root.0,
            &[candidate("obsolete.dll", b"obsolete")],
            &mut local,
            &mut plan,
        );
        delete_backups(&plan);
        prune_empty_parents(&root.0, &parents);

        assert_eq!(fs::read(changed).unwrap(), b"new");
        assert!(!obsolete.exists());
        assert_eq!(local.files.get("changed.dll"), Some(&hash_bytes(b"new")));
        assert!(!local.files.contains_key("obsolete.dll"));
    }

    #[test]
    fn modified_file_is_preserved_and_owned_until_a_retry_matches() {
        let root = TestDir::new();
        let path = root.0.join("obsolete.dll");
        fs::write(&path, b"modified").unwrap();
        let removal = candidate("obsolete.dll", b"owned");
        let mut local = local("obsolete.dll", b"owned");
        let mut first_plan = Vec::new();

        stage_removals(
            &root.0,
            std::slice::from_ref(&removal),
            &mut local,
            &mut first_plan,
        );

        assert_eq!(fs::read(&path).unwrap(), b"modified");
        assert!(local.files.contains_key("obsolete.dll"));
        assert_eq!(first_plan.len(), 0);

        fs::write(&path, b"owned").unwrap();
        let mut retry_plan = Vec::new();
        stage_removals(&root.0, &[removal], &mut local, &mut retry_plan);
        delete_backups(&retry_plan);

        assert!(!path.exists());
        assert!(!local.files.contains_key("obsolete.dll"));
    }

    #[test]
    fn failed_removal_does_not_overwrite_existing_backup() {
        let root = TestDir::new();
        let path = root.0.join("obsolete.dll");
        let backup = bak_path(&path);
        fs::write(&path, b"owned").unwrap();
        fs::write(&backup, b"unrelated backup").unwrap();
        let mut local = local("obsolete.dll", b"owned");
        let mut plan = Vec::new();

        stage_removals(
            &root.0,
            &[candidate("obsolete.dll", b"owned")],
            &mut local,
            &mut plan,
        );

        assert_eq!(fs::read(path).unwrap(), b"owned");
        assert_eq!(fs::read(backup).unwrap(), b"unrelated backup");
        assert!(local.files.contains_key("obsolete.dll"));
        assert_eq!(plan.len(), 0);
    }

    #[test]
    fn rollback_restores_removed_file_and_original_ownership() {
        let root = TestDir::new();
        let path = root.0.join("obsolete.dll");
        fs::write(&path, b"owned").unwrap();
        let original = local("obsolete.dll", b"owned");
        let mut local = original.clone();
        let mut plan = Vec::new();

        stage_removals(
            &root.0,
            &[candidate("obsolete.dll", b"owned")],
            &mut local,
            &mut plan,
        );
        assert!(!path.exists());

        assert!(roll_back(&root.0, &empty_manifest(), &original, &plan).is_none());
        assert_eq!(fs::read(path).unwrap(), b"owned");
        assert_eq!(
            LocalManifest::load(&root.0).unwrap().unwrap().files,
            original.files
        );
    }

    #[test]
    fn rollback_resync_keeps_stale_ownership_for_future_retry() {
        let root = TestDir::new();
        fs::write(root.0.join("current.dll"), b"current").unwrap();
        let original = LocalManifest {
            version: "2.0.0".to_owned(),
            files: BTreeMap::from([
                ("current.dll".to_owned(), hash_bytes(b"old current")),
                ("obsolete.dll".to_owned(), hash_bytes(b"obsolete")),
            ]),
        };
        let remote = Manifest {
            version: "2.1.0".to_owned(),
            updater_hash: String::new(),
            files: vec![FileEntry {
                path: "current.dll".to_owned(),
                sha256: hash_bytes(b"current"),
                url: String::new(),
            }],
        };

        resync_local_manifest(&root.0, &remote, &original);

        let saved = LocalManifest::load(&root.0).unwrap().unwrap();
        assert_eq!(saved.version, "2.0.0");
        assert_eq!(
            saved.files.get("current.dll"),
            Some(&hash_bytes(b"current"))
        );
        assert_eq!(
            saved.files.get("obsolete.dll"),
            Some(&hash_bytes(b"obsolete"))
        );
    }

    #[test]
    fn interrupted_removal_is_restored_before_retrying() {
        let root = TestDir::new();
        let path = root.0.join("obsolete.dll");
        let backup = bak_path(&path);
        fs::write(&path, b"owned").unwrap();
        fs::rename(&path, &backup).unwrap();
        let local = local("obsolete.dll", b"owned");

        reconcile_orphans(&root.0, &empty_manifest(), &local);

        assert_eq!(fs::read(path).unwrap(), b"owned");
        assert!(!backup.exists());
    }

    #[test]
    fn interrupted_removal_does_not_restore_an_unrelated_backup() {
        let root = TestDir::new();
        let path = root.0.join("obsolete.dll");
        let backup = bak_path(&path);
        fs::write(&backup, b"unrelated").unwrap();
        let local = local("obsolete.dll", b"owned");

        reconcile_orphans(&root.0, &empty_manifest(), &local);

        assert!(!path.exists());
        assert_eq!(fs::read(backup).unwrap(), b"unrelated");
    }

    #[cfg(unix)]
    #[test]
    fn symlink_removal_candidate_is_preserved() {
        use std::os::unix::fs::symlink;

        let root = TestDir::new();
        let outside = root.0.with_extension("outside");
        fs::write(&outside, b"owned").unwrap();
        let link = root.0.join("obsolete.dll");
        symlink(&outside, &link).unwrap();
        let mut local = local("obsolete.dll", b"owned");
        let mut plan = Vec::new();

        stage_removals(
            &root.0,
            &[candidate("obsolete.dll", b"owned")],
            &mut local,
            &mut plan,
        );

        assert!(fs::symlink_metadata(link).unwrap().file_type().is_symlink());
        assert_eq!(fs::read(&outside).unwrap(), b"owned");
        assert!(local.files.contains_key("obsolete.dll"));
        assert_eq!(plan.len(), 0);
        fs::remove_file(outside).unwrap();
    }

    #[test]
    fn already_missing_owned_file_is_forgotten_without_scanning() {
        let root = TestDir::new();
        let mut local = local("nested/obsolete.dll", b"owned");
        let mut plan = Vec::new();

        stage_removals(
            &root.0,
            &[candidate("nested/obsolete.dll", b"owned")],
            &mut local,
            &mut plan,
        );

        assert!(!local.files.contains_key("nested/obsolete.dll"));
        assert_eq!(plan.len(), 0);
    }

    #[test]
    fn nonempty_parent_directory_is_preserved() {
        let root = TestDir::new();
        let nested = root.0.join("Bin").join("Legacy");
        fs::create_dir_all(&nested).unwrap();
        fs::write(nested.join("old.dll"), b"owned").unwrap();
        fs::write(nested.join("user.txt"), b"unowned").unwrap();
        let mut local = local("Bin/Legacy/old.dll", b"owned");
        let mut plan = Vec::new();

        let parents = stage_removals(
            &root.0,
            &[candidate("Bin/Legacy/old.dll", b"owned")],
            &mut local,
            &mut plan,
        );
        delete_backups(&plan);
        prune_empty_parents(&root.0, &parents);

        assert_eq!(fs::read(nested.join("user.txt")).unwrap(), b"unowned");
        assert!(nested.exists());
    }
}
