//! Browser helper binary for tiny-dfr, providing browser control and status via DBus.
//! 
//! This helper process:
//! 1. Connects to the main process via Unix socket
//! 2. Receives browser type from main process
//! 3. Monitors browser via DBus and sends status updates to main process
//! 4. Receives commands from main process and executes them on the specified browser
//! 
//! Supported commands:
//! - back: Go back in browser history
//! - forward: Go forward in browser history
//! - refresh: Refresh current page
//! - home: Go to home page
//! - add_bookmark: Add current page to bookmarks
//! - new_tab: Open new tab
//! - close_tab: Close current tab
//! - focus_address_bar: Focus on address bar
//! - get_url_info: Get current URL and favicon info

use std::os::unix::net::UnixStream;
use std::io::{Write, Read};
use std::thread;
use std::time::Duration;
use std::process::Command;
use serde_json::json;

#[derive(Debug)]
struct BrowserInfo {
    url: String,
    title: String,
    favicon_url: Option<String>,
    can_go_back: bool,
    can_go_forward: bool,
    is_loading: bool,
}

fn get_firefox_info() -> Option<BrowserInfo> {
    // Try to get Firefox info via DBus
    let output = Command::new("dbus-send")
        .arg("--session")
        .arg("--dest=org.mozilla.firefox")
        .arg("--type=method_call")
        .arg("--print-reply")
        .arg("/org/mozilla/firefox")
        .arg("org.mozilla.firefox.GetCurrentURL")
        .output();
    
    match output {
        Ok(output) => {
            if output.status.success() {
                let url = String::from_utf8_lossy(&output.stdout).trim().to_string();
                // For now, assume these values - in a real implementation you'd query them
    Some(BrowserInfo {
        url,
                    title: "Firefox".to_string(),
                    favicon_url: None,
                    can_go_back: true,
                    can_go_forward: true,
                    is_loading: false,
                })
            } else {
                None
            }
        }
        Err(_) => None,
    }
}

fn get_chrome_info() -> Option<BrowserInfo> {
    // Try to get Chrome info via DBus
    let output = Command::new("dbus-send")
        .arg("--session")
        .arg("--dest=org.chromium.Chromium")
        .arg("--type=method_call")
        .arg("--print-reply")
        .arg("/org/chromium/Chromium")
        .arg("org.chromium.Chromium.GetCurrentURL")
        .output();
    
    match output {
        Ok(output) => {
            if output.status.success() {
                let url = String::from_utf8_lossy(&output.stdout).trim().to_string();
                // For now, assume these values - in a real implementation you'd query them
    Some(BrowserInfo {
        url,
                    title: "Chrome".to_string(),
                    favicon_url: None,
                    can_go_back: true,
                    can_go_forward: true,
                    is_loading: false,
                })
            } else {
                None
            }
        }
        Err(_) => None,
    }
}

fn get_brave_info() -> Option<BrowserInfo> {
    // Try to get Brave info via DBus
    let output = Command::new("dbus-send")
        .arg("--session")
        .arg("--dest=org.brave.Browser")
        .arg("--type=method_call")
        .arg("--print-reply")
        .arg("/org/brave/Browser")
        .arg("org.brave.Browser.GetCurrentURL")
        .output();
    
    match output {
        Ok(output) => {
            if output.status.success() {
                let url = String::from_utf8_lossy(&output.stdout).trim().to_string();
                // For now, assume these values - in a real implementation you'd query them
    Some(BrowserInfo {
        url,
                    title: "Brave".to_string(),
                    favicon_url: None,
                    can_go_back: true,
                    can_go_forward: true,
                    is_loading: false,
                })
            } else {
                None
            }
        }
        Err(_) => None,
    }
}

