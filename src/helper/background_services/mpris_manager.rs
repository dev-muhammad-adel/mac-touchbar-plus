// MPRIS Manager for handling service selection and state management
// This module provides a centralized way to manage MPRIS services using OOP approach

use std::sync::{Arc, Mutex};
use std::os::unix::net::UnixStream;
use std::sync::atomic::{AtomicBool, Ordering};
use std::io::Write;
use serde_json::json;
use chrono;

// Helper functions to check if services are active (delegated to service modules)
pub fn is_spotify_active() -> bool {
    spotify::is_spotify_active()
}

pub fn is_chromium_active() -> bool {
    chromium::is_chromium_active()
}

// Use external service modules (they are siblings in the same binary)
use super::spotify;
use super::chromium;

// Trait for MPRIS service implementations
pub trait MprisService {
    fn set_service_name(&mut self, service_name: &str);
    fn get_service_name(&self) -> Option<String>;
    fn get_status(&self) -> Option<MediaStatus>;
    fn execute_command(&self, command: &str, status_sender: Arc<Mutex<Option<UnixStream>>>) -> bool;
    fn start_monitoring(&self, status_sender: Arc<Mutex<Option<UnixStream>>>);
    fn stop_monitoring(&self);
}

#[derive(Debug, Clone, PartialEq)]
pub struct MediaStatus {
    pub is_playing: bool,
    pub duration: f64,
    pub position: f64,
}

impl MediaStatus {
    pub fn empty() -> Self {
        Self {
            is_playing: false,
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

    pub fn select_service(&self, service_name: &str, status_sender: Arc<Mutex<Option<UnixStream>>>) -> Result<(), String> {
        println!("[mpris_manager] Selecting service: {}", service_name);
        
        // Stop monitoring for the currently selected service first
        let current_type = self.current_service_type.lock().unwrap();
        match current_type.as_ref() {
            Some(ServiceType::Spotify) => {
                println!("[mpris_manager] Stopping previous Spotify monitoring");
                spotify::set_spotify_active(false);
                spotify::stop_spotify_monitoring();
            }
            Some(ServiceType::Chromium) => {
                println!("[mpris_manager] Stopping previous Chromium monitoring");
                chromium::set_chromium_active(false);
                chromium::stop_chromium_monitoring();
            }
            None => {
                println!("[mpris_manager] No previous service to stop");
            }
        }
        drop(current_type);
        
        if service_name.contains("spotify") {
            // Set Spotify as active
            spotify::set_spotify_active(true);
            chromium::set_chromium_active(false);
            
            let mut spotify = self.spotify_service.lock().unwrap();
            spotify.set_service_name(service_name);
            drop(spotify);
            
            let mut current_type = self.current_service_type.lock().unwrap();
            *current_type = Some(ServiceType::Spotify);
            drop(current_type);
            
            println!("[mpris_manager] Selected Spotify service: {}", service_name);
            
            // Start monitoring for the newly selected service
            spotify::start_spotify_monitoring(status_sender);
            
            Ok(())
        } else if service_name.contains("chromium") {
            // Set Chromium as active
            chromium::set_chromium_active(true);
            spotify::set_spotify_active(false);
            
            let mut chromium = self.chromium_service.lock().unwrap();
            chromium.set_service_name(service_name);
            drop(chromium);
            
            let mut current_type = self.current_service_type.lock().unwrap();
            *current_type = Some(ServiceType::Chromium);
            drop(current_type);
            
            println!("[mpris_manager] Selected Chromium service: {}", service_name);
            
            // Start monitoring for the newly selected service
            chromium::start_chromium_monitoring(status_sender);
            
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

    pub fn execute_command(&self, command: &str, status_sender: Arc<Mutex<Option<UnixStream>>>) -> bool {
        let current_type = self.current_service_type.lock().unwrap();
        
        match current_type.as_ref() {
            Some(ServiceType::Spotify) => {
                println!("[mpris_manager] Executing command on Spotify: {}", command);
                let spotify = self.spotify_service.lock().unwrap();
                spotify.execute_command(command, status_sender)
            }
            Some(ServiceType::Chromium) => {
                println!("[mpris_manager] Executing command on Chromium: {}", command);
                let chromium = self.chromium_service.lock().unwrap();
                chromium.execute_command(command, status_sender)
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

    pub fn stop_monitoring(&self) {
        let current_type = self.current_service_type.lock().unwrap();
        
        match current_type.as_ref() {
            Some(ServiceType::Spotify) => {
                println!("[mpris_manager] Stopping Spotify monitoring");
                let spotify = self.spotify_service.lock().unwrap();
                spotify.stop_monitoring();
            }
            Some(ServiceType::Chromium) => {
                println!("[mpris_manager] Stopping Chromium monitoring");
                let chromium = self.chromium_service.lock().unwrap();
                chromium.stop_monitoring();
            }
            None => {
                println!("[mpris_manager] No service selected, not stopping monitoring");
            }
        }
    }

    pub fn send_status_update(&self, status_sender: &Arc<Mutex<Option<UnixStream>>>) {
        if let Some(status) = self.get_current_status() {
            println!("[mpris_manager] Retrieved status: is_playing={}, duration={}, position={}", 
                status.is_playing, status.duration, status.position);
            
            if let Ok(mut sender_guard) = status_sender.lock() {
                if let Some(ref mut stream) = *sender_guard {
                    let status_json = json!({
                        "is_playing": status.is_playing,
                        "duration": status.duration,
                        "position": status.position,
                    });
                    
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

    fn execute_command(&self, command: &str, status_sender: Arc<Mutex<Option<UnixStream>>>) -> bool {
        println!("[spotify_service] Executing command: {}", command);
        
        let command = command.to_string();
        // Use spawn_blocking to avoid runtime conflicts
        std::thread::spawn(move || {
            let rt = tokio::runtime::Runtime::new().unwrap();
            rt.block_on(async {
                spotify::handle_spotify_command(&command, &status_sender).await;
            });
        });
        
        true
    }

    fn start_monitoring(&self, status_sender: Arc<Mutex<Option<UnixStream>>>) {
        println!("[spotify_service] Starting monitoring");
        spotify::start_spotify_monitoring(status_sender);
    }
    
    fn stop_monitoring(&self) {
        println!("[spotify_service] Stopping monitoring");
        spotify::stop_spotify_monitoring();
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

    fn execute_command(&self, command: &str, status_sender: Arc<Mutex<Option<UnixStream>>>) -> bool {
        println!("[chromium_service] Executing command: {}", command);
        
        let command = command.to_string();
        // Use spawn_blocking to avoid runtime conflicts
        std::thread::spawn(move || {
            let rt = tokio::runtime::Runtime::new().unwrap();
            rt.block_on(async {
                chromium::handle_chromium_command(&command, &status_sender).await;
            });
        });
        
        true
    }

    fn start_monitoring(&self, status_sender: Arc<Mutex<Option<UnixStream>>>) {
        println!("[chromium_service] Starting monitoring");
        chromium::start_chromium_monitoring(status_sender);
    }
    
    fn stop_monitoring(&self) {
        println!("[chromium_service] Stopping monitoring");
        chromium::stop_chromium_monitoring();
    }
}
