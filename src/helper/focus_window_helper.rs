//! Helper binary for tiny-dfr, providing auxiliary functionality.
use std::os::unix::net::UnixStream;
use std::io::{Write, BufRead, BufReader};
use std::thread;
use std::time::Duration;
use x11rb::connection::Connection;

use x11rb::protocol::xproto::{self, ConnectionExt, Window};
use x11rb::rust_connection::RustConnection;
use std::collections::HashMap;
use std::sync::mpsc;
use zbus::{Connection as ZbusConnection, MessageType, MessageStream, MatchRule};
use zbus::fdo::DBusProxy;
use futures_lite::stream::StreamExt;

struct X11WindowMonitor {
    conn: RustConnection,
    root: Window,
    active_window: Option<Window>,
    window_classes: HashMap<Window, String>,
    net_active_window_atom: xproto::Atom,
}

impl X11WindowMonitor {
    fn new() -> Result<Self, Box<dyn std::error::Error>> {
        let (conn, screen_num) = x11rb::connect(None)?;
        let screen = &conn.setup().roots[screen_num];
        let root = screen.root;
        
        // Get the _NET_ACTIVE_WINDOW atom
        let net_active_window_atom = conn.intern_atom(false, b"_NET_ACTIVE_WINDOW")?.reply()?.atom;
        
        Ok(Self {
            conn,
            root,
            active_window: None,
            window_classes: HashMap::new(),
            net_active_window_atom,
        })
    }
    
    fn get_window_class(&mut self, window: Window) -> Option<String> {
        // Check cache first
        if let Some(class) = self.window_classes.get(&window) {
            return Some(class.clone());
        }
        
        // Get WM_CLASS property
        let cookie = self.conn.get_property(
            false,
            window,
            xproto::AtomEnum::WM_CLASS,
            xproto::AtomEnum::STRING,
            0,
            1024,
        );
        
        match cookie.ok()?.reply() {
            Ok(reply) => {
                if let Ok(class_name) = String::from_utf8(reply.value) {
                    // WM_CLASS format: "instance\0class\0"
                    if let Some(class) = class_name.split('\0').nth(1) {
                        if !class.is_empty() {
                            let class_str = class.to_string();
                            self.window_classes.insert(window, class_str.clone());
                            return Some(class_str);
                        }
                    }
                }
            }
            Err(_) => {}
        }
        
        None
    }
    
    fn get_active_window(&mut self) -> Option<Window> {
        let cookie = self.conn.get_property(
            false,
            self.root,
            self.net_active_window_atom,
            xproto::AtomEnum::WINDOW,
            0,
            1,
        );
        
        match cookie.ok()?.reply() {
            Ok(reply) => {
                if reply.value.len() >= 4 {
                    let window_id = u32::from_ne_bytes([
                        reply.value[0],
                        reply.value[1],
                        reply.value[2],
                        reply.value[3],
                    ]);
                    
                    // Return the window ID even if it's 0 (empty workspace)
                    // We'll handle the 0 case in get_active_window_class()
                    eprintln!("[helper] Active window ID: {}", window_id);
                    return Some(window_id);
                }
            }
            Err(e) => {
                eprintln!("[helper] Error getting active window property: {:?}", e);
            }
        }
        
        None
    }
    
    fn get_active_window_class(&mut self) -> Option<String> {
        let active_window = self.get_active_window()?;
        
        // Check if there's actually an active window (not 0 or invalid)
        if active_window == 0 {
            eprintln!("[helper] No active window (empty workspace)");
            return Some("Desktop".to_string());
        }
        
        // Try to get the window class
        if let Some(class) = self.get_window_class(active_window) {
            // Update active window only if we successfully got a class
            if self.active_window != Some(active_window) {
                self.active_window = Some(active_window);
            }
            return Some(class);
        }
        
        // If we can't get the window class, fall back to Desktop
        eprintln!("[helper] Could not get window class for window {}, using Desktop", active_window);
        Some("Desktop".to_string())
    }
    
    fn setup_event_listening(&mut self) -> Result<(), Box<dyn std::error::Error>> {
        // Monitor the _NET_ACTIVE_WINDOW property on the root window
        // This is the most reliable way to detect focus changes in X11
        self.conn.change_window_attributes(
            self.root,
            &xproto::ChangeWindowAttributesAux::new().event_mask(
                xproto::EventMask::PROPERTY_CHANGE
            ),
        )?;
        
        eprintln!("[helper] X11 event listening setup complete - monitoring _NET_ACTIVE_WINDOW property changes");
        
        Ok(())
    }
    
    fn wait_for_focus_change(&mut self) -> Result<Option<String>, Box<dyn std::error::Error>> {
        // Wait for events
        let event = self.conn.wait_for_event()?;
        
        match event {
            x11rb::protocol::Event::PropertyNotify(ev) => {
                // Check if it's the active window property that changed
                if ev.atom == self.net_active_window_atom {
                    eprintln!("[helper] Active window property changed!");
                    return Ok(self.get_active_window_class());
                }
                
                // Ignore other property changes
                eprintln!("[helper] Other property changed: atom={}", ev.atom);
            }
            _ => {
                // Other events, ignore
                eprintln!("[helper] Ignoring non-property event: {:?}", event);
            }
        }
        
        Ok(None)
    }
}

struct SwayWindowMonitor;

