//! VLC helper binary for tiny-dfr, providing VLC status via DBus.
//! 
//! This helper process:
//! 1. Connects to the main process via Unix socket
//! 2. Monitors VLC via DBus and sends status updates to main process
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
use std::io::{Write, Read};
use std::thread;
use std::time::Duration;
use std::process::Command;
use serde_json::json;

fn get_vlc_status() -> Option<serde_json::Value> {
    // Use dbus-send to get VLC status
    let output = Command::new("dbus-send")
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
    
    let status_output = String::from_utf8_lossy(&output.stdout);
    let is_playing = status_output.contains("Playing");

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
    
    let title = metadata_text.lines()
        .find(|line| line.contains("xesam:title"))
        .and_then(|line| line.split('"').nth(1))
        .unwrap_or("Unknown");
    
    let artist = metadata_text.lines()
        .find(|line| line.contains("xesam:artist"))
        .and_then(|line| line.split('"').nth(1))
        .unwrap_or("Unknown");

    // Get duration from metadata - look for "mpris:length" followed by "int64"
    let duration = metadata_text.lines()
        .find(|line| line.contains("mpris:length"))
        .and_then(|_line| {
            // Find the next line that contains "int64"
            let lines: Vec<&str> = metadata_text.lines().collect();
            for (i, l) in lines.iter().enumerate() {
                if l.contains("mpris:length") {
                    // Look for the next line with int64
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
                    // Reduced debug logging
    
    Some(json!({
        "is_playing": is_playing,
        "position": progress,
        "title": title,
        "artist": artist,
        "duration": duration / 1000000 // Convert to seconds
    }))
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
                    
                    eprintln!("[vlc-helper] Executing seek command to position: {} (target time: {} microseconds, current: {}, offset: {}, duration: {})", position, target_time, current_pos, seek_offset, duration);
                    execute_vlc_command("org.mpris.MediaPlayer2.Player.Seek", &[&format!("int64:{}", seek_offset)]);
                } else {
                    eprintln!("[vlc-helper] Invalid seek position: {}", position_str);
                }
            }
        }
        cmd if cmd.starts_with("set_position:") => {
            if let Some(position_str) = cmd.strip_prefix("set_position:") {
                if let Ok(position) = position_str.parse::<f64>() {
                    let seek_position = (position * 1000000.0) as i64;
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

fn main() -> std::io::Result<()> {
    let socket_path = "/tmp/touchbar-vlc.sock";
    
    // Print environment info for debugging
    if let Ok(addr) = std::env::var("DBUS_SESSION_BUS_ADDRESS") {
        eprintln!("[vlc-helper] DBUS_SESSION_BUS_ADDRESS={}", addr);
    } else {
        eprintln!("[vlc-helper] DBUS_SESSION_BUS_ADDRESS is not set");
    }
    
    let mut stream = loop {
        match UnixStream::connect(socket_path) {
            Ok(stream) => {
                let mut stream = stream;
                stream.set_nonblocking(true)?;
                break stream;
            }
            Err(_) => {
                thread::sleep(Duration::from_millis(100));
                continue;
            }
        }
    };
    
    eprintln!("[vlc-helper] Connected to socket, starting VLC monitoring...");
    
    // Create a reader for incoming commands
    let mut stream_clone = stream.try_clone()?;
    let mut buffer = Vec::new();
    
    loop {
        // Check for incoming commands (non-blocking)
        let mut temp_buffer = [0u8; 1024];
        match stream_clone.read(&mut temp_buffer) {
            Ok(0) => {
                // EOF - connection closed
                eprintln!("[vlc-helper] Connection closed by main process");
                break;
            }
            Ok(n) => {
                // Append new data to buffer
                buffer.extend_from_slice(&temp_buffer[..n]);
                
                // Process complete lines
                while let Some(newline_pos) = buffer.iter().position(|&b| b == b'\n') {
                    let line_data = buffer.drain(..=newline_pos).collect::<Vec<_>>();
                    let line = String::from_utf8_lossy(&line_data[..line_data.len()-1]); // Remove newline
                    let command = line.trim();
                    if !command.is_empty() {
                        eprintln!("[vlc-helper] Received command: {}", command);
                        handle_command(command);
                    }
                }
            }
            Err(e) => {
                if e.kind() == std::io::ErrorKind::WouldBlock {
                    // No data available, continue with status update
                    // This is normal for non-blocking I/O
                } else {
                    eprintln!("[vlc-helper] Error reading from socket: {}", e);
                    break;
                }
            }
        }
        
        // Send status update
        if let Some(vlc_status) = get_vlc_status() {
            let status_json = vlc_status.to_string();
            if stream.write_all(format!("{}\n", status_json).as_bytes()).is_ok() {
                                    // Reduced debug logging
            } else {
                eprintln!("[vlc-helper] Failed to send VLC status, will retry");
                // Don't break, just continue and retry
            }
        } else {
            eprintln!("[vlc-helper] No VLC status available");
        }
        thread::sleep(Duration::from_millis(100)); // Update every 100ms for more responsive UI
    }
    
    Ok(())
} 