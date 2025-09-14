// Chromium helper module for tiny-dfr, providing Chromium media control via DBus.
// 
// This helper module:
// 1. Connects to the main process via Unix socket
// 2. Monitors Chromium via DBus signals and sends status updates to main process
// 3. Receives commands from main process and executes them on Chromium
// 
// Supported commands:
// - play_pause: Toggle play/pause
// - play: Start playback
// - pause: Pause playback
// - next: Next track
// - previous: Previous track
// - stop: Stop playback
// - raise: Raise Chromium window
// - quit: Quit Chromium
// - seek:position: Seek to position (0.0 to 1.0)
// - set_position:position: Set absolute position (0.0 to 1.0)

use std::os::unix::net::UnixStream;
use std::io::Write;
use std::thread;

use std::sync::{Arc, Mutex};
use serde_json::json;
use chrono;

// MediaStatus struct for Chromium
#[derive(Clone, Debug)]
pub struct MediaStatus {
    pub is_playing: bool,
    pub title: String,
    pub artist: String,
    pub album: String,
    pub duration: f64,
    pub position: f64,
}

// DBus imports for native MPRIS communication
use zbus::{Connection, MessageType, MessageStream, MatchRule, Proxy};
use zbus::fdo::DBusProxy;
use futures_lite::stream::StreamExt;
use std::collections::HashMap;
use tokio::runtime::Runtime;
use lazy_static::lazy_static;

// Chromium-specific data structures and functions

// Helper functions for native D-Bus implementation

fn extract_interface_from_method(method: &str) -> &str {
    if method.starts_with("org.mpris.MediaPlayer2.Player.") {
        "org.mpris.MediaPlayer2.Player"
    } else if method.starts_with("org.mpris.MediaPlayer2.") {
        "org.mpris.MediaPlayer2"
    } else {
        "org.mpris.MediaPlayer2.Player" // Default to Player interface
    }
}

fn extract_method_name(method: &str) -> String {
    if method.starts_with("org.mpris.MediaPlayer2.Player.") {
        method.strip_prefix("org.mpris.MediaPlayer2.Player.").unwrap_or(method).to_string()
    } else if method.starts_with("org.mpris.MediaPlayer2.") {
        method.strip_prefix("org.mpris.MediaPlayer2.").unwrap_or(method).to_string()
    } else {
        method.to_string()
    }
}

// Global state for Chromium MPRIS service
lazy_static! {
    static ref CURRENT_MPRIS_SERVICE: Arc<Mutex<Option<String>>> = Arc::new(Mutex::new(None));
    static ref LAST_KNOWN_POSITION: Arc<Mutex<f64>> = Arc::new(Mutex::new(0.0));
    static ref LAST_POSITION_UPDATE: Arc<Mutex<std::time::Instant>> = Arc::new(Mutex::new(std::time::Instant::now()));
}

// Function to get or create a DBus connection
async fn get_dbus_connection() -> Result<Connection, Box<dyn std::error::Error>> {
    // Always create a new connection to avoid Send issues
    // DBus connections are lightweight and this ensures thread safety
    Connection::session().await.map_err(|e| e.into())
}

// Function to set the current MPRIS service for Chromium
pub fn set_current_mpris_service(service_name: &str) {
    let mut current = CURRENT_MPRIS_SERVICE.lock().unwrap();
    *current = Some(service_name.to_string());
    println!("[chromium-helper] set_current_mpris_service called with MPRIS name: '{}'", service_name);
    println!("[chromium-helper] Set current MPRIS service to: {}", service_name);
}

// Function to get the current MPRIS service for Chromium
pub fn get_current_mpris_service() -> Option<String> {
    let current = CURRENT_MPRIS_SERVICE.lock().unwrap();
    current.clone()
}