impl SwayWindowMonitor {
    fn new() -> Result<Self, Box<dyn std::error::Error>> {
        Ok(Self)
    }
    
    fn get_active_window_class() -> Option<String> {
        if let Ok(mut connection) = swayipc::Connection::new() {
            if let Ok(tree) = connection.get_tree() {
                if let Some(focused) = tree.find_focused(|n| n.focused) {
                    // Check if this is actually a window (not a workspace)
                    if focused.node_type == swayipc::reply::NodeType::Con {
                        // First try to get window_properties.class (for XWayland windows)
                        if let Some(window_properties) = &focused.window_properties {
                            if let Some(class) = &window_properties.class {
                                if !class.is_empty() && class != "null" {
                                    return Some(class.clone());
                                }
                            }
                            // Try instance as fallback
                            if let Some(instance) = &window_properties.instance {
                                if !instance.is_empty() && instance != "null" {
                                    return Some(instance.clone());
                                }
                            }
                        }
                        
                        // For Wayland native windows, use app_id
                        if let Some(app_id) = &focused.app_id {
                            if !app_id.is_empty() && app_id != "null" {
                                return Some(app_id.clone());
                            }
                        }
                        
                        // Fallback to window name if nothing else is available
                        if let Some(name) = &focused.name {
                            if !name.is_empty() && name != "null" {
                                return Some(name.clone());
                            }
                        }
                    } else {
                        // This is a workspace, not a window - return Desktop
                        return Some("Desktop".to_string());
                    }
                }
            }
        }
        // If no focused window or error, return Desktop
        Some("Desktop".to_string())
    }
    
    fn run_event_monitor(tx: mpsc::Sender<String>) -> Result<(), Box<dyn std::error::Error>> {
        // Create a new connection for event subscription
        let connection = swayipc::Connection::new()?;
        
        // Subscribe to workspace and window events
        let events = connection.subscribe(&[swayipc::EventType::Workspace, swayipc::EventType::Window])?;
        
        // Get initial active window
        if let Some(class) = Self::get_active_window_class() {
            let _ = tx.send(class);
        }
        
        // Event loop
        for event in events {
            match event? {
                swayipc::reply::Event::Workspace(_) | swayipc::reply::Event::Window(_) => {
                    // Window or workspace changed, check active window
                    if let Some(class) = Self::get_active_window_class() {
                        let _ = tx.send(class);
                    }
                }
                _ => {}
            }
        }
        
        Ok(())
    }
}

struct GnomeWindowMonitor;

impl GnomeWindowMonitor {
    fn new() -> Result<Self, Box<dyn std::error::Error>> {
        Ok(Self)
    }
    
    fn get_initial_focused_class() -> Option<String> {
        // Get initial focused window class using WindowMonitorPro's FocusClass method
        let output = std::process::Command::new("gdbus")
            .arg("call")
            .arg("--session")
            .arg("--dest")
            .arg("org.gnome.Shell")
            .arg("--object-path")
            .arg("/org/gnome/Shell/Extensions/WindowMonitorPro")
            .arg("--method")
            .arg("org.gnome.Shell.Extensions.WindowMonitorPro.FocusClass")
            .output();
        
        match output {
            Ok(output) => {
                eprintln!("[helper] FocusClass command output: status={}, stdout='{}'", output.status, String::from_utf8_lossy(&output.stdout));
                if output.status.success() {
                    let stdout = String::from_utf8_lossy(&output.stdout);
                    // Parse the output: ('Cursor',) -> Cursor or ('') -> Desktop
                    if let Some(start) = stdout.find("('") {
                        if let Some(end) = stdout[start..].find("',") {
                            let class = &stdout[start + 2..start + end];
                            eprintln!("[helper] Parsed window class: '{}'", class);
                            if class.is_empty() {
                                // Empty string means no window focused (desktop)
                                eprintln!("[helper] Empty window class, returning Desktop");
                                return Some("Desktop".to_string());
                            } else if class != "null" {
                                eprintln!("[helper] Valid window class, returning: {}", class);
                                return Some(class.to_string());
                            }
                        }
                    }
                    eprintln!("[helper] Failed to parse FocusClass output, will return install message");
                } else {
                    eprintln!("[helper] FocusClass command failed with status: {}", output.status);
                }
            }
            Err(e) => {
                eprintln!("[helper] FocusClass command error: {:?}", e);
            }
        }
        
        // If FocusClass failed, WindowMonitorPro is likely not working properly
        // Send install message to help user
        eprintln!("[helper] Returning install message for WindowMonitorPro");
        Some("Install WindowMonitorPro".to_string())
    }
    
    fn extract_window_class_from_signal(signal_body: &str) -> Option<String> {
        // Parse signal body format: (window_id, window_title, window_class, window_pid)
        // Example: ('2882485532', 'kitty', 'kitty', '47887')
        
        // Find the third parameter (window_class) which is the 3rd quoted string
        let mut quote_count = 0;
        let mut start_pos = None;
        let mut end_pos = None;
        
        for (i, ch) in signal_body.chars().enumerate() {
            if ch == '\'' {
                quote_count += 1;
                if quote_count == 5 { // Start of 3rd parameter (window_class)
                    start_pos = Some(i + 1);
                } else if quote_count == 6 { // End of 3rd parameter
                    end_pos = Some(i);
                    break;
                }
            }
        }
        
        if let (Some(start), Some(end)) = (start_pos, end_pos) {
            if start < end {
                let window_class = &signal_body[start..end];
                if window_class.is_empty() {
                    // Empty window class means desktop
                    return Some("Desktop".to_string());
                } else if window_class != "null" {
                    return Some(window_class.to_string());
                }
            }
        }
        
        None
    }
    
