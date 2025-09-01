//! Media Player helper binary for tiny-dfr, providing VLC and Dragon Player status via DBus.
//! 
//! This helper process:
//! 1. Connects to the main process via Unix socket
//! 2. Monitors VLC or Dragon Player via DBus signals and sends status updates to main process
//! 3. Receives commands from main process and executes them on the active media player
//! 
//! Supported commands:
//! - play_pause: Toggle play/pause
//! - play: Start playback
//! - pause: Pause playback
//! - next: Next track
//! - previous: Previous track
//! - stop: Stop playback
//! - raise: Raise VLC window
//! - quit: Quit VLC
//! - seek:position: Seek to position (0.0 to 1.0)
//! - set_position:position: Set absolute position (0.0 to 1.0)

use std::os::unix::net::UnixStream;
use std::io::{Write, Read};
use std::thread;
use std::time::Duration;

use std::process::Command;
use std::sync::{Arc, Mutex};
use serde_json::json;

// DBus imports for proper event-driven monitoring
use zbus::{Connection, MessageType, MessageStream, MatchRule};
use zbus::fdo::DBusProxy;
use futures_lite::stream::StreamExt;


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

fn set_current_media_player(class: &str, pid: Option<u32>) {
    eprintln!("[media-helper] set_current_media_player called with class: '{}', pid: {:?}", class, pid);
    
    let mpris_dest = get_mpris_destination(class, pid);
    eprintln!("[media-helper] Determined MPRIS destination: {}", mpris_dest);
    
    unsafe {
        CURRENT_MEDIA_PLAYER_INSTANCE = Some(MediaPlayerInstance::new(
            mpris_dest,
            class.to_string(),
            pid,
        ));
        eprintln!("[media-helper] Set current media player instance to: {:?}", CURRENT_MEDIA_PLAYER_INSTANCE);
    }
}

fn get_current_media_player_instance() -> Option<MediaPlayerInstance> {
    unsafe {
        CURRENT_MEDIA_PLAYER_INSTANCE.clone()
    }
}

// VLC-specific functions
fn get_vlc_mpris_destination(pid: Option<u32>) -> String {
    if let Some(pid) = pid {
        let instance_name = format!("org.mpris.MediaPlayer2.vlc.instance{}", pid);
        eprintln!("[vlc-helper] Using VLC instance-specific MPRIS: {}", instance_name);
        instance_name
    } else {
        eprintln!("[vlc-helper] No PID available, using legacy VLC MPRIS");
        "org.mpris.MediaPlayer2.vlc".to_string()
    }
}

fn is_vlc_running() -> bool {
    get_current_media_player_instance()
        .map(|instance| instance.window_class == "vlc")
        .unwrap_or(false)
}

fn get_vlc_status() -> Option<MediaStatus> {
    let instance = get_current_media_player_instance()?;
    if instance.window_class != "vlc" {
        return None;
    }
    
    let primary_dest = &instance.mpris_name;
    let fallback_dest = "org.mpris.MediaPlayer2.vlc";
    
    let destinations = if primary_dest != fallback_dest {
        vec![primary_dest.as_str(), fallback_dest]
    } else {
        vec![primary_dest.as_str()]
    };
    
    for (i, mpris_dest) in destinations.iter().enumerate() {
        if i == 0 {
            eprintln!("[vlc-helper] Getting VLC status from: {} (primary)", mpris_dest);
        } else {
            eprintln!("[vlc-helper] Trying VLC fallback: {}", mpris_dest);
        }
        
        if let Some(status) = get_vlc_status_from_dest(mpris_dest) {
            eprintln!("[vlc-helper] Successfully connected to: {}", mpris_dest);
            return Some(status);
        }
    }
    
    eprintln!("[vlc-helper] All VLC MPRIS destinations failed");
    None
}

fn execute_vlc_command(command: &str, args: &[&str]) -> bool {
    let instance = match get_current_media_player_instance() {
        Some(instance) => instance,
        None => {
            eprintln!("[vlc-helper] No VLC instance detected");
            return false;
        }
    };
    
    if instance.window_class != "vlc" {
        eprintln!("[vlc-helper] Instance is not VLC: {}", instance.window_class);
        return false;
    }
    
    let primary_dest = &instance.mpris_name;
    let fallback_dest = "org.mpris.MediaPlayer2.vlc";
    
    let destinations = if primary_dest != fallback_dest {
        vec![primary_dest.as_str(), fallback_dest]
    } else {
        vec![primary_dest.as_str()]
    };
    
    for (i, mpris_dest) in destinations.iter().enumerate() {
        if i == 0 {
            eprintln!("[vlc-helper] Executing VLC command '{}' on {} (primary, class: {}, PID: {:?})", 
                      command, mpris_dest, instance.window_class, instance.pid);
        } else {
            eprintln!("[vlc-helper] Trying VLC fallback: {}", mpris_dest);
        }
        
        if try_execute_command_on_destination(command, args, mpris_dest) {
            return true;
        }
    }
    
    false
}

// Dragon Player-specific functions
fn get_dragon_player_mpris_destination(pid: Option<u32>) -> String {
    if let Some(pid) = pid {
        let instance_name = format!("org.mpris.MediaPlayer2.dragonplayer.instance{}", pid);
        eprintln!("[dragon-helper] Using Dragon Player instance-specific MPRIS: {}", instance_name);
        instance_name
    } else {
        eprintln!("[dragon-helper] No PID available, using legacy Dragon Player MPRIS");
        "org.mpris.MediaPlayer2.dragonplayer".to_string()
    }
}

fn is_dragon_player_running() -> bool {
    get_current_media_player_instance()
        .map(|instance| instance.window_class == "org.kde.dragonplayer")
        .unwrap_or(false)
}

fn get_dragon_player_status() -> Option<MediaStatus> {
    let instance = get_current_media_player_instance()?;
    if instance.window_class != "org.kde.dragonplayer" {
        return None;
    }
    
    let primary_dest = &instance.mpris_name;
    let fallback_dest = "org.mpris.MediaPlayer2.dragonplayer";
    
    let destinations = if primary_dest != fallback_dest {
        vec![primary_dest.as_str(), fallback_dest]
    } else {
        vec![primary_dest.as_str()]
    };
    
    for (i, mpris_dest) in destinations.iter().enumerate() {
        if i == 0 {
            eprintln!("[dragon-helper] Getting Dragon Player status from: {} (primary)", mpris_dest);
        } else {
            eprintln!("[dragon-helper] Trying Dragon Player fallback: {}", mpris_dest);
        }
        
        if let Some(status) = get_dragon_player_status_from_dest(mpris_dest) {
            eprintln!("[dragon-helper] Successfully connected to: {}", mpris_dest);
            return Some(status);
        }
    }
    
    eprintln!("[dragon-helper] All Dragon Player MPRIS destinations failed");
    None
}

