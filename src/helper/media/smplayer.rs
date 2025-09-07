// SMPlayer helper module for tiny-dfr, providing SMPlayer status via DBus.
// 
// This helper module:
// 1. Connects to the main process via Unix socket
// 2. Monitors SMPlayer via DBus signals and sends status updates to main process
// 3. Receives commands from main process and executes them on SMPlayer
// 
// Supported commands:
// - play_pause: Toggle play/pause
// - play: Start playback
// - pause: Pause playback
// - next: Next track
// - previous: Previous track
// - stop: Stop playback
// - raise: Raise SMPlayer window
// - quit: Quit SMPlayer
// - seek:position: Seek to position (0.0 to 1.0)
// - set_position:position: Set absolute position (0.0 to 1.0)

use std::os::unix::net::UnixStream;
use std::io::Write;
use std::thread;
use std::time::Duration;
use std::sync::{Arc, Mutex};
use serde_json::json;

// DBus imports for native MPRIS communication
use zbus::{Connection, MessageType, MessageStream, MatchRule, Proxy};
use zbus::fdo::DBusProxy;
use futures_lite::stream::StreamExt;
use std::collections::HashMap;
use tokio::runtime::Runtime;
use lazy_static::lazy_static;

// SMPlayer-specific data structures and functions

// Constants
const SMPLAYER_BASE_MPRIS_NAME: &str = "org.mpris.MediaPlayer2.smplayer";
const SMPLAYER_WINDOW_CLASSES: &[&str] = &["SMPlayer", "smplayer"];
const POSITION_SAFETY_MARGIN: f64 = 0.001;
const MICROSECONDS_PER_SECOND: i64 = 1_000_000;

// Helper functions for native D-Bus implementation

// Common helper functions to eliminate code duplication
async fn update_and_send_status(
    status_sender: &Arc<Mutex<Option<UnixStream>>>,
    playback_state: &Arc<Mutex<(bool, MediaStatus)>>,
    is_playing: bool,
    status: MediaStatus
) {
    // Update shared state
    if let Ok(mut state) = playback_state.lock() {
        state.0 = is_playing;
        state.1 = status.clone();
    }
    
    // Send status update
    send_status_update(status_sender, &status);
}

fn calculate_seek_offset(current_status: &MediaStatus, target_position: f64) -> i64 {
    let duration_microseconds = current_status.duration * MICROSECONDS_PER_SECOND;
    let target_position_microseconds = (target_position * duration_microseconds as f64) as i64;
    let current_position_microseconds = (current_status.position * duration_microseconds as f64) as i64;
    target_position_microseconds - current_position_microseconds
}


fn log_error(operation: &str, error: &str) {
    eprintln!("[smplayer-helper] {} failed: {}", operation, error);
}

fn log_info(message: &str) {
    eprintln!("[smplayer-helper] {}", message);
}

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
    method.split('.').last().unwrap_or(method).to_string()
}



// Global connection for reuse across async contexts
static mut SHARED_DBUS_CONNECTION: Option<Connection> = None;

// Shared Tokio runtime for all async DBus work
lazy_static! {
    static ref TOKIO_RT: Runtime = Runtime::new().expect("Failed to create shared Tokio runtime");
}

async fn get_shared_connection() -> Result<Connection, zbus::Error> {
    // Try to reuse shared connection if available, otherwise create a new one
    unsafe {
        if let Some(ref conn) = SHARED_DBUS_CONNECTION {
            // Try to clone the existing connection
            Ok(conn.clone())
        } else {
            // Create a new connection
            let conn = Connection::session().await?;
            SHARED_DBUS_CONNECTION = Some(conn.clone());
            Ok(conn)
        }
    }
}

