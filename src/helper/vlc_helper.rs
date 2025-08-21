//! VLC helper binary for tiny-dfr, providing VLC status via DBus.
//! 
//! This helper process:
//! 1. Connects to the main process via Unix socket
//! 2. Monitors VLC via DBus signals and sends status updates to main process
//! 3. Receives commands from main process and executes them on VLC
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
use std::io::{Write, Read, BufRead};
use std::thread;
use std::time::Duration;

use std::process::Command;
use std::sync::{Arc, Mutex};
use serde_json::json;


#[derive(Debug, Clone, PartialEq)]
struct VlcStatus {
    is_playing: bool,
    position: f64,
    title: String,
    artist: String,
    duration: i64,
}

impl VlcStatus {
    fn empty() -> Self {
        Self {
            is_playing: false,
            position: 0.0,
            title: String::new(),
            artist: String::new(),
            duration: 0,
        }
    }
}

fn is_vlc_running() -> bool {
    Command::new("dbus-send")
        .arg("--session")
        .arg("--dest=org.mpris.MediaPlayer2.vlc")
        .arg("--type=method_call")
        .arg("--print-reply")
        .arg("/org/mpris/MediaPlayer2")
        .arg("org.freedesktop.DBus.Properties.Get")
        .arg("string:org.mpris.MediaPlayer2.Player")
        .arg("string:PlaybackStatus")
        .output()
        .map(|output| output.status.success())
        .unwrap_or(false)
}

fn get_vlc_status() -> Option<VlcStatus> {
    // Get playback status
    let status_output = Command::new("dbus-send")
        .arg("--session")
        .arg("--dest=org.mpris.MediaPlayer2.vlc")
        .arg("--type=method_call")
        .arg("--print-reply")
        .arg("/org/mpris/MediaPlayer2")
        .arg("org.freedesktop.DBus.Properties.Get")
        .arg("string:org.mpris.MediaPlayer2.Player")
        .arg("string:PlaybackStatus")
        .output()
        .ok()?;
    
    if !status_output.status.success() {
        return None;
    }
    
    let status_text = String::from_utf8_lossy(&status_output.stdout);
    let is_playing = status_text.contains("Playing");

    // Get position
    let position_output = Command::new("dbus-send")
        .arg("--session")
        .arg("--dest=org.mpris.MediaPlayer2.vlc")
        .arg("--type=method_call")
        .arg("--print-reply")
        .arg("/org/mpris/MediaPlayer2")
        .arg("org.freedesktop.DBus.Properties.Get")
        .arg("string:org.mpris.MediaPlayer2.Player")
        .arg("string:Position")
        .output()
        .ok()?;
    
    let position_text = String::from_utf8_lossy(&position_output.stdout);
    let position = position_text.lines()
        .find(|line| line.contains("int64"))
        .and_then(|line| line.split_whitespace().last())
        .and_then(|s| s.parse::<i64>().ok())
        .unwrap_or(0);

    // Get metadata
    let metadata_output = Command::new("dbus-send")
        .arg("--session")
        .arg("--dest=org.mpris.MediaPlayer2.vlc")
        .arg("--type=method_call")
        .arg("--print-reply")
        .arg("/org/mpris/MediaPlayer2")
        .arg("org.freedesktop.DBus.Properties.Get")
        .arg("string:org.mpris.MediaPlayer2.Player")
        .arg("string:Metadata")
        .output()
        .ok()?;
    
    let metadata_text = String::from_utf8_lossy(&metadata_output.stdout);
    
    // Extract title
    let title = metadata_text.lines()
        .find(|line| line.contains("xesam:title"))
        .and_then(|_line| {
            // Look for the next line with a quoted string
            let lines: Vec<&str> = metadata_text.lines().collect();
            for (i, l) in lines.iter().enumerate() {
                if l.contains("xesam:title") {
                    for j in (i+1)..lines.len() {
                        let trimmed = lines[j].trim();
                        if trimmed.starts_with('"') && trimmed.ends_with('"') {
                            return Some(trimmed[1..trimmed.len()-1].to_string());
                        }
                    }
                }
            }
            None
        })
        .unwrap_or_else(|| "Unknown".to_string());
    
    // Extract artist
    let artist = metadata_text.lines()
        .find(|line| line.contains("xesam:artist"))
        .and_then(|_line| {
            // Look for the next line with a quoted string
            let lines: Vec<&str> = metadata_text.lines().collect();
            for (i, l) in lines.iter().enumerate() {
                if l.contains("xesam:artist") {
                    for j in (i+1)..lines.len() {
                        let trimmed = lines[j].trim();
                        if trimmed.starts_with('"') && trimmed.ends_with('"') {
                            return Some(trimmed[1..trimmed.len()-1].to_string());
                        }
                    }
                }
            }
            None
        })
        .unwrap_or_else(|| "Unknown".to_string());

    // Get duration from metadata
    let duration = metadata_text.lines()
        .find(|line| line.contains("mpris:length"))
        .and_then(|_line| {
            let lines: Vec<&str> = metadata_text.lines().collect();
            for (i, l) in lines.iter().enumerate() {
                if l.contains("mpris:length") {
                    for j in (i+1)..lines.len() {
                        if lines[j].contains("int64") {
                            return lines[j].split_whitespace().last().and_then(|s| s.parse::<i64>().ok());
                        }
                    }
                }
            }
            None
        })
        .unwrap_or(0);
    
    // Calculate progress
    let progress = if duration > 0 { position as f64 / duration as f64 } else { 0.0 };
    
    Some(VlcStatus {
        is_playing,
        position: progress,
        title,
        artist,
        duration: duration / 1_000_000, // Convert to seconds
    })
}

