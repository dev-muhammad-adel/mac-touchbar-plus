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

fn get_sway_active_window_title() -> Option<String> {
    use swayipc::Connection as SwayConnection;
    let mut connection = SwayConnection::new().ok()?;
    let tree = connection.get_tree().ok()?;
    let focused = tree.find_focused(|n| n.focused);
    match focused {
        Some(node) => node.name.or(Some("Desktop".to_string())),
        None => Some("Desktop".to_string()),
    }
}



fn detect_and_get_active_window_class() -> Option<String> {
    if std::env::var("WAYLAND_DISPLAY").is_ok() {
        // Sway logic (unchanged)
        if let Some(title) = get_sway_active_window_title() {
            return Some(title);
        } else {
            eprintln!("[helper] Sway IPC not available or no focused window");
        }
    }
    if std::env::var("DISPLAY").is_ok() {
        if let Some(class) = get_active_window_class() {
            return Some(class);
        } else {
            eprintln!("[helper] xprop: Could not get active window class");
        }
    }
    eprintln!("[helper] No supported compositor detected (not X11 or Sway)");
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