fn get_edge_info() -> Option<BrowserInfo> {
    // Try to get Edge info via DBus
    let output = Command::new("dbus-send")
        .arg("--session")
        .arg("--dest=org.microsoft.Edge")
        .arg("--type=method_call")
        .arg("--print-reply")
        .arg("/org/microsoft/Edge")
        .arg("org.microsoft.Edge.GetCurrentURL")
        .output();
    
    match output {
        Ok(output) => {
            if output.status.success() {
                let url = String::from_utf8_lossy(&output.stdout).trim().to_string();
                // For now, assume these values - in a real implementation you'd query them
    Some(BrowserInfo {
        url,
                    title: "Edge".to_string(),
                    favicon_url: None,
                    can_go_back: true,
                    can_go_forward: true,
                    is_loading: false,
                })
            } else {
                None
            }
        }
        Err(_) => None,
    }
}

fn get_browser_info_for_type(browser_type: &str) -> Option<BrowserInfo> {
    match browser_type {
        "firefox" => get_firefox_info(),
        "chrome" | "chromium" | "google-chrome" => get_chrome_info(),
        "brave" | "brave-browser" => get_brave_info(),
        "edge" => get_edge_info(),
        "safari" => get_chrome_info(), // Safari might not have DBus, fallback to chrome
        "opera" => get_chrome_info(),  // Opera might not have DBus, fallback to chrome
        _ => {
            // Try to get info for unknown browser types
            eprintln!("[browser-helper] Unknown browser type: {}, trying chrome fallback", browser_type);
            get_chrome_info()
        }
    }
}

fn execute_browser_command(command: &str, args: &[&str]) -> bool {
    // This is a simplified version - in practice you'd parse the command string
    // and execute the appropriate DBus call
    eprintln!("[browser-helper] Would execute DBus command: {} with args: {:?}", command, args);
    false // For now, always fall back to xdotool
}

fn focus_address_bar() -> bool {
    // Use xdotool to focus on address bar
    // This simulates Ctrl+L which focuses the address bar in most browsers
    let output = Command::new("xdotool")
        .arg("key")
        .arg("ctrl+l")
        .output();
    
    match output {
        Ok(output) => {
            if output.status.success() {
                eprintln!("[browser-helper] Focused address bar successfully");
                true
            } else {
                eprintln!("[browser-helper] Failed to focus address bar: {:?}", String::from_utf8_lossy(&output.stderr));
                false
            }
        }
        Err(e) => {
            eprintln!("[browser-helper] Failed to execute xdotool: {}", e);
            false
        }
    }
}

fn execute_xdotool_command_for_browser(key_combination: &str, browser_type: &str) -> bool {
    // Find the active browser window and send keys to it
    let window_pattern = if browser_type == "firefox" {
        ".*Firefox.*"
    } else if browser_type == "chrome" || browser_type == "chromium" {
        ".*Chrome.*"
    } else if browser_type == "brave" || browser_type == "brave-browser" {
        ".*Brave.*"
    } else if browser_type == "edge" {
        ".*Edge.*"
    } else if browser_type == "google-chrome" {
        ".*Google Chrome.*"
    } else if browser_type == "safari" {
        ".*Safari.*"
    } else if browser_type == "opera" {
        ".*Opera.*"
    } else {
        // For unknown browser types, try to use a generic pattern
        ".*Chrome.*"
    };
    
    eprintln!("[browser-helper] Using xdotool window pattern: '{}' for browser type: '{}'", window_pattern, browser_type);
    
    let window_id = Command::new("xdotool")
        .arg("search")
        .arg("--name")
        .arg(window_pattern)
        .output();
    
    match window_id {
        Ok(output) => {
            if output.status.success() {
                let window_id_str = String::from_utf8_lossy(&output.stdout);
                let trimmed = window_id_str.trim();
                if !trimmed.is_empty() {
                    // Get the first window ID
                    let first_window = trimmed.lines().next().unwrap_or("").trim();
                    if !first_window.is_empty() {
                        // Activate the window and send keys
                        let activate_result = Command::new("xdotool")
                            .arg("windowactivate")
                            .arg(first_window)
                            .output();
                        
                        if activate_result.is_ok() {
                            // Small delay to ensure window is active
                            thread::sleep(Duration::from_millis(100));
                            
                            let key_result = Command::new("xdotool")
                                .arg("key")
                                .arg(key_combination)
                                .output();
                            
                            match key_result {
                                Ok(output) => {
                                    if output.status.success() {
                                        eprintln!("[browser-helper] Successfully sent keys '{}' to {} window", key_combination, browser_type);
                                        return true;
                                    } else {
                                        eprintln!("[browser-helper] Failed to send keys: {:?}", String::from_utf8_lossy(&output.stderr));
                                    }
                                }
                                Err(e) => {
                                    eprintln!("[browser-helper] Failed to execute xdotool key command: {}", e);
                                }
                            }
                        } else {
                            eprintln!("[browser-helper] Failed to activate {} window", browser_type);
                        }
                    }
                }
            }
        }
        Err(e) => {
            eprintln!("[browser-helper] Failed to find {} window: {}", browser_type, e);
        }
    }
    
    false
}