    fn run_event_monitor(tx: mpsc::Sender<String>) -> Result<(), Box<dyn std::error::Error>> {
        // Get initial focused window class
        if let Some(initial_class) = Self::get_initial_focused_class() {
            eprintln!("[helper] Initial focused window class: {}", initial_class);
            let _ = tx.send(initial_class);
        }
        
        // Use D-Bus signal subscription for true event-driven monitoring
        let rt = tokio::runtime::Runtime::new()?;
        rt.block_on(async {
            Self::run_dbus_event_monitor(tx).await
        })
    }
    
    async fn run_dbus_event_monitor(tx: mpsc::Sender<String>) -> Result<(), Box<dyn std::error::Error>> {
        let connection = ZbusConnection::session().await?;
        let mut stream = MessageStream::from(&connection);
        
        // Subscribe to GNOME Shell window manager signals
        let dbus_proxy = DBusProxy::new(&connection).await?;
        
        // Subscribe to the WindowMonitorPro extension signals for true event-driven window monitoring
        let rule = MatchRule::builder()
            .msg_type(MessageType::Signal)
            .interface("org.gnome.Shell.Extensions.WindowMonitorPro")?
            .member("WindowFocusChanged")?
            .build();
        

        
        match dbus_proxy.add_match_rule(rule).await {
            Ok(_) => {
                eprintln!("[helper] GNOME D-Bus signal subscription added for WindowMonitorPro.WindowFocusChanged");
            }
            Err(_) => {
                eprintln!("[helper] Failed to subscribe to WindowMonitorPro signals - extension not available");
                // Send message to install WindowMonitorPro
                let _ = tx.send("Install WindowMonitorPro".to_string());
                return Ok(());
            }
        }
        
        // Start a separate thread to monitor extension state using the bash filter
        let tx_clone = tx.clone();
        std::thread::spawn(move || {
            eprintln!("[helper] Starting bash filter extension monitor...");
            let mut child = std::process::Command::new("bash")
                .arg("-c")
                .arg("dbus-monitor \"interface='org.gnome.Shell.Extensions'\" | awk '/member=EnableExtension/  { next_action=\"true\" } /member=DisableExtension/ { next_action=\"false\" } /string/ && next_action != \"\" { if ($0 ~ /window-monitor-pro@muhammed\\.hussien2030\\.gmail\\.com/) { if (last_action != next_action) { print next_action; fflush(); last_action = next_action } } next_action = \"\" }'")
                .stdout(std::process::Stdio::piped())
                .spawn();
            
            if let Ok(mut child) = child {
                if let Some(stdout) = child.stdout.take() {
                    use std::io::{BufRead, BufReader};
                    let reader = BufReader::new(stdout);
                    for line in reader.lines() {
                        if let Ok(line) = line {
                            eprintln!("[helper] Bash filter output: '{}'", line);
                            if line.trim() == "false" {
                                eprintln!("[helper] Bash filter detected WindowMonitorPro extension disabled");
                                let _ = tx_clone.send("Install WindowMonitorPro".to_string());
                                // Don't break - keep monitoring for future events
                            }
                        }
                    }
                }
            }
        });
        
        let mut last_class = String::new();
        
        // Event loop for D-Bus signals
        while let Some(msg) = stream.next().await {
            if let Ok(msg) = msg {

                
                // Check if this is the WindowFocusChanged signal from WindowMonitorPro
                if let Some(interface) = msg.interface() {
                    let interface_str = interface.as_str();
                    if interface_str == "org.gnome.Shell.Extensions.WindowMonitorPro" {
                        // WindowFocusChanged signal received, extract window class directly from signal
                        eprintln!("[helper] GNOME WindowFocusChanged signal received");
                        
                        // WindowFocusChanged signal received - extract window class from signal
                        eprintln!("[helper] GNOME WindowFocusChanged signal received - extracting window class");
                        
                        // Try to parse the signal body to get window class
                        // Signal format: (window_id, window_title, window_class, window_pid)
                        eprintln!("[helper] Attempting to parse signal body...");
                        
                        // The signal body contains the window information in this format:
                        // ('window_id', 'window_title', 'window_class', 'window_pid')
                        // Example: ('2882485533', 'aura@systemos:~', 'Alacritty', '48572')
                        
                        // Try to get the signal body as a tuple of strings
                        if let Ok((_window_id, _window_title, window_class, _window_pid)) = msg.body::<(String, String, String, String)>() {
                            eprintln!("[helper] Signal body parsed successfully: window_class = {}", window_class);
                            
                            // Handle empty window class (desktop)
                            let display_class = if window_class.is_empty() {
                                "Desktop".to_string()
                            } else {
                                window_class
                            };
                            
                            if display_class != last_class {
                                eprintln!("[helper] GNOME window focus changed via WindowMonitorPro signal: {}", display_class);
                                let _ = tx.send(display_class.clone());
                                last_class = display_class;

                            }
                        } else {
                            eprintln!("[helper] Failed to parse signal body as tuple, trying alternative approach");
                            // Fallback: try to get as individual parameters
                            if let Ok(window_class) = msg.body::<String>() {
                                eprintln!("[helper] Got window class as single string: {}", window_class);
                                if window_class != last_class {
                                    let _ = tx.send(window_class.clone());
                                    last_class = window_class;
                                }
                            } else {
                                eprintln!("[helper] All parsing methods failed, signal body format not supported");
                            }
                        }
                    } else if interface_str == "org.gnome.Shell.Extensions" {
                        // Handle GNOME extension enable/disable signals
                        if let Some(member) = msg.member() {
                            let member_str = member.as_str();
                            eprintln!("[helper] Received GNOME Extensions signal: member={}", member_str);
                            
                            if member_str == "DisableExtension" || member_str == "EnableExtension" {
                                eprintln!("[helper] Processing {} signal", member_str);
                                
                                // The DisableExtension/EnableExtension signals don't contain the extension UUID
                                // We need to wait for the next signal that contains the extension info
                                // For now, let's just send the install message on any DisableExtension signal
                                // since we're specifically monitoring for WindowMonitorPro
                                if member_str == "DisableExtension" {
                                    eprintln!("[helper] DisableExtension signal received - sending install message");
                                    // Extension was disabled - send install message
                                    let _ = tx.send("Install WindowMonitorPro".to_string());
                                    break;
                                } else {
                                    eprintln!("[helper] EnableExtension signal received - no action needed");
                                }
                            }
                        }
                    }
                }
            }
        }
        
        Ok(())
    }
}