// Function to inspect all available MPRIS properties for debugging
pub async fn inspect_chromium_mpris_properties() -> Result<(), Box<dyn std::error::Error>> {
    let current_service = get_current_mpris_service().ok_or("No MPRIS service selected")?;
    
    let connection = get_dbus_connection().await?;
    let proxy = Proxy::new(
        &connection,
        current_service.as_str(),
        "/org/mpris/MediaPlayer2",
        "org.mpris.MediaPlayer2.Player",
    ).await?;
    
    println!("[chromium-helper] Inspecting MPRIS properties for service: {}", current_service);
    
    // List of properties to check
    let properties = vec![
        "PlaybackStatus", "Rate", "Position", "Metadata", "CanPlay", "CanPause", 
        "CanSeek", "CanGoNext", "CanGoPrevious", "Shuffle", "LoopStatus",
        "Volume", "MinimumRate", "MaximumRate", "CanControl", "CanGoNext", 
        "CanGoPrevious", "CanSeek", "CanPlay", "CanPause"
    ];
    
    for prop in properties {
        match proxy.get_property::<String>(prop).await {
            Ok(value) => println!("[chromium-helper] {} = '{}'", prop, value),
            Err(e) => println!("[chromium-helper] {} = ERROR: {}", prop, e),
        }
    }
    
    // Try to get properties as different types
    println!("\n[chromium-helper] Trying different property types:");
    
    // Try Rate as f64
    match proxy.get_property::<f64>("Rate").await {
        Ok(value) => println!("[chromium-helper] Rate (f64) = {}", value),
        Err(e) => println!("[chromium-helper] Rate (f64) = ERROR: {}", e),
    }
    
    // Try Position as i64
    match proxy.get_property::<i64>("Position").await {
        Ok(value) => println!("[chromium-helper] Position (i64) = {}", value),
        Err(e) => println!("[chromium-helper] Position (i64) = ERROR: {}", e),
    }
    
    // Try Metadata as HashMap
    match proxy.get_property::<HashMap<String, zbus::zvariant::Value>>("Metadata").await {
        Ok(value) => {
            println!("[chromium-helper] Metadata contains {} entries:", value.len());
            for (key, val) in value.iter() {
                println!("[chromium-helper]   {} = {:?}", key, val);
            }
        },
        Err(e) => println!("[chromium-helper] Metadata = ERROR: {}", e),
    }
    
    Ok(())
}

// Function to get current Chromium status with optional delay for property updates
pub async fn get_chromium_status() -> Option<MediaStatus> {
    get_chromium_status_with_delay(0).await
}