fn execute_vlc_command(command: &str, args: &[&str]) -> bool {
    let mut cmd = Command::new("dbus-send");
    cmd.arg("--session")
       .arg("--dest=org.mpris.MediaPlayer2.vlc")
       .arg("--type=method_call")
       .arg("/org/mpris/MediaPlayer2")
       .arg(command);
    
    for arg in args {
        cmd.arg(arg);
    }
    
    match cmd.output() {
        Ok(output) => {
            if output.status.success() {
                eprintln!("[vlc-helper] Command '{}' executed successfully", command);
                true
            } else {
                eprintln!("[vlc-helper] Command '{}' failed: {:?}", command, String::from_utf8_lossy(&output.stderr));
                false
            }
        }
        Err(e) => {
            eprintln!("[vlc-helper] Failed to execute command '{}': {}", command, e);
            false
        }
    }
}

fn handle_command(command: &str) {
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
                    // Prevent seeking to exactly 0.0 or 1.0 to avoid VLC closing
                    if position <= 0.001 {
                        position = 0.001;
                    } else if position >= 0.999 {
                        position = 0.999;
                    }
                    
                    // Get current position and duration
                    let current_pos_output = Command::new("dbus-send")
                        .arg("--session")
                        .arg("--dest=org.mpris.MediaPlayer2.vlc")
                        .arg("--type=method_call")
                        .arg("--print-reply")
                        .arg("/org/mpris/MediaPlayer2")
                        .arg("org.freedesktop.DBus.Properties.Get")
                        .arg("string:org.mpris.MediaPlayer2.Player")
                        .arg("string:Position")
                        .output()
                        .ok();

                    let current_pos = current_pos_output
                        .and_then(|output| {
                            let text = String::from_utf8_lossy(&output.stdout);
                            text.lines()
                                .find(|line| line.contains("int64"))
                                .and_then(|line| line.split_whitespace().last())
                                .and_then(|s| s.parse::<i64>().ok())
                        })
                        .unwrap_or(0);

                    // Get duration
                    let duration_output = Command::new("dbus-send")
                        .arg("--session")
                        .arg("--dest=org.mpris.MediaPlayer2.vlc")
                        .arg("--type=method_call")
                        .arg("--print-reply")
                        .arg("/org/mpris/MediaPlayer2")
                        .arg("org.freedesktop.DBus.Properties.Get")
                        .arg("string:org.mpris.MediaPlayer2.Player")
                        .arg("string:Metadata")
                        .output()
                        .ok();

                    let duration = duration_output
                        .and_then(|output| {
                            let text = String::from_utf8_lossy(&output.stdout);
                            text.lines()
                                .find(|line| line.contains("mpris:length"))
                                .and_then(|_line| {
                                    let lines: Vec<&str> = text.lines().collect();
                                    for (i, l) in lines.iter().enumerate() {
                                        if l.contains("mpris:length") {
                                            for j in (i+1)..lines.len() {
                                                if lines[j].contains("int64") {
                                                    return lines[j].split_whitespace().last().and_then(|s| s.parse::<i64>().ok());
                                                }
                                            }
                                        }
                                    }
                                    None
                                })
                        })
                        .unwrap_or(0);

                    // Calculate the target time in microseconds
                    let target_time = (position * duration as f64) as i64;
                    
                    // Calculate the seek offset (difference from current position)
                    let seek_offset = target_time - current_pos;
                    
                    eprintln!("[vlc-helper] Executing seek command to position: {} (target time: {} microseconds, current: {}, offset: {}, duration: {})", 
                             position, target_time, current_pos, seek_offset, duration);
                    execute_vlc_command("org.mpris.MediaPlayer2.Player.Seek", &[&format!("int64:{}", seek_offset)]);
                } else {
                    eprintln!("[vlc-helper] Invalid seek position: {}", position_str);
                }
            }
        }
        cmd if cmd.starts_with("set_position:") => {
            if let Some(position_str) = cmd.strip_prefix("set_position:") {
                if let Ok(position) = position_str.parse::<f64>() {
                    let seek_position = (position * 1_000_000.0) as i64;
                    eprintln!("[vlc-helper] Executing set position command to: {}", position);
                    execute_vlc_command("org.mpris.MediaPlayer2.Player.SetPosition", &[&format!("objectpath:/org/mpris/MediaPlayer2/TrackList/NoTrack"), &format!("int64:{}", seek_position)]);
                } else {
                    eprintln!("[vlc-helper] Invalid set position: {}", position_str);
                }
            }
        }
        _ => {
            eprintln!("[vlc-helper] Unknown command: {}", command);
        }
    }
}

