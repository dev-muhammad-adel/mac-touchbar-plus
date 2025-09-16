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
use std::sync::atomic::{AtomicBool, Ordering};
use serde_json::json;
// Global state to track if Chromium is active
static CHROMIUM_ACTIVE: AtomicBool = AtomicBool::new(false);

// Global state to track monitoring thread
static mut CHROMIUM_MONITORING_THREAD: Option<std::thread::JoinHandle<()>> = None;

// Helper function to check if Chromium is active
pub fn is_chromium_active() -> bool {
    CHROMIUM_ACTIVE.load(Ordering::SeqCst)
}

// Helper function to set Chromium as active
pub fn set_chromium_active(active: bool) {
    CHROMIUM_ACTIVE.store(active, Ordering::SeqCst);
}

// Helper function to start Chromium monitoring
pub fn start_chromium_monitoring(status_sender: Arc<Mutex<Option<UnixStream>>>) {
    // Stop any existing monitoring thread
    stop_chromium_monitoring();
    
    // Set Chromium as active before starting monitoring
    set_chromium_active(true);
    
    // Start new monitoring thread
    let handle = std::thread::spawn(move || {
        monitor_chromium_events(status_sender);
    });
    
    unsafe {
        CHROMIUM_MONITORING_THREAD = Some(handle);
    }
}

// Helper function to stop Chromium monitoring
pub fn stop_chromium_monitoring() {
    unsafe {
        if let Some(handle) = CHROMIUM_MONITORING_THREAD.take() {
            println!("[chromium-helper] Stopping Chromium monitoring thread");
            // Set the flag to false so the thread knows to exit
            CHROMIUM_ACTIVE.store(false, Ordering::SeqCst);
            // Give the thread a moment to exit naturally
            std::thread::sleep(std::time::Duration::from_millis(100));
        }
    }
}

// MediaStatus struct for Chromium
#[derive(Clone, Debug)]
pub struct MediaStatus {
    pub is_playing: bool,
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
    // Shared playback state for position polling (is_playing, MediaStatus)
    static ref PLAYBACK_STATE: Arc<Mutex<(bool, MediaStatus)>> = Arc::new(Mutex::new((false, MediaStatus {
        is_playing: false,
        duration: 0.0,
        position: 0.0,
    })));
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
    
    // Chromium's MPRIS Position property has quirks:
    // 1. SetPosition/Seek don't update position immediately when paused
    // 2. Position only updates during actual playback
    // 3. We need to track position ourselves and sync with MPRIS during playback
    let position_ratio = if duration > 0.0 {
        let current_time = std::time::Instant::now();
        let mut last_position = LAST_KNOWN_POSITION.lock().unwrap();
        let mut last_update = LAST_POSITION_UPDATE.lock().unwrap();
        
        // Always check MPRIS position first, regardless of play state
        let mpris_ratio = position_seconds / duration;
        let mpris_valid = mpris_ratio >= 0.0 && mpris_ratio <= 1.0;
        
        println!("[chromium-helper] DEBUG: Position calculation - position_seconds={}, duration={}, ratio={}, is_playing={}", 
            position_seconds, duration, mpris_ratio, is_playing);
        println!("[chromium-helper] DEBUG: MPRIS valid: {}, last_position: {}", mpris_valid, *last_position);
        
        if mpris_valid {
            // MPRIS position looks valid, use it and update our tracking
            *last_position = mpris_ratio;
            *last_update = current_time;
            println!("[chromium-helper] DEBUG: Using MPRIS position: {}", mpris_ratio);
            mpris_ratio
        } else if is_playing {
            // During playback, MPRIS position is invalid, use our tracking
            let elapsed = current_time.duration_since(*last_update).as_secs_f64();
            let new_position = (*last_position + elapsed / duration).min(1.0);
            *last_position = new_position;
            *last_update = current_time;
            println!("[chromium-helper] DEBUG: Using tracked position during playback: {}", new_position);
            new_position
        } else {
            // When paused and MPRIS is invalid, use our tracked position
            println!("[chromium-helper] DEBUG: Using tracked position when paused: {}", *last_position);
            *last_position
        }
    } else {
        0.0
    };
    