fn execute_dragon_player_command(command: &str, args: &[&str]) -> bool {
    let instance = match get_current_media_player_instance() {
        Some(instance) => instance,
        None => {
            eprintln!("[dragon-helper] No Dragon Player instance detected");
            return false;
        }
    };
    
    if instance.window_class != "org.kde.dragonplayer" {
        eprintln!("[dragon-helper] Instance is not Dragon Player: {}", instance.window_class);
        return false;
    }
    
    let primary_dest = &instance.mpris_name;
    let fallback_dest = "org.mpris.MediaPlayer2.dragonplayer";
    
    let destinations = if primary_dest != fallback_dest {
        vec![primary_dest.as_str(), fallback_dest]
    } else {
        vec![primary_dest.as_str()]
    };
    
    for (i, mpris_dest) in destinations.iter().enumerate() {
        if i == 0 {
            eprintln!("[dragon-helper] Executing Dragon Player command '{}' on {} (primary, class: {}, PID: {:?})", 
                      command, mpris_dest, instance.window_class, instance.pid);
        } else {
            eprintln!("[dragon-helper] Trying Dragon Player fallback: {}", mpris_dest);
        }
        
        if try_execute_command_on_destination(command, args, mpris_dest) {
            return true;
        }
    }
    
    false
}

// Generic functions that route to the appropriate player-specific function
fn get_mpris_destination(class: &str, pid: Option<u32>) -> String {
    eprintln!("[media-helper] get_mpris_destination called with class: '{}', pid: {:?}", class, pid);
    
    match class {
        "vlc" => get_vlc_mpris_destination(pid),
        "org.kde.dragonplayer" => get_dragon_player_mpris_destination(pid),
        _ => {
            eprintln!("[media-helper] Unknown media player class: {}, using VLC fallback", class);
            get_vlc_mpris_destination(pid)
        }
    }
}

fn is_media_player_running() -> bool {
    // Check if we have a current media player instance
    get_current_media_player_instance().is_some()
}

fn get_media_player_status() -> Option<MediaStatus> {
    let instance = get_current_media_player_instance()?;
    
    // Route to the appropriate player-specific function
    match instance.window_class.as_str() {
        "vlc" => get_vlc_status(),
        "org.kde.dragonplayer" => get_dragon_player_status(),
        _ => {
            eprintln!("[media-helper] Unknown media player class: {}, defaulting to VLC", instance.window_class);
            get_vlc_status()
        }
    }
}

fn execute_media_player_command(command: &str, args: &[&str]) -> bool {
    let instance = match get_current_media_player_instance() {
        Some(instance) => instance,
        None => {
            eprintln!("[media-helper] No media player instance detected");
            return false;
        }
    };
    
    // Route to the appropriate player-specific function
    match instance.window_class.as_str() {
        "vlc" => execute_vlc_command(command, args),
        "org.kde.dragonplayer" => execute_dragon_player_command(command, args),
        _ => {
            eprintln!("[media-helper] Unknown media player class: {}, defaulting to VLC", instance.window_class);
            execute_vlc_command(command, args)
        }
    }
}

fn get_vlc_status_from_dest(mpris_dest: &str) -> Option<MediaStatus> {
    // Get all properties in a single DBus call to reduce latency
    let output = Command::new("timeout")
        .arg("1") // 1 second timeout
        .arg("dbus-send")
        .arg("--session")
        .arg(format!("--dest={}", mpris_dest))
        .arg("--type=method_call")
        .arg("--print-reply")
        .arg("/org/mpris/MediaPlayer2")
        .arg("org.freedesktop.DBus.Properties.GetAll")
        .arg("string:org.mpris.MediaPlayer2.Player")
        .output()
        .ok()?;
    
    if !output.status.success() {
        return None;
    }
    
    let text = String::from_utf8_lossy(&output.stdout);
    let lines: Vec<&str> = text.lines().collect();
    
    // Extract playback status
    let is_playing = lines.iter().any(|line| line.contains("Playing"));
    
    // Extract position
    let position = lines.iter()
        .find(|line| line.contains("Position"))
        .and_then(|line| {
            // Look for the next line with int64 value
            if let Some(pos) = lines.iter().position(|l| l == line) {
                for i in (pos + 1)..lines.len() {
                    if lines[i].contains("int64") {
                        return lines[i].split_whitespace().last().and_then(|s| s.parse::<i64>().ok());
                    }
                }
            }
            None
        })
        .unwrap_or(0);

    // Extract duration from metadata
    let duration = lines.iter()
        .find(|line| line.contains("mpris:length"))
        .and_then(|line| {
            if let Some(pos) = lines.iter().position(|l| l == line) {
                for i in (pos + 1)..lines.len() {
                    if lines[i].contains("int64") {
                        return lines[i].split_whitespace().last().and_then(|s| s.parse::<i64>().ok());
                    }
                }
            }
            None
        })
        .unwrap_or(0);
    
    // Calculate progress
    let progress = if duration > 0 { position as f64 / duration as f64 } else { 0.0 };
    
    Some(MediaStatus {
        is_playing,
        position: progress,
        duration: duration / 1_000_000, // Convert to seconds
    })
}

fn get_dragon_player_status_from_dest(mpris_dest: &str) -> Option<MediaStatus> {
    // Get all properties in a single DBus call to reduce latency
    let output = Command::new("timeout")
        .arg("1") // 1 second timeout
        .arg("dbus-send")
        .arg("--session")
        .arg(format!("--dest={}", mpris_dest))
        .arg("--type=method_call")
        .arg("--print-reply")
        .arg("/org/mpris/MediaPlayer2")
        .arg("org.freedesktop.DBus.Properties.GetAll")
        .arg("string:org.mpris.MediaPlayer2.Player")
        .output()
        .ok()?;
    
    if !output.status.success() {
        return None;
    }
    
    let text = String::from_utf8_lossy(&output.stdout);
    let lines: Vec<&str> = text.lines().collect();
    
    // Extract playback status
    let is_playing = lines.iter().any(|line| line.contains("Playing"));
    
    // Extract position
    let position = lines.iter()
        .find(|line| line.contains("Position"))
        .and_then(|line| {
            // Look for the next line with int64 value
            if let Some(pos) = lines.iter().position(|l| l == line) {
                for i in (pos + 1)..lines.len() {
                    if lines[i].contains("int64") {
                        return lines[i].split_whitespace().last().and_then(|s| s.parse::<i64>().ok());
                    }
                }
            }
            None
        })
        .unwrap_or(0);
    
    // Extract duration from metadata
    let duration = lines.iter()
        .find(|line| line.contains("mpris:length"))
        .and_then(|line| {
            if let Some(pos) = lines.iter().position(|l| l == line) {
                for i in (pos + 1)..lines.len() {
                    if lines[i].contains("int64") {
                        return lines[i].split_whitespace().last().and_then(|s| s.parse::<i64>().ok());
                    }
                }
            }
            None
        })
        .unwrap_or(0);
    
    // Calculate progress
    let progress = if duration > 0 { position as f64 / duration as f64 } else { 0.0 };
    
    Some(MediaStatus {
        is_playing,
        position: progress,
        duration: duration / 1_000, // Dragon Player uses milliseconds, convert to seconds
    })
}