fn monitor_vlc_events(status_sender: Arc<Mutex<Option<UnixStream>>>) {
    // Event-driven monitoring with smart position polling
    // Only polls position during playback, stops when paused/stopped
    
    let mut current_status = VlcStatus::empty();
    let mut vlc_running = false;
    
    // Check initial VLC status
    if is_vlc_running() {
        vlc_running = true;
        if let Some(initial_status) = get_vlc_status() {
            current_status = initial_status.clone();
            send_status_update(&status_sender, &current_status);
            eprintln!("[vlc-helper] Initial VLC status detected: playing={}, title={}", current_status.is_playing, current_status.title);
        }
    }
    
    // Clone status_sender for position updates
    let position_sender = status_sender.clone();
    
    // Start smart position tracking thread
    thread::spawn(move || {
        let mut last_status = VlcStatus::empty();
        let mut last_update_time = std::time::Instant::now();
        
        loop {
            // Only poll if VLC is actually playing
            if last_status.is_playing && last_status.duration > 0 {
                let now = std::time::Instant::now();
                let elapsed = now.duration_since(last_update_time).as_secs_f64();
                
                if elapsed >= 0.5 { // Update every 500ms during playback only
                    // Get current position from VLC (minimal polling only during playback)
                    if let Some(status) = get_vlc_status() {
                        if status.is_playing && status.duration > 0 {
                            // Only update if position actually changed
                            if (status.position - last_status.position).abs() > 0.001 {
                                let position_percent = status.position * 100.0;
                                send_status_update(&position_sender, &status);
                                last_status = status;
                                eprintln!("[vlc-helper] Position updated: {:.2}%", position_percent);
                            }
                        } else {
                            // VLC stopped playing - stop polling and update status
                            send_status_update(&position_sender, &status);
                            last_status = status;
                            eprintln!("[vlc-helper] VLC stopped playing - polling stopped");
                        }
                    }
                    
                    last_update_time = now;
                }
            } else {
                // Not playing - no polling, just check occasionally for status changes
                if let Some(status) = get_vlc_status() {
                    if status != last_status {
                        let is_playing = status.is_playing;
                        send_status_update(&position_sender, &status);
                        last_status = status;
                        if is_playing {
                            eprintln!("[vlc-helper] VLC started playing - polling started");
                        }
                    }
                }
            }
            
            // Small sleep to prevent busy waiting
            thread::sleep(Duration::from_millis(100));
        }
    });
    
    // Use dbus-monitor to listen for VLC signals
    let mut dbus_monitor = match Command::new("dbus-monitor")
        .arg("--session")
        .arg("type='signal'")
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn() {
            Ok(child) => child,
            Err(e) => {
                eprintln!("[vlc-helper] Failed to start dbus-monitor: {}", e);
                return;
            }
        };
    
    let stdout = match dbus_monitor.stdout.take() {
        Some(stdout) => stdout,
        None => {
            eprintln!("[vlc-helper] Failed to get dbus-monitor stdout");
            return;
        }
    };
    
    let mut reader = std::io::BufReader::new(stdout);
    let mut line = String::new();
    let mut signal_buffer = Vec::new();
    let mut in_signal = false;
    let mut signal_sender = "";
    
    loop {
        line.clear();
        match reader.read_line(&mut line) {
            Ok(0) => {
                eprintln!("[vlc-helper] dbus-monitor closed");
                break;
            }
            Ok(_) => {
                let trimmed = line.trim();
                
                // Detect signal start
                if trimmed.starts_with("signal") && trimmed.contains("org.mpris.MediaPlayer2.vlc") {
                    in_signal = true;
                    signal_buffer.clear();
                    
                    if trimmed.contains("PropertiesChanged") {
                        signal_sender = "PropertiesChanged";
                    } else if trimmed.contains("Seeked") {
                        signal_sender = "Seeked";
                    } else {
                        signal_sender = "other";
                    }
                    
                    signal_buffer.push(trimmed.to_string());
                    eprintln!("[vlc-helper] VLC signal detected: {}", signal_sender);
                }
                // Detect signal end
                else if in_signal && trimmed.is_empty() {
                    in_signal = false;
                    
                    // Process the complete signal
                    match signal_sender {
                        "PropertiesChanged" => {
                            process_properties_changed_signal(&signal_buffer, &mut current_status, &mut vlc_running, &status_sender);
                        }
                        "Seeked" => {
                            process_seeked_signal(&signal_buffer, &mut current_status, &status_sender);
                        }
                        _ => {
                            if signal_buffer.iter().any(|l| l.contains("NameOwnerChanged")) {
                                process_name_owner_changed_signal(&signal_buffer, &mut vlc_running, &status_sender);
                            }
                        }
                    }
                    
                    signal_buffer.clear();
                }
                // Collect signal lines
                else if in_signal {
                    signal_buffer.push(trimmed.to_string());
                }
                
                // Also check for any VLC-related activity and get status if needed
                if trimmed.contains("org.mpris.MediaPlayer2.vlc") && !vlc_running {
                    vlc_running = true;
                    if let Some(status) = get_vlc_status() {
                        current_status = status.clone();
                        send_status_update(&status_sender, &current_status);
                        eprintln!("[vlc-helper] VLC detected and status updated: playing={}, title={}", current_status.is_playing, current_status.title);
                    }
                }
                
                // If we see VLC activity but haven't received any status updates, try to get current status
                if trimmed.contains("org.mpris.MediaPlayer2.vlc") && vlc_running && current_status.title.is_empty() {
                    if let Some(status) = get_vlc_status() {
                        current_status = status.clone();
                        send_status_update(&status_sender, &current_status);
                        eprintln!("[vlc-helper] VLC status retrieved: playing={}, title={}", current_status.is_playing, current_status.title);
                    }
                }
            }
            Err(e) => {
                eprintln!("[vlc-helper] Error reading from dbus-monitor: {}", e);
                break;
            }
        }
    }
    
    eprintln!("[vlc-helper] Restarting dbus-monitor...");
    monitor_vlc_events(status_sender);
}

