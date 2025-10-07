use std::collections::HashMap;
use std::fs;
use nix::unistd::User;



fn collect_first_two(pid: i32, out: &mut Vec<i32>) {
    let children = fs::read_to_string(format!("/proc/{}/task/{}/children", pid, pid))
        .unwrap_or_default();

    let mut iter = children.split_whitespace().filter_map(|c| c.parse::<i32>().ok());

    // take first two children
    for child in iter.by_ref().take(2) {
        out.push(child);
        collect_first_two(child, out);
    }
}

pub fn get_env_from_session(user: &str, leader_pid: u32) -> HashMap<String, String> {
    let mut env = HashMap::new();
    
    // NEW APPROACH: Use collect_first_two instead of pstree
    
    let mut main_level_pids: Vec<u32> = Vec::new();
    
    // Use collect_first_two to get the first two children at each level
    let mut collected_pids = Vec::new();
    collect_first_two(leader_pid as i32, &mut collected_pids);
    
    // Add the leader PID first, then the collected children
    main_level_pids.push(leader_pid);
    for &pid in &collected_pids {
        main_level_pids.push(pid as u32);
    }
    
    
    // Accumulate environment from each main tree level, with later levels overriding earlier ones
    for (i, &pid) in main_level_pids.iter().enumerate() {
        let path = format!("/proc/{}/environ", pid);
        if let Ok(data) = fs::read(&path) {
            let mut env_count = 0;
            for entry in data.split(|&b| b == 0) {
                if entry.is_empty() {
                    continue;
                }
                
                if let Some(eq) = entry.iter().position(|&b| b == b'=') {
                    let key = String::from_utf8_lossy(&entry[..eq]).to_string();
                    let value = String::from_utf8_lossy(&entry[eq+1..]).to_string();
                    
                    // Insert or override (later levels take precedence)
                    env.insert(key, value);
                    env_count += 1;
                }
            }
        } else {
           
        }
    }
    
    // Debug: Show key environment variables collected
    for key in ["DISPLAY", "WAYLAND_DISPLAY", "XDG_RUNTIME_DIR", "DBUS_SESSION_BUS_ADDRESS", "HYPRLAND_INSTANCE_SIGNATURE", "NIRI_SOCKET", "SWAYSOCK", "I3SOCK", "XDG_CURRENT_DESKTOP"] {
        if let Some(value) = env.get(key) {
            // println!("[get_env_from_session]   {}={}", key, value);
        } else {
            // println!("[get_env_from_session]   {} (not found)", key);
        }
    }
    
    // FALLBACK MECHANISMS
    
    // Fallback 1: XDG_RUNTIME_DIR fallback to /run/user/<UID>
    if !env.contains_key("XDG_RUNTIME_DIR") {
        // Get user UID
        if let Ok(Some(userinfo)) = User::from_name(user) {
            let xdg_runtime_dir = format!("/run/user/{}", userinfo.uid);
            if std::path::Path::new(&xdg_runtime_dir).exists() {
                env.insert("XDG_RUNTIME_DIR".to_string(), xdg_runtime_dir);
            }
        }
    }
    
    // Fallback 2: DBUS_SESSION_BUS_ADDRESS fallback
    if !env.contains_key("DBUS_SESSION_BUS_ADDRESS") {
        if let Some(xdg_runtime) = env.get("XDG_RUNTIME_DIR") {
            let dbus_socket_path = format!("{}/bus", xdg_runtime);
            if std::path::Path::new(&dbus_socket_path).exists() {
                let dbus_address = format!("unix:path={}", dbus_socket_path);
                env.insert("DBUS_SESSION_BUS_ADDRESS".to_string(), dbus_address);
            }
        }
    }
    
    // Fallback 3: WAYLAND_DISPLAY fallback
    if !env.contains_key("WAYLAND_DISPLAY") {
        if let Some(xdg_runtime) = env.get("XDG_RUNTIME_DIR") {
            let xdg_runtime = xdg_runtime.clone(); // Clone to avoid borrow conflict
            
            // Look for wayland socket files with numeric suffix (wayland-0.sock, wayland-1.sock, etc.)
            let mut wayland_display = None;
            if let Ok(entries) = fs::read_dir(&xdg_runtime) {
                for entry in entries {
                    if let Ok(entry) = entry {
                        let file_name = entry.file_name();
                        if let Some(name) = file_name.to_str() {
                            // Match pattern: wayland- followed by digits and ending with .sock
                            if name.starts_with("wayland-") && name.ends_with(".sock") {
                                // Extract the part between "wayland-" and ".sock"
                                let suffix = &name[8..name.len()-5]; // Remove "wayland-" prefix and ".sock" suffix
                                // Check if the suffix is numeric
                                if suffix.chars().all(|c| c.is_ascii_digit()) {
                                    wayland_display = Some(name.to_string());
                                    break;
                                }
                            }
                        }
                    }
                }
            }
            
            // If still no wayland socket found, try common fallbacks
            if wayland_display.is_none() {
                let common_wayland_names = ["wayland-0", "wayland-1", "wayland-2"];
                for wayland_name in &common_wayland_names {
                    let wayland_path = format!("{}/{}", xdg_runtime, wayland_name);
                    if std::path::Path::new(&wayland_path).exists() {
                        wayland_display = Some(wayland_name.to_string());
                        break;
                    }
                }
            }
            
            // Insert the wayland display if found
            if let Some(display) = wayland_display {
                env.insert("WAYLAND_DISPLAY".to_string(), display);
            }
        }
    }
    

    // Fallback 4: NIRI_SOCKET fallback
    if !env.contains_key("NIRI_SOCKET") {
        if let Some(xdg_runtime) = env.get("XDG_RUNTIME_DIR") {
            let xdg_runtime = xdg_runtime.clone(); // Clone to avoid borrow conflict
            
            // Look for niri.*.sock files
            if let Ok(entries) = fs::read_dir(&xdg_runtime) {
                for entry in entries {
                    if let Ok(entry) = entry {
                        let file_name = entry.file_name();
                        if let Some(name) = file_name.to_str() {
                            if name.starts_with("niri.") && name.ends_with(".sock") {
                                let niri_socket_path = format!("{}/{}", xdg_runtime, name);
                                env.insert("NIRI_SOCKET".to_string(), niri_socket_path);
                                break;
                            }
                        }
                    }
                }
            }
        }
    }
    
    // Fallback 5: SWAYSOCK fallback
    if !env.contains_key("SWAYSOCK") {
        if let Some(xdg_runtime) = env.get("XDG_RUNTIME_DIR") {
            let xdg_runtime = xdg_runtime.clone(); // Clone to avoid borrow conflict
            
            // Look for sway-ipc.*.sock files
            if let Ok(entries) = fs::read_dir(&xdg_runtime) {
                for entry in entries {
                    if let Ok(entry) = entry {
                        let file_name = entry.file_name();
                        if let Some(name) = file_name.to_str() {
                            if name.starts_with("sway-ipc.") && name.ends_with(".sock") {
                                let sway_socket_path = format!("{}/{}", xdg_runtime, name);
                                env.insert("SWAYSOCK".to_string(), sway_socket_path);
                                break;
                            }
                        }
                    }
                }
            }
        }
    }
    
    // Fallback 6: I3SOCK fallback
    if !env.contains_key("I3SOCK") {
        if let Some(xdg_runtime) = env.get("XDG_RUNTIME_DIR") {
            let xdg_runtime = xdg_runtime.clone(); // Clone to avoid borrow conflict
            let i3_socket_dir = format!("{}/i3", xdg_runtime);
            if let Ok(entries) = fs::read_dir(&i3_socket_dir) {
                for entry in entries {
                    if let Ok(entry) = entry {
                        let file_name = entry.file_name();
                        if let Some(name) = file_name.to_str() {
                            if name.starts_with("ipc-socket.") {
                                let i3_socket_path = format!("{}/{}", i3_socket_dir, name);
                                env.insert("I3SOCK".to_string(), i3_socket_path);
                                break;
                            }
                        }
                    }
                }
            }
        }
    }
    
    // Fallback 7: HYPRLAND_INSTANCE_SIGNATURE fallback (only for Hyprland)
    if !env.contains_key("HYPRLAND_INSTANCE_SIGNATURE") {
        // Only do this fallback if XDG_CURRENT_DESKTOP is set to Hyprland
        if let Some(current_desktop) = env.get("XDG_CURRENT_DESKTOP") {
            if current_desktop.to_lowercase() == "hyprland" {
                if let Some(xdg_runtime) = env.get("XDG_RUNTIME_DIR") {
                    let xdg_runtime = xdg_runtime.clone(); // Clone to avoid borrow conflict
                    let hypr_dir = format!("{}/hypr", xdg_runtime);
                    
                    // Look for hypr directory and get the instance signature from subdirectory names
                    if let Ok(entries) = fs::read_dir(&hypr_dir) {
                        let mut instances = Vec::new();
                        
                        for entry in entries {
                            if let Ok(entry) = entry {
                                let file_name = entry.file_name();
                                if let Some(name) = file_name.to_str() {
                                    // Check if this is a directory (Hyprland instance directories)
                                    if entry.file_type().map(|ft| ft.is_dir()).unwrap_or(false) {
                                        // Get metadata to sort by modification time
                                        if let Ok(metadata) = entry.metadata() {
                                            if let Ok(modified) = metadata.modified() {
                                                instances.push((name.to_string(), modified));
                                            } else {
                                                instances.push((name.to_string(), std::time::SystemTime::UNIX_EPOCH));
                                            }
                                        } else {
                                            instances.push((name.to_string(), std::time::SystemTime::UNIX_EPOCH));
                                        }
                                    }
                                }
                            }
                        }
                        
                        // Sort by modification time (most recent first) and take the first one
                        instances.sort_by(|a, b| b.1.cmp(&a.1));
                        
                        if let Some((instance_signature, _)) = instances.first() {
                            env.insert("HYPRLAND_INSTANCE_SIGNATURE".to_string(), instance_signature.clone());
                        }
                    }
                }
            }
        }
    }
    
    // Fallback: Set GNOME_DESKTOP_SESSION_ID if we detect GNOME from other variables
    if !env.contains_key("GNOME_DESKTOP_SESSION_ID") {
        if let Some(desktop_session) = env.get("DESKTOP_SESSION") {
            if desktop_session.to_lowercase().contains("gnome") {
                env.insert("GNOME_DESKTOP_SESSION_ID".to_string(), "gnome".to_string());
            }
        }
    }
    
    env
}
fn get_env_from_pid(pid: u32) -> HashMap<String, String> {
    let mut env = HashMap::new();
    let path = format!("/proc/{}/environ", pid);
    
    if let Ok(data) = fs::read(&path) {
        for entry in data.split(|&b| b == 0) {
            if entry.is_empty() {
                continue;
            }
            
            if let Some(eq) = entry.iter().position(|&b| b == b'=') {
                let key = String::from_utf8_lossy(&entry[..eq]).to_string();
                let value = String::from_utf8_lossy(&entry[eq+1..]).to_string();
                env.insert(key, value);
            }
        }
    } else {
        println!("[get_env_from_pid] ERROR: Failed to read environment from /proc/{}/environ", pid);
    }
    
    env
}
