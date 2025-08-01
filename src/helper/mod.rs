use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VlcStatus {
    pub is_playing: bool,
    pub position: f64, // 0.0 to 1.0
    pub duration: i64, // in seconds
    pub title: String,
    pub artist: String,
} 