fn get_media_player_status_from_dest(mpris_dest: &str) -> Option<MediaStatus> {
    // Get the current media player instance to determine the class
    let instance = get_current_media_player_instance();
    
    // Determine which function to use based on the window class name
    match instance.as_ref().map(|inst| inst.window_class.as_str()) {
        Some("vlc") => get_vlc_status_from_dest(mpris_dest),
        Some("org.kde.dragonplayer") => get_dragon_player_status_from_dest(mpris_dest),
        _ => {
            // Fallback: try to determine from MPRIS destination if no instance info
            if mpris_dest.contains("vlc") {
                get_vlc_status_from_dest(mpris_dest)
            } else if mpris_dest.contains("dragonplayer") {
                get_dragon_player_status_from_dest(mpris_dest)
        } else {
                // Default fallback to VLC
                get_vlc_status_from_dest(mpris_dest)
            }
        }
    }
}

fn try_execute_command_on_destination(command: &str, args: &[&str], mpris_dest: &str) -> bool {
    let mut cmd = Command::new("timeout");
    cmd.arg("1") // 1 second timeout
       .arg("dbus-send")
       .arg("--session")
       .arg(format!("--dest={}", mpris_dest))
       .arg("--type=method_call")
       .arg("--print-reply")
       .arg("/org/mpris/MediaPlayer2")
       .arg(command);
    
    for arg in args {
        cmd.arg(arg);
    }
    
    match cmd.output() {
        Ok(output) => {
            if output.status.success() {
                eprintln!("[media-helper] Command '{}' executed successfully on {}", command, mpris_dest);
                true
            } else {
                eprintln!("[media-helper] Command '{}' failed on {}: {:?}", command, mpris_dest, String::from_utf8_lossy(&output.stderr));
                false
            }
        }
        Err(e) => {
            eprintln!("[media-helper] Failed to execute command '{}' on {}: {}", command, mpris_dest, e);
            false
        }
    }
}

fn handle_command(command: &str, status_sender: &Arc<Mutex<Option<UnixStream>>>) {
    // Get the current media player instance to determine which player we're working with
    let player_info = get_current_media_player_instance();
    let player_name = player_info.as_ref().map(|inst| inst.window_class.as_str()).unwrap_or("unknown");
    
    // Route to the appropriate player-specific command handler
    match player_name {
        "vlc" => {
            eprintln!("[media-helper] Routing to VLC-specific command handler");
            handle_vlc_command(command, status_sender)
        }
        "org.kde.dragonplayer" => {
            eprintln!("[media-helper] Routing to Dragon Player-specific command handler");
            handle_dragon_player_command(command, status_sender)
        }
        _ => {
            eprintln!("[media-helper] Unknown media player class: {}, defaulting to VLC command handler", player_name);
            handle_vlc_command(command, status_sender)
        }
    }
}

fn handle_vlc_command(command: &str, status_sender: &Arc<Mutex<Option<UnixStream>>>) {
    // Command debouncing to prevent spam during fast movement
    static mut LAST_SEEK_TIME: Option<std::time::Instant> = None;
    static mut PENDING_SEEK: Option<f64> = None;
    
    const MIN_SEEK_INTERVAL: u64 = 150; // Minimum 150ms between seeks
    
    match command.trim() {
        "play_pause" => {
            eprintln!("[vlc-helper] Executing play/pause command");
            execute_vlc_command("org.mpris.MediaPlayer2.Player.PlayPause", &[]);
        }
        "play" => {
            eprintln!("[vlc-helper] Executing play command");
            execute_vlc_command("org.mpris.MediaPlayer2.Player.Play", &[]);
        }
        "pause" => {
            eprintln!("[vlc-helper] Executing pause command");
            execute_vlc_command("org.mpris.MediaPlayer2.Player.Pause", &[]);
        }
        "next" => {
            eprintln!("[vlc-helper] Executing next command");
            execute_vlc_command("org.mpris.MediaPlayer2.Player.Next", &[]);
        }
        "previous" => {
            eprintln!("[vlc-helper] Executing previous command");
            execute_vlc_command("org.mpris.MediaPlayer2.Player.Previous", &[]);
        }
        "stop" => {
            eprintln!("[vlc-helper] Executing stop command");
            execute_vlc_command("org.mpris.MediaPlayer2.Player.Stop", &[]);
        }
        "raise" => {
            eprintln!("[vlc-helper] Executing raise command");
            execute_vlc_command("org.mpris.MediaPlayer2.Raise", &[]);
        }
        "quit" => {
            eprintln!("[vlc-helper] Executing quit command");
            execute_vlc_command("org.mpris.MediaPlayer2.Quit", &[]);
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
                        
                        eprintln!("[vlc-helper] Executing seek command to position: {} (fast mode, debounced)", position);
                        
                        // Use Seek method instead of SetPosition - more reliable
                        // First get current position and duration to calculate seek offset
                        if let Some(current_status) = get_vlc_status() {
                            let duration_microseconds = current_status.duration * 1_000_000;
                            let target_position_microseconds = (position * duration_microseconds as f64) as i64;
                            let current_position_microseconds = (current_status.position * duration_microseconds as f64) as i64;
                            let seek_offset = target_position_microseconds - current_position_microseconds;
                            
                            eprintln!("[vlc-helper] Seeking (VLC): current={}μs, target={}μs, offset={}μs", 
                                     current_position_microseconds, target_position_microseconds, seek_offset);
                            
                            // Execute seek command with offset
                            let success = execute_vlc_command("org.mpris.MediaPlayer2.Player.Seek", &[&format!("int64:{}", seek_offset)]);
                            
                            if success {
                                // IMMEDIATELY send status update to move the header
                                // This prevents the delay and makes the UI responsive
                                let mut updated_status = current_status;
                                updated_status.position = position;
                                
                                // Send immediate update to move header
                                send_status_update(status_sender, &updated_status);
                                eprintln!("[vlc-helper] Header updated immediately to position: {:.2}%", position * 100.0);
                            } else {
                                eprintln!("[vlc-helper] Seek command failed");
                            }
                        } else {
                            eprintln!("[vlc-helper] Failed to get current status for seek");
                        }
                    } else {
                        // Store this seek for later execution
                        unsafe {
                            PENDING_SEEK = Some(position);
                        }
                        eprintln!("[vlc-helper] Seek throttled: position {} (too soon after last seek, will execute later)", position);
                        
                        // Still update header immediately for visual feedback
                        if let Some(current_status) = get_vlc_status() {
                            let mut updated_status = current_status;
                            updated_status.position = position;
                            send_status_update(status_sender, &updated_status);
                            eprintln!("[vlc-helper] Header updated immediately (throttled seek: {:.2}%)", position * 100.0);
                        }
                    }
                } else {
                    eprintln!("[vlc-helper] Invalid seek position: {}", position_str);
                }
            }
        }
        cmd if cmd.starts_with("set_position:") => {
            if let Some(position_str) = cmd.strip_prefix("set_position:") {
                if let Ok(position) = position_str.parse::<f64>() {
                    eprintln!("[vlc-helper] Executing set position command to: {}", position);
                    
                    // Use Seek method instead of SetPosition - more reliable
                    if let Some(current_status) = get_vlc_status() {
                        let duration_microseconds = current_status.duration * 1_000_000;
                        let target_position_microseconds = (position * duration_microseconds as f64) as i64;
                        let current_position_microseconds = (current_status.position * duration_microseconds as f64) as i64;
                        let seek_offset = target_position_microseconds - current_position_microseconds;
                        
                        eprintln!("[vlc-helper] Set position (VLC): current={}μs, target={}μs, offset={}μs", 
                                 current_position_microseconds, target_position_microseconds, seek_offset);
                        
                        // Execute seek command with offset
                        let success = execute_vlc_command("org.mpris.MediaPlayer2.Player.Seek", &[&format!("int64:{}", seek_offset)]);
                        
                        if success {
                            // IMMEDIATELY send status update to move the header
                            let mut updated_status = current_status;
                            updated_status.position = position;
                            send_status_update(status_sender, &updated_status);
                            eprintln!("[vlc-helper] Header updated immediately to position: {:.2}%", position * 100.0);
                } else {
                            eprintln!("[vlc-helper] Set position command failed");
                        }
                    } else {
                        eprintln!("[vlc-helper] Failed to get current status for set position");
                    }
                } else {
                    eprintln!("[vlc-helper] Invalid set position: {}", position_str);
                }
            }
        }
        _ => {
            eprintln!("[vlc-helper] Unknown command: {}", command);
        }
    }
    
    // Process any pending seek if enough time has passed
    unsafe {
        if let Some(pending_position) = PENDING_SEEK {
            if let Some(last_seek) = LAST_SEEK_TIME {
                let now = std::time::Instant::now();
                if now.duration_since(last_seek).as_millis() >= MIN_SEEK_INTERVAL as u128 {
                    eprintln!("[vlc-helper] Processing pending seek to position: {}", pending_position);
                    
                    // Execute the pending seek
                    if let Some(current_status) = get_vlc_status() {
                        let duration_microseconds = current_status.duration * 1_000_000;
                        let target_position_microseconds = (pending_position * duration_microseconds as f64) as i64;
                        let current_position_microseconds = (current_status.position * duration_microseconds as f64) as i64;
                        let seek_offset = target_position_microseconds - current_position_microseconds;
                        
                        eprintln!("[vlc-helper] Executing pending seek (VLC): current={}μs, target={}μs, offset={}μs", 
                                 current_position_microseconds, target_position_microseconds, seek_offset);
                        
                        let success = execute_vlc_command("org.mpris.MediaPlayer2.Player.Seek", &[&format!("int64:{}", seek_offset)]);
                        
                        if success {
                            eprintln!("[vlc-helper] Pending seek executed successfully to position: {:.2}%", pending_position * 100.0);
                            LAST_SEEK_TIME = Some(now);
                            PENDING_SEEK = None;
                        } else {
                            eprintln!("[vlc-helper] Pending seek failed");
                        }
                    }
                }
            }
        }
    }
}