// Function to get current Chromium status with a delay to allow MPRIS properties to update
pub async fn get_chromium_status_with_delay(delay_ms: u64) -> Option<MediaStatus> {
    if delay_ms > 0 {
        tokio::time::sleep(tokio::time::Duration::from_millis(delay_ms)).await;
    }
    let current_service = get_current_mpris_service()?;
    
    let connection = get_dbus_connection().await.ok()?;
    let proxy = Proxy::new(
        &connection,
        current_service.as_str(),
        "/org/mpris/MediaPlayer2",
        "org.mpris.MediaPlayer2.Player",
    ).await.ok()?;
    
    // Get playback status
    let playback_status: String = proxy.get_property("PlaybackStatus").await.ok()?;
    println!("[chromium-helper] DEBUG: PlaybackStatus = '{}'", playback_status);
    
    // Try to get Rate property to see if it's playing
    let rate: f64 = proxy.get_property("Rate").await.unwrap_or(1.0);
    println!("[chromium-helper] DEBUG: Rate = {}", rate);
    
    // Get additional properties that might help detect pause state
    let can_play: bool = proxy.get_property("CanPlay").await.unwrap_or(false);
    let can_pause: bool = proxy.get_property("CanPause").await.unwrap_or(false);
    let _can_seek: bool = proxy.get_property("CanSeek").await.unwrap_or(false);
    let _can_go_next: bool = proxy.get_property("CanGoNext").await.unwrap_or(false);
    let _can_go_previous: bool = proxy.get_property("CanGoPrevious").await.unwrap_or(false);
    
    println!("[chromium-helper] DEBUG: CanPlay = {}, CanPause = {}", can_play, can_pause);
    
    // Chromium's MPRIS implementation is inconsistent - we need to trust PlaybackStatus
    // and ignore Rate as it's often wrong (always 1 even when paused)
    let is_playing = match playback_status.as_str() {
        "Playing" => {
            println!("[chromium-helper] DEBUG: PlaybackStatus='Playing' -> is_playing=true");
            true
        },
        "Paused" => {
            println!("[chromium-helper] DEBUG: PlaybackStatus='Paused' -> is_playing=false");
            false
        },
        "Stopped" => {
            println!("[chromium-helper] DEBUG: PlaybackStatus='Stopped' -> is_playing=false");
            false
        },
        _ => {
            // Unknown status, use Rate as last resort
            println!("[chromium-helper] WARNING: Unknown PlaybackStatus '{}', using Rate={}", playback_status, rate);
            rate > 0.0
        }
    };
    
    // Get position - try different approaches for Chromium
    let position: i64 = proxy.get_property("Position").await.unwrap_or(0);
    println!("[chromium-helper] DEBUG: Raw position from MPRIS: {}μs", position);
    
    // Try to get Shuffle property
    let _shuffle: bool = proxy.get_property("Shuffle").await.unwrap_or(false);
    
    // Try to get LoopStatus property
    let _loop_status: String = proxy.get_property("LoopStatus").await.unwrap_or_else(|_| "None".to_string());
    
    
    // Get metadata
    let metadata: HashMap<String, zbus::zvariant::Value> = proxy
        .get_property("Metadata")
        .await
        .unwrap_or_else(|_| HashMap::new());
    
    // Extract metadata fields
    let title = metadata.get("xesam:title")
        .and_then(|v| match v {
            zbus::zvariant::Value::Str(s) => Some(s.as_str().to_string()),
            _ => None,
        })
        .unwrap_or_default();
    
    let artist = metadata.get("xesam:artist")
        .and_then(|v| match v {
            zbus::zvariant::Value::Array(arr) => {
                let artists: Vec<String> = arr.iter()
                    .filter_map(|item| match item {
                        zbus::zvariant::Value::Str(s) => Some(s.as_str().to_string()),
                        _ => None,
                    })
                    .collect();
                Some(artists.join(", "))
            },
            _ => None,
        })
        .unwrap_or_default();
    
    let album = metadata.get("xesam:album")
        .and_then(|v| match v {
            zbus::zvariant::Value::Str(s) => Some(s.as_str().to_string()),
            _ => None,
        })
        .unwrap_or_default();
    
    let duration = metadata.get("mpris:length")
        .and_then(|v| match v {
            zbus::zvariant::Value::I64(d) => Some(*d),
            _ => None,
        })
        .unwrap_or(0) as f64 / 1_000_000.0; // Convert to seconds
    
    let position_seconds = position as f64 / 1_000_000.0; // Convert to seconds
    
    // Try to get position from metadata as well
    let _metadata_position = metadata.get("mpris:position")
        .and_then(|v| match v {
            zbus::zvariant::Value::I64(p) => Some(*p as f64 / 1_000_000.0),
            _ => None,
        })
        .unwrap_or(0.0);
    
    // Chromium's MPRIS Position property is unreliable - it often shows the full duration
    // even when not at the end. We need to implement our own position tracking.
    let position_ratio = if duration > 0.0 {
        let current_time = std::time::Instant::now();
        let mut last_position = LAST_KNOWN_POSITION.lock().unwrap();
        let mut last_update = LAST_POSITION_UPDATE.lock().unwrap();
        
        // If the MPRIS position looks reasonable (not equal to duration), use it
        if position_seconds < duration - 1.0 {
            // MPRIS position looks valid, use it
            let ratio = position_seconds / duration;
            let clamped_ratio = if ratio > 1.0 { 1.0 } else if ratio < 0.0 { 0.0 } else { ratio };
            *last_position = clamped_ratio;
            *last_update = current_time;
            clamped_ratio
        } else {
            // MPRIS position is unreliable, use our tracking
            if is_playing {
                // If playing, advance position based on time elapsed
                let elapsed = current_time.duration_since(*last_update).as_secs_f64();
                let new_position = (*last_position + elapsed / duration).min(1.0);
                *last_position = new_position;
                *last_update = current_time;
                new_position
            } else {
                // If paused, keep the last known position
                *last_position
            }
        }
    } else {
        0.0
    };
    
    println!("[chromium-helper] DEBUG: Position calculation - position_seconds={}, duration={}, ratio={}, is_playing={}", 
        position_seconds, duration, position_ratio, is_playing);
    
    Some(MediaStatus {
        is_playing,
        title,
        artist,
        album,
        duration,
        position: position_ratio,
    })
}

