//! Helper binary for tiny-dfr, providing auxiliary functionality.
use std::os::unix::net::UnixStream;
use std::io::Write;
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
                    if window_id != 0 {
                        return Some(window_id);
                    }
                }
            }
            Err(_) => {}
        }
        
        None
    }
    
    fn get_active_window_class(&mut self) -> Option<String> {
        let active_window = self.get_active_window()?;
        
        // Update active window
        if self.active_window != Some(active_window) {
            self.active_window = Some(active_window);
        }
        
        self.get_window_class(active_window)
    }
    
    fn setup_event_listening(&mut self) -> Result<(), Box<dyn std::error::Error>> {
        // Select for property change events on root window
        self.conn.change_window_attributes(
            self.root,
            &xproto::ChangeWindowAttributesAux::new().event_mask(
                xproto::EventMask::PROPERTY_CHANGE
            ),
        )?;
        
        // Also listen for window focus events
        self.conn.change_window_attributes(
            self.root,
            &xproto::ChangeWindowAttributesAux::new().event_mask(
                xproto::EventMask::SUBSTRUCTURE_NOTIFY
            ),
        )?;
        
        Ok(())
    }
    
    fn wait_for_focus_change(&mut self) -> Result<Option<String>, Box<dyn std::error::Error>> {
        // Wait for events
        let event = self.conn.wait_for_event()?;
        
        match event {
            x11rb::protocol::Event::PropertyNotify(ev) => {
                // Check if it's the active window property that changed
                if ev.atom == self.net_active_window_atom {
                    eprintln!("[helper] Active window property changed");
                    return Ok(self.get_active_window_class());
                }
            }
            x11rb::protocol::Event::ConfigureNotify(_ev) => {
                // Window configuration changed, might indicate focus change
                eprintln!("[helper] Window configuration changed");
                return Ok(self.get_active_window_class());
            }
            x11rb::protocol::Event::FocusIn(ev) => {
                // Focus changed
                eprintln!("[helper] Focus changed to window {}", ev.event);
                return Ok(self.get_window_class(ev.event));
            }
            _ => {
                // Other events, ignore
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
                if output.status.success() {
                    let stdout = String::from_utf8_lossy(&output.stdout);
                    // Parse the output: ('Cursor',) -> Cursor or ('') -> Desktop
                    if let Some(start) = stdout.find("('") {
                        if let Some(end) = stdout[start..].find("',") {
                            let class = &stdout[start + 2..start + end];
                            if class.is_empty() {
                                // Empty string means no window focused (desktop)
                                return Some("Desktop".to_string());
                            } else if class != "null" {
                                return Some(class.to_string());
                            }
                        }
                    }
                }
            }
            Err(_) => {}
        }
        
        // Fallback to Desktop if we can't get window info
        Some("Desktop".to_string())
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
        dbus_proxy.add_match_rule(rule).await?;
        
        eprintln!("[helper] GNOME D-Bus signal subscription added for WindowMonitorPro.WindowFocusChanged");
        
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
        // Use hyprctl to get active window info
        let output = std::process::Command::new("hyprctl")
            .arg("activewindow")
            .arg("-j")
        .output();
    
    match output {
        Ok(output) => {
                if let Ok(json) = serde_json::from_slice::<serde_json::Value>(&output.stdout) {
                    // Check if there's actually an active window
                    if json.is_null() || json.as_object().map_or(true, |obj| obj.is_empty()) {
                        // No active window, return Desktop
                        return Some("Desktop".to_string());
                    }
                    
                    if let Some(class) = json["class"].as_str() {
                        if !class.is_empty() && class != "null" {
                            return Some(class.to_string());
                        }
                    }
                    if let Some(title) = json["title"].as_str() {
                        if !title.is_empty() && title != "null" {
                            return Some(title.to_string());
                        }
                    }
                }
            }
            Err(_) => {}
        }
        
        // If we can't get window info or no valid window, return Desktop
        Some("Desktop".to_string())
    }
    
    fn run_event_monitor(tx: mpsc::Sender<String>) -> Result<(), Box<dyn std::error::Error>> {
        // Get initial active window
        if let Some(class) = Self::get_active_window_class() {
            let _ = tx.send(class);
        }
        
        // Hyprland doesn't have a direct event subscription API in the Rust crate,
        // so we'll use a more efficient polling approach with shorter intervals
        // and monitor for changes
        let mut last_class = String::new();
        
        loop {
            if let Some(class) = Self::get_active_window_class() {
                if class != last_class {
                    let _ = tx.send(class.clone());
                    last_class = class;
                }
            }
            std::thread::sleep(std::time::Duration::from_millis(100)); // Much shorter than the old 500ms
        }
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
    eprintln!("[helper] No supported compositor detected (not X11, Sway, Hyprland, or generic Wayland)");
    return Err(std::io::Error::new(std::io::ErrorKind::Unsupported, "No supported compositor detected"));
}

 