fn handle_dragon_player_command(command: &str, status_sender: &Arc<Mutex<Option<UnixStream>>>) {
    // Command debouncing to prevent spam during fast movement
    static mut LAST_SEEK_TIME: Option<std::time::Instant> = None;
    static mut PENDING_SEEK: Option<f64> = None;
    
    const MIN_SEEK_INTERVAL: u64 = 150; // Minimum 150ms between seeks
    
    match command.trim() {
        "play_pause" => {
            eprintln!("[dragon-helper] Executing play/pause command");
            execute_dragon_player_command("org.mpris.MediaPlayer2.Player.PlayPause", &[]);
        }
        "play" => {
            eprintln!("[dragon-helper] Executing play command");
            execute_dragon_player_command("org.mpris.MediaPlayer2.Player.Play", &[]);
        }
        "pause" => {
            eprintln!("[dragon-helper] Executing pause command");
            execute_dragon_player_command("org.mpris.MediaPlayer2.Player.Pause", &[]);
        }
        "next" => {
            eprintln!("[dragon-helper] Executing next command");
            execute_dragon_player_command("org.mpris.MediaPlayer2.Player.Next", &[]);
        }
        "previous" => {
            eprintln!("[dragon-helper] Executing previous command");
            execute_dragon_player_command("org.mpris.MediaPlayer2.Player.Previous", &[]);
        }
        "stop" => {
            eprintln!("[dragon-helper] Executing stop command");
            execute_dragon_player_command("org.mpris.MediaPlayer2.Player.Stop", &[]);
        }
        "raise" => {
            eprintln!("[dragon-helper] Executing raise command");
            execute_dragon_player_command("org.mpris.MediaPlayer2.Raise", &[]);
        }
        "quit" => {
            eprintln!("[dragon-helper] Executing quit command");
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
                        
                        eprintln!("[dragon-helper] Executing seek command to position: {} (fast mode, debounced)", position);
                        
                        // Use Seek method instead of SetPosition - more reliable
                        // First get current position and duration to calculate seek offset
                        if let Some(current_status) = get_dragon_player_status() {
                            let duration_milliseconds = current_status.duration * 1_000;
                            let target_position_milliseconds = (position * duration_milliseconds as f64) as i64;
                            let current_position_milliseconds = (current_status.position * duration_milliseconds as f64) as i64;
                            let seek_offset = target_position_milliseconds - current_position_milliseconds;
                            
                            eprintln!("[dragon-helper] Seeking (Dragon Player): current={}ms, target={}ms, offset={}ms", 
                                     current_position_milliseconds, target_position_milliseconds, seek_offset);
                            
                            // Execute seek command with offset
                            let success = execute_dragon_player_command("org.mpris.MediaPlayer2.Player.Seek", &[&format!("int64:{}", seek_offset)]);
                            
                            if success {
                                // IMMEDIATELY send status update to move the header
                                // This prevents the delay and makes the UI responsive
                                let mut updated_status = current_status;
                                updated_status.position = position;
                                
                                // Send immediate update to move header
                                send_status_update(status_sender, &updated_status);
                                eprintln!("[dragon-helper] Header updated immediately to position: {:.2}%", position * 100.0);
                            } else {
                                eprintln!("[dragon-helper] Seek command failed");
                            }
                        } else {
                            eprintln!("[dragon-helper] Failed to get current status for seek");
                        }
                    } else {
                        // Store this seek for later execution
                        unsafe {
                            PENDING_SEEK = Some(position);
                        }
                        eprintln!("[dragon-helper] Seek throttled: position {} (too soon after last seek, will execute later)", position);
                        
                        // Still update header immediately for visual feedback
                        if let Some(current_status) = get_dragon_player_status() {
                            let mut updated_status = current_status;
                            updated_status.position = position;
                            send_status_update(status_sender, &updated_status);
                            eprintln!("[dragon-helper] Header updated immediately (throttled seek: {:.2}%)", position * 100.0);
                        }
                    }
                } else {
                    eprintln!("[dragon-helper] Invalid seek position: {}", position_str);
                }
            }
        }
        cmd if cmd.starts_with("set_position:") => {
            if let Some(position_str) = cmd.strip_prefix("set_position:") {
                if let Ok(position) = position_str.parse::<f64>() {
                    eprintln!("[dragon-helper] Executing set position command to: {}", position);
                    
                    // Use Seek method instead of SetPosition - more reliable
                    if let Some(current_status) = get_dragon_player_status() {
                        let duration_milliseconds = current_status.duration * 1_000;
                        let target_position_milliseconds = (position * duration_milliseconds as f64) as i64;
                        let current_position_milliseconds = (current_status.position * duration_milliseconds as f64) as i64;
                        let seek_offset = target_position_milliseconds - current_position_milliseconds;
                        
                        eprintln!("[dragon-helper] Set position (Dragon Player): current={}ms, target={}ms, offset={}ms", 
                                 current_position_milliseconds, target_position_milliseconds, seek_offset);
                        
                        // Execute seek command with offset
                        let success = execute_dragon_player_command("org.mpris.MediaPlayer2.Player.Seek", &[&format!("int64:{}", seek_offset)]);
                        
                        if success {
                            // IMMEDIATELY send status update to move the header
                            let mut updated_status = current_status;
                            updated_status.position = position;
                            send_status_update(status_sender, &updated_status);
                            eprintln!("[dragon-helper] Header updated immediately to position: {:.2}%", position * 100.0);
                        } else {
                            eprintln!("[dragon-helper] Set position command failed");
                        }
                    } else {
                        eprintln!("[dragon-helper] Failed to get current status for set position");
                    }
                } else {
                    eprintln!("[dragon-helper] Invalid set position: {}", position_str);
                }
            }
        }
        _ => {
            eprintln!("[dragon-helper] Unknown command: {}", command);
        }
    }
    
    // Process any pending seek if enough time has passed
    unsafe {
        if let Some(pending_position) = PENDING_SEEK {
            if let Some(last_seek) = LAST_SEEK_TIME {
                let now = std::time::Instant::now();
                if now.duration_since(last_seek).as_millis() >= MIN_SEEK_INTERVAL as u128 {
                    eprintln!("[dragon-helper] Processing pending seek to position: {}", pending_position);
                    
                    // Execute the pending seek
                    if let Some(current_status) = get_dragon_player_status() {
                        let duration_milliseconds = current_status.duration * 1_000;
                        let target_position_milliseconds = (pending_position * duration_milliseconds as f64) as i64;
                        let current_position_milliseconds = (current_status.position * duration_milliseconds as f64) as i64;
                        let seek_offset = target_position_milliseconds - current_position_milliseconds;
                        
                        eprintln!("[dragon-helper] Executing pending seek (Dragon Player): current={}ms, target={}ms, offset={}ms", 
                                 current_position_milliseconds, target_position_milliseconds, seek_offset);
                        
                        let success = execute_dragon_player_command("org.mpris.MediaPlayer2.Player.Seek", &[&format!("int64:{}", seek_offset)]);
                        
                        if success {
                            eprintln!("[dragon-helper] Pending seek executed successfully to position: {:.2}%", pending_position * 100.0);
                            LAST_SEEK_TIME = Some(now);
                            PENDING_SEEK = None;
                        } else {
                            eprintln!("[dragon-helper] Pending seek failed");
                        }
                    }
                }
            }
        }
    }
}