// Function to send status update
fn send_status_update(status_sender: &Arc<Mutex<Option<UnixStream>>>, status: &MediaStatus) {
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
        }
    }
}

// Function to handle Chromium commands
pub async fn handle_chromium_command(action: &str, status_sender: &Arc<Mutex<Option<UnixStream>>>) {
    println!("[chromium-helper] Handling Chromium command: {}", action);
    
    let current_service = get_current_mpris_service();
    if current_service.is_none() {
        println!("[chromium-helper] No MPRIS service selected for Chromium");
        return;
    }
    
    let service_name = current_service.unwrap();
    println!("[chromium-helper] Using MPRIS service: {}", service_name);
    
    // Parse the action
    let parts: Vec<&str> = action.split(':').collect();
    let command = parts[0];
    let args = if parts.len() > 1 { Some(parts[1]) } else { None };
    
    match command {
        "play_pause" => {
            if let Err(e) = toggle_play_pause(&service_name).await {
                eprintln!("[chromium-helper] Failed to toggle play/pause: {}", e);
            } else {
                // After play/pause, wait a bit for MPRIS properties to update
                // and then send a status update
                tokio::time::sleep(tokio::time::Duration::from_millis(500)).await;
                if let Some(status) = get_chromium_status_with_delay(0).await {
                    send_status_update(status_sender, &status);
                }
            }
        }
        "play" => {
            if let Err(e) = play(&service_name).await {
                eprintln!("[chromium-helper] Failed to play: {}", e);
            } else {
                // After play, wait a bit for MPRIS properties to update
                tokio::time::sleep(tokio::time::Duration::from_millis(500)).await;
                if let Some(status) = get_chromium_status_with_delay(0).await {
                    send_status_update(status_sender, &status);
                }
            }
        }
        "pause" => {
            if let Err(e) = pause(&service_name).await {
                eprintln!("[chromium-helper] Failed to pause: {}", e);
            } else {
                // After pause, wait a bit for MPRIS properties to update
                tokio::time::sleep(tokio::time::Duration::from_millis(500)).await;
                if let Some(status) = get_chromium_status_with_delay(0).await {
                    send_status_update(status_sender, &status);
                }
            }
        }
        "next" => {
            if let Err(e) = next_track(&service_name).await {
                eprintln!("[chromium-helper] Failed to go to next track: {}", e);
            }
        }
        "previous" => {
            if let Err(e) = previous_track(&service_name).await {
                eprintln!("[chromium-helper] Failed to go to previous track: {}", e);
            }
        }
        "stop" => {
            if let Err(e) = stop(&service_name).await {
                eprintln!("[chromium-helper] Failed to stop: {}", e);
            }
        }
        "raise" => {
            if let Err(e) = raise_chromium(&service_name).await {
                eprintln!("[chromium-helper] Failed to raise Chromium: {}", e);
            }
        }
        "quit" => {
            if let Err(e) = quit_chromium(&service_name).await {
                eprintln!("[chromium-helper] Failed to quit Chromium: {}", e);
            }
        }
        "seek" => {
            if let Some(position_str) = args {
                if let Ok(position) = position_str.parse::<f64>() {
                    if let Err(e) = seek(&service_name, position).await {
                        eprintln!("[chromium-helper] Failed to seek: {}", e);
                    }
                } else {
                    eprintln!("[chromium-helper] Invalid seek position: {}", position_str);
                }
            } else {
                eprintln!("[chromium-helper] Missing seek position argument");
            }
        }
        "set_position" => {
            if let Some(position_str) = args {
                if let Ok(position) = position_str.parse::<f64>() {
                    if let Err(e) = set_position(&service_name, position).await {
                        eprintln!("[chromium-helper] Failed to set position: {}", e);
                    }
                } else {
                    eprintln!("[chromium-helper] Invalid position: {}", position_str);
                }
            } else {
                eprintln!("[chromium-helper] Missing position argument");
            }
        }
        "inspect" => {
            if let Err(e) = inspect_chromium_mpris_properties().await {
                eprintln!("[chromium-helper] Failed to inspect MPRIS properties: {}", e);
            }
        }
        _ => {
            eprintln!("[chromium-helper] Unknown command: {}", action);
        }
    }
}

