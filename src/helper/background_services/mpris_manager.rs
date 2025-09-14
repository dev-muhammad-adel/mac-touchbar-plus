// MPRIS Manager for handling service selection and state management
// This module provides a centralized way to manage MPRIS services using OOP approach

use std::sync::{Arc, Mutex};
use std::os::unix::net::UnixStream;
use std::io::Write;
use serde_json::json;
use chrono;

// Include the actual service implementations
mod spotify {
    include!("spotify.rs");
}

mod chromium {
    include!("chromium.rs");
}

// Trait for MPRIS service implementations
pub trait MprisService {
    fn set_service_name(&mut self, service_name: &str);
    fn get_service_name(&self) -> Option<String>;
    fn get_status(&self) -> Option<MediaStatus>;
    fn execute_command(&self, command: &str) -> bool;
    fn start_monitoring(&self, status_sender: Arc<Mutex<Option<UnixStream>>>);
}

#[derive(Debug, Clone, PartialEq)]
pub struct MediaStatus {
    pub is_playing: bool,
    pub title: String,
    pub artist: String,
    pub album: String,
    pub duration: f64,
    pub position: f64,
}

impl MediaStatus {
    pub fn empty() -> Self {
        Self {
            is_playing: false,
            title: String::new(),
            artist: String::new(),
            album: String::new(),
            duration: 0.0,
            position: 0.0,
        }
    }
}

// MPRIS Manager class
pub struct MprisManager {
    spotify_service: Arc<Mutex<SpotifyService>>,
    chromium_service: Arc<Mutex<ChromiumService>>,
    current_service_type: Arc<Mutex<Option<ServiceType>>>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum ServiceType {
    Spotify,
    Chromium,
}

impl MprisManager {
    pub fn new() -> Self {
        Self {
            spotify_service: Arc::new(Mutex::new(SpotifyService::new())),
            chromium_service: Arc::new(Mutex::new(ChromiumService::new())),
            current_service_type: Arc::new(Mutex::new(None)),
        }
    }

    pub fn select_service(&self, service_name: &str) -> Result<(), String> {
        println!("[mpris_manager] Selecting service: {}", service_name);
        
        if service_name.contains("spotify") {
            let mut spotify = self.spotify_service.lock().unwrap();
            spotify.set_service_name(service_name);
            drop(spotify);
            
            let mut current_type = self.current_service_type.lock().unwrap();
            *current_type = Some(ServiceType::Spotify);
            drop(current_type);
            
            println!("[mpris_manager] Selected Spotify service: {}", service_name);
            Ok(())
        } else if service_name.contains("chromium") {
            let mut chromium = self.chromium_service.lock().unwrap();
            chromium.set_service_name(service_name);
            drop(chromium);
            
            let mut current_type = self.current_service_type.lock().unwrap();
            *current_type = Some(ServiceType::Chromium);
            drop(current_type);
            
            println!("[mpris_manager] Selected Chromium service: {}", service_name);
            Ok(())
        } else {
            Err(format!("Unknown service type: {}", service_name))
        }
    }

    pub fn get_current_status(&self) -> Option<MediaStatus> {
        let current_type = self.current_service_type.lock().unwrap();
        
        match current_type.as_ref() {
            Some(ServiceType::Spotify) => {
                let spotify = self.spotify_service.lock().unwrap();
                spotify.get_status()
            }
            Some(ServiceType::Chromium) => {
                let chromium = self.chromium_service.lock().unwrap();
                chromium.get_status()
            }
            None => {
                println!("[mpris_manager] No service selected");
                None
            }
        }
    }

    pub fn execute_command(&self, command: &str) -> bool {
        let current_type = self.current_service_type.lock().unwrap();
        
        match current_type.as_ref() {
            Some(ServiceType::Spotify) => {
                println!("[mpris_manager] Executing command on Spotify: {}", command);
                let spotify = self.spotify_service.lock().unwrap();
                spotify.execute_command(command)
            }
            Some(ServiceType::Chromium) => {
                println!("[mpris_manager] Executing command on Chromium: {}", command);
                let chromium = self.chromium_service.lock().unwrap();
                chromium.execute_command(command)
            }
            None => {
                println!("[mpris_manager] No service selected for command: {}", command);
                false
            }
        }
    }

    pub fn start_monitoring(&self, status_sender: Arc<Mutex<Option<UnixStream>>>) {
        let current_type = self.current_service_type.lock().unwrap();
        
        match current_type.as_ref() {
            Some(ServiceType::Spotify) => {
                println!("[mpris_manager] Starting Spotify monitoring");
                let spotify = self.spotify_service.lock().unwrap();
                spotify.start_monitoring(status_sender);
            }
            Some(ServiceType::Chromium) => {
                println!("[mpris_manager] Starting Chromium monitoring");
                let chromium = self.chromium_service.lock().unwrap();
                chromium.start_monitoring(status_sender);
            }
            None => {
                println!("[mpris_manager] No service selected, not starting monitoring");
            }
        }
    }