fn monitor_media_player_events(status_sender: Arc<Mutex<Option<UnixStream>>>) {
    // Pure event-driven approach with minimal position polling only during playback
    
    // Check initial media player status
    if is_media_player_running() {
        if let Some(initial_status) = get_media_player_status() {
            send_status_update(&status_sender, &initial_status);
            
            let player_name = get_current_media_player_instance()
                .map(|inst| inst.window_class.clone())
                .unwrap_or_else(|| "unknown".to_string());
            
            eprintln!("[{}] Initial status detected: playing={}, position={:.2}%", 
                     player_name, initial_status.is_playing, initial_status.position * 100.0);
        }
    }
    
    // Create a shared state for playback status to coordinate between threads
    let playback_state = Arc::new(Mutex::new((false, MediaStatus::empty())));
    let playback_state_clone = playback_state.clone();
    
    // Initialize shared state with current status if available
    if let Some(current_status) = get_media_player_status() {
        if let Ok(mut state) = playback_state.lock() {
            state.0 = current_status.is_playing;
            state.1 = current_status.clone();
        }
        
        let player_name = get_current_media_player_instance()
            .map(|inst| inst.window_class.clone())
            .unwrap_or_else(|| "unknown".to_string());
        
        eprintln!("[{}] Shared state initialized: playing={}, position={:.2}%", 
                 player_name, current_status.is_playing, current_status.position * 100.0);
    }
    
    // Clone status_sender for position updates
    let position_sender = status_sender.clone();
    
    // Start simple position polling thread - ONLY when playing=true
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
                // Get current position and send update
                if let Some(status) = get_media_player_status() {
                    if status.is_playing && status.duration > 0 {
                        send_status_update(&position_sender, &status);
                        
                        let player_name = get_current_media_player_instance()
                            .map(|inst| inst.window_class.clone())
                            .unwrap_or_else(|| "unknown".to_string());
                        
                        eprintln!("[{}] Position polling update: {:.2}%", 
                                 player_name, status.position * 100.0);
                    }
                }
                
                // Poll every 1 second when playing
                thread::sleep(Duration::from_millis(1000));
            } else {
                // Not playing - sleep longer and wait for events
                thread::sleep(Duration::from_millis(500));
            }
        }
    });
    
    // Start event-driven DBus monitoring
    let status_sender_clone = status_sender.clone();
    let playback_state_clone = playback_state.clone();
    thread::spawn(move || {
        if let Err(e) = run_dbus_event_monitor(status_sender_clone, playback_state_clone) {
            eprintln!("[media-helper] DBus event monitor failed: {}, restarting...", e);
            thread::sleep(Duration::from_millis(1000));
            monitor_media_player_events(status_sender);
        }
    });
}

fn run_dbus_event_monitor(
    status_sender: Arc<Mutex<Option<UnixStream>>>, 
    playback_state: Arc<Mutex<(bool, MediaStatus)>>
) -> Result<(), Box<dyn std::error::Error>> {
    // Get the current media player instance to determine which player we're monitoring
    let instance = get_current_media_player_instance();
    
    // Route to the appropriate player-specific DBus event monitor
    match instance.as_ref().map(|inst| inst.window_class.as_str()) {
        Some("vlc") => {
            eprintln!("[media-helper] Routing to VLC-specific DBus event monitor");
            run_vlc_dbus_event_monitor(status_sender, playback_state)
        }
        Some("org.kde.dragonplayer") => {
            eprintln!("[media-helper] Routing to Dragon Player-specific DBus event monitor");
            run_dragon_player_dbus_event_monitor(status_sender, playback_state)
        }
        _ => {
            eprintln!("[media-helper] Unknown media player class, defaulting to VLC DBus event monitor");
            run_vlc_dbus_event_monitor(status_sender, playback_state)
        }
    }
}