// MPRIS command implementations for Chromium

async fn toggle_play_pause(service_name: &str) -> Result<(), Box<dyn std::error::Error>> {
    let connection = get_dbus_connection().await?;
    let proxy = Proxy::new(
        &connection,
        service_name,
        "/org/mpris/MediaPlayer2",
        "org.mpris.MediaPlayer2.Player",
    ).await?;
    
    proxy.call_method("PlayPause", &()).await?;
    println!("[chromium-helper] Toggled play/pause for {}", service_name);
    Ok(())
}

async fn play(service_name: &str) -> Result<(), Box<dyn std::error::Error>> {
    let connection = get_dbus_connection().await?;
    let proxy = Proxy::new(
        &connection,
        service_name,
        "/org/mpris/MediaPlayer2",
        "org.mpris.MediaPlayer2.Player",
    ).await?;
    
    proxy.call_method("Play", &()).await?;
    println!("[chromium-helper] Started playback for {}", service_name);
    Ok(())
}

async fn pause(service_name: &str) -> Result<(), Box<dyn std::error::Error>> {
    let connection = get_dbus_connection().await?;
    let proxy = Proxy::new(
        &connection,
        service_name,
        "/org/mpris/MediaPlayer2",
        "org.mpris.MediaPlayer2.Player",
    ).await?;
    
    proxy.call_method("Pause", &()).await?;
    println!("[chromium-helper] Paused playback for {}", service_name);
    Ok(())
}

async fn next_track(service_name: &str) -> Result<(), Box<dyn std::error::Error>> {
    let connection = get_dbus_connection().await?;
    let proxy = Proxy::new(
        &connection,
        service_name,
        "/org/mpris/MediaPlayer2",
        "org.mpris.MediaPlayer2.Player",
    ).await?;
    
    proxy.call_method("Next", &()).await?;
    println!("[chromium-helper] Went to next track for {}", service_name);
    Ok(())
}

async fn previous_track(service_name: &str) -> Result<(), Box<dyn std::error::Error>> {
    let connection = get_dbus_connection().await?;
    let proxy = Proxy::new(
        &connection,
        service_name,
        "/org/mpris/MediaPlayer2",
        "org.mpris.MediaPlayer2.Player",
    ).await?;
    
    proxy.call_method("Previous", &()).await?;
    println!("[chromium-helper] Went to previous track for {}", service_name);
    Ok(())
}

