use std::thread;
use windows_sys::Win32::Foundation::{CloseHandle, ERROR_ALREADY_EXISTS, GetLastError};
use windows_sys::Win32::System::Threading::CreateEventW;
use crate::loaders::ModLoader;
use crate::loaders::sigbypass::SigBypass;
use crate::sig::{self, LoadMethod, Target};
use crate::{log, paths};
const TAG: &str = "Everlight";

pub fn start() {
    if let Err(e) = thread::Builder::new().name("everlight".into()).spawn(run) {
        log_error!(TAG, "Could not start the Everlight thread: {e}");
    }
}

fn run() {
    if !claim_process() {
        log_info!(TAG, "Everlight is already running in this process, so this module will skip.");
        return;
    }
    main();
    log_info!(TAG, "Everlight finished.");
}

fn claim_process() -> bool {
    let name: Vec<u16> = format!("Local\\Everlight_{}", std::process::id())
        .encode_utf16()
        .chain(Some(0))
        .collect();
    unsafe {
        let event = CreateEventW(std::ptr::null(), 1, 0, name.as_ptr());
        if event.is_null() {
            return true;
        }
        if GetLastError() == ERROR_ALREADY_EXISTS {
            CloseHandle(event);
            return false;
        }
    }
    true
}

fn main() {
    let host = paths::host_exe_name();
    log_info!(
        TAG,
        "Everlight {} loaded. host=\"{host}\" pid={}",
        env!("CARGO_PKG_VERSION"),
        std::process::id()
    );

    let sig_path = paths::module_dir().join(sig::FILE_NAME);
    let parsed = match sig::load(&sig_path) {
        Ok(parsed) => parsed,
        Err(e) => {
            log_error!(TAG, "Could not load {}: {e}", sig_path.display());
            return;
        }
    };
    let cfg = &parsed.config;
    log::set_debug(cfg.settings.debug);
    for warning in &parsed.warnings {
        log_warn!(TAG, "{}: {warning}", sig::FILE_NAME);
    }
    for ignored in &parsed.ignored {
        log_debug!(TAG, "{}: ignored {ignored}", sig::FILE_NAME);
    }
    log_info!(
        TAG,
        "Config loaded. schema={} targets={} folder=\"{}\" debug={} default_method={}",
        cfg.schema,
        cfg.targets.len(),
        cfg.settings.folder,
        cfg.settings.debug,
        cfg.settings.method.name()
    );

    let for_host = |method| -> Vec<&Target> {
        cfg.targets
            .iter()
            .filter(|t| t.method == method && t.exe.eq_ignore_ascii_case(&host))
            .collect()
    };
    let sig_targets = for_host(LoadMethod::SigBypass);
    let asset_targets = for_host(LoadMethod::AssetLoader);

    if !sig_targets.is_empty() {
        let result = SigBypass.run(&sig_targets);
        let detail = result.detail();
        if result.ok() {
            log_info!(TAG, "SigBypass: {detail}.");
        } else {
            log_error!(TAG, "SigBypass: {detail}.");
        }
    }
    if !asset_targets.is_empty() {
        log_warn!(
            TAG,
            "{} assetloader target(s) match this host, but the Asset Loader of Everlight is not implemented yet - will skip.",
            asset_targets.len()
        );
    }
    if sig_targets.is_empty() && asset_targets.is_empty() {
        log_info!(TAG, "No targets match host \"{host}\" - nothing to do in this process (aborted).");
    }
}
