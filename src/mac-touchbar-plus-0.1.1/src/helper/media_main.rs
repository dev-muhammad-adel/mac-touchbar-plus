//! Main function for the media helper binary

use std::os::unix::net::UnixStream;
use std::io::Read;
use std::thread;
use std::time::Duration;
use std::sync::{Arc, Mutex};

mod vlc {
    include!("media/vlc.rs");
}

mod dragon_player {
    include!("media/dragon_player.rs");
}


mod smplayer {
    include!("media/smplayer.rs");
}

mod spotify {
    include!("media/spotify.rs");
}

// Import specific functions we need
use vlc::set_current_media_player as set_vlc_media_player;
use dragon_player::set_current_media_player as set_dragon_media_player;
use smplayer::set_current_media_player as set_smplayer_media_player;
use spotify::set_current_media_player as set_spotify_media_player;

fn main() -> std::io::Result<()> {
    let socket_path = "/tmp/touchbar-media.sock";
    
    // Print environment info for debugging
    if let Ok(addr) = std::env::var("DBUS_SESSION_BUS_ADDRESS") {
        eprintln!("[vlc-helper] DBUS_SESSION_BUS_ADDRESS={}", addr);
    } else {
        eprintln!("[vlc-helper] DBUS_SESSION_BUS_ADDRESS is not set");
    }
    
    // Get the window class, ID, and PID from environment variables
    let window_class = std::env::var("TINY_DFR_WINDOW_CLASS").unwrap_or_default();
    let window_pid = std::env::var("TINY_DFR_WINDOW_PID")
        .ok()
        .and_then(|pid_str| pid_str.parse::<u32>().ok());
    
    eprintln!("[media-helper] Window class: {}", window_class);
    if let Some(pid) = window_pid {
        eprintln!("[media-helper] Window PID: {}", pid);
    }
    
    // Determine which media player to use based on window class
    let window_class_lower = window_class.to_lowercase();
    let is_vlc = window_class_lower == "vlc";
    let is_dragon = window_class_lower == "org.kde.dragonplayer" || window_class_lower == "dragonplayer";
    let is_smplayer = window_class_lower == "smplayer";
    let is_spotify = window_class_lower == "spotify";
    
    if is_vlc {
        eprintln!("[media-helper] Detected VLC player");
    } else if is_dragon {
        eprintln!("[media-helper] Detected Dragon Player");
    } else if is_smplayer {
        eprintln!("[media-helper] Detected SMPlayer");
    } else if is_spotify {
        eprintln!("[media-helper] Detected Spotify");
    } else {
        eprintln!("[media-helper] Unknown player, defaulting to VLC");
    }
    
    // Set the current media player instance for the helper functions
    if is_vlc {
        set_vlc_media_player(&window_class, window_pid);
    } else if is_dragon {
        set_dragon_media_player(&window_class, window_pid);
    } else if is_smplayer {
        set_smplayer_media_player(&window_class, window_pid);
    } else if is_spotify {
        set_spotify_media_player(&window_class, window_pid);
    }
    
    // Also read window ID for future use
    if let Ok(window_id_str) = std::env::var("TINY_DFR_WINDOW_ID") {
        if let Ok(window_id) = window_id_str.parse::<u64>() {
            eprintln!("[media-helper] Window ID: {}", window_id);
        } else {
            eprintln!("[media-helper] Invalid window ID format: {}", window_id_str);
        }
    } else {
        eprintln!("[media-helper] TINY_DFR_WINDOW_ID is not set");
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
    
    eprintln!("[media-helper] Connected to socket, starting media player monitoring...");
    
    // Create a reader for incoming commands
    let mut stream_clone = stream.try_clone()?;
    let mut buffer = Vec::new();
    
    // Create a shared sender for status updates
    let status_sender = Arc::new(Mutex::new(Some(stream)));
    
    // Start event monitoring in a separate thread based on player type
    let status_sender_clone = status_sender.clone();
    thread::spawn(move || {
        if is_vlc {
            vlc::monitor_vlc_events(status_sender_clone);
        } else if is_dragon {
            dragon_player::monitor_dragon_player_events(status_sender_clone);
        } else if is_smplayer {
            smplayer::monitor_smplayer_events(status_sender_clone);
        } else if is_spotify {
            spotify::monitor_spotify_events(status_sender_clone);
        } else {
            // Default to VLC
            vlc::monitor_vlc_events(status_sender_clone);
        }
    });
    
    loop {
        // Event-driven command processing (non-blocking)
        let mut temp_buffer = [0u8; 1024];
        match stream_clone.read(&mut temp_buffer) {
            Ok(0) => {
                // EOF - connection closed
                eprintln!("[media-helper] Connection closed by main process");
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
                        eprintln!("[media-helper] Received command: {}", command);
                        // Execute command immediately (event-driven) based on player type
                        if is_vlc {
                            vlc::handle_vlc_command(command, &status_sender);
                        } else if is_dragon {
                            dragon_player::handle_dragon_player_command(command, &status_sender);
                        } else if is_smplayer {
                            smplayer::handle_smplayer_command(command, &status_sender);
                        } else if is_spotify {
                            spotify::handle_spotify_command(command, &status_sender);
                        } else {
                            // Default to VLC
                            vlc::handle_vlc_command(command, &status_sender);
                        }
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
                    eprintln!("[media-helper] Error reading from socket: {}", e);
                    break;
                }
            }
        }
    }
    
    Ok(())
}
