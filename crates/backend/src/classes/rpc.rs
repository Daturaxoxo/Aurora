use anyhow::{anyhow, Context, Result};
use discord_rich_presence::{
    activity::{Activity, Assets, Button, Timestamps},
    DiscordIpc, DiscordIpcClient,
};
use log::*;
use shared::config::{self, key};
use shared::utils::{self, get_current_timestamp};

use std::sync::mpsc::{self, RecvTimeoutError, Sender};
use std::thread;
use std::time::Duration;

const APPLICATION_ID: &str = "1505644188060876920";

const MIN_BACKOFF: Duration = Duration::from_secs(15);
const MAX_BACKOFF: Duration = Duration::from_secs(5 * 60);

/// Global Discord RPC client
pub static RPC: std::sync::LazyLock<DiscordRpc> = std::sync::LazyLock::new(|| {
    DiscordRpc::new(utils::get_current_timestamp()).unwrap_or_default()
});

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum RpcCommand {
    SetIdle,
    SetLaunching,
    SetInGame,
    ClearActivity,
    Reconnect,
    Stop,
}

pub struct DiscordRpc {
    sender: Option<Sender<RpcCommand>>,
}

impl Default for DiscordRpc {
    fn default() -> Self {
        Self { sender: None }
    }
}

impl DiscordRpc {
    pub fn new(start_timestamp: i64) -> Result<Self> {
        let (tx, rx) = mpsc::channel::<RpcCommand>();

        thread::spawn(move || {
            fn get_buttons() -> Vec<Button<'static>> {
                vec![
                    Button::new("Join Discord Server", "https://discord.gg/565jfeYsbp"),
                    Button::new("Github", "https://github.com/Daturaxoxo/Aurora"),
                ]
            }

            fn get_assets() -> Assets<'static> {
                Assets::new()
                    .large_image("launcher")
                    .large_text("Aurora Mod Launcher")
            }

            fn rpc_enabled() -> bool {
                config::get(key::DISCORD_RPC).as_bool().unwrap_or(true)
            }

            fn apply(
                client: &mut DiscordIpcClient,
                state: RpcCommand,
                start_timestamp: i64,
            ) -> std::result::Result<(), discord_rich_presence::error::Error> {
                match state {
                    RpcCommand::SetIdle => client.set_activity(
                        Activity::new()
                            .state("Idle")
                            .details("In launcher")
                            .timestamps(Timestamps::new().start(start_timestamp))
                            .assets(
                                get_assets()
                                    .small_image("version")
                                    .small_text(format!("v{}", utils::get_local_version())),
                            )
                            .buttons(get_buttons()),
                    ),
                    RpcCommand::SetLaunching => client.set_activity(
                        Activity::new()
                            .state("Launching…")
                            .details("Starting NTE")
                            .assets(get_assets())
                            .timestamps(Timestamps::new().start(get_current_timestamp()))
                            .buttons(get_buttons()),
                    ),
                    RpcCommand::SetInGame => {
                        let v = [1, 2, 3, 4, 5];
                        let i = fastrand::usize(..v.len());
                        let elem = v[i];
                        client.set_activity(
                            Activity::new()
                                .state("In-game")
                                .details("Playing NTE")
                                .assets(
                                    Assets::new()
                                        .large_image(format!("in-game-{elem}"))
                                        .large_text("Playing NTE using Aurora!")
                                        .small_image("version")
                                        .small_text(format!("v{}", utils::get_local_version())),
                                )
                                .timestamps(Timestamps::new().start(get_current_timestamp()))
                                .buttons(get_buttons()),
                        )
                    }
                    RpcCommand::ClearActivity | RpcCommand::Reconnect | RpcCommand::Stop => {
                        unreachable!("non-activity command reached apply()")
                    }
                }
            }

            let mut client = DiscordIpcClient::new(APPLICATION_ID);
            let mut connected = false;
            let mut desired: Option<RpcCommand> = None;
            let mut backoff = MIN_BACKOFF;

            loop {
                let want_retry = !connected && desired.is_some() && rpc_enabled();
                let cmd = if want_retry {
                    match rx.recv_timeout(backoff) {
                        Ok(cmd) => Some(cmd),
                        Err(RecvTimeoutError::Timeout) => None, // time to retry connect
                        Err(RecvTimeoutError::Disconnected) => break,
                    }
                } else {
                    match rx.recv() {
                        Ok(cmd) => Some(cmd),
                        Err(_) => break,
                    }
                };

                if let Some(cmd) = cmd {
                    match cmd {
                        RpcCommand::Stop => {
                            if connected {
                                let _ = client.close();
                                connected = false;
                            }
                            desired = None;
                            continue;
                        }
                        RpcCommand::ClearActivity => {
                            desired = None;
                            if connected {
                                if let Err(e) = client.clear_activity() {
                                    debug!("Discord RPC clear failed, dropping connection: {e:?}");
                                    let _ = client.close();
                                    connected = false;
                                }
                            }
                            continue;
                        }
                        RpcCommand::Reconnect => {
                            if connected {
                                let _ = client.close();
                                connected = false;
                            }
                            backoff = MIN_BACKOFF;
                            // Fall through to the (re)connect + apply below
                        }
                        state => desired = Some(state),
                    }
                }

                if !rpc_enabled() {
                    continue;
                }

                let Some(state) = desired else {
                    continue;
                };

                if !connected {
                    match client.connect() {
                        Ok(()) => {
                            connected = true;
                            backoff = MIN_BACKOFF;
                            info!("Connected to Discord IPC");
                        }
                        Err(e) => {
                            warn!(
                                "Discord IPC unavailable, retrying in {}s: {e:?}",
                                backoff.as_secs()
                            );
                            backoff = (backoff * 2).min(MAX_BACKOFF);
                            continue;
                        }
                    }
                }

                if let Err(e) = apply(&mut client, state, start_timestamp) {
                    debug!("Discord RPC error, dropping connection: {e:?}");
                    let _ = client.close();
                    connected = false;
                    backoff = MIN_BACKOFF;
                }
            }
        });

        Ok(Self { sender: Some(tx) })
    }

    pub fn reconnect(&self) -> Result<()> {
        self.sender
            .as_ref()
            .ok_or_else(|| anyhow!("RPC thread not started"))?
            .send(RpcCommand::Reconnect)
            .context("Failed to send Reconnect command to RPC thread")
    }

    pub fn clear_activity(&self) -> Result<()> {
        self.sender
            .as_ref()
            .ok_or_else(|| anyhow!("RPC thread not started"))?
            .send(RpcCommand::ClearActivity)
            .context("Failed to send ClearActivity command to RPC thread")
    }

    pub fn set_idle(&self) -> Result<()> {
        self.sender
            .as_ref()
            .ok_or_else(|| anyhow!("RPC thread not started"))?
            .send(RpcCommand::SetIdle)
            .context("Failed to send SetIdle command to RPC thread")
    }

    pub fn set_launching(&self) -> Result<()> {
        self.sender
            .as_ref()
            .ok_or_else(|| anyhow!("RPC thread not started"))?
            .send(RpcCommand::SetLaunching)
            .context("Failed to send SetLaunching command to RPC thread")
    }

    pub fn set_ingame(&self) -> Result<()> {
        self.sender
            .as_ref()
            .ok_or_else(|| anyhow!("RPC thread not started"))?
            .send(RpcCommand::SetInGame)
            .context("Failed to send SetInGame command to RPC thread")
    }

    pub fn stop(&self) -> Result<()> {
        self.sender
            .as_ref()
            .ok_or_else(|| anyhow!("RPC thread not started"))?
            .send(RpcCommand::Stop)
            .context("Failed to send Stop command to RPC thread")
    }
}