struct HyprlandWindowMonitor;

impl HyprlandWindowMonitor {
    fn new() -> Result<Self, Box<dyn std::error::Error>> {
        Ok(Self)
    }
    
    fn get_active_window_class() -> Option<String> {
        // This function is no longer needed since we get all info from socket events
        Some("Desktop".to_string())
    }
    
    fn run_event_monitor(tx: mpsc::Sender<String>) -> Result<(), Box<dyn std::error::Error>> {
        // Send initial active window
        if let Some(class) = Self::get_active_window_class() {
            let _ = tx.send(class);
        }
        
        // Use D-Bus to listen for window focus changes, then query hyprctl for current window
        let rt = tokio::runtime::Runtime::new()?;
        rt.block_on(async {
            Self::run_dbus_event_monitor(tx).await
        })
    }
    
    async fn run_dbus_event_monitor(tx: mpsc::Sender<String>) -> Result<(), Box<dyn std::error::Error>> {
        // Try multiple approaches for event-driven window focus detection
        
        // Approach 1: Try to use hyprland crate event listener
        match Self::try_hyprland_event_listener(tx.clone()).await {
            Ok(_) => return Ok(()),
            Err(e) => {
                eprintln!("[helper] Hyprland event listener failed: {}, trying file monitoring approach", e);
            }
        }
        
        // Approach 2: Monitor Hyprland's socket file for changes
        eprintln!("[helper] Using file monitoring approach for Hyprland");
        Self::monitor_hyprland_socket(tx).await
    }
    
    async fn monitor_hyprland_socket(tx: mpsc::Sender<String>) -> Result<(), Box<dyn std::error::Error>> {
        // Direct socket connection approach - much simpler and more reliable!
        eprintln!("[helper] Using direct socket connection for Hyprland events");
        
        // Get Hyprland socket path
        let socket_path = if let Ok(signature) = std::env::var("HYPRLAND_INSTANCE_SIGNATURE") {
            if let Ok(runtime_dir) = std::env::var("XDG_RUNTIME_DIR") {
                format!("{}/hypr/{}/.socket2.sock", runtime_dir, signature)
            } else {
                format!("/tmp/hypr/{}/.socket2.sock", signature)
            }
        } else {
            return Err(Box::new(std::io::Error::new(
                std::io::ErrorKind::NotFound,
                "HYPRLAND_INSTANCE_SIGNATURE not found"
            )));
        };
        
        eprintln!("[helper] Connecting to socket: {}", socket_path);
        
        // Connect to the Hyprland IPC socket
        let mut stream = UnixStream::connect(socket_path)?;
        eprintln!("[helper] Connected to Hyprland socket successfully");
        
        // Send the subscribe command
        stream.write_all(b"subscribe\n")?;
        eprintln!("[helper] Sent subscribe command");
        
        let reader = BufReader::new(stream);
        
        // Read lines from the socket
        for line in reader.lines() {
            match line {
                Ok(line) => {
                    eprintln!("[helper] Received line: {}", line);
                    if line.starts_with("activewindow>>") {
                        // Extract window name (after >> and before comma)
                        let window_name = if let Some(pos) = line.find(',') {
                            &line[14..pos] // skip "activewindow>>"
                        } else {
                            &line[14..]
                        };
                        
                        if window_name.trim().is_empty() {
                            eprintln!("[helper] Focused window: desktop");
                            let _ = tx.send("Desktop".to_string());
                        } else {
                            eprintln!("[helper] Focused window: {}", window_name);
                            let _ = tx.send(window_name.to_string());
                        }
                    }
                }
                Err(e) => {
                    eprintln!("[helper] Error reading from socket: {:?}", e);
                    break;
                }
            }
        }
        
        Ok(())
    }
    