// Native D-Bus implementations
async fn get_status_from_dest_native(mpris_dest: &str) -> Option<MediaStatus> {
    let connection = match get_shared_connection().await {
        Ok(conn) => conn,
        Err(e) => {
            eprintln!("[media-helper] Failed to get D-Bus connection: {}", e);
            return None;
        }
    };
    
    let proxy = match Proxy::new(
        &connection,
        mpris_dest,
        "/org/mpris/MediaPlayer2",
        "org.mpris.MediaPlayer2.Player",
    ).await {
        Ok(p) => p,
        Err(e) => {
            eprintln!("[media-helper] Failed to create proxy for {}: {}", mpris_dest, e);
            return None;
        }
    };
    
    // Get individual properties using get_property
    let playback_status: String = match proxy.get_property("PlaybackStatus").await {
        Ok(status) => status,
        Err(e) => {
            eprintln!("[media-helper] Failed to get PlaybackStatus: {}", e);
            return None;
        }
    };
    
    let position_raw: i64 = proxy.get_property("Position").await.unwrap_or(0);
    
    let metadata: HashMap<String, zbus::zvariant::Value> = proxy
        .get_property("Metadata")
        .await
        .unwrap_or_else(|_| HashMap::new());
    
    let length_raw = metadata.get("mpris:length")
        .and_then(|v| Some(v.clone()))
        .and_then(|v| f64::try_from(v).ok())
        .map(|v| v as i64)
        .unwrap_or(0);
    
    // SMPlayer uses microseconds (like most MPRIS players)
    let duration_seconds = length_raw / 1_000_000;
    let position_seconds = position_raw as f64 / 1_000_000.0;
    let is_playing = playback_status == "Playing";
    let position_ratio = if duration_seconds > 0 { position_seconds / duration_seconds as f64 } else { 0.0 };
    
    Some(MediaStatus {
        is_playing,
        position: position_ratio,
        duration: duration_seconds,
    })
}

