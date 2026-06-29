use anyhow::{Context, Result};
use discord_rich_presence::{
    activity::{Activity, Assets, Button, Timestamps},
    DiscordIpc, DiscordIpcClient,
};
use shared::utils::get_current_timestamp;
use std::sync::mpsc::{self, Sender};
use std::thread;

const APPLICATION_ID: &str = "1505644188060876920";

#[derive(Debug)]
enum RpcCommand {
    SetIdle,
    SetLaunching,
    SetInGame,
    ClearActivity,
    Reconnect,
    Stop,
}

pub struct DiscordRpc {
    sender: Sender<RpcCommand>,
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
                    .large_image("aurora_logo")
                    .large_text("Aurora Mod Launcher")
            }

            let mut client = DiscordIpcClient::new(APPLICATION_ID);

            if let Err(e) = client.connect() {
                eprintln!("Failed to connect to Discord IPC: {e:?}");
                return;
            }

            for cmd in rx {
                let res = match cmd {
                    RpcCommand::SetIdle => client.set_activity(
                        Activity::new()
                            .state("Idle")
                            .details("In launcher")
                            .timestamps(Timestamps::new().start(start_timestamp))
                            .assets(get_assets())
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
                    RpcCommand::SetInGame => client.set_activity(
                        Activity::new()
                            .state("In-game")
                            .details("Playing NTE")
                            .assets(get_assets().small_image("playing").small_text("In-game"))
                            .timestamps(Timestamps::new().start(get_current_timestamp()))
                            .buttons(get_buttons()),
                    ),
                    RpcCommand::ClearActivity => client.clear_activity(),
                    RpcCommand::Reconnect => client.reconnect(),
                    RpcCommand::Stop => {
                        let _ = client.close();
                        break;
                    }
                };

                if let Err(e) = res {
                    eprintln!("Discord RPC error processing command: {e:?}");
                }
            }
        });

        Ok(Self { sender: tx })
    }

    pub fn reconnect(&self) -> Result<()> {
        self.sender
            .send(RpcCommand::Reconnect)
            .context("Failed to send Reconnect command to RPC thread")
    }

    pub fn clear_activity(&self) -> Result<()> {
        self.sender
            .send(RpcCommand::ClearActivity)
            .context("Failed to send ClearActivity command to RPC thread")
    }

    pub fn set_idle(&self) -> Result<()> {
        self.sender
            .send(RpcCommand::SetIdle)
            .context("Failed to send SetIdle command to RPC thread")
    }

    pub fn set_launching(&self) -> Result<()> {
        self.sender
            .send(RpcCommand::SetLaunching)
            .context("Failed to send SetLaunching command to RPC thread")
    }

    pub fn set_ingame(&self) -> Result<()> {
        self.sender
            .send(RpcCommand::SetInGame)
            .context("Failed to send SetInGame command to RPC thread")
    }

    pub fn stop(&self) -> Result<()> {
        self.sender
            .send(RpcCommand::Stop)
            .context("Failed to send Stop command to RPC thread")
    }
}