    async fn try_hyprland_event_listener(tx: mpsc::Sender<String>) -> Result<(), Box<dyn std::error::Error>> {
        // Try to use the hyprland crate event listener
        let mut event_listener = hyprland::event_listener::EventListener::new();
        
        let tx_title = tx.clone();
        event_listener.add_window_title_change_handler(move |_| {
            if let Some(class) = HyprlandWindowMonitor::get_active_window_class() {
                let _ = tx_title.send(class);
            }
        });
        
        let tx_open = tx.clone();
        event_listener.add_window_open_handler(move |_| {
            if let Some(class) = HyprlandWindowMonitor::get_active_window_class() {
                let _ = tx_open.send(class);
            }
        });
        
        event_listener.add_window_close_handler(move |_| {
            if let Some(class) = HyprlandWindowMonitor::get_active_window_class() {
                let _ = tx.send(class);
            }
        });
        
        event_listener.start_listener()?;
        Ok(())
    }
}

struct NiriWindowMonitor;

impl NiriWindowMonitor {
    fn new() -> Result<Self, Box<dyn std::error::Error>> {
        Ok(Self)
    }
    
    fn get_active_window_class() -> Option<String> {
        // Use niri msg to get active window info
        let output = std::process::Command::new("niri")
            .arg("msg")
            .arg("focused-window")
            .env("WAYLAND_DISPLAY", std::env::var("WAYLAND_DISPLAY").unwrap_or_else(|_| "wayland-1".to_string()))
            .env("NIRI_SOCKET", std::env::var("NIRI_SOCKET").unwrap_or_else(|_| {
                // Try to find the actual Niri socket path
                let user_id = unsafe { libc::getuid() };
                let wayland_display = std::env::var("WAYLAND_DISPLAY").unwrap_or_else(|_| "wayland-1".to_string());
                let socket_pattern = format!("/run/user/{}/niri.{}.", user_id, wayland_display);
                
                // Try to find the socket file with the pattern
                if let Ok(entries) = std::fs::read_dir(format!("/run/user/{}", user_id)) {
                    for entry in entries {
                        if let Ok(entry) = entry {
                            let path = entry.path();
                            if let Some(name) = path.file_name() {
                                if let Some(name_str) = name.to_str() {
                                    if name_str.starts_with(&format!("niri.{}.", wayland_display)) && name_str.ends_with(".sock") {
                                        eprintln!("[helper] Found Niri socket: {}", path.display());
                                        return path.to_string_lossy().to_string();
                                    }
                                }
                            }
                        }
                    }
                }
                
                // Fallback to the basic pattern
                format!("/run/user/{}/niri.{}.sock", user_id, wayland_display)
            }))
            .output();
        
        match output {
            Ok(output) => {
                let stdout = String::from_utf8_lossy(&output.stdout);
                eprintln!("[helper] Niri focused-window output: '{}'", stdout);
                
                // Check if the command failed
                if !output.status.success() {
                    let stderr = String::from_utf8_lossy(&output.stderr);
                    eprintln!("[helper] Niri command failed: status={}, stderr='{}'", output.status, stderr);
                    return Some("Desktop".to_string());
                }
                
                // Parse the text output format:
                // Window ID 4: (focused)
                //   Title: "focus_window_helper.rs - tiny-dfr - Cursor"
                //   App ID: "cursor"
                //   Is floating: no
                //   PID: 3765
                //   Workspace ID: 2
                
                let lines: Vec<&str> = stdout.lines().collect();
                
                // Check if there's actually a focused window
                if lines.is_empty() || !lines[0].contains("Window ID") {
                    // No active window, return Desktop
                    eprintln!("[helper] Niri no focused window found");
                    return Some("Desktop".to_string());
                }
                
                // Look for App ID line first (preferred)
                for line in &lines {
                    if line.trim().starts_with("App ID:") {
                        if let Some(app_id) = line.split("App ID:").nth(1) {
                            let app_id = app_id.trim().trim_matches('"');
                            if !app_id.is_empty() && app_id != "null" {
                                eprintln!("[helper] Niri found app_id: {}", app_id);
                                return Some(app_id.to_string());
                            }
                        }
                    }
                }
                
                // Fallback to Title if App ID is not available
                for line in &lines {
                    if line.trim().starts_with("Title:") {
                        if let Some(title) = line.split("Title:").nth(1) {
                            let title = title.trim().trim_matches('"');
                            if !title.is_empty() && title != "null" {
                                eprintln!("[helper] Niri found title: {}", title);
                                return Some(title.to_string());
                            }
                        }
                    }
                }
            }
            Err(e) => {
                eprintln!("[helper] Niri command error: {:?}", e);
            }
        }
        
        // If we can't get window info or no valid window, return Desktop
        eprintln!("[helper] Niri no valid window found, returning Desktop");
        Some("Desktop".to_string())
    }
    
