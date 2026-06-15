use serde::{Deserialize, Serialize};

const CONFIG_FILE: &str = "config.json";

#[derive(Serialize, Deserialize, Default)]
pub struct Config {
    pub game_path: String,
}

impl Config {
    pub fn new(game_path: String) -> Self {
        Self { game_path }
    }

    pub fn game_path(&self) -> String {
        self.game_path.clone()
    }
}

pub fn save_config(config: Config) -> Result<(), std::io::Error> {
    let serialized = serde_json::to_string(&config).unwrap();
    std::fs::write(CONFIG_FILE, serialized)?;

    Ok(())
}

pub fn load_config() -> Result<Config, std::io::Error> {
    let serialized = std::fs::read_to_string(CONFIG_FILE);
    if serialized.is_err() {
        return Ok(Config::default());
    }
    let config: Config = serde_json::from_str(&serialized?)?;
    
    Ok(config)
}