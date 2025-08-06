//! Browser helper binary for tiny-dfr
//! 
//! This is a standalone binary that provides browser control and status via DBus.
//! It connects to the main process via Unix socket and sends status updates.

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

fn get_browser_info() -> Option<BrowserInfo> {
    // Try to get browser info via DBus for different browsers
    // Firefox
    if let Some(info) = get_firefox_info() {
        return Some(info);
    }
    
    // Chrome/Chromium
    if let Some(info) = get_chrome_info() {
        return Some(info);
    }
    
    // Brave
    if let Some(info) = get_brave_info() {
        return Some(info);
    }
    
    // Edge
    if let Some(info) = get_edge_info() {
        return Some(info);
    }
    
    None
}

fn get_firefox_info() -> Option<BrowserInfo> {
    // Firefox uses org.mozilla.firefox DBus interface
    let output = Command::new("dbus-send")
        .arg("--session")
        .arg("--dest=org.mozilla.firefox")
        .arg("--type=method_call")
        .arg("--print-reply")
        .arg("/org/mozilla/firefox")
        .arg("org.mozilla.firefox.GetCurrentURL")
        .output()
        .ok()?;
    
    let url_output = String::from_utf8_lossy(&output.stdout);
    let url = url_output.lines()
        .find(|line| line.contains("string"))
        .and_then(|line| line.split('"').nth(1))
        .unwrap_or("")
        .to_string();
    
    if url.is_empty() {
        return None;
    }
    
    // Get title
    let title_output = Command::new("dbus-send")
        .arg("--session")
        .arg("--dest=org.mozilla.firefox")
        .arg("--type=method_call")
        .arg("--print-reply")
        .arg("/org/mozilla/firefox")
        .arg("org.mozilla.firefox.GetCurrentTitle")
        .output()
        .ok()?;
    
    let title_text = String::from_utf8_lossy(&title_output.stdout);
    let title = title_text.lines()
        .find(|line| line.contains("string"))
        .and_then(|line| line.split('"').nth(1))
        .unwrap_or("Unknown")
        .to_string();
    
    Some(BrowserInfo {
        url,
        title,
        favicon_url: None, // Firefox doesn't expose favicon via DBus
        can_go_back: true, // Assume true for now
        can_go_forward: true, // Assume true for now
        is_loading: false, // Assume false for now
    })
}

fn get_chrome_info() -> Option<BrowserInfo> {
    // Chrome/Chromium uses org.chromium.Chromium DBus interface
    let output = Command::new("dbus-send")
        .arg("--session")
        .arg("--dest=org.chromium.Chromium")
        .arg("--type=method_call")
        .arg("--print-reply")
        .arg("/org/chromium/Chromium")
        .arg("org.chromium.Chromium.GetCurrentURL")
        .output()
        .ok()?;
    
    let url_output = String::from_utf8_lossy(&output.stdout);
    let url = url_output.lines()
        .find(|line| line.contains("string"))
        .and_then(|line| line.split('"').nth(1))
        .unwrap_or("")
        .to_string();
    
    if url.is_empty() {
        return None;
    }
    
    // Get title
    let title_output = Command::new("dbus-send")
        .arg("--session")
        .arg("--dest=org.chromium.Chromium")
        .arg("--type=method_call")
        .arg("--print-reply")
        .arg("/org/chromium/Chromium")
        .arg("org.chromium.Chromium.GetCurrentTitle")
        .output()
        .ok()?;
    
    let title_text = String::from_utf8_lossy(&title_output.stdout);
    let title = title_text.lines()
        .find(|line| line.contains("string"))
        .and_then(|line| line.split('"').nth(1))
        .unwrap_or("Unknown")
        .to_string();
    
    Some(BrowserInfo {
        url,
        title,
        favicon_url: None, // Chrome doesn't expose favicon via DBus
        can_go_back: true, // Assume true for now
        can_go_forward: true, // Assume true for now
        is_loading: false, // Assume false for now
    })
}