    fn run_event_monitor(tx: mpsc::Sender<String>) -> Result<(), Box<dyn std::error::Error>> {
        // Get initial active window
        if let Some(class) = Self::get_active_window_class() {
            let _ = tx.send(class);
        }
        
        // Try to use Niri's event stream for true event-driven monitoring
        let mut child = std::process::Command::new("niri")
            .arg("msg")
            .arg("event-stream")
            .env("WAYLAND_DISPLAY", std::env::var("WAYLAND_DISPLAY").unwrap_or_else(|_| "wayland-1".to_string()))
            .env("NIRI_SOCKET", std::env::var("NIRI_SOCKET").unwrap_or_else(|_| {
                // Try to find the actual Niri socket path
                let user_id = unsafe { libc::getuid() };
                let wayland_display = std::env::var("WAYLAND_DISPLAY").unwrap_or_else(|_| "wayland-1".to_string());
                let socket_pattern = format!("/run/user/{}/niri.{}.", user_id, wayland_display);
                
                // Try to find the socket file with the pattern
                if let Ok(entries) = std::fs::read_dir(format!("/run/user/{}", user_id)) {
                    for entry in entries {
                        if let Ok(entry) = entry {
                            let path = entry.path();
                            if let Some(name) = path.file_name() {
                                if let Some(name_str) = name.to_str() {
                                    if name_str.starts_with(&format!("niri.{}.", wayland_display)) && name_str.ends_with(".sock") {
                                        eprintln!("[helper] Found Niri socket: {}", path.display());
                                        return path.to_string_lossy().to_string();
                                    }
                                }
                            }
                        }
                    }
                }
                
                // Fallback to the basic pattern
                format!("/run/user/{}/niri.{}.sock", user_id, wayland_display)
            }))
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .spawn();
        
        match child {
            Ok(mut child) => {
                eprintln!("[helper] Niri event stream started successfully");
                
                if let Some(stdout) = child.stdout.take() {
                    use std::io::{BufRead, BufReader};
                    let reader = BufReader::new(stdout);
                    
                    for line in reader.lines() {
                        if let Ok(line) = line {
                            eprintln!("[helper] Niri event: {}", line);
                            
                                                // Check if this is a "Windows changed" event
                    if line.contains("Windows changed:") {
                        eprintln!("[helper] Niri windows changed event detected");
                        // Get the current focused window
                        if let Some(class) = Self::get_active_window_class() {
                            let _ = tx.send(class);
                        }
                    }
                    // Check if this is a "Window focus changed" event
                    else if line.contains("Window focus changed:") {
                        eprintln!("[helper] Niri window focus changed event detected");
                        // Get the current focused window
                        if let Some(class) = Self::get_active_window_class() {
                            let _ = tx.send(class);
                        }
                    }
                        }
                    }
                }
                
                // If we reach here, the event stream has ended
                eprintln!("[helper] Niri event stream ended");
            }
            Err(e) => {
                eprintln!("[helper] Failed to start Niri event stream: {:?}", e);
                return Err(Box::new(std::io::Error::new(
                    std::io::ErrorKind::Other,
                    format!("Failed to start Niri event stream: {}", e)
                )));
            }
        }
        
        Ok(())
    }
}

fn run_sway_event_driven() -> Result<(), Box<dyn std::error::Error>> {
    let socket_path = "/tmp/touchbar.sock";
    
    // Connect to the socket
    let mut stream = loop {
        match UnixStream::connect(socket_path) {
            Ok(stream) => break stream,
            Err(_) => {
                thread::sleep(Duration::from_millis(100));
                continue;
            }
        }
    };
    
    // Create channel for communication between event monitor and main thread
    let (tx, rx) = mpsc::channel();
    
    // Initialize Sway monitor
    let _monitor = SwayWindowMonitor::new()?;
    eprintln!("[helper] Sway event-driven monitor initialized");
    
    // Spawn event monitor in separate thread
    let monitor_thread = std::thread::spawn(move || {
        if let Err(e) = SwayWindowMonitor::run_event_monitor(tx) {
            eprintln!("[helper] Sway event monitor error: {}", e);
        }
    });
    
    // Main thread: receive events and send to socket
    let mut last_class = String::new();
    
    loop {
        match rx.recv() {
            Ok(class) => {
                if class != last_class {
                    eprintln!("[helper] Sway window focus changed from '{}' to '{}'", last_class, class);
                    if stream.write_all(class.as_bytes()).is_ok() && stream.write_all(b"\n").is_ok() {
                        last_class = class;
                    } else {
                        eprintln!("[helper] Failed to write to socket, breaking");
                        break;
                    }
                }
            }
            Err(e) => {
                eprintln!("[helper] Error receiving from Sway event monitor: {}", e);
                break;
            }
        }
    }
    
    // Wait for monitor thread to finish
    let _ = monitor_thread.join();
    Ok(())
}

fn run_gnome_event_driven() -> Result<(), Box<dyn std::error::Error>> {
    let socket_path = "/tmp/touchbar.sock";
    
    // Connect to the socket
    let mut stream = loop {
        match UnixStream::connect(socket_path) {
            Ok(stream) => break stream,
            Err(_) => {
                thread::sleep(Duration::from_millis(100));
                continue;
            }
        }
    };
    
    // Create channel for communication between event monitor and main thread
    let (tx, rx) = mpsc::channel();
    
    // Initialize GNOME monitor
    let _monitor = GnomeWindowMonitor::new()?;
    eprintln!("[helper] GNOME event-driven monitor initialized");
    
    // Spawn event monitor in separate thread
    let monitor_thread = std::thread::spawn(move || {
        if let Err(e) = GnomeWindowMonitor::run_event_monitor(tx) {
            eprintln!("[helper] GNOME event monitor error: {}", e);
        }
    });
    
    // Main thread: receive events and send to socket
    let mut last_class = String::new();
    
    loop {
        match rx.recv() {
            Ok(class) => {
                if class != last_class {
                    eprintln!("[helper] GNOME window focus changed from '{}' to '{}'", last_class, class);
                    if stream.write_all(class.as_bytes()).is_ok() && stream.write_all(b"\n").is_ok() {
                        last_class = class;
                    } else {
                        eprintln!("[helper] Failed to write to socket, breaking");
                        break;
                    }
                }
            }
            Err(e) => {
                eprintln!("[helper] Error receiving from GNOME event monitor: {}", e);
                break;
            }
        }
    }
    
    // Wait for monitor thread to finish
    let _ = monitor_thread.join();
    Ok(())
}