fn process_properties_changed_signal(signal_lines: &[String], current_status: &mut VlcStatus, _vlc_running: &mut bool, status_sender: &Arc<Mutex<Option<UnixStream>>>) {
    let signal_text = signal_lines.join("\n");
    eprintln!("[vlc-helper] Processing PropertiesChanged signal: {}", signal_text);
    
    let mut status_updated = false;
    
    // Extract changed properties from the signal
    if signal_text.contains("PlaybackStatus") {
        if let Some(status) = extract_playback_status(&signal_text) {
            let was_playing = current_status.is_playing;
            current_status.is_playing = status == "Playing";
            
            if current_status.is_playing && !was_playing {
                // Started playing - record start time and position
                // This is now handled by the smart polling thread
            } else if !current_status.is_playing && was_playing {
                // Stopped playing - clear start time
                // This is now handled by the smart polling thread
            }
            
            eprintln!("[vlc-helper] Playback status changed to: {}", status);
            status_updated = true;
        }
    }
    
    if signal_text.contains("Position") {
        if let Some(position) = extract_position(&signal_text) {
            let duration = current_status.duration * 1_000_000; // Convert to microseconds
            current_status.position = if duration > 0 { position as f64 / duration as f64 } else { 0.0 };
            
            // Update playback start time with new position
            // This is now handled by the smart polling thread
            
            eprintln!("[vlc-helper] Position changed to: {}", current_status.position);
            status_updated = true;
        }
    }
    
    if signal_text.contains("Metadata") {
        // Extract metadata changes
        if let Some(title) = extract_metadata_value(&signal_text, "xesam:title") {
            current_status.title = title;
            eprintln!("[vlc-helper] Title changed to: {}", current_status.title);
            status_updated = true;
        }
        
        if let Some(artist) = extract_metadata_value(&signal_text, "xesam:artist") {
            current_status.artist = artist;
            eprintln!("[vlc-helper] Artist changed to: {}", current_status.artist);
            status_updated = true;
        }
        
        if let Some(duration) = extract_metadata_value(&signal_text, "mpris:length") {
            if let Ok(duration_int) = duration.parse::<i64>() {
                current_status.duration = duration_int / 1_000_000; // Convert to seconds
                eprintln!("[vlc-helper] Duration changed to: {} seconds", current_status.duration);
                status_updated = true;
            }
        }
    }
    
    // If any property changed, send the updated status
    if status_updated {
        send_status_update(status_sender, current_status);
        eprintln!("[vlc-helper] Status updated and sent: playing={}, position={}, title={}", 
                 current_status.is_playing, current_status.position, current_status.title);
    }
}