fn get_brave_info() -> Option<BrowserInfo> {
    // Brave uses org.brave.Browser DBus interface
    let output = Command::new("dbus-send")
        .arg("--session")
        .arg("--dest=org.brave.Browser")
        .arg("--type=method_call")
        .arg("--print-reply")
        .arg("/org/brave/Browser")
        .arg("org.brave.Browser.GetCurrentURL")
        .output()
        .ok()?;
    
    let url_output = String::from_utf8_lossy(&output.stdout);
    let url = url_output.lines()
        .find(|line| line.contains("string"))
        .and_then(|line| line.split('"').nth(1))
        .unwrap_or("")
        .to_string();
    
    if url.is_empty() {
        return None;
    }
    
    // Get title
    let title_output = Command::new("dbus-send")
        .arg("--session")
        .arg("--dest=org.brave.Browser")
        .arg("--type=method_call")
        .arg("--print-reply")
        .arg("/org/brave/Browser")
        .arg("org.brave.Browser.GetCurrentTitle")
        .output()
        .ok()?;
    
    let title_text = String::from_utf8_lossy(&title_output.stdout);
    let title = title_text.lines()
        .find(|line| line.contains("string"))
        .and_then(|line| line.split('"').nth(1))
        .unwrap_or("Unknown")
        .to_string();
    
    Some(BrowserInfo {
        url,
        title,
        favicon_url: None, // Brave doesn't expose favicon via DBus
        can_go_back: true, // Assume true for now
        can_go_forward: true, // Assume true for now
        is_loading: false, // Assume false for now
    })
}

fn get_edge_info() -> Option<BrowserInfo> {
    // Edge uses org.microsoft.Edge DBus interface
    let output = Command::new("dbus-send")
        .arg("--session")
        .arg("--dest=org.microsoft.Edge")
        .arg("--type=method_call")
        .arg("--print-reply")
        .arg("/org/microsoft/Edge")
        .arg("org.microsoft.Edge.GetCurrentURL")
        .output()
        .ok()?;
    
    let url_output = String::from_utf8_lossy(&output.stdout);
    let url = url_output.lines()
        .find(|line| line.contains("string"))
        .and_then(|line| line.split('"').nth(1))
        .unwrap_or("")
        .to_string();
    
    if url.is_empty() {
        return None;
    }
    
    // Get title
    let title_output = Command::new("dbus-send")
        .arg("--session")
        .arg("--dest=org.microsoft.Edge")
        .arg("--type=method_call")
        .arg("--print-reply")
        .arg("/org/microsoft/Edge")
        .arg("org.microsoft.Edge.GetCurrentTitle")
        .output()
        .ok()?;
    
    let title_text = String::from_utf8_lossy(&title_output.stdout);
    let title = title_text.lines()
        .find(|line| line.contains("string"))
        .and_then(|line| line.split('"').nth(1))
        .unwrap_or("Unknown")
        .to_string();
    
    Some(BrowserInfo {
        url,
        title,
        favicon_url: None, // Edge doesn't expose favicon via DBus
        can_go_back: true, // Assume true for now
        can_go_forward: true, // Assume true for now
        is_loading: false, // Assume false for now
    })
}

