use std::collections::HashMap;

use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Debug, Serialize, Deserialize)]
pub struct Memory {
    #[serde(flatten)]
    pub rooms: HashMap<String, Room>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct Room {
    #[serde(flatten)]
    pub variables: HashMap<String, Value>,
}

impl Memory {
    pub fn new() -> Self {
        let rooms = HashMap::new();
        Self { rooms }
    }
}

impl Room {
    pub fn new() -> Self {
        let variables = HashMap::new();
        Self { variables }
    }
}
