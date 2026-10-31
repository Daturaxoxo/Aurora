use super::{LoadResult, ModLoader};
use crate::process::{memory, scan};
use crate::sig::{PatchType, Target};
const TAG: &str = "SigBypass";
pub struct SigBypass;

impl ModLoader for SigBypass {
    fn run(&self, targets: &[&Target]) -> LoadResult {
        let applied = targets.iter().filter(|t| apply(t)).count();
        LoadResult { matched: targets.len(), applied }
    }
}

fn apply(t: &Target) -> bool {
    let Some((dst, patch)) = resolve(t) else { return false };
    if memory::mem_equals(dst, &patch) {
        log_info!(TAG, "Target '{}' already patched at 0x{dst:X} ({} bytes), skipping.", t.label, patch.len());
        return true;
    }
    if !memory::write_bytes(dst, &patch) {
        return false;
    }
    log_info!(TAG, "Patched '{}' at 0x{dst:X} ({} bytes).", t.label, patch.len());
    true
}

fn resolve(t: &Target) -> Option<(usize, Vec<u8>)> {
    let Some(hit) = scan::find_in_main_module(&t.pattern) else {
        log_error!(TAG, "Pattern for '{}' not found.", t.label);
        return None;
    };
    log_debug!(TAG, "Target '{}' pattern hit at 0x{hit:X} (offset +0x{:X}).", t.label, t.offset);

    if !memory::validate_address(hit, "pattern hit") {
        return None;
    }
    let at = hit.wrapping_add(t.offset as usize);
    if !memory::validate_address(at, "pattern hit + offset") {
        return None;
    }

    match t.kind {
        PatchType::FollowJump => {
            let Some(dst) = memory::follow_jump(at) else {
                log_error!(TAG, "Could not follow the jump for '{}'.", t.label);
                return None;
            };
            Some((dst, t.bytes.clone()))
        }
        PatchType::Overwrite => Some((at, t.bytes.clone())),
        PatchType::Nop => Some((at, vec![0x90; t.bytes.len()])),
        PatchType::ResolveCall => {
            log_warn!(TAG, "Target '{}' uses resolve_call, which SigBypass doesn't handle; skipping.", t.label);
            None
        }
    }
}