fn run_hyprland_event_driven() -> Result<(), Box<dyn std::error::Error>> {
    let socket_path = "/tmp/touchbar.sock";
    
    // Connect to the socket
    let mut stream = loop {
        match UnixStream::connect(socket_path) {
            Ok(stream) => break stream,
            Err(_) => {
                thread::sleep(Duration::from_millis(100));
                continue;
            }
        }
    };
    
    // Create channel for communication between event monitor and main thread
    let (tx, rx) = mpsc::channel();
    
    // Initialize Hyprland monitor
    let _monitor = HyprlandWindowMonitor::new()?;
    eprintln!("[helper] Hyprland event-driven monitor initialized");
    
    // Spawn event monitor in separate thread
    let monitor_thread = std::thread::spawn(move || {
        if let Err(e) = HyprlandWindowMonitor::run_event_monitor(tx) {
            eprintln!("[helper] Hyprland event monitor error: {}", e);
        }
    });
    
    // Main thread: receive events and send to socket
    let mut last_class = String::new();
    
    loop {
        match rx.recv() {
            Ok(class) => {
                if class != last_class {
                    eprintln!("[helper] Hyprland window focus changed from '{}' to '{}'", last_class, class);
                    if stream.write_all(class.as_bytes()).is_ok() && stream.write_all(b"\n").is_ok() {
                        last_class = class;
                    } else {
                        eprintln!("[helper] Failed to write to socket, breaking");
                        break;
                    }
                }
            }
            Err(e) => {
                eprintln!("[helper] Error receiving from Hyprland event monitor: {}", e);
                break;
            }
        }
    }
    
    // Wait for monitor thread to finish
    let _ = monitor_thread.join();
    Ok(())
}

fn run_niri_event_driven() -> Result<(), Box<dyn std::error::Error>> {
    let socket_path = "/tmp/touchbar.sock";
    
    // Connect to the socket
    let mut stream = loop {
        match UnixStream::connect(socket_path) {
            Ok(stream) => break stream,
            Err(_) => {
                thread::sleep(Duration::from_millis(100));
                continue;
            }
        }
    };
    
    // Create channel for communication between event monitor and main thread
    let (tx, rx) = mpsc::channel();
    
    // Initialize Niri monitor
    let _monitor = NiriWindowMonitor::new()?;
    eprintln!("[helper] Niri event-driven monitor initialized");
    
    // Spawn event monitor in separate thread
    let monitor_thread = std::thread::spawn(move || {
        if let Err(e) = NiriWindowMonitor::run_event_monitor(tx) {
            eprintln!("[helper] Niri event monitor error: {}", e);
        }
    });
    
    // Main thread: receive events and send to socket
    let mut last_class = String::new();
    
    loop {
        match rx.recv() {
            Ok(class) => {
                if class != last_class {
                    eprintln!("[helper] Niri window focus changed from '{}' to '{}'", last_class, class);
                    if stream.write_all(class.as_bytes()).is_ok() && stream.write_all(b"\n").is_ok() {
                        last_class = class;
                    } else {
                        eprintln!("[helper] Failed to write to socket, breaking");
                        break;
                    }
                }
            }
            Err(e) => {
                eprintln!("[helper] Error receiving from Niri event monitor: {}", e);
                break;
            }
        }
    }
    
    // Wait for monitor thread to finish
    let _ = monitor_thread.join();
    Ok(())
}

fn run_x11_event_driven() -> Result<(), Box<dyn std::error::Error>> {
    let socket_path = "/tmp/touchbar.sock";
    
    // Connect to the socket
    let mut stream = loop {
        match UnixStream::connect(socket_path) {
            Ok(stream) => break stream,
            Err(_) => {
                thread::sleep(Duration::from_millis(100));
                continue;
            }
        }
    };
    
    // Initialize X11 monitor
    let mut monitor = X11WindowMonitor::new()?;
    eprintln!("[helper] X11 event-driven monitor initialized");
    
    // Setup event listening
    monitor.setup_event_listening()?;
    eprintln!("[helper] X11 event listening setup complete");
    
    // Get initial active window class
    let mut last_class = monitor.get_active_window_class().unwrap_or_else(|| "Desktop".to_string());
    eprintln!("[helper] Initial active window class: {}", last_class);
    
    // Send initial class
    if stream.write_all(last_class.as_bytes()).is_ok() && stream.write_all(b"\n").is_ok() {
        eprintln!("[helper] Sent initial window class: {}", last_class);
    }
    
    // Event loop
    loop {
        match monitor.wait_for_focus_change() {
            Ok(Some(new_class)) => {
                if new_class != last_class {
                    eprintln!("[helper] Window focus changed from '{}' to '{}'", last_class, new_class);
                    if stream.write_all(new_class.as_bytes()).is_ok() && stream.write_all(b"\n").is_ok() {
                        last_class = new_class;
                    } else {
                        eprintln!("[helper] Failed to write to socket, breaking");
                        break;
                    }
                }
            }
            Ok(None) => {
                // No focus change, continue waiting
                // This can happen when switching to an empty workspace
            }
            Err(e) => {
                eprintln!("[helper] Error waiting for X11 events: {}", e);
                break;
            }
        }
    }
    
    Ok(())
}

