// Dragon Player helper module for tiny-dfr, providing Dragon Player status via DBus.
// 
// This helper module:
// 1. Connects to the main process via Unix socket
// 2. Monitors Dragon Player via DBus signals and sends status updates to main process
// 3. Receives commands from main process and executes them on Dragon Player
// 
// Supported commands:
// - play_pause: Toggle play/pause
// - play: Start playback
// - pause: Pause playback
// - next: Next track
// - previous: Previous track
// - stop: Stop playback
// - raise: Raise Dragon Player window
// - quit: Quit Dragon Player
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

// Dragon Player-specific data structures and functions

// Constants
const DRAGON_PLAYER_BASE_MPRIS_NAME: &str = "org.mpris.MediaPlayer2.dragonplayer";
const DRAGON_PLAYER_WINDOW_CLASSES: &[&str] = &["org.kde.dragonplayer", "dragonplayer"];
const POSITION_SAFETY_MARGIN: f64 = 0.001;
const MILLISECONDS_PER_SECOND: i64 = 1_000;

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
    let duration_milliseconds = current_status.duration * MILLISECONDS_PER_SECOND;
    let target_position_milliseconds = (target_position * duration_milliseconds as f64) as i64;
    let current_position_milliseconds = (current_status.position * duration_milliseconds as f64) as i64;
    target_position_milliseconds - current_position_milliseconds
}


fn log_error(operation: &str, error: &str) {
    eprintln!("[dragon-helper] {} failed: {}", operation, error);
}

fn log_info(message: &str) {
    eprintln!("[dragon-helper] {}", message);
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
        .and_then(|v| i64::try_from(v).ok())
        .unwrap_or(0);
    
    // Dragon Player uses milliseconds
    let duration_seconds = length_raw / 1_000;
    let position_seconds = position_raw as f64 / 1_000.0;
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
    
    let instance_dest = get_dragon_player_mpris_destination(pid);
    let base_dest = DRAGON_PLAYER_BASE_MPRIS_NAME;
    
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
                DRAGON_PLAYER_BASE_MPRIS_NAME.to_string()
            }
    } else {
            DRAGON_PLAYER_BASE_MPRIS_NAME.to_string()
        }
    }
}

// Dragon Player-specific functions
fn get_dragon_player_mpris_destination(pid: Option<u32>) -> String {
    if let Some(pid) = pid {
        let instance_name = format!("{}.instance{}", DRAGON_PLAYER_BASE_MPRIS_NAME, pid);
        log_info(&format!("Using Dragon Player instance-specific DBus: {}", instance_name));
        instance_name
    } else {
        log_info("No PID available, using Dragon Player base DBus");
        DRAGON_PLAYER_BASE_MPRIS_NAME.to_string()
    }
}


fn get_dragon_player_status() -> Option<MediaStatus> {
    let instance = get_current_media_player_instance()?;
    if !DRAGON_PLAYER_WINDOW_CLASSES.contains(&instance.window_class.as_str()) {
        return None;
    }
    
    let mpris_dest = get_current_mpris_destination();
    log_info(&format!("Getting Dragon Player status from: {} (instance: {})", mpris_dest, unsafe { IS_USING_INSTANCE }));
    
    if let Some(status) = TOKIO_RT.block_on(get_status_from_dest_native(&mpris_dest)) {
        log_info(&format!("Successfully connected to: {}", mpris_dest));
            return Some(status);
    }
    
    log_error("Dragon Player MPRIS", "connection failed");
    None
}

// Async variant for use inside async DBus handlers
async fn get_dragon_player_status_async() -> Option<MediaStatus> {
    let instance = get_current_media_player_instance()?;
    if !DRAGON_PLAYER_WINDOW_CLASSES.contains(&instance.window_class.as_str()) {
        return None;
    }
    
    let mpris_dest = get_current_mpris_destination();
    log_info(&format!("Getting Dragon Player status from: {} (instance: {}, async)", mpris_dest, unsafe { IS_USING_INSTANCE }));
    
    if let Some(status) = get_status_from_dest_native(&mpris_dest).await {
        log_info(&format!("Successfully connected (async) to: {}", mpris_dest));
            return Some(status);
        }
    
    log_error("Dragon Player MPRIS (async)", "connection failed");
    None
}

fn execute_dragon_player_command(command: &str, args: &[&str]) -> bool {
    let instance = match get_current_media_player_instance() {
        Some(instance) => instance,
        None => {
            log_error("Dragon Player command execution", "No Dragon Player instance detected");
            return false;
        }
    };
    
    if !DRAGON_PLAYER_WINDOW_CLASSES.contains(&instance.window_class.as_str()) {
        log_error("Dragon Player command execution", &format!("Instance is not Dragon Player: {}", instance.window_class));
        return false;
    }
    
    let mpris_dest = get_current_mpris_destination();
    log_info(&format!("Executing Dragon Player command '{}' on {} (instance: {}, class: {}, PID: {:?})", 
              command, mpris_dest, unsafe { IS_USING_INSTANCE }, instance.window_class, instance.pid));
    
    TOKIO_RT.block_on(try_execute_command_on_destination_native(command, args, &mpris_dest))
}
















