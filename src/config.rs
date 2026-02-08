use std::fs::read_to_string;

use log::*;
use serde::Deserialize;

pub const PATH: &str = "config.ron";

pub fn get_config() -> Option<Config> {
    match read_to_string(PATH) {
        Ok(content) => {
            info!("Reading {PATH}");
            match ron::from_str(content.as_str()) {
                Ok(config) => Some(config),
                Err(e) => {
                    error!("Couldn't deserealize {PATH}:\n{e}");
                    None
                }
            }
        }
        Err(_) => {
            info!("{PATH} not found, using defaults");
            None
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
pub struct Config {
    pub bind: String,
    pub policy: Policy,
    pub mpsc_channel_buffer: usize,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Policy {
    pub variable_name_max_length: usize,
    pub variable_name_min_length: usize,
    pub value_max_length: usize,
    pub username_max_length: usize,
    pub username_min_length: usize,
}
