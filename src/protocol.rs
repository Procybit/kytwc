use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Debug, Deserialize, Serialize)]
#[serde(tag = "method", rename_all = "snake_case")]
pub enum Operation {
    Handshake { project_id: String, user: String },
    Set { name: String, value: Value },
    Create { name: String, value: Value },
    Rename { name: String, new_name: String },
    Delete { name: String },
}