fn handle_command_with_browser_type(command: &str, browser_type: &str) {
    match command.trim() {
        "back" => {
            eprintln!("[browser-helper] Executing back command for {}", browser_type);
            // Try DBus for the specific browser first
            let mut success = match browser_type {
                "firefox" => {
                    eprintln!("[browser-helper] Trying Firefox DBus: org.mozilla.firefox.GoBack");
                    execute_browser_command("org.mozilla.firefox.GoBack", &[])
                },
                "chrome" | "chromium" | "google-chrome" => {
                    eprintln!("[browser-helper] Trying Chrome DBus: org.chromium.Chromium.GoBack");
                    execute_browser_command("org.chromium.Chromium.GoBack", &[])
                },
                "brave" | "brave-browser" => {
                    eprintln!("[browser-helper] Trying Brave DBus: org.brave.Browser.GoBack");
                    execute_browser_command("org.brave.Browser.GoBack", &[])
                },
                "edge" => {
                    eprintln!("[browser-helper] Trying Edge DBus: org.microsoft.Edge.GoBack");
                    execute_browser_command("org.microsoft.Edge.GoBack", &[])
                },
                _ => {
                    eprintln!("[browser-helper] Unknown browser type, skipping DBus attempt");
                    false
                },
            };
            
            // If DBus fails, use xdotool as fallback
            if !success {
                eprintln!("[browser-helper] DBus failed for back on {}, trying xdotool fallback", browser_type);
                eprintln!("[browser-helper] Will use xdotool with pattern for: {}", browser_type);
                success = execute_xdotool_command_for_browser("alt+left", browser_type);
            }
            
            if success {
                eprintln!("[browser-helper] Back command executed successfully for {}", browser_type);
            } else {
                eprintln!("[browser-helper] Back command failed on all methods for {}", browser_type);
            }
        }
        "forward" => {
            eprintln!("[browser-helper] Executing forward command for {}", browser_type);
            // Try DBus for the specific browser first
            let mut success = match browser_type {
                "firefox" => execute_browser_command("org.mozilla.firefox.GoForward", &[]),
                "chrome" | "chromium" | "google-chrome" => execute_browser_command("org.chromium.Chromium.GoForward", &[]),
                "brave" | "brave-browser" => execute_browser_command("org.brave.Browser.GoForward", &[]),
                "edge" => execute_browser_command("org.microsoft.Edge.GoForward", &[]),
                _ => false,
            };
            
            // If DBus fails, use xdotool as fallback
            if !success {
                eprintln!("[browser-helper] DBus failed for forward on {}, trying xdotool fallback", browser_type);
                success = execute_xdotool_command_for_browser("alt+right", browser_type);
            }
            
            if success {
                eprintln!("[browser-helper] Forward command executed successfully for {}", browser_type);
            } else {
                eprintln!("[browser-helper] Forward command failed on all methods for {}", browser_type);
            }
        }
        "refresh" => {
            eprintln!("[browser-helper] Executing refresh command for {}", browser_type);
            // Try DBus for the specific browser first
            let mut success = match browser_type {
                "firefox" => execute_browser_command("org.mozilla.firefox.Reload", &[]),
                "chrome" | "chromium" | "google-chrome" => execute_browser_command("org.chromium.Chromium.Reload", &[]),
                "brave" | "brave-browser" => execute_browser_command("org.brave.Browser.Reload", &[]),
                "edge" => execute_browser_command("org.microsoft.Edge.Reload", &[]),
                _ => false,
            };
            
            // If DBus fails, use xdotool as fallback
            if !success {
                eprintln!("[browser-helper] DBus failed for refresh on {}, trying xdotool fallback", browser_type);
                success = execute_xdotool_command_for_browser("ctrl+r", browser_type);
            }
            
            if success {
                eprintln!("[browser-helper] Refresh command executed successfully for {}", browser_type);
            } else {
                eprintln!("[browser-helper] Refresh command failed on all methods for {}", browser_type);
            }
        }
        "home" => {
            eprintln!("[browser-helper] Executing home command for {}", browser_type);
            // Try DBus for the specific browser first
            let mut success = match browser_type {
                "firefox" => execute_browser_command("org.mozilla.firefox.GoHome", &[]),
                "chrome" | "chromium" | "google-chrome" => execute_browser_command("org.chromium.Chromium.GoHome", &[]),
                "brave" | "brave-browser" => execute_browser_command("org.brave.Browser.GoHome", &[]),
                "edge" => execute_browser_command("org.microsoft.Edge.GoHome", &[]),
                _ => false,
            };
            
            // If DBus fails, use xdotool as fallback
            if !success {
                eprintln!("[browser-helper] DBus failed for home on {}, trying xdotool fallback", browser_type);
                success = execute_xdotool_command_for_browser("alt+home", browser_type);
            }
            
            if success {
                eprintln!("[browser-helper] Home command executed successfully for {}", browser_type);
            } else {
                eprintln!("[browser-helper] Home command failed on all methods for {}", browser_type);
            }
        }
        "add_bookmark" => {
            eprintln!("[browser-helper] Executing add bookmark command for {}", browser_type);
            // Use xdotool to simulate Ctrl+D (add bookmark)
            let success = execute_xdotool_command_for_browser("ctrl+d", browser_type);
            if success {
                eprintln!("[browser-helper] Add bookmark command executed successfully for {}", browser_type);
            } else {
                eprintln!("[browser-helper] Add bookmark command failed for {}", browser_type);
            }
        }
        "close_tab" => {
            eprintln!("[browser-helper] Executing close tab command for {}", browser_type);
            // Use xdotool to simulate Ctrl+W (close tab)
            let success = execute_xdotool_command_for_browser("ctrl+w", browser_type);
            if success {
                eprintln!("[browser-helper] Close tab command executed successfully for {}", browser_type);
            } else {
                eprintln!("[browser-helper] Close tab command failed for {}", browser_type);
            }
        }
        "new_tab" => {
            eprintln!("[browser-helper] Executing new tab command for {}", browser_type);
            // Use xdotool to simulate Ctrl+T (new tab)
            let success = execute_xdotool_command_for_browser("ctrl+t", browser_type);
            if success {
                eprintln!("[browser-helper] New tab command executed successfully for {}", browser_type);
            } else {
                eprintln!("[browser-helper] New tab command failed for {}", browser_type);
            }
        }
        "focus_address_bar" => {
            eprintln!("[browser-helper] Executing focus address bar command for {}", browser_type);
            let success = focus_address_bar();
            if success {
                eprintln!("[browser-helper] Focus address bar command executed successfully for {}", browser_type);
            } else {
                eprintln!("[browser-helper] Focus address bar command failed for {}", browser_type);
            }
        }
        "get_url_info" => {
            eprintln!("[browser-helper] Executing get URL info command for {}", browser_type);
            // This will be handled by the status update loop
        }
        _ => {
            eprintln!("[browser-helper] Unknown command: {} for {}", command, browser_type);
        }
    }
}

