pub const GAME_FOLDER_NAME: &str = "Neverness To Everness";
pub const PICTURE_FOLDER: &str = "NevernessToEverness";

cfg_select! {
    windows => {
        pub const CLIENT_WIN64: &str = "Client\\WindowsNoEditor\\HT\\Binaries\\Win64";
        pub const CLIENT_PAK_DIR: &str = "Client\\WindowsNoEditor\\HT\\Content\\Paks\\AuroraMods";
    }
    unix => {
        pub const CLIENT_WIN64: &str = "Client/WindowsNoEditor/HT/Binaries/Win64";
        pub const CLIENT_PAK_DIR: &str = "Client/WindowsNoEditor/HT/Content/Paks/AuroraMods";
    }
}
