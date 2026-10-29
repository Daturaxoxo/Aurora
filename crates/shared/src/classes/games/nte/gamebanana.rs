use super::Nte;
use crate::classes::games::capabilities::{GameBananaSupport, RemoteCharacter};

const GAMEBANANA_ID: u32 = 23012;

const fn remote(name: &'static str, category: u32) -> RemoteCharacter {
    RemoteCharacter { name, category }
}

const REMOTE_CHARACTERS: &[RemoteCharacter] = &[
    remote("Adler", 43034),
    remote("Akane", 49661),
    remote("Aurelia", 46387),
    remote("Baicang", 43035),
    remote("Blackbird", 49659),
    remote("Chaos", 46559),
    remote("Chiz", 45472),
    remote("Daffodill", 45474),
    remote("Edgar", 45475),
    remote("Fadia", 43036),
    remote("Haniel", 45473),
    remote("Hathor", 43037),
    remote("Hotori", 43038),
    remote("Iroi", 46560),
    remote("Jiuyuan", 45476),
    remote("Lacrimosa", 43039),
    remote("Linko", 48013),
    remote("Mint", 43040),
    remote("Nanally", 43041),
    remote("Sakiri", 43042),
    remote("Shinku", 46561),
    remote("Skia", 43043),
    remote("Zankou", 48011),
    remote("Zero (F)", 43032),
    remote("Zero (M)", 43033),
];

impl GameBananaSupport for Nte {
    fn remote_game_id(&self) -> u32 {
        GAMEBANANA_ID
    }

    fn characters(&self) -> &[RemoteCharacter] {
        REMOTE_CHARACTERS
    }
}
