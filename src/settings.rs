use serde::Deserialize;
use config::{ File, Environment, Config };

#[derive(Debug, Deserialize)]
pub struct Settings {
    pub feed_url: String,
    pub bot_token: String,
    pub channel_name: String,
    pub interval_sec: u64,
}

impl Settings {
    pub fn new() -> Self {
        let mut config = Config::new();

        config.merge(File::with_name("config.json")).unwrap();
        config.merge(Environment::new()).unwrap();

        config.try_into().expect("Could not parse app config")
    }
}