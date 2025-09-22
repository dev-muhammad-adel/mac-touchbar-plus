use std::process::Command;
use anyhow::Result;
use crate::services::sessionmanager::SessionState;
use crate::utils::get_env::get_env_from_session;

/// Send a desktop notification to the user
pub fn send_notification(title: &str, message: &str, session: &Option<SessionState>) -> Result<()> {
    // Get the current user from session or environment
    let user = if let Some(s) = session {
        if !s.user.is_empty() {
            s.user.clone()
        } else {
            std::env::var("SUDO_USER")
                .or_else(|_| std::env::var("USER"))
                .unwrap_or_else(|_| "root".to_string())
        }
    } else {
        std::env::var("SUDO_USER")
            .or_else(|_| std::env::var("USER"))
            .unwrap_or_else(|_| "root".to_string())
    };
    
    // Get environment variables from session if available
    let env_vars = if let Some(s) = session {
        if let Some(leader_pid) = s.leader {
            get_env_from_session(&user, leader_pid)
        } else {
            std::collections::HashMap::new()
        }
    } else {
        std::collections::HashMap::new()
    };
    
    // Try to get the user's display and session info from environment or session
    let display = env_vars.get("DISPLAY")
        .or_else(|| env_vars.get("WAYLAND_DISPLAY"))
        .map(|s| s.clone())
        .unwrap_or_else(|| std::env::var("DISPLAY").unwrap_or_else(|_| ":0".to_string()));
    
    let xdg_runtime_dir = env_vars.get("XDG_RUNTIME_DIR")
        .map(|s| s.clone())
        .unwrap_or_else(|| {
            match get_user_id(&user) {
                Ok(uid) => format!("/run/user/{}", uid),
                Err(_) => "/tmp".to_string(), // Fallback to /tmp if we can't get user ID
            }
        });
    
    // Try multiple approaches to send notification
    let approaches = vec![
        // Approach 1: Use sudo with full environment
        try_notify_with_sudo(&user, &display, &xdg_runtime_dir, &env_vars, title, message),
        // Approach 2: Try direct notify-send (if running as user)
        try_notify_direct(&display, title, message),
        // Approach 3: Use systemd-run to run as user
        try_notify_with_systemd(&user, &display, title, message),
    ];
    
    // Try each approach until one succeeds
    for approach in approaches {
        match approach {
            Ok(_) => return Ok(()),
            Err(e) => {
                eprintln!("[notification] Notification approach failed: {}", e);
                continue;
            }
        }
    }
    
    Err(anyhow::anyhow!("All notification approaches failed"))
}

/// Send a screenshot notification with custom title and message
pub fn send_screenshot_notification(filename: &str, session: &Option<SessionState>) {
    // Extract just the filename from the full path for display
    let display_name = std::path::Path::new(filename)
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("screenshot.png");
    
    let title = "Screenshot Captured";
    let message = format!("Touchbar screenshot saved as {}\nPath: {}", display_name, filename);
    
    // Try to send notification using notify-send as the user
    if let Err(e) = send_notification(title, &message, session) {
        eprintln!("[screenshot] Warning: Failed to send notification: {}", e);
    }
}

/// Try to send notification using sudo with full environment
fn try_notify_with_sudo(
    user: &str, 
    display: &str, 
    xdg_runtime_dir: &str, 
    env_vars: &std::collections::HashMap<String, String>, 
    title: &str,
    message: &str
) -> Result<()> {
    // Build environment variables for sudo
    let mut env_args = Vec::new();
    env_args.push("-u".to_string());
    env_args.push(user.to_string());
    env_args.push("env".to_string());
    
    // Add essential environment variables
    env_args.push(format!("DISPLAY={}", display));
    env_args.push(format!("XDG_RUNTIME_DIR={}", xdg_runtime_dir));
    
    // Add other important environment variables from session
    for (key, value) in env_vars {
        match key.as_str() {
            "DBUS_SESSION_BUS_ADDRESS" | "WAYLAND_DISPLAY" | "HYPRLAND_INSTANCE_SIGNATURE" | 
            "NIRI_SOCKET" | "SWAYSOCK" | "I3SOCK" | "XDG_CURRENT_DESKTOP" | "DESKTOP_SESSION" => {
                env_args.push(format!("{}={}", key, value));
            }
            _ => {} // Skip other variables to avoid clutter
        }
    }
    
    // Add notify-send command and arguments
    env_args.push("notify-send".to_string());
    env_args.push(title.to_string());
    env_args.push(message.to_string());
    env_args.push("--icon=camera-photo".to_string());
    env_args.push("--expire-time=6000".to_string());
    
    let output = Command::new("sudo")
        .args(&env_args)
        .output()?;
    
    if !output.status.success() {
        let error_msg = String::from_utf8_lossy(&output.stderr);
        return Err(anyhow::anyhow!("sudo notify-send failed: {}", error_msg));
    }
    Ok(())
}

/// Try to send notification directly (if running as user)
fn try_notify_direct(display: &str, title: &str, message: &str) -> Result<()> {
    let output = Command::new("notify-send")
        .args(&[
            title,
            message,
            "--icon=camera-photo",
            "--expire-time=6000"
        ])
        .env("DISPLAY", display)
        .output()?;
    
    if !output.status.success() {
        let error_msg = String::from_utf8_lossy(&output.stderr);
        return Err(anyhow::anyhow!("notify-send failed: {}", error_msg));
    }
    Ok(())
}

/// Try to send notification using systemd-run
fn try_notify_with_systemd(user: &str, display: &str, title: &str, message: &str) -> Result<()> {
    let output = Command::new("systemd-run")
        .args(&[
            "--user",
            "--uid", user,
            "notify-send",
            title,
            message,
            "--icon=camera-photo",
            "--expire-time=6000"
        ])
        .env("DISPLAY", display)
        .output()?;
    
    if !output.status.success() {
        let error_msg = String::from_utf8_lossy(&output.stderr);
        return Err(anyhow::anyhow!("systemd-run notify-send failed: {}", error_msg));
    }
    Ok(())
}

/// Get user ID from username
fn get_user_id(username: &str) -> Result<String> {
    let output = Command::new("id")
        .arg("-u")
        .arg(username)
        .output()
        .map_err(|e| anyhow::anyhow!("Failed to get user ID for '{}': {}", username, e))?;
    
    if !output.status.success() {
        let error_msg = String::from_utf8_lossy(&output.stderr);
        return Err(anyhow::anyhow!("User '{}' not found: {}", username, error_msg));
    }
    
    let uid_str = String::from_utf8(output.stdout)
        .map_err(|e| anyhow::anyhow!("Failed to parse user ID: {}", e))?;
    Ok(uid_str.trim().to_string())
}
