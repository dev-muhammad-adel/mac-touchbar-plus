//! Main function for the generic media helper binary

use std::os::unix::net::UnixStream;
use std::io::{Read, Write};
use std::thread;
use std::time::Duration;

mod spotify {
    include!("generic_media/spotify.rs");
}

// Import specific functions we need
use spotify::set_current_media_pluse spotify::set_current_media_playerayer as set_spotify_media_player;

fn main() -> std::io::Result<()> {
    let socket_path = "/tmp/touchbar-generic-media.sock";
    
    // Print environment info for debugging
    if let Ok(addr) = std::env::var("DBUS_SESSION_BUS_ADDRESS") {
        eprintln!("[generic-media-helper] DBUS_SESSION_BUS_ADDRESS={}", addr);
    } else {
        eprintln!("[generic-media-helper] DBUS_SESSION_BUS_ADDRESS is not set");
    }
    
    // Get the window class, ID, and PID from environment variables
    let window_class = std::env::var("TINY_DFR_WINDOW_CLASS").unwrap_or_default();
    let window_pid = std::env::var("TINY_DFR_WINDOW_PID")
        .ok()
        .and_then(|pid_str| pid_str.parse::<u32>().ok());
    
    eprintln!("[generic-media-helper] Window class: {}", window_class);
    if let Some(pid) = window_pid {
        eprintln!("[generic-media-helper] Window PID: {}", pid);
    }
    
    // Generic media helper only supports Spotify
    let is_spotify = window_class.to_lowercase() == "spotify";
    
    if is_spotify {
        eprintln!("[generic-media-helper] Starting Spotify media player integration");
        set_spotify_media_player("spotify", window_pid);
    } else {
        eprintln!("[generic-media-helper] Generic media helper only supports Spotify, but window class is: {}", window_class);
        return Ok(());
    }
    
    // Connect to the main process
    let mut stream = match UnixStream::connect(socket_path) {
        Ok(stream) => {
            eprintln!("[generic-media-helper] Connected to main process");
            stream
        }
        Err(e) => {
            eprintln!("[generic-media-helper] ERROR: Failed to connect to main process: {}", e);
            return Err(e);
        }
    };
    
    // Main loop - send status updates
    loop {
        // Get current media status
        let status = if is_spotify {
            get_spotify_status()
        } else {
            None
        };
        
        if let Some(status) = status {
            // Send status to main process
            let status_json = serde_json::to_string(&status).unwrap_or_else(|_| "{}".to_string());
            if let Err(e) = stream.write_all(format!("{}\n", status_json).as_bytes()) {
                eprintln!("[generic-media-helper] ERROR: Failed to send status: {}", e);
                break;
            }
        }
        
        // Sleep for a bit before next update
        thread::sleep(Duration::from_millis(500));
    }
    
    Ok(())
}

fn get_spotify_status() -> Option<serde_json::Value> {
    // This would integrate with Spotify's D-Bus interface
    // For now, return a placeholder status
    Some(serde_json::json!({
        "is_playing": false,
        "position": 0.0,
        "duration": 0
    }))
}