fn run_vlc_dbus_event_monitor(
    status_sender: Arc<Mutex<Option<UnixStream>>>, 
    playback_state: Arc<Mutex<(bool, MediaStatus)>>
) -> Result<(), Box<dyn std::error::Error>> {
    // Create async runtime for zbus
    let rt = tokio::runtime::Runtime::new()?;
    
    rt.block_on(async {
        let connection = Connection::session().await?;
        let mut stream = MessageStream::from(&connection);
        let dbus_proxy = DBusProxy::new(&connection).await?;
        
        // Subscribe to MPRIS signals specifically for VLC
        let rules = vec![
            // PropertiesChanged signals for playback status, position, metadata
            MatchRule::builder()
                .msg_type(MessageType::Signal)
                .interface("org.freedesktop.DBus.Properties")?
                .path_namespace("/org/mpris/MediaPlayer2")?
                .member("PropertiesChanged")?
                .build(),
            // Seeked signals for position changes
            MatchRule::builder()
                .msg_type(MessageType::Signal)
                .interface("org.mpris.MediaPlayer2.Player")?
                .member("Seeked")?
                .build(),
            // NameOwnerChanged signals for VLC service appearance/disappearance
            MatchRule::builder()
                .msg_type(MessageType::Signal)
                .interface("org.freedesktop.DBus")?
                .member("NameOwnerChanged")?
                .arg0ns("org.mpris.MediaPlayer2.vlc")?
                .build(),
        ];
        
        // Add all match rules
        for rule in rules {
            if let Err(e) = dbus_proxy.add_match_rule(rule).await {
                eprintln!("[vlc-helper] Failed to add VLC match rule: {}", e);
            }
        }
        
        eprintln!("[vlc-helper] VLC-specific DBus signal subscription active");
        
        // Process incoming DBus messages
        while let Some(msg) = stream.next().await {
            let msg = msg?;
            let header = msg.header()?;
            
            // Only process signal messages
            if msg.message_type() != MessageType::Signal {
                continue;
            }
            
            if let (Some(interface), Some(member)) = (header.interface()?, header.member()?) {
                let interface_str = interface.as_str();
                let member_str = member.as_str();
                
                eprintln!("[vlc-helper] Received VLC DBus signal: {}.{}", interface_str, member_str);
                
                match (interface_str, member_str) {
                    ("org.freedesktop.DBus.Properties", "PropertiesChanged") => {
                        // Handle PropertiesChanged signal for VLC
                        if let Ok((interface_name, changed_props, _invalidated_props)) = 
                            msg.body::<(String, std::collections::HashMap<String, zbus::zvariant::Value>, Vec<String>)>() {
                            
                            if interface_name == "org.mpris.MediaPlayer2.Player" {
                                eprintln!("[vlc-helper] VLC player properties changed: {:?}", changed_props);
                                // Process the changed properties for VLC
                                process_vlc_properties_changed_signal_dbus(changed_props, &status_sender, &playback_state);
                            }
                        }
                    }
                    ("org.mpris.MediaPlayer2.Player", "Seeked") => {
                        // Handle Seeked signal for VLC
                        if let Ok(position) = msg.body::<i64>() {
                            eprintln!("[vlc-helper] VLC seeked to position: {} microseconds", position);
                            process_vlc_seeked_signal_dbus(position, &status_sender, &playback_state);
                        }
                    }
                    ("org.freedesktop.DBus", "NameOwnerChanged") => {
                        // Handle NameOwnerChanged signal for VLC
                        if let Ok((name, old_owner, new_owner)) = 
                            msg.body::<(String, String, String)>() {
                            
                            if name.starts_with("org.mpris.MediaPlayer2.vlc") {
                                eprintln!("[vlc-helper] VLC service changed: {} (old: {}, new: {})", name, old_owner, new_owner);
                                process_vlc_name_owner_changed_signal_dbus(&name, &old_owner, &new_owner, &status_sender, &playback_state);
                            }
                        }
                    }
                    _ => {
                        // Other signals - log for debugging
                        if let Ok(body) = msg.body::<String>() {
                            eprintln!("[vlc-helper] Unhandled VLC signal: {}.{} - {}", interface_str, member_str, body);
                        }
                    }
                }
            }
        }
        
        Ok::<(), zbus::Error>(())
    })?;
    
    Ok(())
}

fn run_dragon_player_dbus_event_monitor(
    status_sender: Arc<Mutex<Option<UnixStream>>>, 
    playback_state: Arc<Mutex<(bool, MediaStatus)>>
) -> Result<(), Box<dyn std::error::Error>> {
    // Create async runtime for zbus
    let rt = tokio::runtime::Runtime::new()?;
    
    rt.block_on(async {
        let connection = Connection::session().await?;
        let mut stream = MessageStream::from(&connection);
        let dbus_proxy = DBusProxy::new(&connection).await?;
        
        // Subscribe to MPRIS signals specifically for Dragon Player
        let rules = vec![
            // PropertiesChanged signals for playback status, position, metadata
            MatchRule::builder()
                .msg_type(MessageType::Signal)
                .interface("org.freedesktop.DBus.Properties")?
                .path_namespace("/org/mpris/MediaPlayer2")?
                .member("PropertiesChanged")?
                .build(),
            // Seeked signals for position changes
            MatchRule::builder()
                .msg_type(MessageType::Signal)
                .interface("org.mpris.MediaPlayer2.Player")?
                .member("Seeked")?
                .build(),
            // NameOwnerChanged signals for Dragon Player service appearance/disappearance
            MatchRule::builder()
                .msg_type(MessageType::Signal)
                .interface("org.freedesktop.DBus")?
                .member("NameOwnerChanged")?
                .arg0ns("org.mpris.MediaPlayer2.dragonplayer")?
                .build(),
        ];
        
        // Add all match rules
        for rule in rules {
            if let Err(e) = dbus_proxy.add_match_rule(rule).await {
                eprintln!("[dragon-helper] Failed to add Dragon Player match rule: {}", e);
            }
        }
        
        eprintln!("[dragon-helper] Dragon Player-specific DBus signal subscription active");
        
        // Process incoming DBus messages
        while let Some(msg) = stream.next().await {
            let msg = msg?;
            let header = msg.header()?;
            
            // Only process signal messages
            if msg.message_type() != MessageType::Signal {
                continue;
            }
            
            if let (Some(interface), Some(member)) = (header.interface()?, header.member()?) {
                let interface_str = interface.as_str();
                let member_str = member.as_str();
                
                eprintln!("[dragon-helper] Received Dragon Player DBus signal: {}.{}", interface_str, member_str);
                
                match (interface_str, member_str) {
                    ("org.freedesktop.DBus.Properties", "PropertiesChanged") => {
                        // Handle PropertiesChanged signal for Dragon Player
                        if let Ok((interface_name, changed_props, _invalidated_props)) = 
                            msg.body::<(String, std::collections::HashMap<String, zbus::zvariant::Value>, Vec<String>)>() {
                            
                            if interface_name == "org.mpris.MediaPlayer2.Player" {
                                eprintln!("[dragon-helper] Dragon Player properties changed: {:?}", changed_props);
                                // Process the changed properties for Dragon Player
                                process_dragon_player_properties_changed_signal_dbus(changed_props, &status_sender, &playback_state);
                            }
                        }
                    }
                    ("org.mpris.MediaPlayer2.Player", "Seeked") => {
                        // Handle Seeked signal for Dragon Player
                        if let Ok(position) = msg.body::<i64>() {
                            eprintln!("[dragon-helper] Dragon Player seeked to position: {} microseconds", position);
                            process_dragon_player_seeked_signal_dbus(position, &status_sender, &playback_state);
                        }
                    }
                    ("org.freedesktop.DBus", "NameOwnerChanged") => {
                        // Handle NameOwnerChanged signal for Dragon Player
                        if let Ok((name, old_owner, new_owner)) = 
                            msg.body::<(String, String, String)>() {
                            
                            if name.starts_with("org.mpris.MediaPlayer2.dragonplayer") {
                                eprintln!("[dragon-helper] Dragon Player service changed: {} (old: {}, new: {})", name, old_owner, new_owner);
                                process_dragon_player_name_owner_changed_signal_dbus(&name, &old_owner, &new_owner, &status_sender, &playback_state);
                            }
                        }
                    }
                    _ => {
                        // Other signals - log for debugging
                        if let Ok(body) = msg.body::<String>() {
                            eprintln!("[dragon-helper] Unhandled Dragon Player signal: {}.{} - {}", interface_str, member_str, body);
                        }
                    }
                }
            }
        }
        
        Ok::<(), zbus::Error>(())
    })?;
    
    Ok(())
}