fn process_seeked_signal(signal_lines: &[String], current_status: &mut VlcStatus, status_sender: &Arc<Mutex<Option<UnixStream>>>) {
    let signal_text = signal_lines.join("\n");
    
    // Extract position from Seeked signal
    if let Some(position) = extract_position(&signal_text) {
        let duration = current_status.duration * 1_000_000; // Convert to microseconds
        current_status.position = if duration > 0 { position as f64 / duration as f64 } else { 0.0 };
        
        // Update playback start time with new position
        // This is now handled by the smart polling thread
        
        eprintln!("[vlc-helper] Seeked to position: {}", current_status.position);
        send_status_update(status_sender, current_status);
    }
}

fn process_name_owner_changed_signal(signal_lines: &[String], vlc_running: &mut bool, status_sender: &Arc<Mutex<Option<UnixStream>>>) {
    let signal_text = signal_lines.join("\n");
    
    // Check if VLC service appeared or disappeared
    if signal_text.contains("org.mpris.MediaPlayer2.vlc") {
        let new_vlc_running = !signal_text.contains(":1.") || signal_text.contains(":1.") && !signal_text.contains("unix:abstract=");
        
        if new_vlc_running != *vlc_running {
            *vlc_running = new_vlc_running;
            eprintln!("[vlc-helper] VLC running state changed: {}", new_vlc_running);
            
            if !new_vlc_running {
                // VLC stopped, send empty status
                let empty_status = VlcStatus::empty();
                send_status_update(status_sender, &empty_status);
            }
        }
    }
}

fn extract_playback_status(signal_text: &str) -> Option<String> {
    // Parse the signal to extract PlaybackStatus value
    for line in signal_text.lines() {
        if line.contains("PlaybackStatus") {
            // Look for the next line with a quoted string
            let lines: Vec<&str> = signal_text.lines().collect();
            for (i, l) in lines.iter().enumerate() {
                if l.contains("PlaybackStatus") {
                    for j in (i+1)..lines.len() {
                        let trimmed = lines[j].trim();
                        if trimmed.starts_with('"') && trimmed.ends_with('"') {
                            return Some(trimmed[1..trimmed.len()-1].to_string());
                        }
                    }
                }
            }
        }
    }
    None
}

fn extract_position(signal_text: &str) -> Option<i64> {
    // Parse the signal to extract Position value
    for line in signal_text.lines() {
        if line.contains("Position") && line.contains("int64") {
            return line.split_whitespace().last().and_then(|s| s.parse::<i64>().ok());
        }
    }
    None
}

fn extract_metadata_value(signal_text: &str, key: &str) -> Option<String> {
    // Parse the signal to extract metadata values
    for line in signal_text.lines() {
        if line.contains(key) {
            // Look for the next line with a quoted string
            let lines: Vec<&str> = signal_text.lines().collect();
            for (i, l) in lines.iter().enumerate() {
                if l.contains(key) {
                    for j in (i+1)..lines.len() {
                        let trimmed = lines[j].trim();
                        if trimmed.starts_with('"') && trimmed.ends_with('"') {
                            return Some(trimmed[1..trimmed.len()-1].to_string());
                        }
                    }
                }
            }
        }
    }
    None
}

fn send_status_update(status_sender: &Arc<Mutex<Option<UnixStream>>>, status: &VlcStatus) {
    if let Ok(mut sender_guard) = status_sender.lock() {
        if let Some(ref mut stream) = *sender_guard {
            let status_json = json!({
                "is_playing": status.is_playing,
                "position": status.position,
                "title": status.title,
                "artist": status.artist,
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
    
    let stream = loop {
        match UnixStream::connect(socket_path) {
            Ok(stream) => {
                let stream = stream;
                stream.set_nonblocking(true)?;
                break stream;
            }
            Err(_) => {
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
        monitor_vlc_events(status_sender_clone);
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
                        handle_command(command);
                    }
                }
            }
            Err(e) => {
                if e.kind() == std::io::ErrorKind::WouldBlock {
                    // No data available, continue immediately (fully event-driven)
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