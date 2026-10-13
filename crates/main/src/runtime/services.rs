use super::GameContext;
use backend::classes::rpc::RPC;
use log::*;
use shared::api::ccu;
use shared::telemetry::{self, TelemetryContext};

pub(super) fn publish(context: &GameContext) {
    let d = context.game().descriptor();
    telemetry::set_context(TelemetryContext {
        generation: context.generation(),
        game: d.id.clone(),
        game_root: context.installation().map(|i| i.root().to_path_buf()),
        region: ccu::region_of(context.game(), context.installation()),
    });

    if let Err(e) = RPC.set_game(d.display_name) {
        warn!("Could not hand {} to Discord RPC: {e}", d.id);
    }
}