fn process_vlc_properties_changed_signal_dbus(
    changed_props: std::collections::HashMap<String, zbus::zvariant::Value>, 
    status_sender: &Arc<Mutex<Option<UnixStream>>>,
    playback_state: &Arc<Mutex<(bool, MediaStatus)>>
) {
    // Process changed properties from DBus signal for VLC
    for (prop_name, prop_value) in changed_props {
        match prop_name.as_str() {
            "PlaybackStatus" => {
                if let Some(status_str) = prop_value.downcast::<String>() {
                    let is_playing = status_str == "Playing";
                    eprintln!("[vlc-helper] VLC playback status changed to: {}", status_str);
                    
                    // Get current VLC status and update
                    if let Some(mut status) = get_vlc_status() {
                        status.is_playing = is_playing;
                        
                        // Update shared playback state
                        if let Ok(mut state) = playback_state.lock() {
                            state.0 = is_playing;
                            state.1 = status.clone();
                        }
                        
                        send_status_update(status_sender, &status);
                        
                        if is_playing {
                            eprintln!("[vlc-helper] VLC playback started - position polling activated");
                        } else {
                            eprintln!("[vlc-helper] VLC playback stopped - position polling deactivated");
                        }
                    }
                }
            }
            "Position" => {
                if let Some(position) = prop_value.downcast::<i64>() {
                    eprintln!("[vlc-helper] VLC position changed to: {} microseconds", position);
                    
                    // Get current VLC status and update position immediately
                    if let Some(mut status) = get_vlc_status() {
                        let duration = status.duration * 1_000_000; // Convert to microseconds
                        status.position = if duration > 0 { position as f64 / duration as f64 } else { 0.0 };
                        
                        // Update shared playback state
                        if let Ok(mut state) = playback_state.lock() {
                            state.1 = status.clone();
                        }
                        
                        // Send immediate update for instant header movement
                        send_status_update(status_sender, &status);
                        eprintln!("[vlc-helper] VLC position updated via DBus signal: {:.2}% (immediate)", status.position * 100.0);
                    }
                }
            }
            "Metadata" => {
                eprintln!("[vlc-helper] VLC metadata changed");
                
                // Get updated VLC status with new metadata
                if let Some(status) = get_vlc_status() {
                    // Update shared playback state
                    if let Ok(mut state) = playback_state.lock() {
                        state.1 = status.clone();
                    }
                    
                    send_status_update(status_sender, &status);
                }
            }
            _ => {
                eprintln!("[vlc-helper] VLC property changed: {} = {:?}", prop_name, prop_value);
            }
        }
    }
}

fn process_vlc_seeked_signal_dbus(
    position: i64, 
    status_sender: &Arc<Mutex<Option<UnixStream>>>,
    playback_state: &Arc<Mutex<(bool, MediaStatus)>>
) {
    // Process seeked signal from DBus for VLC - use the position directly from the signal
    // This avoids an extra DBus call and makes the response immediate
    
    // Get current VLC status to get duration and other metadata
    if let Some(mut status) = get_vlc_status() {
        let duration = status.duration * 1_000_000; // Convert to microseconds
        status.position = if duration > 0 { position as f64 / duration as f64 } else { 0.0 };
        
        // Update shared playback state
        if let Ok(mut state) = playback_state.lock() {
            state.1 = status.clone();
        }
        
        eprintln!("[vlc-helper] VLC seeked to position: {:.2}% (from DBus signal)", status.position * 100.0);
        send_status_update(status_sender, &status);
    }
}

fn process_vlc_name_owner_changed_signal_dbus(
    name: &str, 
    _old_owner: &str, 
    new_owner: &str, 
    status_sender: &Arc<Mutex<Option<UnixStream>>>,
    playback_state: &Arc<Mutex<(bool, MediaStatus)>>
) {
    // Process name owner changed signal from DBus for VLC
    let vlc_running = !new_owner.is_empty();
    
    if vlc_running {
        eprintln!("[vlc-helper] VLC service appeared: {}", name);
        // Get initial VLC status
        if let Some(status) = get_vlc_status() {
            // Update shared playback state
            if let Ok(mut state) = playback_state.lock() {
                state.0 = status.is_playing;
                state.1 = status.clone();
            }
            
            send_status_update(status_sender, &status);
        }
    } else {
        eprintln!("[vlc-helper] VLC service disappeared: {}", name);
        // Send empty status and update shared state
        let empty_status = MediaStatus::empty();
        
        if let Ok(mut state) = playback_state.lock() {
            state.0 = false;
            state.1 = empty_status.clone();
        }
        
        send_status_update(status_sender, &empty_status);
    }
}

fn process_dragon_player_properties_changed_signal_dbus(
    changed_props: std::collections::HashMap<String, zbus::zvariant::Value>, 
    status_sender: &Arc<Mutex<Option<UnixStream>>>,
    playback_state: &Arc<Mutex<(bool, MediaStatus)>>
) {
    // Process changed properties from DBus signal for Dragon Player
    for (prop_name, prop_value) in changed_props {
        match prop_name.as_str() {
            "PlaybackStatus" => {
                if let Some(status_str) = prop_value.downcast::<String>() {
                    let is_playing = status_str == "Playing";
                    eprintln!("[dragon-helper] Dragon Player playback status changed to: {}", status_str);
                    
                    // Get current Dragon Player status and update
                    if let Some(mut status) = get_dragon_player_status() {
                        status.is_playing = is_playing;
                        
                        // Update shared playback state
                        if let Ok(mut state) = playback_state.lock() {
                            state.0 = is_playing;
                            state.1 = status.clone();
                        }
                        
                        send_status_update(status_sender, &status);
                        
                        if is_playing {
                            eprintln!("[dragon-helper] Dragon Player playback started - position polling activated");
                        } else {
                            eprintln!("[dragon-helper] Dragon Player playback stopped - position polling deactivated");
                        }
                    }
                }
            }
            "Position" => {
                if let Some(position) = prop_value.downcast::<i64>() {
                    eprintln!("[dragon-helper] Dragon Player position changed to: {} milliseconds", position);
                    
                    // Get current Dragon Player status and update position immediately
                    if let Some(mut status) = get_dragon_player_status() {
                        let duration = status.duration * 1_000; // Convert to milliseconds
                        status.position = if duration > 0 { position as f64 / duration as f64 } else { 0.0 };
                        
                        // Update shared playback state
                        if let Ok(mut state) = playback_state.lock() {
                            state.1 = status.clone();
                        }
                        
                        // Send immediate update for instant header movement
                        send_status_update(status_sender, &status);
                        eprintln!("[dragon-helper] Dragon Player position updated via DBus signal: {:.2}% (immediate)", status.position * 100.0);
                    }
                }
            }
            "Metadata" => {
                eprintln!("[dragon-helper] Dragon Player metadata changed");
                
                // Get updated Dragon Player status with new metadata
                if let Some(status) = get_dragon_player_status() {
                    // Update shared playback state
                    if let Ok(mut state) = playback_state.lock() {
                        state.1 = status.clone();
                    }
                    
                    send_status_update(status_sender, &status);
                }
            }
            _ => {
                eprintln!("[dragon-helper] Dragon Player property changed: {} = {:?}", prop_name, prop_value);
            }
        }
    }
}