fn main() -> std::io::Result<()> {
    // Debug environment variables
    eprintln!("[helper] Environment variables:");
    eprintln!("[helper] DISPLAY={:?}", std::env::var("DISPLAY"));
    eprintln!("[helper] WAYLAND_DISPLAY={:?}", std::env::var("WAYLAND_DISPLAY"));
    eprintln!("[helper] SWAYSOCK={:?}", std::env::var("SWAYSOCK"));
    eprintln!("[helper] HYPRLAND_INSTANCE_SIGNATURE={:?}", std::env::var("HYPRLAND_INSTANCE_SIGNATURE"));
    eprintln!("[helper] NIRI_SOCKET={:?}", std::env::var("NIRI_SOCKET"));
    eprintln!("[helper] UID={:?}", std::env::var("UID"));
    eprintln!("[helper] GNOME_DESKTOP_SESSION_ID={:?}", std::env::var("GNOME_DESKTOP_SESSION_ID"));
    eprintln!("[helper] XDG_CURRENT_DESKTOP={:?}", std::env::var("XDG_CURRENT_DESKTOP"));
    eprintln!("[helper] XDG_RUNTIME_DIR={:?}", std::env::var("XDG_RUNTIME_DIR"));
    
    if let Ok(addr) = std::env::var("DBUS_SESSION_BUS_ADDRESS") {
        eprintln!("[helper] DBUS_SESSION_BUS_ADDRESS={}", addr);
    } else {
        eprintln!("[helper] DBUS_SESSION_BUS_ADDRESS is not set");
    }
    
    // Check for Wayland first (prioritize over X11 when both are present)
    if std::env::var("WAYLAND_DISPLAY").is_ok() {
        eprintln!("[helper] Wayland detected, checking for specific compositor");
        
        // Check for Hyprland first (more specific detection)
        if std::env::var("HYPRLAND_INSTANCE_SIGNATURE").is_ok() {
            eprintln!("[helper] Hyprland detected, using event-driven approach");
            if let Err(e) = run_hyprland_event_driven() {
                eprintln!("[helper] Hyprland event-driven approach failed: {}", e);
                return Err(std::io::Error::new(std::io::ErrorKind::Other, e.to_string()));
            }
            return Ok(());
        }
        
        // Check for Niri (more specific detection)
        if std::env::var("XDG_CURRENT_DESKTOP").map_or(false, |desktop| desktop.to_lowercase() == "niri") {
            eprintln!("[helper] Niri detected, using event-driven approach");
            if let Err(e) = run_niri_event_driven() {
                eprintln!("[helper] Niri event-driven approach failed: {}", e);
                return Err(std::io::Error::new(std::io::ErrorKind::Other, e.to_string()));
            }
            return Ok(());
        }
        
        // Check for Sway (more specific detection)
        if std::env::var("SWAYSOCK").is_ok() {
            eprintln!("[helper] Sway detected, using event-driven approach");
            if let Err(e) = run_sway_event_driven() {
                eprintln!("[helper] Sway event-driven approach failed: {}", e);
                return Err(std::io::Error::new(std::io::ErrorKind::Other, e.to_string()));
            }
            return Ok(());
        }
        
        // Check for GNOME Wayland
        if std::env::var("GNOME_DESKTOP_SESSION_ID").is_ok() || 
           std::env::var("XDG_CURRENT_DESKTOP").map_or(false, |desktop| desktop.to_lowercase().contains("gnome")) {
            eprintln!("[helper] GNOME Wayland detected, using event-driven approach with WindowMonitorPro extension");
            if let Err(e) = run_gnome_event_driven() {
                eprintln!("[helper] GNOME event-driven approach failed: {}", e);
                return Err(std::io::Error::new(std::io::ErrorKind::Other, e.to_string()));
            }
            return Ok(());
        }
        
        eprintln!("[helper] Generic Wayland detected, but no event-driven support available");
        return Err(std::io::Error::new(std::io::ErrorKind::Unsupported, "No event-driven support for generic Wayland"));
    }
    
    // Check for X11 (only if DISPLAY is set but no WAYLAND_DISPLAY)
    if std::env::var("DISPLAY").is_ok() && std::env::var("WAYLAND_DISPLAY").is_err() {
        eprintln!("[helper] X11 detected (no Wayland), using event-driven approach");
        if let Err(e) = run_x11_event_driven() {
            eprintln!("[helper] X11 event-driven approach failed: {}", e);
            return Err(std::io::Error::new(std::io::ErrorKind::Other, e.to_string()));
        }
        return Ok(());
    }
    
    // No supported compositor detected
    eprintln!("[helper] No supported compositor detected (not X11, Sway, Hyprland, Niri, or generic Wayland)");
    return Err(std::io::Error::new(std::io::ErrorKind::Unsupported, "No supported compositor detected"));
}

 
 