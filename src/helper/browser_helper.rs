//! Browser helper binary for tiny-dfr, providing browser control via key simulation.
//! 
//! This helper process:
//! 1. Connects to the main process via Unix socket
//! 2. Receives browser type from main process
//! 3. Receives commands from main process and executes them on the specified browser
//! 
//! Supported commands:
//! - back: Go back in browser history
//! - forward: Go forward in browser history
//! - refresh: Refresh current page
//! - new_tab: Open new tab
//! - focus_address_bar: Focus on address bar

use std::os::unix::net::UnixStream;
use std::io::{Write, Read};
use std::thread;
use std::time::Duration;
use std::process::Command;
use std::env;
use std::fs;

// Add Unix socket-based browser control
fn execute_socket_browser_command(command: &str, browser_type: &str) -> bool {
    eprintln!("[browser-helper] Using Unix socket for {}: '{}'", browser_type, command);
    
    // Try to connect to a browser-specific socket
    let socket_paths = vec![
        format!("/tmp/{}-control.sock", browser_type.to_lowercase()),
        "/tmp/browser-control.sock".to_string(),
        format!("/run/user/{}/browser-control.sock", env::var("UID").unwrap_or_else(|_| "1000".to_string())),
    ];
    
    for socket_path in socket_paths {
        if let Ok(mut stream) = UnixStream::connect(&socket_path) {
            eprintln!("[browser-helper] Connected to socket: {}", socket_path);
            
            // Send command to the socket
            let command_with_newline = format!("{}\n", command);
            if let Ok(_) = stream.write_all(command_with_newline.as_bytes()) {
                eprintln!("[browser-helper] Successfully sent command to socket: {}", command);
                return true;
            } else {
                eprintln!("[browser-helper] Failed to write to socket: {}", socket_path);
            }
        }
    }
    
    eprintln!("[browser-helper] No browser control socket found");
    false
}


#[derive(Debug, Clone, Copy)]
enum DisplayServer {
    X11,
    Wayland,
    Unknown,
}

fn detect_display_server() -> DisplayServer {
    // Check XDG_SESSION_TYPE first (most reliable)
    if let Ok(session_type) = env::var("XDG_SESSION_TYPE") {
        if session_type.to_lowercase() == "wayland" {
            eprintln!("[browser-helper] Detected Wayland display server via XDG_SESSION_TYPE");
            return DisplayServer::Wayland;
        } else if session_type.to_lowercase() == "x11" {
            eprintln!("[browser-helper] Detected X11 display server via XDG_SESSION_TYPE");
            return DisplayServer::X11;
        }
    }
    
    // Check for Wayland
    if env::var("WAYLAND_DISPLAY").is_ok() {
        eprintln!("[browser-helper] Detected Wayland display server via WAYLAND_DISPLAY");
        return DisplayServer::Wayland;
    }
    
    // Check for X11
    if env::var("DISPLAY").is_ok() {
        eprintln!("[browser-helper] Detected X11 display server via DISPLAY");
        return DisplayServer::X11;
    }
    
    eprintln!("[browser-helper] Could not detect display server");
    DisplayServer::Unknown
}





fn execute_browser_command_cross_platform(command: &str, browser_type: &str) -> bool {
    let display_server = detect_display_server();
    
    // Try Unix socket first for all browsers
    eprintln!("[browser-helper] Trying Unix socket first for {}", browser_type);
    if execute_socket_browser_command(command, browser_type) {
        return true;
    }
    eprintln!("[browser-helper] Socket failed, falling back to key simulation");
    
    match display_server {
        DisplayServer::X11 => {
            // Use xdotool for X11
            execute_xdotool_command_for_browser(command, browser_type)
        }
        DisplayServer::Wayland => {
            // Use xdotool for Wayland since it's working on this system
            eprintln!("[browser-helper] Using xdotool for Wayland: '{}' for browser type: '{}'", command, browser_type);
            execute_xdotool_command_for_browser(command, browser_type)
        }
        DisplayServer::Unknown => {
            // Try both methods
            eprintln!("[browser-helper] Unknown display server, trying both X11 and Wayland methods");
            let x11_result = execute_xdotool_command_for_browser(command, browser_type);
            if x11_result {
                return true;
            }
            execute_wtype_command_for_browser(command, browser_type)
        }
    }
}