async fn stop(service_name: &str) -> Result<(), Box<dyn std::error::Error>> {
    let connection = get_dbus_connection().await?;
    let proxy = Proxy::new(
        &connection,
        service_name,
        "/org/mpris/MediaPlayer2",
        "org.mpris.MediaPlayer2.Player",
    ).await?;
    
    proxy.call_method("Stop", &()).await?;
    println!("[chromium-helper] Stopped playback for {}", service_name);
    Ok(())
}

async fn raise_chromium(service_name: &str) -> Result<(), Box<dyn std::error::Error>> {
    let connection = get_dbus_connection().await?;
    let proxy = Proxy::new(
        &connection,
        service_name,
        "/org/mpris/MediaPlayer2",
        "org.mpris.MediaPlayer2",
    ).await?;
    
    proxy.call_method("Raise", &()).await?;
    println!("[chromium-helper] Raised Chromium window for {}", service_name);
    Ok(())
}

async fn quit_chromium(service_name: &str) -> Result<(), Box<dyn std::error::Error>> {
    let connection = get_dbus_connection().await?;
    let proxy = Proxy::new(
        &connection,
        service_name,
        "/org/mpris/MediaPlayer2",
        "org.mpris.MediaPlayer2",
    ).await?;
    
    proxy.call_method("Quit", &()).await?;
    println!("[chromium-helper] Quit Chromium for {}", service_name);
    Ok(())
}

async fn seek(service_name: &str, position: f64) -> Result<(), Box<dyn std::error::Error>> {
    // Validate position range
    if position < 0.0 || position > 1.0 {
        return Err(format!("Invalid seek position: {}. Must be between 0.0 and 1.0", position).into());
    }
    
    let connection = get_dbus_connection().await?;
    let proxy = Proxy::new(
        &connection,
        service_name,
        "/org/mpris/MediaPlayer2",
        "org.mpris.MediaPlayer2.Player",
    ).await?;
    
    // Get current duration from metadata first
    let metadata: HashMap<String, zbus::zvariant::Value> = proxy
        .get_property("Metadata")
        .await
        .unwrap_or_else(|_| HashMap::new());
    
    let duration = metadata.get("mpris:length")
        .and_then(|v| match v {
            zbus::zvariant::Value::I64(d) => Some(*d),
            _ => None,
        })
        .unwrap_or(0);
    
    if duration == 0 {
        return Err("Cannot seek: no duration available".into());
    }
    
    // Convert position ratio to microseconds based on duration
    let position_us = (position * duration as f64) as i64;
    proxy.call_method("Seek", &(position_us,)).await?;
    
    // Update our position tracking
    {
        let mut last_position = LAST_KNOWN_POSITION.lock().unwrap();
        let mut last_update = LAST_POSITION_UPDATE.lock().unwrap();
        *last_position = position;
        *last_update = std::time::Instant::now();
    }
    
    println!("[chromium-helper] Seeked to position {} ({}μs) for {}", position, position_us, service_name);
    Ok(())
}

async fn set_position(service_name: &str, position: f64) -> Result<(), Box<dyn std::error::Error>> {
    // Validate position range
    if position < 0.0 || position > 1.0 {
        return Err(format!("Invalid position: {}. Must be between 0.0 and 1.0", position).into());
    }
    
    let connection = get_dbus_connection().await?;
    let proxy = Proxy::new(
        &connection,
        service_name,
        "/org/mpris/MediaPlayer2",
        "org.mpris.MediaPlayer2.Player",
    ).await?;
    
    // Get current duration from metadata first
    let metadata: HashMap<String, zbus::zvariant::Value> = proxy
        .get_property("Metadata")
        .await
        .unwrap_or_else(|_| HashMap::new());
    
    let duration = metadata.get("mpris:length")
        .and_then(|v| match v {
            zbus::zvariant::Value::I64(d) => Some(*d),
            _ => None,
        })
        .unwrap_or(0);
    
    if duration == 0 {
        return Err("Cannot set position: no duration available".into());
    }
    
    // Convert position ratio to microseconds based on duration
    let position_us = (position * duration as f64) as i64;
    proxy.call_method("SetPosition", &(position_us,)).await?;
    
    // Update our position tracking
    {
        let mut last_position = LAST_KNOWN_POSITION.lock().unwrap();
        let mut last_update = LAST_POSITION_UPDATE.lock().unwrap();
        *last_position = position;
        *last_update = std::time::Instant::now();
    }
    
    println!("[chromium-helper] Set position to {} ({}μs) for {}", position, position_us, service_name);
    Ok(())
}