    pub fn send_status_update(&self, status_sender: &Arc<Mutex<Option<UnixStream>>>) {
        if let Some(status) = self.get_current_status() {
            println!("[mpris_manager] Retrieved status: is_playing={}, title='{}', artist='{}'", 
                status.is_playing, status.title, status.artist);
            
            if let Ok(mut sender_guard) = status_sender.lock() {
                if let Some(ref mut stream) = *sender_guard {
                    let status_json = serde_json::to_string(&json!({
                        "is_playing": status.is_playing,
                        "title": status.title,
                        "artist": status.artist,
                        "album": status.album,
                        "duration": status.duration,
                        "position": status.position,
                        "timestamp": chrono::Utc::now().timestamp_millis(),
                    })).unwrap_or_default();
                    
                    let message = format!("status_update:{}\n", status_json);
                    let _ = stream.write_all(message.as_bytes());
                    println!("[mpris_manager] Sent status update: {}", status_json);
                } else {
                    println!("[mpris_manager] No stream available for status update");
                }
            } else {
                println!("[mpris_manager] Failed to lock status sender");
            }
        } else {
            println!("[mpris_manager] No status available to send");
        }
    }

    pub fn get_current_service_type(&self) -> Option<ServiceType> {
        let current_type = self.current_service_type.lock().unwrap();
        current_type.clone()
    }
}

// Spotify Service Implementation
pub struct SpotifyService {
    service_name: Option<String>,
}

impl SpotifyService {
    pub fn new() -> Self {
        Self {
            service_name: None,
        }
    }
}

impl MprisService for SpotifyService {
    fn set_service_name(&mut self, service_name: &str) {
        self.service_name = Some(service_name.to_string());
        println!("[spotify_service] Set service name to: {}", service_name);
        
        // Set the service name in the actual Spotify module
        spotify::set_current_mpris_service(service_name);
    }

    fn get_service_name(&self) -> Option<String> {
        self.service_name.clone()
    }

    fn get_status(&self) -> Option<MediaStatus> {
        // Use spawn_blocking to avoid runtime conflicts
        let result = std::thread::spawn(move || {
            let rt = tokio::runtime::Runtime::new().unwrap();
            rt.block_on(async {
                if let Some(spotify_status) = spotify::get_spotify_status_async().await {
                    Some(MediaStatus {
                        is_playing: spotify_status.is_playing,
                        title: spotify_status.title,
                        artist: spotify_status.artist,
                        album: spotify_status.album,
                        duration: spotify_status.duration,
                        position: spotify_status.position,
                    })
                } else {
                    None
                }
            })
        });
        
        result.join().unwrap_or(None)
    }

    fn execute_command(&self, command: &str) -> bool {
        println!("[spotify_service] Executing command: {}", command);
        
        let command = command.to_string();
        // Use spawn_blocking to avoid runtime conflicts
        std::thread::spawn(move || {
            let rt = tokio::runtime::Runtime::new().unwrap();
            rt.block_on(async {
                // Create a dummy status sender for command execution
                let status_sender = Arc::new(Mutex::new(None));
                spotify::handle_spotify_command(&command, &status_sender).await;
            });
        });
        
        true
    }

    fn start_monitoring(&self, status_sender: Arc<Mutex<Option<UnixStream>>>) {
        println!("[spotify_service] Starting monitoring");
        spotify::monitor_spotify_events(status_sender);
    }
}

// Chromium Service Implementation
pub struct ChromiumService {
    service_name: Option<String>,
}

impl ChromiumService {
    pub fn new() -> Self {
        Self {
            service_name: None,
        }
    }
}

impl MprisService for ChromiumService {
    fn set_service_name(&mut self, service_name: &str) {
        self.service_name = Some(service_name.to_string());
        println!("[chromium_service] Set service name to: {}", service_name);
        
        // Set the service name in the actual Chromium module
        chromium::set_current_mpris_service(service_name);
    }

    fn get_service_name(&self) -> Option<String> {
        self.service_name.clone()
    }

    fn get_status(&self) -> Option<MediaStatus> {
        // Use spawn_blocking to avoid runtime conflicts
        let result = std::thread::spawn(move || {
            let rt = tokio::runtime::Runtime::new().unwrap();
            rt.block_on(async {
                if let Some(chromium_status) = chromium::get_chromium_status().await {
                    Some(MediaStatus {
                        is_playing: chromium_status.is_playing,
                        title: chromium_status.title,
                        artist: chromium_status.artist,
                        album: chromium_status.album,
                        duration: chromium_status.duration,
                        position: chromium_status.position,
                    })
                } else {
                    None
                }
            })
        });
        
        result.join().unwrap_or(None)
    }

    fn execute_command(&self, command: &str) -> bool {
        println!("[chromium_service] Executing command: {}", command);
        
        let command = command.to_string();
        // Use spawn_blocking to avoid runtime conflicts
        std::thread::spawn(move || {
            let rt = tokio::runtime::Runtime::new().unwrap();
            rt.block_on(async {
                // Create a dummy status sender for command execution
                let status_sender = Arc::new(Mutex::new(None));
                chromium::handle_chromium_command(&command, &status_sender).await;
            });
        });
        
        true
    }

    fn start_monitoring(&self, status_sender: Arc<Mutex<Option<UnixStream>>>) {
        println!("[chromium_service] Starting monitoring");
        chromium::monitor_chromium_events(status_sender);
    }
}