fn execute_wtype_command_for_browser(key_combination: &str, browser_type: &str) -> bool {
    // Use wtype for Wayland (more reliable than xdotool on Wayland)
    eprintln!("[browser-helper] Using wtype for Wayland: '{}' for browser type: '{}'", key_combination, browser_type);
    
    // First, try to focus the browser window using wtype
    let focus_result = Command::new("wtype")
        .arg("-M")
        .arg("ctrl")
        .arg("-k")
        .arg("1")
        .output();
    
    if focus_result.is_ok() {
        // Small delay to ensure window is active
        thread::sleep(Duration::from_millis(100));
        
        // Now send the actual command
        let key_result = Command::new("wtype")
            .arg(key_combination)
            .output();
        
        match key_result {
            Ok(output) => {
                if output.status.success() {
                    eprintln!("[browser-helper] Successfully sent keys '{}' to {} window via wtype", key_combination, browser_type);
                    // Add a small delay to ensure the browser processes the key
                    thread::sleep(Duration::from_millis(50));
                    return true;
                } else {
                    eprintln!("[browser-helper] Failed to send keys via wtype: {:?}", String::from_utf8_lossy(&output.stderr));
                }
            }
            Err(e) => {
                eprintln!("[browser-helper] Failed to execute wtype command: {}", e);
            }
        }
    } else {
        eprintln!("[browser-helper] Failed to focus browser window via wtype");
    }
    
    false
}

fn execute_xdotool_command_for_browser(key_combination: &str, browser_type: &str) -> bool {
    eprintln!("[browser-helper] Using xdotool for {}: '{}'", browser_type, key_combination);
    
    // Just send keys to the currently focused window - simpler and more reliable
    let key_result = Command::new("xdotool")
        .arg("key")
        .arg(key_combination)
        .output();
    
    match key_result {
        Ok(output) => {
            if output.status.success() {
                eprintln!("[browser-helper] Successfully sent keys '{}' to focused window", key_combination);
                // Add a small delay to ensure the browser processes the key
                thread::sleep(Duration::from_millis(50));
                return true;
            } else {
                eprintln!("[browser-helper] Failed to send keys: {:?}", String::from_utf8_lossy(&output.stderr));
                eprintln!("[browser-helper] xdotool exit code: {}", output.status);
                eprintln!("[browser-helper] xdotool stdout: {:?}", String::from_utf8_lossy(&output.stdout));
            }
        }
        Err(e) => {
            eprintln!("[browser-helper] Failed to execute xdotool key command: {}", e);
        }
    }
    
    false
}

fn execute_command(command_name: &str, key_combination: &str, browser_type: &str) {
    eprintln!("[browser-helper] Executing {} command for {} with keys: '{}'", command_name, browser_type, key_combination);
    
    // Try the primary key combination
    let success = execute_browser_command_cross_platform(key_combination, browser_type);
    
    if success {
        eprintln!("[browser-helper] {} command executed successfully for {}", command_name, browser_type);
    } else {
        eprintln!("[browser-helper] {} command failed for {} with keys '{}', trying fallback...", command_name, browser_type, key_combination);
        
        // Try fallback key combinations for back/forward
        let fallback_success = match command_name {
            "back" => {
                let fallbacks = ["alt+left", "ctrl+[", "BackSpace"];
                for fallback in &fallbacks {
                    eprintln!("[browser-helper] Trying fallback key combination: '{}'", fallback);
                    if execute_browser_command_cross_platform(fallback, browser_type) {
                        eprintln!("[browser-helper] {} command succeeded with fallback key: '{}'", command_name, fallback);
                        return;
                    }
                }
                false
            }
            "forward" => {
                let fallbacks = ["alt+right", "ctrl+]", "Shift+BackSpace"];
                for fallback in &fallbacks {
                    eprintln!("[browser-helper] Trying fallback key combination: '{}'", fallback);
                    if execute_browser_command_cross_platform(fallback, browser_type) {
                        eprintln!("[browser-helper] {} command succeeded with fallback key: '{}'", command_name, fallback);
                        return;
                    }
                }
                false
            }
            "new tab" => {
                let fallbacks = ["ctrl+n", "F6", "ctrl+shift+t"];
                for fallback in &fallbacks {
                    eprintln!("[browser-helper] Trying fallback key combination: '{}'", fallback);
                    if execute_browser_command_cross_platform(fallback, browser_type) {
                        eprintln!("[browser-helper] {} command succeeded with fallback key: '{}'", command_name, fallback);
                        return;
                    }
                }
                false
            }
            "close tab" => {
                let fallbacks = ["ctrl+F4", "alt+F4", "ctrl+shift+w"];
                for fallback in &fallbacks {
                    eprintln!("[browser-helper] Trying fallback key combination: '{}'", fallback);
                    if execute_browser_command_cross_platform(fallback, browser_type) {
                        eprintln!("[browser-helper] {} command succeeded with fallback key: '{}'", command_name, fallback);
                        return;
                    }
                }
                false
            }
            _ => false
        };
        
        if !fallback_success {
            eprintln!("[browser-helper] {} command failed for {} with all key combinations", command_name, browser_type);
        }
    }
}