// Function to monitor Chromium events and send status updates
pub fn monitor_chromium_events(status_sender: Arc<Mutex<Option<UnixStream>>>) {
    thread::spawn(move || {
        let rt = Runtime::new().unwrap();
        rt.block_on(async {
            let mut last_service = None;
            let mut monitoring_task: Option<tokio::task::JoinHandle<()>> = None;
            
            loop {
                // Check if the service has changed
                let current_service = get_current_mpris_service();
                if current_service != last_service {
                    println!("[chromium-helper] Service changed from {:?} to {:?}, restarting event monitoring", 
                        last_service, current_service);
                    
                    // Cancel the old monitoring task if it exists
                    if let Some(task) = monitoring_task.take() {
                        task.abort();
                    }
                    
                    // Start new monitoring task for the new service
                    if current_service.is_some() {
                        println!("[chromium-helper] Starting event monitoring task for service: {:?}", current_service);
                        let status_sender_clone = status_sender.clone();
                        monitoring_task = Some(tokio::spawn(async move {
                            println!("[chromium-helper] Event monitoring task started");
                            if let Err(e) = monitor_chromium_events_async(status_sender_clone).await {
                                eprintln!("[chromium-helper] Error in Chromium event monitoring: {}", e);
                            }
                        }));
                    }
                    
                    last_service = current_service.clone();
                }
                
                // Wait a bit before checking again
                tokio::time::sleep(tokio::time::Duration::from_secs(1)).await;
            }
        });
    });
}