pub fn handle_dragon_player_command(command: &str, status_sender: &Arc<Mutex<Option<UnixStream>>>) {
    // Command debouncing to prevent spam during fast movement
    static mut LAST_SEEK_TIME: Option<std::time::Instant> = None;
    static mut PENDING_SEEK: Option<f64> = None;
    
    const MIN_SEEK_INTERVAL: u64 = 150; // Minimum 150ms between seeks
    
    match command.trim() {
        "play_pause" => {
            log_info("Executing play/pause command");
            execute_dragon_player_command("org.mpris.MediaPlayer2.Player.PlayPause", &[]);
            send_status_if_available(status_sender);
        }
        "play" => {
            log_info("Executing play command");
            execute_dragon_player_command("org.mpris.MediaPlayer2.Player.Play", &[]);
            send_status_if_available(status_sender);
        }
        "pause" => {
            log_info("Executing pause command");
            execute_dragon_player_command("org.mpris.MediaPlayer2.Player.Pause", &[]);
            send_status_if_available(status_sender);
        }
        "next" => {
            log_info("Executing next command");
            execute_dragon_player_command("org.mpris.MediaPlayer2.Player.Next", &[]);
        }
        "previous" => {
            log_info("Executing previous command");
            execute_dragon_player_command("org.mpris.MediaPlayer2.Player.Previous", &[]);
        }
        "stop" => {
            log_info("Executing stop command");
            execute_dragon_player_command("org.mpris.MediaPlayer2.Player.Stop", &[]);
            send_status_if_available(status_sender);
        }
        "raise" => {
            log_info("Executing raise command");
            execute_dragon_player_command("org.mpris.MediaPlayer2.Raise", &[]);
        }
        "quit" => {
            log_info("Executing quit command");
            execute_dragon_player_command("org.mpris.MediaPlayer2.Quit", &[]);
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
                        if let Some(current_status) = get_dragon_player_status() {
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
                        if let Some(current_status) = get_dragon_player_status() {
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
    if let Some(status) = get_dragon_player_status() {
        send_status_update(status_sender, &status);
    }
}

fn handle_seek_command(position: f64, status_sender: &Arc<Mutex<Option<UnixStream>>>) -> bool {
    // Clamp position to safe range
    let position = position.clamp(POSITION_SAFETY_MARGIN, 1.0 - POSITION_SAFETY_MARGIN);
    
    if let Some(current_status) = get_dragon_player_status() {
        let seek_offset = calculate_seek_offset(&current_status, position);
        log_info(&format!("Seeking to {:.2}% (offset: {}ms)", position * 100.0, seek_offset));
        
        if execute_dragon_player_command("org.mpris.MediaPlayer2.Player.Seek", &[&format!("int64:{}", seek_offset)]) {
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



pub fn monitor_dragon_player_events(status_sender: Arc<Mutex<Option<UnixStream>>>) {
    // Dragon Player-specific event monitoring - fully event-driven, no polling needed
    
    // Create a shared state for playback status to coordinate between threads
    let playback_state = Arc::new(Mutex::new((false, MediaStatus::empty())));
    
    // Initialize shared state with current Dragon Player status if available
    if let Some(current_status) = get_dragon_player_status() {
        if let Ok(mut state) = playback_state.lock() {
            state.0 = current_status.is_playing;
            state.1 = current_status.clone();
        }
        
        // Send initial status
        send_status_update(&status_sender, &current_status);
        log_info(&format!("Initial status detected: playing={}, position={:.2}%", 
                 current_status.is_playing, current_status.position * 100.0));
    }
    
    // Start Dragon Player-specific DBus event monitoring (no polling thread needed)
    let status_sender_clone = status_sender.clone();
    let playback_state_clone = playback_state.clone();
    thread::spawn(move || {
        let result = run_dragon_player_dbus_event_monitor(status_sender_clone.clone(), playback_state_clone.clone());
        
        if let Err(e) = result {
            log_error("Dragon Player DBus event monitor", &format!("{}, restarting...", e));
            thread::sleep(Duration::from_millis(1000));
            monitor_dragon_player_events(status_sender);
        }
    });
}




fn run_dragon_player_dbus_event_monitor(
    status_sender: Arc<Mutex<Option<UnixStream>>>, 
    playback_state: Arc<Mutex<(bool, MediaStatus)>>
) -> Result<(), Box<dyn std::error::Error>> {
    TOKIO_RT.block_on(async {
        // Setup DBus connection and stream
        let connection = get_shared_connection().await?;
        let mut stream = MessageStream::from(&connection);
        let dbus_proxy = DBusProxy::new(&connection).await?;
        
        // Setup Dragon Player-specific signal subscriptions
        let signal_dest = get_current_mpris_destination();
        log_info(&format!("Subscribing to Dragon Player DBus signals: {} (instance: {})", signal_dest, unsafe { IS_USING_INSTANCE }));
        
        let match_rules = vec![
            MatchRule::builder()
                .msg_type(MessageType::Signal)
                .interface("org.freedesktop.DBus.Properties")?
                .path_namespace("/org/mpris/MediaPlayer2")?
                .member("PropertiesChanged")?
                .sender(signal_dest.as_str())?
                .build(),
            MatchRule::builder()
                .msg_type(MessageType::Signal)
                .interface("org.mpris.MediaPlayer2.Player")?
                .member("Seeked")?
                .sender(signal_dest.as_str())?
                .build(),
        ];
        
        // Register all match rules
        for rule in match_rules {
            if let Err(e) = dbus_proxy.add_match_rule(rule).await {
            log_error("Dragon Player match rule", &e.to_string());
            }
        }
    log_info("Dragon Player-specific DBus signal subscription active");
        
        // Main signal processing loop
        while let Some(msg) = stream.next().await {
            let msg = msg?;
            let header = msg.header()?;
            
            // Skip non-signal messages
            if msg.message_type() != MessageType::Signal {
                continue;
            }
            
            // Process Dragon Player signals
            if let (Some(interface), Some(member)) = (header.interface()?, header.member()?) {
                let interface_str = interface.as_str();
                let member_str = member.as_str();
                
                log_info(&format!("Received Dragon Player DBus signal: {}.{}", interface_str, member_str));
                
                match (interface_str, member_str) {
                    ("org.freedesktop.DBus.Properties", "PropertiesChanged") => {
                        if let Ok((interface_name, changed_props, _invalidated_props)) = 
                            msg.body::<(String, std::collections::HashMap<String, zbus::zvariant::Value>, Vec<String>)>() {
                            
                            if interface_name == "org.mpris.MediaPlayer2.Player" {
                                log_info(&format!("Dragon Player properties changed: {:?}", changed_props));
                                process_dragon_player_properties_changed_signal_dbus(changed_props, &status_sender, &playback_state).await;
                            }
                        }
                    }
                    ("org.mpris.MediaPlayer2.Player", "Seeked") => {
                        if let Ok(position) = msg.body::<i64>() {
                            log_info(&format!("Dragon Player seeked to position: {} microseconds", position));
                            process_dragon_player_seeked_signal_dbus(position, &status_sender, &playback_state).await;
                        }
                    }
                    _ => {
                        if let Ok(body) = msg.body::<String>() {
                            log_info(&format!("Unhandled Dragon Player signal: {}.{} - {}", interface_str, member_str, body));
                        }
                    }
                }
            }
        }
        
        Ok::<(), zbus::Error>(())
    })?;
    
    Ok(())
}



async fn process_dragon_player_properties_changed_signal_dbus(
    changed_props: std::collections::HashMap<String, zbus::zvariant::Value<'_>>, 
    status_sender: &Arc<Mutex<Option<UnixStream>>>,
    playback_state: &Arc<Mutex<(bool, MediaStatus)>>
) {
    // Process changed properties from DBus signal for Dragon Player
    for (prop_name, prop_value) in changed_props {
        match prop_name.as_str() {
            "PlaybackStatus" => {
                if let Some(status_str) = prop_value.downcast::<String>() {
                    let is_playing = status_str == "Playing";
                    log_info(&format!("Dragon Player playback status changed to: {}", status_str));
                    
                    if let Some(mut status) = get_dragon_player_status_async().await {
                        status.is_playing = is_playing;
                        update_and_send_status(status_sender, playback_state, is_playing, status).await;
                    }
                }
            }
            "Position" => {
                if let Some(position) = prop_value.downcast::<i64>() {
                    log_info(&format!("Dragon Player position changed to: {} milliseconds", position));
                    
                    if let Some(mut status) = get_dragon_player_status() {
                        let duration = status.duration * MILLISECONDS_PER_SECOND;
                        status.position = if duration > 0 { position as f64 / duration as f64 } else { 0.0 };
                        
                        if let Ok(mut state) = playback_state.lock() {
                            state.1 = status.clone();
                        }
                        
                        send_status_update(status_sender, &status);
                    }
                }
            }
            "Metadata" => {
                log_info("Dragon Player metadata changed");
                if let Some(status) = get_dragon_player_status_async().await {
                    update_and_send_status(status_sender, playback_state, status.is_playing, status).await;
                }
            }
            _ => {
                log_info(&format!("Dragon Player property changed: {} = {:?}", prop_name, prop_value));
            }
        }
    }
}

async fn process_dragon_player_seeked_signal_dbus(
    position: i64, 
    status_sender: &Arc<Mutex<Option<UnixStream>>>,
    playback_state: &Arc<Mutex<(bool, MediaStatus)>>
) {
    if let Some(mut status) = get_dragon_player_status_async().await {
        let duration = status.duration * MILLISECONDS_PER_SECOND;
        status.position = if duration > 0 { position as f64 / duration as f64 } else { 0.0 };
        
        let position_percentage = status.position * 100.0;
        update_and_send_status(status_sender, playback_state, status.is_playing, status).await;
        log_info(&format!("Dragon Player seeked to position: {:.2}%", position_percentage));
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

