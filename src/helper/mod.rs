use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VlcStatus {
    pub is_playing: bool,
    pub position: f64, // 0.0 to 1.0
    pub duration: i64, // in seconds
    pub title: String,
    pub artist: String,
} 

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BrowserStatus {
    pub url: String,
    pub title: String,
    pub favicon_url: Option<String>,
    pub can_go_back: bool,
    pub can_go_forward: bool,
    pub is_loading: bool,
}

pub mod browser_helper;
pub mod manager; 