async fn try_execute_command_on_destination_native(command: &str, args: &[&str], mpris_dest: &str) -> bool {
    let connection = match get_shared_connection().await {
        Ok(conn) => conn,
        Err(e) => {
            eprintln!("[media-helper] Failed to get D-Bus connection: {}", e);
            return false;
        }
    };
    
    let interface = extract_interface_from_method(command);
    let proxy = match Proxy::new(
        &connection,
        mpris_dest,
        "/org/mpris/MediaPlayer2",
        interface,
    ).await {
        Ok(p) => p,
        Err(e) => {
            eprintln!("[media-helper] Failed to create proxy for {}: {}", mpris_dest, e);
            return false;
        }
    };
    
    let method_name = extract_method_name(command);
    
    // Handle different method signatures
    let result = match method_name.as_str() {
        "PlayPause" | "Play" | "Pause" | "Stop" | "Next" | "Previous" => {
            proxy.call_method(method_name.as_str(), &()).await
        }
        "Seek" => {
            if let Some(arg) = args.get(0) {
                if let Some(offset_str) = arg.strip_prefix("int64:") {
                    if let Ok(offset) = offset_str.parse::<i64>() {
                        proxy.call_method(method_name.as_str(), &(offset,)).await
                    } else {
                        return false;
                    }
                } else {
                    return false;
                }
            } else {
                return false;
            }
        }
        "Raise" | "Quit" => {
            proxy.call_method(method_name.as_str(), &()).await
        }
        _ => {
            eprintln!("[media-helper] Unknown method: {}", method_name);
            return false;
        }
    };
    
    match result {
        Ok(_) => {
            eprintln!("[media-helper] Command '{}' executed successfully on {}", command, mpris_dest);
            true
        }
        Err(e) => {
            eprintln!("[media-helper] Command '{}' failed on {}: {}", command, mpris_dest, e);
            false
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
struct MediaStatus {
    is_playing: bool,
    position: f64,
    duration: i64,
}

impl MediaStatus {
    fn empty() -> Self {
        Self {
            is_playing: false,
            position: 0.0,
            duration: 0,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
struct MediaPlayerInstance {
    mpris_name: String,
    window_class: String,
    pid: Option<u32>,
    is_active: bool,
}

impl MediaPlayerInstance {
    fn new(mpris_name: String, window_class: String, pid: Option<u32>) -> Self {
        Self {
            mpris_name,
            window_class,
            pid,
            is_active: false,
        }
    }
}

// Global variable to store the current focused media player instance
static mut CURRENT_MEDIA_PLAYER_INSTANCE: Option<MediaPlayerInstance> = None;

// Global state to track if we're using instance-specific or base MPRIS name
static mut IS_USING_INSTANCE: bool = false;

pub fn set_current_media_player(class: &str, pid: Option<u32>) {
    log_info(&format!("set_current_media_player called with class: '{}', pid: {:?}", class, pid));
    
    let instance_dest = get_smplayer_mpris_destination(pid);
    let base_dest = SMPLAYER_BASE_MPRIS_NAME;
    
    // Test instance first - if it works, use it for all requests
    let (final_dest, is_instance) = if pid.is_some() && instance_dest != base_dest {
        log_info(&format!("Testing instance connection: {}", instance_dest));
        if TOKIO_RT.block_on(get_status_from_dest_native(&instance_dest)).is_some() {
            log_info(&format!("Instance connection successful, using: {} (instance: true)", instance_dest));
            (instance_dest, true)
        } else {
            log_info(&format!("Instance connection failed, falling back to base: {} (instance: false)", base_dest));
            (base_dest.to_string(), false)
        }
    } else {
        log_info(&format!("No PID provided, using base: {} (instance: false)", base_dest));
        (base_dest.to_string(), false)
    };
    
    let instance = MediaPlayerInstance::new(final_dest, class.to_string(), pid);
    
    // Update the global instance and state
    unsafe {
        CURRENT_MEDIA_PLAYER_INSTANCE = Some(instance.clone());
        IS_USING_INSTANCE = is_instance;
        log_info(&format!("Set current media player instance to: {:?} (using instance: {})", CURRENT_MEDIA_PLAYER_INSTANCE, IS_USING_INSTANCE));
    }
}

fn get_current_media_player_instance() -> Option<MediaPlayerInstance> {
    unsafe {
        CURRENT_MEDIA_PLAYER_INSTANCE.clone()
    }
}

fn get_current_mpris_destination() -> String {
    unsafe {
        if IS_USING_INSTANCE {
            if let Some(instance) = &CURRENT_MEDIA_PLAYER_INSTANCE {
                instance.mpris_name.clone()
            } else {
                SMPLAYER_BASE_MPRIS_NAME.to_string()
            }
    } else {
            SMPLAYER_BASE_MPRIS_NAME.to_string()
        }
    }
}

// SMPlayer-specific functions
fn get_smplayer_mpris_destination(pid: Option<u32>) -> String {
    if let Some(pid) = pid {
        let instance_name = format!("{}.instance{}", SMPLAYER_BASE_MPRIS_NAME, pid);
        log_info(&format!("Using SMPlayer instance-specific DBus: {}", instance_name));
        instance_name
    } else {
        log_info("No PID available, using SMPlayer base DBus");
        SMPLAYER_BASE_MPRIS_NAME.to_string()
    }
}


fn get_smplayer_status() -> Option<MediaStatus> {
    let instance = get_current_media_player_instance()?;
    let window_class_lower = instance.window_class.to_lowercase();
    if !SMPLAYER_WINDOW_CLASSES.iter().any(|&class| class.to_lowercase() == window_class_lower) {
        return None;
    }
    
    let mpris_dest = get_current_mpris_destination();
    log_info(&format!("Getting SMPlayer status from: {} (instance: {})", mpris_dest, unsafe { IS_USING_INSTANCE }));
    
    if let Some(status) = TOKIO_RT.block_on(get_status_from_dest_native(&mpris_dest)) {
        log_info(&format!("Successfully connected to: {}", mpris_dest));
            return Some(status);
    }
    
    log_error("SMPlayer MPRIS", "connection failed");
    None
}

// Async variant for use inside async DBus handlers
async fn get_smplayer_status_async() -> Option<MediaStatus> {
    let instance = get_current_media_player_instance()?;
    let window_class_lower = instance.window_class.to_lowercase();
    if !SMPLAYER_WINDOW_CLASSES.iter().any(|&class| class.to_lowercase() == window_class_lower) {
        return None;
    }
    
    let mpris_dest = get_current_mpris_destination();
    log_info(&format!("Getting SMPlayer status from: {} (instance: {}, async)", mpris_dest, unsafe { IS_USING_INSTANCE }));
    
    if let Some(status) = get_status_from_dest_native(&mpris_dest).await {
        log_info(&format!("Successfully connected (async) to: {}", mpris_dest));
            return Some(status);
        }
    
    log_error("SMPlayer MPRIS (async)", "connection failed");
    None
}

fn execute_smplayer_command(command: &str, args: &[&str]) -> bool {
    let instance = match get_current_media_player_instance() {
        Some(instance) => instance,
        None => {
            log_error("SMPlayer command execution", "No SMPlayer instance detected");
            return false;
        }
    };
    
    let window_class_lower = instance.window_class.to_lowercase();
    if !SMPLAYER_WINDOW_CLASSES.iter().any(|&class| class.to_lowercase() == window_class_lower) {
        log_error("SMPlayer command execution", &format!("Instance is not SMPlayer: {}", instance.window_class));
        return false;
    }
    
    let mpris_dest = get_current_mpris_destination();
    log_info(&format!("Executing SMPlayer command '{}' on {} (instance: {}, class: {}, PID: {:?})", 
              command, mpris_dest, unsafe { IS_USING_INSTANCE }, instance.window_class, instance.pid));
    
    TOKIO_RT.block_on(try_execute_command_on_destination_native(command, args, &mpris_dest))
}




pub fn handle_smplayer_command(command: &str, status_sender: &Arc<Mutex<Option<UnixStream>>>) {
    // Command debouncing to prevent spam during fast movement
    static mut LAST_SEEK_TIME: Option<std::time::Instant> = None;
    static mut PENDING_SEEK: Option<f64> = None;
    
    const MIN_SEEK_INTERVAL: u64 = 150; // Minimum 150ms between seeks
    
    match command.trim() {
        "play_pause" => {
            log_info("Executing play/pause command");
            execute_smplayer_command("org.mpris.MediaPlayer2.Player.PlayPause", &[]);
            send_status_if_available(status_sender);
        }
        "play" => {
            log_info("Executing play command");
            execute_smplayer_command("org.mpris.MediaPlayer2.Player.Play", &[]);
            send_status_if_available(status_sender);
        }
        "pause" => {
            log_info("Executing pause command");
            execute_smplayer_command("org.mpris.MediaPlayer2.Player.Pause", &[]);
            send_status_if_available(status_sender);
        }
        "next" => {
            log_info("Executing next command");
            execute_smplayer_command("org.mpris.MediaPlayer2.Player.Next", &[]);
        }
        "previous" => {
            log_info("Executing previous command");
            execute_smplayer_command("org.mpris.MediaPlayer2.Player.Previous", &[]);
        }
        "stop" => {
            log_info("Executing stop command");
            execute_smplayer_command("org.mpris.MediaPlayer2.Player.Stop", &[]);
            send_status_if_available(status_sender);
        }
        "raise" => {
            log_info("Executing raise command");
            execute_smplayer_command("org.mpris.MediaPlayer2.Raise", &[]);
        }
        "quit" => {
            log_info("Executing quit command");
            execute_smplayer_command("org.mpris.MediaPlayer2.Quit", &[]);
        }
        cmd if cmd.starts_with("seek:") => {
            if let Some(position_str) = cmd.strip_prefix("seek:") {
                if let Ok(mut position) = position_str.parse::<f64>() {
                    // Prevent seeking to exactly 0.0 or 1.0 to avoid media player closing
                    if position <= 0.001 {
                        position = 0.001;
                    } else if position >= 0.999 {
                        position = 0.999;
                    }
                    
                    let now = std::time::Instant::now();
                    let can_seek = unsafe {
                        if let Some(last_seek) = LAST_SEEK_TIME {
                            now.duration_since(last_seek).as_millis() >= MIN_SEEK_INTERVAL as u128
                        } else {
                            true // First seek, always allow
                        }
                    };
                    
                    if can_seek {
                        // Execute seek immediately
                        unsafe {
                            LAST_SEEK_TIME = Some(now);
                            PENDING_SEEK = None; // Clear any pending seek
                        }
                        
                        log_info(&format!("Executing seek command to position: {} (fast mode, debounced)", position));
                        handle_seek_command(position, status_sender);
                    } else {
                        // Store this seek for later execution
                        unsafe {
                            PENDING_SEEK = Some(position);
                        }
                        log_info(&format!("Seek throttled: position {} (too soon after last seek, will execute later)", position));
                        
                        // Still update header immediately for visual feedback
                        if let Some(current_status) = get_smplayer_status() {
                            let mut updated_status = current_status;
                            updated_status.position = position;
                            send_status_update(status_sender, &updated_status);
                            log_info(&format!("Header updated immediately (throttled seek: {:.2}%)", position * 100.0));
                        }
                    }
                        } else {
                    log_error("Seek command", &format!("Invalid position: {}", position_str));
                }
            }
        }
        cmd if cmd.starts_with("set_position:") => {
            if let Some(position_str) = cmd.strip_prefix("set_position:") {
                if let Ok(mut position) = position_str.parse::<f64>() {
                    // Prevent seeking to exactly 0.0 or 1.0 to avoid media player closing
                    if position <= 0.001 {
                        position = 0.001;
                    } else if position >= 0.999 {
                        position = 0.999;
                    }
                    
                    let now = std::time::Instant::now();
                    let can_seek = unsafe {
                        if let Some(last_seek) = LAST_SEEK_TIME {
                            now.duration_since(last_seek).as_millis() >= MIN_SEEK_INTERVAL as u128
                        } else {
                            true // First seek, always allow
                        }
                    };
                    
                    if can_seek {
                        // Execute seek immediately
                        unsafe {
                            LAST_SEEK_TIME = Some(now);
                            PENDING_SEEK = None; // Clear any pending seek
                        }
                        
                        log_info(&format!("Executing set position command to: {} (debounced)", position));
                        handle_seek_command(position, status_sender);
                    } else {
                        // Store this seek for later execution
                        unsafe {
                            PENDING_SEEK = Some(position);
                        }
                        log_info(&format!("Set position throttled: position {} (too soon after last seek, will execute later)", position));
                        
                        // Still update header immediately for visual feedback
                        if let Some(current_status) = get_smplayer_status() {
                            let mut updated_status = current_status;
                            updated_status.position = position;
                            send_status_update(status_sender, &updated_status);
                            log_info(&format!("Header updated immediately (throttled set position: {:.2}%)", position * 100.0));
                        }
                    }
                } else {
                    log_error("Set position command", &format!("Invalid position: {}", position_str));
                }
            }
        }
        _ => {
            log_error("Command handling", &format!("Unknown command: {}", command));
        }
    }
    
    // Process any pending seek if enough time has passed
    unsafe {
        if let Some(pending_position) = PENDING_SEEK {
            if let Some(last_seek) = LAST_SEEK_TIME {
                let now = std::time::Instant::now();
                if now.duration_since(last_seek).as_millis() >= MIN_SEEK_INTERVAL as u128 {
                    log_info(&format!("Processing pending seek to position: {}", pending_position));
                    
                    // Execute the pending seek
                    if handle_seek_command(pending_position, status_sender) {
                        log_info(&format!("Pending seek executed successfully to position: {:.2}%", pending_position * 100.0));
                            LAST_SEEK_TIME = Some(now);
                            PENDING_SEEK = None;
                        } else {
                        log_error("Pending seek", "execution failed");
                        }
                    }
                }
            }
        }
}

fn send_status_if_available(status_sender: &Arc<Mutex<Option<UnixStream>>>) {
    if let Some(status) = get_smplayer_status() {
        send_status_update(status_sender, &status);
    }
}

fn handle_seek_command(position: f64, status_sender: &Arc<Mutex<Option<UnixStream>>>) -> bool {
    // Clamp position to safe range
    let position = position.clamp(POSITION_SAFETY_MARGIN, 1.0 - POSITION_SAFETY_MARGIN);
    
    if let Some(current_status) = get_smplayer_status() {
        log_info(&format!("Current status: duration={}s, position={:.2}%, target={:.2}%", 
                 current_status.duration, current_status.position * 100.0, position * 100.0));
        
        // Check if we have a valid duration
        if current_status.duration <= 0 {
            log_error("Seek command", "Cannot seek: no valid duration available");
            return false;
        }
        
        let seek_offset = calculate_seek_offset(&current_status, position);
        log_info(&format!("Seeking to {:.2}% (offset: {}μs)", position * 100.0, seek_offset));
        
        if execute_smplayer_command("org.mpris.MediaPlayer2.Player.Seek", &[&format!("int64:{}", seek_offset)]) {
            // Update UI immediately
            let mut updated_status = current_status;
            updated_status.position = position;
            send_status_update(status_sender, &updated_status);
            true
        } else {
            log_error("Seek command", "Execution failed");
            false
        }
    } else {
        log_error("Seek command", "Failed to get current status");
        false
    }
}



pub fn monitor_smplayer_events(status_sender: Arc<Mutex<Option<UnixStream>>>) {
    // SMPlayer-specific event monitoring with position polling (like VLC)
    
    // Check initial SMPlayer status
    if let Some(initial_status) = get_smplayer_status() {
        send_status_update(&status_sender, &initial_status);
        log_info(&format!("Initial status detected: playing={}, position={:.2}%", 
                 initial_status.is_playing, initial_status.position * 100.0));
    }
    
    // Create a shared state for playback status to coordinate between threads
    let playback_state = Arc::new(Mutex::new((false, MediaStatus::empty())));
    let playback_state_clone = playback_state.clone();
    
    // Initialize shared state with current SMPlayer status if available
    if let Some(current_status) = get_smplayer_status() {
        if let Ok(mut state) = playback_state.lock() {
            state.0 = current_status.is_playing;
            state.1 = current_status.clone();
        }
        
        log_info(&format!("Shared state initialized: playing={}, position={:.2}%", 
                 current_status.is_playing, current_status.position * 100.0));
    }
    
    // Clone status_sender for position updates
    let position_sender = status_sender.clone();
    
    // Start SMPlayer position polling thread - SMPlayer needs polling for smooth updates
    thread::spawn(move || {
        loop {
            // Check if currently playing from shared state
            let is_playing = {
                if let Ok(state) = playback_state_clone.lock() {
                    state.0
                } else {
                    false
                }
            };
            
            if is_playing {
                // Get SMPlayer position using polling
                if let Some(status) = get_smplayer_status() {
                    if status.is_playing && status.duration > 0 {
                        send_status_update(&position_sender, &status);
                        log_info(&format!("Position polling update: {:.2}%", status.position * 100.0));
                    }
                }
                
                // Poll every 100ms for SMPlayer smooth progress updates
                thread::sleep(Duration::from_millis(100));
            } else {
                // Not playing - sleep longer and wait for events
                thread::sleep(Duration::from_millis(500));
            }
        }
    });
    
    // Start SMPlayer-specific DBus event monitoring for status changes
    let status_sender_clone = status_sender.clone();
    let playback_state_clone = playback_state.clone();
    thread::spawn(move || {
        let result = run_smplayer_dbus_event_monitor(status_sender_clone.clone(), playback_state_clone.clone());
        
        if let Err(e) = result {
            log_error("SMPlayer DBus event monitor", &format!("{}, restarting...", e));
            thread::sleep(Duration::from_millis(1000));
            monitor_smplayer_events(status_sender_clone);
        }
    });
}





fn run_smplayer_dbus_event_monitor(
    status_sender: Arc<Mutex<Option<UnixStream>>>, 
    playback_state: Arc<Mutex<(bool, MediaStatus)>>
) -> Result<(), Box<dyn std::error::Error>> {
    // Use shared async runtime for zbus
    TOKIO_RT.block_on(async {
        let connection = Connection::session().await?;
        
        // Initialize shared connection for native D-Bus calls
        unsafe {
            SHARED_DBUS_CONNECTION = Some(connection.clone());
        }
        let mut stream = MessageStream::from(&connection);
        let dbus_proxy = DBusProxy::new(&connection).await?;
        
        // Setup SMPlayer-specific signal subscriptions
        let signal_dest = get_current_mpris_destination();
        log_info(&format!("Subscribing to SMPlayer DBus signals: {} (instance: {})", signal_dest, unsafe { IS_USING_INSTANCE }));
        
        let match_rules = vec![
            // PropertiesChanged signals for playback status, position, metadata
            MatchRule::builder()
                .msg_type(MessageType::Signal)
                .interface("org.freedesktop.DBus.Properties")?
                .path_namespace("/org/mpris/MediaPlayer2")?
                .member("PropertiesChanged")?
                .build(),
        ];
        
        // Register all match rules
        for rule in match_rules {
            if let Err(e) = dbus_proxy.add_match_rule(rule).await {
            log_error("SMPlayer match rule", &e.to_string());
            }
        }
    log_info("SMPlayer-specific DBus signal subscription active");
        
        // Main signal processing loop
        while let Some(msg) = stream.next().await {
            let msg = msg?;
            let header = msg.header()?;
            
            // Skip non-signal messages
            if msg.message_type() != MessageType::Signal {
                continue;
            }
            
            // Process SMPlayer signals
            if let (Some(interface), Some(member)) = (header.interface()?, header.member()?) {
                let interface_str = interface.as_str();
                let member_str = member.as_str();
                
                log_info(&format!("Received SMPlayer DBus signal: {}.{}", interface_str, member_str));
                
                match (interface_str, member_str) {
                    ("org.freedesktop.DBus.Properties", "PropertiesChanged") => {
                        if let Ok((interface_name, changed_props, _invalidated_props)) = 
                            msg.body::<(String, std::collections::HashMap<String, zbus::zvariant::Value>, Vec<String>)>() {
                            
                            if interface_name == "org.mpris.MediaPlayer2.Player" {
                                log_info(&format!("SMPlayer properties changed: {:?}", changed_props));
                                process_smplayer_properties_changed_signal_dbus(changed_props, &status_sender, &playback_state).await;
                            }
                        }
                    }
                    _ => {
                        if let Ok(body) = msg.body::<String>() {
                            log_info(&format!("Unhandled SMPlayer signal: {}.{} - {}", interface_str, member_str, body));
                        }
                    }
                }
            }
        }
        
        Ok::<(), zbus::Error>(())
    })?;
    
    Ok(())
}



async fn process_smplayer_properties_changed_signal_dbus(
    changed_props: std::collections::HashMap<String, zbus::zvariant::Value<'_>>, 
    status_sender: &Arc<Mutex<Option<UnixStream>>>,
    playback_state: &Arc<Mutex<(bool, MediaStatus)>>
) {
    // Process changed properties from DBus signal for SMPlayer
    for (prop_name, prop_value) in changed_props {
        match prop_name.as_str() {
            "PlaybackStatus" => {
                if let Some(status_str) = prop_value.downcast::<String>() {
                    let is_playing = status_str == "Playing";
                    log_info(&format!("SMPlayer playback status changed to: {}", status_str));
                    
                    if let Some(mut status) = get_smplayer_status_async().await {
                        status.is_playing = is_playing;
                        update_and_send_status(status_sender, playback_state, is_playing, status).await;
                    }
                }
            }
            "Position" => {
                if let Some(position) = prop_value.downcast::<i64>() {
                    log_info(&format!("SMPlayer position changed to: {} microseconds", position));
                    
                    if let Some(mut status) = get_smplayer_status() {
                        let duration = status.duration * MICROSECONDS_PER_SECOND;
                        status.position = if duration > 0 { position as f64 / duration as f64 } else { 0.0 };
                        
                        if let Ok(mut state) = playback_state.lock() {
                            state.1 = status.clone();
                        }
                        
                        send_status_update(status_sender, &status);
                    }
                }
            }
            "Metadata" => {
                log_info("SMPlayer metadata changed");
                if let Some(status) = get_smplayer_status_async().await {
                    update_and_send_status(status_sender, playback_state, status.is_playing, status).await;
                }
            }
            _ => {
                log_info(&format!("SMPlayer property changed: {} = {:?}", prop_name, prop_value));
            }
        }
    }
}



fn send_status_update(status_sender: &Arc<Mutex<Option<UnixStream>>>, status: &MediaStatus) {
    if let Ok(mut sender_guard) = status_sender.lock() {
        if let Some(ref mut stream) = *sender_guard {
            let status_json = json!({
                "is_playing": status.is_playing,
                "position": status.position,
                "duration": status.duration
            });
            
            if let Err(e) = stream.write_all(format!("{}\n", status_json.to_string()).as_bytes()) {
                log_error("Status update", &e.to_string());
            }
        }
    }
}