    Some(MediaStatus {
        is_playing,
        duration,
        position: position_ratio,
    })
}

// Function to send status update
pub fn send_status_update(status_sender: &Arc<Mutex<Option<UnixStream>>>, status: &MediaStatus) {
    if let Ok(mut sender_guard) = status_sender.lock() {
        if let Some(ref mut stream) = *sender_guard {
            let status_json = json!({
                "is_playing": status.is_playing,
                "position": status.position,
                "duration": status.duration
            });
            
            let message = format!("status_update:{}\n", status_json);
            let _ = stream.write_all(message.as_bytes());
        }
    }
}

// Helper function to update and send status (similar to Spotify)
async fn update_and_send_status(
    status_sender: &Arc<Mutex<Option<UnixStream>>>,
    playback_state: &Arc<Mutex<(bool, MediaStatus)>>,
    is_playing: bool,
    status: MediaStatus,
) {
    // Update shared playback state
    if let Ok(mut state) = playback_state.lock() {
        state.0 = is_playing;
        state.1 = status.clone();
    }
    
    // Send status update
    send_status_update(status_sender, &status);
    println!("[chromium-helper] Position polling update: {:.2}%", status.position * 100.0);
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
            }
            // Status update will be sent automatically by D-Bus monitoring
        }
        "play" => {
            if let Err(e) = play(&service_name).await {
                eprintln!("[chromium-helper] Failed to play: {}", e);
            }
            // Status update will be sent automatically by D-Bus monitoring
        }
        "pause" => {
            if let Err(e) = pause(&service_name).await {
                eprintln!("[chromium-helper] Failed to pause: {}", e);
            }
            // Status update will be sent automatically by D-Bus monitoring
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
                    } else {
                        // Send status update immediately after seek completes
                        // Use the position we just set instead of waiting for MPRIS to update
                        if let Some(mut status) = get_chromium_status().await {
                            // Update the position to what we just set
                            status.position = position;
                            send_status_update(status_sender, &status);
                        }
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
                    } else {
                        // Send status update immediately after set_position completes
                        // Use the position we just set instead of waiting for MPRIS to update
                        if let Some(mut status) = get_chromium_status().await {
                            // Update the position to what we just set
                            status.position = position;
                            send_status_update(status_sender, &status);
                        }
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
    
    // Get track ID from metadata (required for SetPosition)
    let track_id = metadata.get("mpris:trackid")
        .and_then(|v| match v {
            zbus::zvariant::Value::ObjectPath(path) => Some(path.clone()),
            _ => None,
        })
        .ok_or("No track ID available for seeking")?;
    
    // Convert position ratio to microseconds based on duration
    let position_us = (position * duration as f64) as i64;
    proxy.call_method("SetPosition", &(track_id, position_us)).await?;
    
    // Update our position tracking immediately
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
    println!("[chromium-helper] Starting Chromium monitoring thread");
    
    // Check if Chromium is still the active service
    if !is_chromium_active() {
        println!("[chromium-helper] Chromium is no longer active, stopping monitoring");
        return;
    }
    
    thread::spawn(move || {
        let rt = Runtime::new().unwrap();
        rt.block_on(async {
            if let Err(e) = monitor_chromium_events_async(status_sender).await {
                    eprintln!("[chromium-helper] Error in Chromium event monitoring: {}", e);
            }
        });
    });
}

