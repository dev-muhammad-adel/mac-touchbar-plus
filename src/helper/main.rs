//! Helper binary for tiny-dfr, providing auxiliary functionality.
use std::os::unix::net::UnixStream;
use std::io::Write;
use std::thread;
use std::time::Duration;
use std::process::Command;

fn get_active_window_class() -> Option<String> {
    // Get the active window ID
    let output = Command::new("xdotool")
        .arg("getactivewindow")
        .output()
        .ok()?;
    let id = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if id.is_empty() || id == "0" {
        return Some("Desktop".to_string());
    }
    // Get the WM_CLASS property
    let output = Command::new("xprop")
        .arg("-id")
        .arg(&id)
        .arg("WM_CLASS")
        .output();
    match output {
        Ok(output) => {
            let out = String::from_utf8_lossy(&output.stdout);
            if out.contains("not found") || out.trim().is_empty() {
                return Some("Desktop".to_string());
            }
            // Example output: WM_CLASS(STRING) = "org.gnome.Nautilus", "org.gnome.Nautilus"
            let class = out.split('=').nth(1)?.trim();
            // Split by comma, take the second part, and strip quotes
            let class_name = class.split(',').last()?.trim().trim_matches('"');
            Some(class_name.to_string())
        }
        Err(_) => Some("Desktop".to_string()),
    }
}

fn get_sway_active_window_class() -> Option<String> {
    // Try using swaymsg with jq first (prioritize class over app_id)
    let output = Command::new("sh")
        .arg("-c")
        .arg("swaymsg -t get_tree | jq -r '.. | objects | select(.focused==true) | .window_properties.class // .app_id // .name // empty'")
        .output()
        .ok()?;
    
    let output_str = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if !output_str.is_empty() && output_str != "null" {
        // Check if the result looks like a workspace number (just digits)
        if output_str.chars().all(|c| c.is_ascii_digit()) {
            // This is likely a workspace number, not a window - return Desktop
            return Some("Desktop".to_string());
        }
        return Some(output_str);
    }
    
    // Fallback: try using swayipc
    if let Ok(mut connection) = swayipc::Connection::new() {
        if let Ok(tree) = connection.get_tree() {
            if let Some(focused) = tree.find_focused(|n| n.focused) {
                // Try to get the window class from the focused node
                if let Some(window_properties) = &focused.window_properties {
                    if let Some(class) = &window_properties.class {
                        if !class.is_empty() && class != "null" {
                            return Some(class.clone());
                        }
                    }
                }
                // Fallback to window name if class is not available
                if let Some(name) = &focused.name {
                    if !name.is_empty() && name != "null" {
                        return Some(name.clone());
                    }
                }
                // Try to get instance as another fallback
                if let Some(window_properties) = &focused.window_properties {
                    if let Some(instance) = &window_properties.instance {
                        if !instance.is_empty() && instance != "null" {
                            return Some(instance.clone());
                        }
                    }
                }
            }
        }
    }
    
    None
}

fn get_hyprland_active_window_class() -> Option<String> {
    // Use hyprctl activewindow with JSON output and jq
    // Environment variables should be passed from the main process
    let output = Command::new("sh")
        .arg("-c")
        .arg("hyprctl activewindow -j | jq -r '.class // .title // empty'")
        .output();
    
    match output {
        Ok(output) => {
            let output_str = String::from_utf8_lossy(&output.stdout).trim().to_string();
            eprintln!("[helper] Hyprland JSON output: '{}'", output_str);
            
            // If we got a valid class/title, return it
            if !output_str.is_empty() && output_str != "null" {
                eprintln!("[helper] Returning class: {}", output_str);
                return Some(output_str);
            }
            
            // Empty string means no active window, return Desktop
            if output_str.is_empty() {
                eprintln!("[helper] Empty output, returning Desktop");
                return Some("Desktop".to_string());
            }
        }
        Err(e) => {
            eprintln!("[helper] jq command failed: {}", e);
            // If jq command failed, fall back to text parsing
        }
    }
    
    // Fallback: parse text output
    let output = Command::new("hyprctl")
        .arg("activewindow")
        .output()
        .ok()?;
    
    let output_str = String::from_utf8_lossy(&output.stdout);
    
    // Check if there's no active window
    if output_str.trim().is_empty() || output_str.contains("Invalid") {
        return Some("Desktop".to_string());
    }
    
    // Parse class from text output
    for line in output_str.lines() {
        if line.trim().starts_with("class:") {
            let class_trimmed = line.split("class:").nth(1)?.trim();
            if !class_trimmed.is_empty() && class_trimmed != "null" {
                return Some(class_trimmed.to_string());
            }
        }
    }
    
    // Parse title as fallback
    for line in output_str.lines() {
        if line.trim().starts_with("title:") {
            let title_trimmed = line.split("title:").nth(1)?.trim();
            if !title_trimmed.is_empty() && title_trimmed != "null" {
                return Some(title_trimmed.to_string());
            }
        }
    }
    
    // If nothing found, return Desktop
    Some("Desktop".to_string())
}