fn handle_command_with_browser_type(command: &str, browser_type: &str) {
    // Get browser-specific key combinations
    let (back_key, forward_key, refresh_key, new_tab_key, focus_address_key) = get_browser_key_combinations(browser_type);
    
    match command.trim() {
        "back" => execute_command("back", back_key, browser_type),
        "forward" => execute_command("forward", forward_key, browser_type),
        "refresh" => execute_command("refresh", refresh_key, browser_type),
        "new_tab" => execute_command("new tab", new_tab_key, browser_type),
        "close_tab" => execute_command("close tab", "ctrl+w", browser_type),
        "focus_address_bar" => execute_command("focus address bar", focus_address_key, browser_type),
        _ => {
            eprintln!("[browser-helper] Unknown command: {} for {}", command, browser_type);
        }
    }
}

fn get_browser_key_combinations(browser_type: &str) -> (&'static str, &'static str, &'static str, &'static str, &'static str) {
    match browser_type.to_lowercase().as_str() {
        "firefox" => {
            // Firefox uses standard combinations
            ("alt+Left", "alt+Right", "ctrl+r", "ctrl+t", "ctrl+l")
        }
        "chrome" | "chromium" | "google-chrome" => {
            // Chrome uses standard combinations
            ("alt+Left", "alt+Right", "ctrl+r", "ctrl+t", "ctrl+l")
        }
        "brave" | "brave-browser" => {
            // Brave is Chromium-based
            ("alt+Left", "alt+Right", "ctrl+r", "ctrl+t", "ctrl+l")
        }
        "edge" => {
            // Edge is Chromium-based
            ("alt+Left", "alt+Right", "ctrl+r", "ctrl+t", "ctrl+l")
        }
        "opera" => {
            // Opera uses standard combinations
            ("alt+Left", "alt+Right", "ctrl+r", "ctrl+t", "ctrl+l")
        }
        _ => {
            // Default to standard combinations for unknown browsers
            eprintln!("[browser-helper] Using default key combinations for unknown browser: {}", browser_type);
            ("alt+Left", "alt+Right", "ctrl+r", "ctrl+t", "ctrl+l")
        }
    }
}


fn log_browser_type(browser_type: &str, prefix: &str) {
    let message = match browser_type {
        "firefox" => "Firefox - will use cross-platform key simulation",
        "chrome" | "chromium" => "Chrome/Chromium - will use cross-platform key simulation",
        "google-chrome" => "Google Chrome - will use cross-platform key simulation",
        "brave" | "brave-browser" => "Brave - will use cross-platform key simulation",
        "edge" => "Edge - will use cross-platform key simulation",
        "opera" => "Opera - will use cross-platform key simulation",
        _ => &format!("unknown browser type '{}' - will use cross-platform key simulation", browser_type),
    };
    eprintln!("[browser-helper] {} {}", prefix, message);
}

fn check_tool_availability(tool: &str) -> bool {
    Command::new(tool).arg("--version").output().is_ok()
}