async fn monitor_chromium_events_async(status_sender: Arc<Mutex<Option<UnixStream>>>) -> Result<(), Box<dyn std::error::Error>> {
    // Check if Chromium is still the active service
    if !is_chromium_active() {
        println!("[chromium-helper] Chromium is no longer active, stopping monitoring");
        return Ok(());
    }
    
    let connection = get_dbus_connection().await?;
    let mut stream = MessageStream::from(&connection);
    let dbus_proxy = DBusProxy::new(&connection).await?;
    
    // Get initial status immediately to avoid "no media" on startup
    if let Some(initial_status) = get_chromium_status().await {
        let is_playing = initial_status.is_playing;
        let position = initial_status.position;
        let duration = initial_status.duration;
        
        update_and_send_status(&status_sender, &PLAYBACK_STATE, is_playing, initial_status).await;
        println!("[chromium-helper] Initial status sent: playing={}, position={:.2}%, duration={:.1}s", 
            is_playing, position * 100.0, duration);
    }
    
    // Start smart position polling for Chromium when playing (similar to Spotify)
    let status_sender_clone = status_sender.clone();
    let playback_state = PLAYBACK_STATE.clone();
    tokio::spawn(async move {
        loop {
            // Check if Chromium is still the active service
            if !is_chromium_active() {
                println!("[chromium-helper] Chromium is no longer active, stopping position polling");
                break;
            }
            
            // Check if currently playing from shared state
            let is_playing = {
                if let Ok(state) = playback_state.lock() {
                    state.0
                } else {
                    false
                }
            };
            
            if is_playing {
                // Get Chromium position using polling when playing
                if let Some(status) = get_chromium_status().await {
                    if status.is_playing && status.duration > 0.0 {
                        update_and_send_status(&status_sender_clone, &playback_state, status.is_playing, status).await;
                    }
                }
                
                // Poll every 100ms for smooth progress updates (like Spotify)
                tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;
            } else {
                // Not playing - sleep longer and wait for events
                tokio::time::sleep(tokio::time::Duration::from_millis(500)).await;
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
    
    // Get the D-Bus unique name for the service
    let bus_name = zbus::names::BusName::try_from(service_name.as_str())?;
    let unique_name = match dbus_proxy.get_name_owner(bus_name).await {
        Ok(name) => name,
        Err(e) => {
            eprintln!("[chromium-helper] Error getting name owner for {}: {}", service_name, e);
            return Ok(());
        }
    };
    
    println!("[chromium-helper] Service {} has unique name: {}", service_name, unique_name);
    
    let rules = vec![
        // PropertiesChanged signals for playback status, position, metadata
        MatchRule::builder()
            .msg_type(MessageType::Signal)
            .interface("org.freedesktop.DBus.Properties")?
            .path_namespace("/org/mpris/MediaPlayer2")?
            .member("PropertiesChanged")?
            .sender(unique_name.as_str())?
            .build(),
    ];
    
    // Add all match rules
    for (i, rule) in rules.iter().enumerate() {
        if let Err(e) = dbus_proxy.add_match_rule(rule.clone()).await {
            eprintln!("[chromium-helper] Failed to add match rule {}: {}", i, e);
        } else {
            println!("[chromium-helper] Successfully added match rule {}", i);
        }
    }
    
    println!("[chromium-helper] Started monitoring Chromium MPRIS events");
    println!("[chromium-helper] Monitoring for sender: {}", unique_name);
    
    while let Some(msg) = stream.next().await {
        // Check if Chromium is still the active service
        if !is_chromium_active() {
            println!("[chromium-helper] Chromium is no longer active, stopping monitoring");
            break;
        }
        
        if let Ok(msg) = msg {
            // Debug: log all messages to see what we're getting
            if let Some(interface) = msg.interface() {
                if let Some(member) = msg.member() {
                    let interface_str = interface.as_str();
                    let member_str = member.as_str();
                    
                    println!("[chromium-helper] Received message: interface={}, member={}", 
                        interface_str, member_str);
                    
                    // Check if this message is from our target service
                    if let Ok(header) = msg.header() {
                        if let Ok(Some(sender)) = header.sender() {
                            let sender_str = sender.as_str();
                            println!("[chromium-helper] DEBUG: Received signal {}.{} from sender: {}", 
                                interface_str, member_str, sender_str);
                            
                            if sender_str != unique_name.as_str() {
                                println!("[chromium-helper] DEBUG: Skipping signal from different service: {} != {}", 
                                    sender_str, unique_name);
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
        let is_playing = full_status.is_playing;
        
        // Update shared playback state and send status
        update_and_send_status(status_sender, &PLAYBACK_STATE, is_playing, full_status).await;
        
        if is_playing {
            println!("[chromium-helper] Chromium playback started - position polling activated");
                } else {
            println!("[chromium-helper] Chromium playback stopped - position polling deactivated");
        }
    }
    
    Ok(())
}