fn main() -> std::io::Result<()> {
    let socket_path = "/tmp/touchbar-browser.sock";
    
    // Print environment info for debugging
    if let Ok(addr) = std::env::var("DBUS_SESSION_BUS_ADDRESS") {
        eprintln!("[browser-helper] DBUS_SESSION_BUS_ADDRESS={}", addr);
    } else {
        eprintln!("[browser-helper] DBUS_SESSION_BUS_ADDRESS is not set");
    }
    
    // Check if xdotool is available
    match Command::new("xdotool").arg("--version").output() {
        Ok(_) => eprintln!("[browser-helper] xdotool is available"),
        Err(_) => eprintln!("[browser-helper] WARNING: xdotool is not available, browser control will be limited"),
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
                            
                            // Print which browser type we're now using and what methods will be used
                            match browser_type.as_str() {
                                "firefox" => eprintln!("[browser-helper] Now using Firefox - will try DBus org.mozilla.firefox.* then xdotool fallback"),
                                "chrome" | "chromium" => eprintln!("[browser-helper] Now using Chrome/Chromium - will try DBus org.chromium.Chromium.* then xdotool fallback"),
                                "google-chrome" => eprintln!("[browser-helper] Now using Google Chrome - will try DBus org.chromium.Chromium.* then xdotool fallback"),
                                "brave" | "brave-browser" => eprintln!("[browser-helper] Now using Brave - will try DBus org.brave.Browser.* then xdotool fallback"),
                                "edge" => eprintln!("[browser-helper] Now using Edge - will try DBus org.microsoft.Edge.* then xdotool fallback"),
                                "safari" => eprintln!("[browser-helper] Now using Safari - will try DBus org.chromium.Chromium.* then xdotool fallback"),
                                "opera" => eprintln!("[browser-helper] Now using Opera - will try DBus org.chromium.Chromium.* then xdotool fallback"),
                                _ => eprintln!("[browser-helper] Now using unknown browser type '{}' - will use Chrome fallback methods", browser_type),
                            }
                            
                            // Send updated browser info
                            if let Some(browser_info) = get_browser_info_for_type(&browser_type) {
                                let status_json = json!({
                                    "url": browser_info.url,
                                    "title": browser_info.title,
                                    "favicon_url": browser_info.favicon_url,
                                    "can_go_back": browser_info.can_go_back,
                                    "can_go_forward": browser_info.can_go_forward,
                                    "is_loading": browser_info.is_loading,
                                });
                                
                                let status_str = status_json.to_string();
                                if stream.write_all(format!("{}\n", status_str).as_bytes()).is_ok() {
                                    eprintln!("[browser-helper] Sent updated browser status after browser type change");
                                } else {
                                    eprintln!("[browser-helper] Failed to send updated browser status after browser type change");
                                }
                            }
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
    
    // Print which browser type we're using and what methods will be used
    match browser_type.as_str() {
        "firefox" => eprintln!("[browser-helper] Using Firefox - will try DBus org.mozilla.firefox.* then xdotool fallback"),
        "chrome" | "chromium" => eprintln!("[browser-helper] Using Chrome/Chromium - will try DBus org.chromium.Chromium.* then xdotool fallback"),
        "google-chrome" => eprintln!("[browser-helper] Using Google Chrome - will try DBus org.chromium.Chromium.* then xdotool fallback"),
        "brave" | "brave-browser" => eprintln!("[browser-helper] Using Brave - will try DBus org.brave.Browser.* then xdotool fallback"),
        "edge" => eprintln!("[browser-helper] Using Edge - will try DBus org.microsoft.Edge.* then xdotool fallback"),
        "safari" => eprintln!("[browser-helper] Using Safari - will try DBus org.chromium.Chromium.* then xdotool fallback"),
        "opera" => eprintln!("[browser-helper] Using Opera - will try DBus org.chromium.Chromium.* then xdotool fallback"),
        _ => eprintln!("[browser-helper] Unknown browser type '{}' - will use Chrome fallback methods", browser_type),
    }
    
    // Send initial browser info immediately
    if let Some(browser_info) = get_browser_info_for_type(&browser_type) {
        let status_json = json!({
            "url": browser_info.url,
            "title": browser_info.title,
            "favicon_url": browser_info.favicon_url,
            "can_go_back": browser_info.can_go_back,
            "can_go_forward": browser_info.can_go_forward,
            "is_loading": browser_info.is_loading,
        });
        
        let status_str = status_json.to_string();
        if stream.write_all(format!("{}\n", status_str).as_bytes()).is_ok() {
            eprintln!("[browser-helper] Sent initial browser status");
        } else {
            eprintln!("[browser-helper] Failed to send initial browser status");
        }
    }
    
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
                        
                        // Print which browser type we're now using and what methods will be used
                        match browser_type.as_str() {
                            "firefox" => eprintln!("[browser-helper] Now using Firefox - will try DBus org.mozilla.firefox.* then xdotool fallback"),
                            "chrome" | "chromium" => eprintln!("[browser-helper] Now using Chrome/Chromium - will try DBus org.chromium.Chromium.* then xdotool fallback"),
                            "google-chrome" => eprintln!("[browser-helper] Now using Google Chrome - will try DBus org.chromium.Chromium.* then xdotool fallback"),
                            "brave" | "brave-browser" => eprintln!("[browser-helper] Now using Brave - will try DBus org.brave.Browser.* then xdotool fallback"),
                            "edge" => eprintln!("[browser-helper] Now using Edge - will try DBus org.microsoft.Edge.* then xdotool fallback"),
                            "safari" => eprintln!("[browser-helper] Now using Safari - will try DBus org.chromium.Chromium.* then xdotool fallback"),
                            "opera" => eprintln!("[browser-helper] Now using Opera - will try DBus org.chromium.Chromium.* then xdotool fallback"),
                            _ => eprintln!("[browser-helper] Now using unknown browser type '{}' - will use Chrome fallback methods", browser_type),
                        }
                        
                        // Send updated browser info
                        if let Some(browser_info) = get_browser_info_for_type(&browser_type) {
                            let status_json = json!({
                                "url": browser_info.url,
                                "title": browser_info.title,
                                "favicon_url": browser_info.favicon_url,
                                "can_go_back": browser_info.can_go_back,
                                "can_go_forward": browser_info.can_go_forward,
                                "is_loading": browser_info.is_loading,
                            });
                            
                            let status_str = status_json.to_string();
                            if stream.write_all(format!("{}\n", status_str).as_bytes()).is_ok() {
                                eprintln!("[browser-helper] Sent updated browser status after browser type change");
                            } else {
                                eprintln!("[browser-helper] Failed to send updated browser status after browser type change");
                            }
                        }
                    } else if !command.is_empty() {
                        eprintln!("[browser-helper] Received command: {}", command);
                        handle_command_with_browser_type(command, &browser_type);
                        
                        // After executing a command, send updated browser status
                        if let Some(browser_info) = get_browser_info_for_type(&browser_type) {
                            let status_json = json!({
                                "url": browser_info.url,
                                "title": browser_info.title,
                                "favicon_url": browser_info.favicon_url,
                                "can_go_back": browser_info.can_go_back,
                                "can_go_forward": browser_info.can_go_forward,
                                "is_loading": browser_info.is_loading,
                            });
                            
                            let status_str = status_json.to_string();
                                if stream.write_all(format!("{}\n", status_str).as_bytes()).is_ok() {
                                    eprintln!("[browser-helper] Sent status update after command");
                                } else {
                                    eprintln!("[browser-helper] Failed to send browser status after command");
                            }
                        }
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