fn log_display_server_tools(display_server: DisplayServer) {
    match display_server {
        DisplayServer::X11 => {
            if check_tool_availability("xdotool") {
                eprintln!("[browser-helper] X11 detected, xdotool is available");
            } else {
                eprintln!("[browser-helper] WARNING: X11 detected but xdotool is not available, browser control will be limited");
            }
        }
        DisplayServer::Wayland => {
            if check_tool_availability("wtype") {
                eprintln!("[browser-helper] Wayland detected, wtype is available");
            } else {
                eprintln!("[browser-helper] WARNING: Wayland detected but wtype is not available, browser control will be limited");
            }
        }
        DisplayServer::Unknown => {
            eprintln!("[browser-helper] Unknown display server, checking for both xdotool and wtype");
            let xdotool_available = check_tool_availability("xdotool");
            let wtype_available = check_tool_availability("wtype");
            
            if xdotool_available {
                eprintln!("[browser-helper] xdotool is available");
            }
            if wtype_available {
                eprintln!("[browser-helper] wtype is available");
            }
            if !xdotool_available && !wtype_available {
                eprintln!("[browser-helper] WARNING: Neither xdotool nor wtype is available, browser control will be limited");
            }
        }
    }
}

fn main() -> std::io::Result<()> {
    let socket_path = "/tmp/touchbar-browser.sock";
    
    // Check for display server and available tools
    let display_server = detect_display_server();
    log_display_server_tools(display_server);
    
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
    
    eprintln!("[browser-helper] Connected to socket, waiting for browser type...");
    
    // Wait for and receive the browser type from main app
    let mut browser_type = String::new();
    let mut buffer = Vec::new();
    let mut received_browser_type = false;
    
    // Wait for browser type message
    while !received_browser_type {
        let mut temp_buffer = [0u8; 1024];
        match stream.read(&mut temp_buffer) {
            Ok(0) => {
                eprintln!("[browser-helper] Connection closed before receiving browser type");
                return Ok(());
            }
            Ok(n) => {
                buffer.extend_from_slice(&temp_buffer[..n]);
                
                // Process complete lines
                while let Some(newline_pos) = buffer.iter().position(|&b| b == b'\n') {
                    let line_data = buffer.drain(..=newline_pos).collect::<Vec<_>>();
                    let line = String::from_utf8_lossy(&line_data[..line_data.len()-1]); // Remove newline
                    let command = line.trim();
                    
                    if command.starts_with("browser_type:") {
                        let new_browser_type = command.trim_start_matches("browser_type:").to_string();
                        if !received_browser_type {
                            // Initial browser type
                            browser_type = new_browser_type;
                            eprintln!("[browser-helper] Received initial browser type: {}", browser_type);
                            received_browser_type = true;
                            break;
                        } else {
                            // Browser type update
                            eprintln!("[browser-helper] Browser type updated from '{}' to '{}'", browser_type, new_browser_type);
                            browser_type = new_browser_type;
                            log_browser_type(&browser_type, "Now using");
                        }
                    }
                }
            }
            Err(e) => {
                if e.kind() == std::io::ErrorKind::WouldBlock {
                    thread::sleep(Duration::from_millis(10));
                    continue;
                } else {
                    eprintln!("[browser-helper] Error reading browser type: {}", e);
                    return Ok(());
                }
            }
        }
    }
    
    eprintln!("[browser-helper] Starting browser monitoring for: {}", browser_type);
    log_browser_type(&browser_type, "Using");
    
    // Create a reader for incoming commands
    let mut stream_clone = stream.try_clone()?;
    let mut buffer = Vec::new();
    
    loop {
        // Check for incoming commands (non-blocking)
        let mut temp_buffer = [0u8; 1024];
        match stream_clone.read(&mut temp_buffer) {
            Ok(0) => {
                // EOF - connection closed
                eprintln!("[browser-helper] Connection closed by main process");
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
                    
                    if command.starts_with("browser_type:") {
                        // Handle browser type updates
                        let new_browser_type = command.trim_start_matches("browser_type:").to_string();
                        eprintln!("[browser-helper] Browser type updated from '{}' to '{}'", browser_type, new_browser_type);
                        browser_type = new_browser_type;
                        log_browser_type(&browser_type, "Now using");
                    } else if !command.is_empty() {
                        eprintln!("[browser-helper] Received command: {}", command);
                        handle_command_with_browser_type(command, &browser_type);
                    }
                }
            }
            Err(e) => {
                if e.kind() == std::io::ErrorKind::WouldBlock {
                    // No data available, just wait
                    thread::sleep(Duration::from_millis(10));
                } else {
                    eprintln!("[browser-helper] Error reading from socket: {}", e);
                    break;
                }
            }
        }
    }
    
    Ok(())
} 