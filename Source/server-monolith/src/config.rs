use serde::{Deserialize, Serialize};
use std::fs;

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct Config {
    pub host: String,
    pub port: u16,
    pub users: Vec<User>,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct User {
    pub name: String,
    pub password: String,
}

impl Default for Config {
    fn default() -> Self {
        Config {
            host: "127.0.0.1".to_string(),
            port: 80,
            users: Vec::new(),
        }
    }
}

pub fn load_config() -> Config {
    // Load the configuration file into a Config struct
    let contents = match fs::read_to_string("server.config.toml") {
        Ok(contents) => contents,
        Err(_) => {
            println!("No configuration file found, creating one...");
            let config = Config::default();
            let toml = toml::to_string(&config).unwrap();
            fs::write("server.config.toml", toml).unwrap();
            return config;
        }
    };

    let config: Config = toml::from_str(&contents).unwrap();
    config
}