fn execute_browser_command(command: &str, args: &[&str]) -> bool {
    // Try different browsers in order of preference
    let browsers = [
        ("org.mozilla.firefox", "/org/mozilla/firefox"),
        ("org.chromium.Chromium", "/org/chromium/Chromium"),
        ("org.brave.Browser", "/org/brave/Browser"),
        ("org.microsoft.Edge", "/org/microsoft/Edge"),
    ];
    
    for (dest, path) in &browsers {
        let mut cmd = Command::new("dbus-send");
        cmd.arg("--session")
           .arg(format!("--dest={}", dest))
           .arg("--type=method_call")
           .arg(path)
           .arg(command);
        
        for arg in args {
            cmd.arg(arg);
        }
        
        match cmd.output() {
            Ok(output) => {
                if output.status.success() {
                    eprintln!("[browser-helper] Command '{}' executed successfully on {}", command, dest);
                    return true;
                }
            }
            Err(_) => {
                // Continue to next browser
                continue;
            }
        }
    }
    
    eprintln!("[browser-helper] Command '{}' failed on all browsers", command);
    false
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

fn handle_command(command: &str) {
    match command.trim() {
        "back" => {
            eprintln!("[browser-helper] Executing back command");
            execute_browser_command("org.mozilla.firefox.GoBack", &[]);
        }
        "forward" => {
            eprintln!("[browser-helper] Executing forward command");
            execute_browser_command("org.mozilla.firefox.GoForward", &[]);
        }
        "refresh" => {
            eprintln!("[browser-helper] Executing refresh command");
            execute_browser_command("org.mozilla.firefox.Reload", &[]);
        }
        "home" => {
            eprintln!("[browser-helper] Executing home command");
            execute_browser_command("org.mozilla.firefox.GoHome", &[]);
        }
        "add_bookmark" => {
            eprintln!("[browser-helper] Executing add bookmark command");
            // Use xdotool to simulate Ctrl+D (add bookmark)
            let _ = Command::new("xdotool").arg("key").arg("ctrl+d").output();
        }
        "new_tab" => {
            eprintln!("[browser-helper] Executing new tab command");
            // Use xdotool to simulate Ctrl+T (new tab)
            let _ = Command::new("xdotool").arg("key").arg("ctrl+t").output();
        }
        "focus_address_bar" => {
            eprintln!("[browser-helper] Executing focus address bar command");
            focus_address_bar();
        }
        "get_url_info" => {
            eprintln!("[browser-helper] Executing get URL info command");
            // This will be handled by the status update loop
        }
        _ => {
            eprintln!("[browser-helper] Unknown command: {}", command);
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
    
    eprintln!("[browser-helper] Connected to socket, starting browser monitoring...");
    
    // Create a reader for incoming commands
    let mut stream_clone = stream.try_clone()?;
    let mut buffer = Vec::new();
    
    // Track last status to avoid sending duplicate updates
    let mut last_status: Option<String> = None;
    
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
                    if !command.is_empty() {
                        eprintln!("[browser-helper] Received command: {}", command);
                        handle_command(command);
                        
                        // After executing a command, immediately send status update
                        if let Some(browser_info) = get_browser_info() {
                            let status_json = json!({
                                "url": browser_info.url,
                                "title": browser_info.title,
                                "favicon_url": browser_info.favicon_url,
                                "can_go_back": browser_info.can_go_back,
                                "can_go_forward": browser_info.can_go_forward,
                                "is_loading": browser_info.is_loading,
                            });
                            
                            let status_str = status_json.to_string();
                            if last_status.as_ref() != Some(&status_str) {
                                if stream.write_all(format!("{}\n", status_str).as_bytes()).is_ok() {
                                    last_status = Some(status_str);
                                    eprintln!("[browser-helper] Sent status update after command");
                                } else {
                                    eprintln!("[browser-helper] Failed to send browser status after command");
                                }
                            }
                        }
                    }
                }
            }
            Err(e) => {
                if e.kind() == std::io::ErrorKind::WouldBlock {
                    // No data available, don't send status updates continuously
                    // Only send when there are commands or when explicitly requested
                } else {
                    eprintln!("[browser-helper] Error reading from socket: {}", e);
                    break;
                }
            }
        }
        
        // Only send status update when explicitly requested or when there's a change
        // This makes it event-driven instead of time-based
        if let Some(browser_info) = get_browser_info() {
            let status_json = json!({
                "url": browser_info.url,
                "title": browser_info.title,
                "favicon_url": browser_info.favicon_url,
                "can_go_back": browser_info.can_go_back,
                "can_go_forward": browser_info.can_go_forward,
                "is_loading": browser_info.is_loading,
            });
            
            let status_str = status_json.to_string();
            if last_status.as_ref() != Some(&status_str) {
                if stream.write_all(format!("{}\n", status_str).as_bytes()).is_ok() {
                    last_status = Some(status_str);
                    eprintln!("[browser-helper] Sent status update due to change");
                } else {
                    eprintln!("[browser-helper] Failed to send browser status, will retry");
                }
            }
        }
        
        // Small sleep to prevent busy waiting, but much shorter than before
        thread::sleep(Duration::from_millis(10)); // 10ms instead of 100ms
    }
    
    Ok(())
} 