fn get_generic_wayland_active_window() -> Option<String> {
    // Try using wlr-foreign-toplevel-management if available
    // This is a generic approach that might work with various Wayland compositors
    
    // Try to get active window using wlrctl if available
    let output = Command::new("wlrctl")
        .arg("toplevel")
        .arg("list")
        .output();
    
    if let Ok(output) = output {
        let output_str = String::from_utf8_lossy(&output.stdout);
        // Parse wlrctl output to find active window
        for line in output_str.lines() {
            if line.contains("active") || line.contains("focused") {
                // Extract app_id or title from the line
                if let Some(app_id) = line.split_whitespace().next() {
                    if !app_id.is_empty() && app_id != "null" {
                        return Some(app_id.to_string());
                    }
                }
            }
        }
    }
    
    // Try using wtype to get window info (if available)
    let output = Command::new("wtype")
        .arg("-M")
        .arg("ctrl")
        .arg("-k")
        .arg("f1")
        .output();
    
    // This is just a test to see if wtype is available
    if output.is_ok() {
        // wtype is available, but we can't easily get window info with it
        // This is just a placeholder for future implementation
    }
    
    None
}

fn detect_and_get_active_window_class() -> Option<String> {
    // Check for Hyprland first (since it's more specific)
    // Check if hyprctl is available to confirm we're on Hyprland
    if Command::new("hyprctl").arg("version").output().is_ok() {
        eprintln!("[helper] Hyprland detected via hyprctl");
        if let Some(class) = get_hyprland_active_window_class() {
            return Some(class);
        }
    }
    
    // Check for Wayland
    if std::env::var("WAYLAND_DISPLAY").is_ok() {
        eprintln!("[helper] Wayland detected via WAYLAND_DISPLAY");
        
        // Fallback to Sway logic
        if let Some(class) = get_sway_active_window_class() {
            return Some(class);
        }
        
        // Try generic Wayland detection
        if let Some(class) = get_generic_wayland_active_window() {
            return Some(class);
        }
        
        eprintln!("[helper] No Wayland compositor detected or no focused window");
    }
    
    // Check for X11
    if std::env::var("DISPLAY").is_ok() {
        eprintln!("[helper] X11 detected via DISPLAY");
        if let Some(class) = get_active_window_class() {
            return Some(class);
        } else {
            eprintln!("[helper] xprop: Could not get active window class");
        }
    }
    
    eprintln!("[helper] No supported compositor detected (not X11, Sway, Hyprland, or generic Wayland)");
    None
}

fn main() -> std::io::Result<()> {
    let socket_path = "/tmp/touchbar.sock";
    if let Ok(addr) = std::env::var("DBUS_SESSION_BUS_ADDRESS") {
        eprintln!("[helper] DBUS_SESSION_BUS_ADDRESS={}", addr);
    } else {
        eprintln!("[helper] DBUS_SESSION_BUS_ADDRESS is not set");
    }
    eprintln!("[helper] DISPLAY={:?}", std::env::var("DISPLAY"));
    eprintln!("[helper] WAYLAND_DISPLAY={:?}", std::env::var("WAYLAND_DISPLAY"));
    let mut stream = loop {
        match UnixStream::connect(socket_path) {
            Ok(stream) => break stream,
            Err(_) => {
                thread::sleep(Duration::from_millis(100));
                continue;
            }
        }
    };
    let mut last_class = String::new();
    loop {
        if let Some(class) = detect_and_get_active_window_class() {
            if class != last_class {
                if stream.write_all(class.as_bytes()).is_ok() {
                    if stream.write_all(b"\n").is_ok() {
                        last_class = class;
                    } else {
                        break;
                    }
                } else {
                    break;
                }
            }
        }
        thread::sleep(Duration::from_millis(500));
    }
    Ok(())
} 