fn process_dragon_player_seeked_signal_dbus(
    position: i64, 
    status_sender: &Arc<Mutex<Option<UnixStream>>>,
    playback_state: &Arc<Mutex<(bool, MediaStatus)>>
) {
    // Process seeked signal from DBus for Dragon Player - use the position directly from the signal
    // This avoids an extra DBus call and makes the response immediate
    
    // Get current Dragon Player status to get duration and other metadata
    if let Some(mut status) = get_dragon_player_status() {
        let duration = status.duration * 1_000; // Dragon Player uses milliseconds, convert to milliseconds
        status.position = if duration > 0 { position as f64 / duration as f64 } else { 0.0 };
        
        // Update shared playback state
        if let Ok(mut state) = playback_state.lock() {
            state.1 = status.clone();
        }
        
        eprintln!("[dragon-helper] Dragon Player seeked to position: {:.2}% (from DBus signal)", status.position * 100.0);
        send_status_update(status_sender, &status);
    }
}

fn process_dragon_player_name_owner_changed_signal_dbus(
    name: &str, 
    _old_owner: &str, 
    new_owner: &str, 
    status_sender: &Arc<Mutex<Option<UnixStream>>>,
    playback_state: &Arc<Mutex<(bool, MediaStatus)>>
) {
    // Process name owner changed signal from DBus for Dragon Player
    let dragon_player_running = !new_owner.is_empty();
    
    if dragon_player_running {
        eprintln!("[dragon-helper] Dragon Player service appeared: {}", name);
        // Get initial Dragon Player status
        if let Some(status) = get_dragon_player_status() {
            // Update shared playback state
            if let Ok(mut state) = playback_state.lock() {
                state.0 = status.is_playing;
                state.1 = status.clone();
            }
            
            send_status_update(status_sender, &status);
        }
    } else {
        eprintln!("[dragon-helper] Dragon Player service disappeared: {}", name);
        // Send empty status and update shared state
        let empty_status = MediaStatus::empty();
        
        if let Ok(mut state) = playback_state.lock() {
            state.0 = false;
            state.1 = empty_status.clone();
        }
        
        send_status_update(status_sender, &empty_status);
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
                eprintln!("[vlc-helper] Failed to send status update: {}", e);
            }
        }
    }
}

fn main() -> std::io::Result<()> {
    let socket_path = "/tmp/touchbar-vlc.sock";
    
    // Print environment info for debugging
    if let Ok(addr) = std::env::var("DBUS_SESSION_BUS_ADDRESS") {
        eprintln!("[vlc-helper] DBUS_SESSION_BUS_ADDRESS={}", addr);
    } else {
        eprintln!("[vlc-helper] DBUS_SESSION_BUS_ADDRESS is not set");
    }
    
    // Get the window class, ID, and PID from environment variables
    if let Ok(window_class) = std::env::var("TINY_DFR_WINDOW_CLASS") {
        eprintln!("[vlc-helper] Window class: {}", window_class);
        
        // Get window PID for instance matching
        let window_pid = std::env::var("TINY_DFR_WINDOW_PID")
            .ok()
            .and_then(|pid_str| pid_str.parse::<u32>().ok());
        
        if let Some(pid) = window_pid {
            eprintln!("[vlc-helper] Window PID: {}", pid);
        }
        
        set_current_media_player(&window_class, window_pid);
        
        // Also read window ID for future use
        if let Ok(window_id_str) = std::env::var("TINY_DFR_WINDOW_ID") {
            if let Ok(window_id) = window_id_str.parse::<u64>() {
                eprintln!("[vlc-helper] Window ID: {}", window_id);
                // Store window ID for future use (you can add a global variable here if needed)
            } else {
                eprintln!("[vlc-helper] Invalid window ID format: {}", window_id_str);
            }
        } else {
            eprintln!("[vlc-helper] TINY_DFR_WINDOW_ID is not set");
        }
    } else {
        eprintln!("[vlc-helper] TINY_DFR_WINDOW_CLASS is not set");
    }
    
    let stream = loop {
        match UnixStream::connect(socket_path) {
            Ok(stream) => {
                let stream = stream;
                stream.set_nonblocking(true)?;
                break stream;
            }
            Err(_) => {
                // Add small delay to prevent busy-waiting during connection attempts
                // This prevents the helper from consuming 100% CPU when the main app is not ready
                thread::sleep(Duration::from_millis(10));
                continue;
            }
        }
    };
    
    eprintln!("[vlc-helper] Connected to socket, starting VLC monitoring...");
    
    // Create a reader for incoming commands
    let mut stream_clone = stream.try_clone()?;
    let mut buffer = Vec::new();
    
    // Create a shared sender for status updates
    let status_sender = Arc::new(Mutex::new(Some(stream)));
    
    // Start event monitoring in a separate thread
    let status_sender_clone = status_sender.clone();
    thread::spawn(move || {
        monitor_media_player_events(status_sender_clone);
    });
    
    loop {
        // Event-driven command processing (non-blocking)
        let mut temp_buffer = [0u8; 1024];
        match stream_clone.read(&mut temp_buffer) {
            Ok(0) => {
                // EOF - connection closed
                eprintln!("[vlc-helper] Connection closed by main process");
                break;
            }
            Ok(n) => {
                // Process incoming data immediately (event-driven)
                buffer.extend_from_slice(&temp_buffer[..n]);
                
                // Process complete lines as they arrive
                while let Some(newline_pos) = buffer.iter().position(|&b| b == b'\n') {
                    let line_data = buffer.drain(..=newline_pos).collect::<Vec<_>>();
                    let line = String::from_utf8_lossy(&line_data[..line_data.len()-1]); // Remove newline
                    let command = line.trim();
                    if !command.is_empty() {
                        eprintln!("[vlc-helper] Received command: {}", command);
                        // Execute command immediately (event-driven)
                        handle_command(command, &status_sender);
                    }
                }
            }
            Err(e) => {
                if e.kind() == std::io::ErrorKind::WouldBlock {
                    // No data available - add small sleep to prevent busy-waiting
                    // This prevents 100% CPU usage and freezing when switching modules
                    thread::sleep(Duration::from_millis(1));
                    continue;
                } else {
                    eprintln!("[vlc-helper] Error reading from socket: {}", e);
                    break;
                }
            }
        }
    }
    
    Ok(())
} 