async fn monitor_chromium_events_async(status_sender: Arc<Mutex<Option<UnixStream>>>) -> Result<(), Box<dyn std::error::Error>> {
    let connection = get_dbus_connection().await?;
    let mut stream = MessageStream::from(&connection);
    let dbus_proxy = DBusProxy::new(&connection).await?;
    
    // Start a periodic status check as a fallback (every 5 seconds)
    let status_sender_clone = status_sender.clone();
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(tokio::time::Duration::from_secs(5));
        loop {
            interval.tick().await;
            if let Some(status) = get_chromium_status().await {
                send_status_update(&status_sender_clone, &status);
            }
        }
    });
    
    // Subscribe to MPRIS signals specifically for Chromium
    let current_service = get_current_mpris_service();
    if current_service.is_none() {
        println!("[chromium-helper] No current MPRIS service set, cannot monitor events");
        return Ok(());
    }
    
    let service_name = current_service.unwrap();
    println!("[chromium-helper] Monitoring events for service: {}", service_name);
    
    let rules = vec![
        // PropertiesChanged signals for playback status, position, metadata
        MatchRule::builder()
            .msg_type(MessageType::Signal)
            .interface("org.freedesktop.DBus.Properties")?
            .path_namespace("/org/mpris/MediaPlayer2")?
            .member("PropertiesChanged")?
            .sender(service_name.as_str())?
            .build(),
        // Also listen to all MPRIS signals to see what's available
        MatchRule::builder()
            .msg_type(MessageType::Signal)
            .path_namespace("/org/mpris/MediaPlayer2")?
            .build(),
        // Listen to ALL signals from Chromium to see what it sends
        MatchRule::builder()
            .msg_type(MessageType::Signal)
            .sender(service_name.as_str())?
            .build(),
    ];
    
    // Add all match rules
    for rule in rules {
        if let Err(e) = dbus_proxy.add_match_rule(rule).await {
            eprintln!("[chromium-helper] Failed to add match rule: {}", e);
        }
    }
    
    println!("[chromium-helper] Started monitoring Chromium MPRIS events");
    
    while let Some(msg) = stream.next().await {
        if let Ok(msg) = msg {
            // Debug: log all messages to see what we're getting
            if let Some(interface) = msg.interface() {
                if let Some(member) = msg.member() {
                    let interface_str = interface.as_str();
                    let member_str = member.as_str();
                    
                    // Check if this message is from our target service
                    if let Ok(header) = msg.header() {
                        if let Ok(Some(sender)) = header.sender() {
                            let sender_str = sender.as_str();
                            println!("[chromium-helper] DEBUG: Received signal {}.{} from sender: {}", 
                                interface_str, member_str, sender_str);
                            
                            if sender_str != service_name {
                                println!("[chromium-helper] DEBUG: Skipping signal from different service: {} != {}", 
                                    sender_str, service_name);
                                continue; // Skip messages from other services
                            }
                        }
                    }
                    
                    match (interface_str, member_str) {
                        ("org.freedesktop.DBus.Properties", "PropertiesChanged") => {
                            println!("[chromium-helper] Received PropertiesChanged signal from {}", service_name);
                            // Handle PropertiesChanged signal for Chromium
                            if let Ok((interface_name, changed_props, _invalidated_props)) = 
                                msg.body::<(String, std::collections::HashMap<String, zbus::zvariant::Value>, Vec<String>)>() {
                                
                                println!("[chromium-helper] PropertiesChanged for interface: {}, changed properties: {:?}", 
                                    interface_name, changed_props.keys().collect::<Vec<_>>());
                                
                                if interface_name == "org.mpris.MediaPlayer2.Player" {
                                    if let Err(e) = handle_properties_changed(&changed_props, &status_sender).await {
                                        eprintln!("[chromium-helper] Error handling PropertiesChanged: {}", e);
                                    }
                                }
                            }
                        }
                        _ => {
                            // Handle other signals if needed
                            println!("[chromium-helper] Received signal: {}.{} from {}", interface_str, member_str, service_name);
                            
                            // For any signal from Chromium, trigger a status check
                            println!("[chromium-helper] Triggering status check due to signal from Chromium");
                            if let Some(status) = get_chromium_status().await {
                                send_status_update(&status_sender, &status);
                            }
                        }
                    }
                }
            }
        }
    }
    
    Ok(())
}

async fn handle_properties_changed(
    _changed_properties: &HashMap<String, zbus::zvariant::Value<'_>>,
    status_sender: &Arc<Mutex<Option<UnixStream>>>,
) -> Result<(), Box<dyn std::error::Error>> {
    let current_service = get_current_mpris_service();
    if current_service.is_none() {
        return Ok(());
    }
    
    // Instead of processing individual properties, get the full current status
    // This ensures we always have duration when calculating position ratio
    if let Some(full_status) = get_chromium_status().await {
        let status_json = serde_json::to_string(&json!({
            "is_playing": full_status.is_playing,
            "title": full_status.title,
            "artist": full_status.artist,
            "album": full_status.album,
            "duration": full_status.duration,
            "position": full_status.position,
            "timestamp": chrono::Utc::now().timestamp_millis(),
        })).unwrap_or_default();
        
        // Send status update
        if let Ok(mut sender_guard) = status_sender.lock() {
            if let Some(ref mut stream) = *sender_guard {
                let message = format!("status_update:{}\n", status_json);
                if let Err(e) = stream.write_all(message.as_bytes()) {
                    eprintln!("[chromium-helper] Failed to send status update: {}", e);
                } else {
                    println!("[chromium-helper] Sent status update: {}", status_json);
                }
            }
        }
    }
    
    Ok(())
}
