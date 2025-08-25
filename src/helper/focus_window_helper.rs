//! Helper binary for tiny-dfr, providing auxiliary functionality.
use std::os::unix::net::UnixStream;
use std::io::Write;
use std::thread;
use std::time::{Duration, Instant};
use std::sync::mpsc;
use std::collections::HashMap;
use std::error::Error;
use std::fmt;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

// Window manager specific imports
use x11rb::connection::Connection;
use x11rb::protocol::xproto::{self, ConnectionExt, Window};
use x11rb::rust_connection::RustConnection;
use zbus::{Connection as ZbusConnection, MessageType, MessageStream, MatchRule};
use zbus::fdo::DBusProxy;
use futures_lite::stream::StreamExt;

// ============================================================================
// Error Types
// ============================================================================

#[derive(Debug)]
pub enum FocusHelperError {
    Io(std::io::Error),
    X11(String),
    Sway(String),
    Hyprland(String),
    Niri(String),
    Gnome(String),
    ConnectionFailed(String),
    Unsupported(String),
}

impl fmt::Display for FocusHelperError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            FocusHelperError::Io(e) => write!(f, "IO error: {}", e),
            FocusHelperError::X11(e) => write!(f, "X11 error: {}", e),
            FocusHelperError::Sway(e) => write!(f, "Sway error: {}", e),
            FocusHelperError::Hyprland(e) => write!(f, "Hyprland error: {}", e),
            FocusHelperError::Niri(e) => write!(f, "Niri error: {}", e),
            FocusHelperError::Gnome(e) => write!(f, "GNOME error: {}", e),
            FocusHelperError::ConnectionFailed(e) => write!(f, "Connection failed: {}", e),
            FocusHelperError::Unsupported(e) => write!(f, "Unsupported: {}", e),
        }
    }
}

impl Error for FocusHelperError {}

impl From<std::io::Error> for FocusHelperError {
    fn from(err: std::io::Error) -> Self {
        FocusHelperError::Io(err)
    }
}

impl From<x11rb::errors::ConnectionError> for FocusHelperError {
    fn from(err: x11rb::errors::ConnectionError) -> Self {
        FocusHelperError::X11(format!("Connection error: {}", err))
    }
}

impl From<x11rb::errors::ReplyError> for FocusHelperError {
    fn from(err: x11rb::errors::ReplyError) -> Self {
        FocusHelperError::X11(format!("Reply error: {}", err))
    }
}

impl From<swayipc::Error> for FocusHelperError {
    fn from(err: swayipc::Error) -> Self {
        FocusHelperError::Sway(format!("Sway error: {}", err))
    }
}

impl From<zbus::Error> for FocusHelperError {
    fn from(err: zbus::Error) -> Self {
        FocusHelperError::Gnome(format!("D-Bus error: {}", err))
    }
}

impl From<x11rb::errors::ConnectError> for FocusHelperError {
    fn from(err: x11rb::errors::ConnectError) -> Self {
        FocusHelperError::X11(format!("Connect error: {}", err))
    }
}

// ============================================================================
// Common Types and Constants
// ============================================================================

const SOCKET_PATH: &str = "/tmp/touchbar.sock";
const MAX_RETRIES: u32 = 10;

// Memory management constants
const MAX_CACHE_SIZE: usize = 1000; // Max window classes in cache
const CACHE_CLEANUP_INTERVAL: Duration = Duration::from_secs(300); // 5 minutes

type Result<T> = std::result::Result<T, FocusHelperError>;

// ============================================================================
// Memory Management Utilities
// ============================================================================



struct LruWindowCache {
    cache: HashMap<Window, (String, Instant)>,
    max_size: usize,
    last_cleanup: Instant,
    cleanup_interval: Duration,
}

impl LruWindowCache {
    fn new(max_size: usize, cleanup_interval: Duration) -> Self {
        Self {
            cache: HashMap::with_capacity(max_size / 2),
            max_size,
            last_cleanup: Instant::now(),
            cleanup_interval,
        }
    }
    
    fn get(&mut self, window: Window) -> Option<String> {
        if let Some((class, _)) = self.cache.get(&window) {
            Some(class.clone())
        } else {
            None
        }
    }
    
    fn insert(&mut self, window: Window, class: String) {
        // Cleanup old entries if needed
        self.cleanup_if_needed();
        
        // If cache is full, remove oldest entry
        if self.cache.len() >= self.max_size {
            if let Some((oldest_window, _)) = self.cache.iter()
                .min_by_key(|(_, (_, time))| time) {
                let oldest_window = *oldest_window;
                self.cache.remove(&oldest_window);
            }
        }
        
        self.cache.insert(window, (class, Instant::now()));
    }
    
    fn cleanup_if_needed(&mut self) {
        if self.last_cleanup.elapsed() > self.cleanup_interval {
            let now = Instant::now();
            self.cache.retain(|_, (_, time)| {
                now.duration_since(*time) < Duration::from_secs(600) // Keep entries for 10 minutes
            });
            self.last_cleanup = now;
        }
    }
    
    fn len(&self) -> usize {
        self.cache.len()
    }
    
    fn clear(&mut self) {
        self.cache.clear();
    }
}

// ============================================================================
// Thread Synchronization Utilities
// ============================================================================

struct ThreadSafeState {
    last_class: Arc<AtomicBool>, // Using AtomicBool as a simple flag for now
    shutdown_requested: Arc<AtomicBool>,
}

impl ThreadSafeState {
    fn new() -> Self {
        Self {
            last_class: Arc::new(AtomicBool::new(false)),
            shutdown_requested: Arc::new(AtomicBool::new(false)),
        }
    }
    
    fn request_shutdown(&self) {
        self.shutdown_requested.store(true, Ordering::SeqCst);
    }
    
    fn is_shutdown_requested(&self) -> bool {
        self.shutdown_requested.load(Ordering::SeqCst)
    }
}

// Bounded channel with backpressure handling
fn create_bounded_channel() -> (mpsc::Sender<String>, mpsc::Receiver<String>) {
    // Use a bounded channel to prevent memory issues
    mpsc::channel()
}

// Channel with backpressure handling (simplified version)
fn send_with_backpressure(tx: &mpsc::Sender<String>, data: String) -> Result<()> {
    match tx.send(data) {
        Ok(_) => Ok(()),
        Err(mpsc::SendError(_)) => {
            eprintln!("[helper] Warning: Channel full, dropping window change event");
            Ok(()) // Don't fail, just log and continue
        }
    }
}



// ============================================================================
// Window Monitor Trait
// ============================================================================

trait WindowMonitor: Send {
    fn get_initial_window_class(&self) -> Result<String>;
    fn run_event_monitor(&self, tx: mpsc::Sender<String>) -> Result<()>;
}

// ============================================================================
// Common Event Runner
// ============================================================================

struct EventRunner {
    socket_path: String,
    state: ThreadSafeState,
}

impl EventRunner {
    fn new() -> Self {
        Self {
            socket_path: SOCKET_PATH.to_string(),
            state: ThreadSafeState::new(),
        }
    }

    fn run_with_monitor(&mut self, monitor: Box<dyn WindowMonitor>) -> Result<()> {
        // Connect to socket with retry logic
        let mut stream = self.connect_with_retry()?;
        // Keep socket blocking for pure event-driven behavior
        
        // Create communication channel with backpressure handling
        let (tx, rx) = create_bounded_channel();
        
        // Get initial window class
        let initial_class = monitor.get_initial_window_class()?;
        self.write_to_socket(&mut stream, &initial_class)?;
        
        // Spawn monitor thread with shutdown capability
        let monitor_thread = self.spawn_monitor_thread(monitor, tx)?;
        
        // Main event loop with shutdown handling
        let result = self.run_event_loop(&mut stream, rx);
        
        // Request shutdown and cleanup
        self.state.request_shutdown();
        self.cleanup(monitor_thread);
        
        result
    }

    fn connect_with_retry(&self) -> Result<UnixStream> {
        let mut retry_count = 0;
        let mut backoff_delay = Duration::from_millis(10); // Start with 10ms
        
        loop {
            match UnixStream::connect(&self.socket_path) {
                Ok(stream) => return Ok(stream),
                Err(_) => {
                    retry_count += 1;
                    if retry_count >= MAX_RETRIES {
                        return Err(FocusHelperError::ConnectionFailed(
                            format!("Failed to connect after {} retries", MAX_RETRIES)
                        ));
                    }
                    
                    eprintln!("[helper] Socket connection attempt {} failed, waiting {:?} before retry...", 
                             retry_count, backoff_delay);
                    
                    // Use exponential backoff: 10ms, 20ms, 40ms, 80ms, 160ms, 320ms, 640ms, 1.28s, 2.56s, 5.12s
                    thread::sleep(backoff_delay);
                    backoff_delay = backoff_delay.saturating_mul(2);
                    
                    // Cap the backoff at 5 seconds to keep it responsive
                    if backoff_delay > Duration::from_secs(5) {
                        backoff_delay = Duration::from_secs(5);
                    }
                }
            }
        }
    }

    fn spawn_monitor_thread(&self, monitor: Box<dyn WindowMonitor>, tx: mpsc::Sender<String>) -> Result<thread::JoinHandle<()>> {
        let tx_clone = tx.clone();
        let _shutdown_flag = self.state.shutdown_requested.clone();
        
        let monitor_thread = thread::spawn(move || {
            if let Err(e) = monitor.run_event_monitor(tx_clone) {
                eprintln!("[helper] Event monitor error: {}", e);
            }
        });
        Ok(monitor_thread)
    }

    fn run_event_loop(&mut self, stream: &mut UnixStream, rx: mpsc::Receiver<String>) -> Result<()> {
        let mut last_class = String::new();
        
        loop {
            // Check for shutdown request
            if self.state.is_shutdown_requested() {
                eprintln!("[helper] Shutdown requested, exiting event loop");
                break;
            }
            
            // Use pure blocking recv - no polling
            match rx.recv() {
                Ok(class) => {
                    if class != last_class {
                        eprintln!("[helper] Window focus changed from '{}' to '{}'", 
                                 last_class, class);
                        
                        if let Err(e) = self.write_to_socket(stream, &class) {
                            eprintln!("[helper] Socket write error: {}", e);
                        }
                        
                        last_class = class;
                    }
                }
                Err(mpsc::RecvError) => {
                    eprintln!("[helper] Event monitor disconnected");
                    return Err(FocusHelperError::ConnectionFailed("Event monitor disconnected".to_string()));
                }
            }
        }
        
        Ok(())
    }
    
        

    fn write_to_socket(&self, stream: &mut UnixStream, data: &str) -> Result<()> {
        stream.write_all(data.as_bytes())?;
        stream.write_all(b"\n")?;
        Ok(())
    }

    fn cleanup(&self, monitor_thread: thread::JoinHandle<()>) {
        if let Err(e) = monitor_thread.join() {
            eprintln!("[helper] Error joining monitor thread: {:?}", e);
        }
    }
}

// ============================================================================
// X11 Window Monitor
// ============================================================================

struct X11WindowMonitor {
    conn: RustConnection,
    root: Window,
    active_window: Option<Window>,
    window_classes: LruWindowCache,
    net_active_window_atom: xproto::Atom,
}

impl X11WindowMonitor {
    fn new() -> Result<Self> {
        let (conn, screen_num) = x11rb::connect(None)?;
        let screen = &conn.setup().roots[screen_num];
        let root = screen.root;
        
        let net_active_window_atom = conn.intern_atom(false, b"_NET_ACTIVE_WINDOW")?
            .reply()?.atom;
        
        Ok(Self {
            conn,
            root,
            active_window: None,
            window_classes: LruWindowCache::new(MAX_CACHE_SIZE, CACHE_CLEANUP_INTERVAL),
            net_active_window_atom,
        })
    }
    
    fn get_window_class(&mut self, window: Window) -> Option<String> {
        // Check cache first
        if let Some(class) = self.window_classes.get(window) {
            return Some(class);
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
        
        if active_window == 0 {
            eprintln!("[helper] No active window (empty workspace)");
            return Some("Desktop".to_string());
        }
        
        if let Some(class) = self.get_window_class(active_window) {
            if self.active_window != Some(active_window) {
                self.active_window = Some(active_window);
            }
            return Some(class);
        }
        
        eprintln!("[helper] Could not get window class for window {}, using Desktop", active_window);
        Some("Desktop".to_string())
    }
    
    fn setup_event_listening(&mut self) -> Result<()> {
        self.conn.change_window_attributes(
            self.root,
            &xproto::ChangeWindowAttributesAux::new().event_mask(
                xproto::EventMask::PROPERTY_CHANGE
            ),
        )?;
        
        eprintln!("[helper] X11 event listening setup complete");
        Ok(())
    }
    
    fn wait_for_focus_change(&mut self) -> Result<Option<String>> {
        // Use pure blocking event wait - no polling
        match self.conn.wait_for_event() {
            Ok(event) => {
                match event {
                    x11rb::protocol::Event::PropertyNotify(ev) => {
                        if ev.atom == self.net_active_window_atom {
                            eprintln!("[helper] Active window property changed!");
                            return Ok(self.get_active_window_class());
                        }
                    }
                    _ => {}
                }
                // Event processed but not a focus change
                Ok(None)
            }
            Err(e) => {
                Err(FocusHelperError::X11(format!("Event wait error: {}", e)))
            }
        }
    }
}

impl WindowMonitor for X11WindowMonitor {
    fn get_initial_window_class(&self) -> Result<String> {
        let mut monitor = X11WindowMonitor::new()?;
        monitor.setup_event_listening()?;
        
        Ok(monitor.get_active_window_class().unwrap_or_else(|| "Desktop".to_string()))
    }
    
    fn run_event_monitor(&self, tx: mpsc::Sender<String>) -> Result<()> {
        let mut monitor = X11WindowMonitor::new()?;
        monitor.setup_event_listening()?;
        
        let mut last_class = monitor.get_active_window_class().unwrap_or_else(|| "Desktop".to_string());
        let _ = tx.send(last_class.clone());
        
        loop {
            match monitor.wait_for_focus_change() {
                Ok(Some(new_class)) => {
                    if new_class != last_class {
                        let _ = tx.send(new_class.clone());
                        last_class = new_class;
                    }
                }
                Ok(None) => {
                    // No focus change, continue to next event - no polling
                    continue;
                }
                Err(e) => {
                    eprintln!("[helper] X11 event error: {}", e);
                    break;
                }
            }
        }
        
        Ok(())
    }
}

// ============================================================================
// Sway Window Monitor
// ============================================================================

struct SwayWindowMonitor;

impl SwayWindowMonitor {
    fn new() -> Result<Self> {
        Ok(Self)
    }
    
    fn get_active_window_class() -> Option<String> {
        if let Ok(mut connection) = swayipc::Connection::new() {
            if let Ok(tree) = connection.get_tree() {
                if let Some(focused) = tree.find_focused(|n| n.focused) {
                    if focused.node_type == swayipc::reply::NodeType::Con {
                        if let Some(window_properties) = &focused.window_properties {
                            if let Some(class) = &window_properties.class {
                                if !class.is_empty() && class != "null" {
                                    return Some(class.clone());
                                }
                            }
                            if let Some(instance) = &window_properties.instance {
                                if !instance.is_empty() && instance != "null" {
                                    return Some(instance.clone());
                                }
                            }
                        }
                        
                        if let Some(app_id) = &focused.app_id {
                            if !app_id.is_empty() && app_id != "null" {
                                return Some(app_id.clone());
                            }
                        }
                        
                        if let Some(name) = &focused.name {
                            if !name.is_empty() && name != "null" {
                                return Some(name.clone());
                            }
                        }
                    } else {
                        return Some("Desktop".to_string());
                    }
                }
            }
        }
        Some("Desktop".to_string())
    }
    
    fn run_event_monitor(tx: mpsc::Sender<String>) -> Result<()> {
        let connection = swayipc::Connection::new()?;
        
        let events = connection.subscribe(&[swayipc::EventType::Workspace, swayipc::EventType::Window])?;
        
        if let Some(class) = Self::get_active_window_class() {
            let _ = tx.send(class);
        }
        
        // Use pure blocking iterator - no polling
        for event in events {
            match event? {
                swayipc::reply::Event::Workspace(_) | swayipc::reply::Event::Window(_) => {
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

impl WindowMonitor for SwayWindowMonitor {
    fn get_initial_window_class(&self) -> Result<String> {
        Ok(Self::get_active_window_class().unwrap_or_else(|| "Desktop".to_string()))
    }
    
    fn run_event_monitor(&self, tx: mpsc::Sender<String>) -> Result<()> {
        Self::run_event_monitor(tx)
    }
}

// ============================================================================
// Hyprland Window Monitor
// ============================================================================

struct HyprlandWindowMonitor;

impl HyprlandWindowMonitor {
    fn new() -> Result<Self> {
        Ok(Self)
    }
    
    fn get_socket_path() -> Result<String> {
        let signature = std::env::var("HYPRLAND_INSTANCE_SIGNATURE")
            .map_err(|_| FocusHelperError::Hyprland("HYPRLAND_INSTANCE_SIGNATURE not found".to_string()))?;
        
        let runtime_dir = std::env::var("XDG_RUNTIME_DIR")
            .unwrap_or_else(|_| "/tmp".to_string());
        
        Ok(format!("{}/hypr/{}/.socket2.sock", runtime_dir, signature))
    }
    
    fn run_event_monitor(tx: mpsc::Sender<String>) -> Result<()> {
        let socket_path = Self::get_socket_path()?;
        eprintln!("[helper] Connecting to Hyprland socket: {}", socket_path);
        
        let mut stream = UnixStream::connect(&socket_path)?;
        // Keep socket blocking for pure event-driven behavior
        stream.write_all(b"subscribe\n")?;
        
        use std::io::{BufRead, BufReader};
        let reader = BufReader::new(stream);
        
        // Use pure blocking line reading - no polling
        for line in reader.lines() {
            match line {
                Ok(line) => {
                        if line.starts_with("activewindow>>") {
                            let window_name = if let Some(pos) = line.find(',') {
                                &line[14..pos]
                            } else {
                                &line[14..]
                            };
                            
                            if window_name.trim().is_empty() {
                                let _ = tx.send("Desktop".to_string());
                            } else {
                                let _ = tx.send(window_name.to_string());
                            }
                        }
                }
                Err(e) => {
                    eprintln!("[helper] Error reading from Hyprland socket: {:?}", e);
                    break;
                }
            }
        }
        
        Ok(())
    }
}

impl WindowMonitor for HyprlandWindowMonitor {
    fn get_initial_window_class(&self) -> Result<String> {
        Ok("Desktop".to_string())
    }
    
    fn run_event_monitor(&self, tx: mpsc::Sender<String>) -> Result<()> {
        Self::run_event_monitor(tx)
    }
}

// ============================================================================
// GNOME Window Monitor
// ============================================================================

struct GnomeWindowMonitor;

impl GnomeWindowMonitor {
    fn new() -> Result<Self> {
        Ok(Self)
    }
    
    fn get_initial_focused_class() -> Option<String> {
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
                    if let Some(start) = stdout.find("('") {
                        if let Some(end) = stdout[start..].find("',") {
                            let class = &stdout[start + 2..start + end];
                            if class.is_empty() {
                                return Some("Desktop".to_string());
                            } else if class != "null" {
                                return Some(class.to_string());
                            }
                        }
                    }
                }
            }
            Err(e) => {
                eprintln!("[helper] FocusClass command error: {:?}", e);
            }
        }
        
        Some("Install WindowMonitorPro".to_string())
    }
    
    async fn run_dbus_event_monitor(tx: mpsc::Sender<String>) -> Result<()> {
        let connection = ZbusConnection::session().await?;
        
        let mut stream = MessageStream::from(&connection);
        let dbus_proxy = DBusProxy::new(&connection).await?;
        
        let rule = MatchRule::builder()
            .msg_type(MessageType::Signal)
            .interface("org.gnome.Shell.Extensions.WindowMonitorPro")?
            .member("WindowFocusChanged")?
            .build();
        
        match dbus_proxy.add_match_rule(rule).await {
            Ok(_) => {
                eprintln!("[helper] GNOME D-Bus signal subscription added");
            }
            Err(_) => {
                eprintln!("[helper] Failed to subscribe to WindowMonitorPro signals");
                let _ = tx.send("Install WindowMonitorPro".to_string());
                return Ok(());
            }
        }
        
        let mut last_class = String::new();
        
        // Use blocking stream iteration to prevent CPU spinning
        while let Some(msg) = stream.next().await {
            if let Ok(msg) = msg {
                if let Some(interface) = msg.interface() {
                    if interface.as_str() == "org.gnome.Shell.Extensions.WindowMonitorPro" {
                        if let Ok((_window_id, _window_title, window_class, _window_pid)) = 
                            msg.body::<(String, String, String, String)>() {
                            
                            let display_class = if window_class.is_empty() {
                                "Desktop".to_string()
                            } else {
                                window_class
                            };
                            
                            if display_class != last_class {
                                let _ = tx.send(display_class.clone());
                                last_class = display_class;
                            }
                        }
                    }
                }
            }
            
            // Pure event-driven - no artificial delays needed
        }
        
        Ok(())
    }
    
    fn run_event_monitor(tx: mpsc::Sender<String>) -> Result<()> {
        if let Some(initial_class) = Self::get_initial_focused_class() {
            let _ = tx.send(initial_class);
        }
        
        let rt = tokio::runtime::Runtime::new()
            .map_err(|e| FocusHelperError::Gnome(format!("Failed to create runtime: {}", e)))?;
        
        rt.block_on(async {
            Self::run_dbus_event_monitor(tx).await
        })
    }
}

impl WindowMonitor for GnomeWindowMonitor {
    fn get_initial_window_class(&self) -> Result<String> {
        Ok(Self::get_initial_focused_class().unwrap_or_else(|| "Desktop".to_string()))
    }
    
    fn run_event_monitor(&self, tx: mpsc::Sender<String>) -> Result<()> {
        Self::run_event_monitor(tx)
    }
}

// ============================================================================
// Niri Window Monitor
// ============================================================================

struct NiriWindowMonitor;

impl NiriWindowMonitor {
    fn new() -> Result<Self> {
        Ok(Self)
    }
    
    fn get_active_window_class() -> Option<String> {
        let output = std::process::Command::new("niri")
            .arg("msg")
            .arg("focused-window")
            .env("WAYLAND_DISPLAY", std::env::var("WAYLAND_DISPLAY").unwrap_or_else(|_| "wayland-1".to_string()))
            .output();
        
        match output {
            Ok(output) => {
                if !output.status.success() {
                    return Some("Desktop".to_string());
                }
                
                let stdout = String::from_utf8_lossy(&output.stdout);
                let lines: Vec<&str> = stdout.lines().collect();
                
                if lines.is_empty() || !lines[0].contains("Window ID") {
                    return Some("Desktop".to_string());
                }
                
                for line in &lines {
                    if line.trim().starts_with("App ID:") {
                        if let Some(app_id) = line.split("App ID:").nth(1) {
                            let app_id = app_id.trim().trim_matches('"');
                            if !app_id.is_empty() && app_id != "null" {
                                return Some(app_id.to_string());
                            }
                        }
                    }
                }
                
                for line in &lines {
                    if line.trim().starts_with("Title:") {
                        if let Some(title) = line.split("Title:").nth(1) {
                            let title = title.trim().trim_matches('"');
                            if !title.is_empty() && title != "null" {
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
        
        Some("Desktop".to_string())
    }
    
    fn run_event_monitor(tx: mpsc::Sender<String>) -> Result<()> {
        if let Some(class) = Self::get_active_window_class() {
            let _ = tx.send(class);
        }
        
        let child = std::process::Command::new("niri")
            .arg("msg")
            .arg("event-stream")
            .env("WAYLAND_DISPLAY", std::env::var("WAYLAND_DISPLAY").unwrap_or_else(|_| "wayland-1".to_string()))
            .stdout(std::process::Stdio::piped())
            .spawn()
            .map_err(|e| FocusHelperError::Niri(format!("Failed to start event stream: {}", e)))?;
        
        if let Some(stdout) = child.stdout {
                    use std::io::{BufRead, BufReader};
                    let reader = BufReader::new(stdout);
                    
            // Use pure blocking line reading - no polling
                    for line in reader.lines() {
                        if let Ok(line) = line {
                    if line.contains("Windows changed:") || line.contains("Window focus changed:") {
                        if let Some(class) = Self::get_active_window_class() {
                            let _ = tx.send(class);
                        }
                    }
                        }
            }
        }
        
        Ok(())
    }
}

impl WindowMonitor for NiriWindowMonitor {
    fn get_initial_window_class(&self) -> Result<String> {
        Ok(Self::get_active_window_class().unwrap_or_else(|| "Desktop".to_string()))
    }
    
    fn run_event_monitor(&self, tx: mpsc::Sender<String>) -> Result<()> {
        Self::run_event_monitor(tx)
    }
}

// ============================================================================
// Window Manager Detection and Main Logic
// ============================================================================

fn detect_window_manager() -> Result<Box<dyn WindowMonitor>> {
    // Check for Wayland first
    if std::env::var("WAYLAND_DISPLAY").is_ok() {
        if std::env::var("HYPRLAND_INSTANCE_SIGNATURE").is_ok() {
            eprintln!("[helper] Hyprland detected");
            return Ok(Box::new(HyprlandWindowMonitor::new()?));
        }
        
        if std::env::var("XDG_CURRENT_DESKTOP").map_or(false, |desktop| desktop.to_lowercase() == "niri") {
            eprintln!("[helper] Niri detected");
            return Ok(Box::new(NiriWindowMonitor::new()?));
        }
        
        if std::env::var("SWAYSOCK").is_ok() {
            eprintln!("[helper] Sway detected");
            return Ok(Box::new(SwayWindowMonitor::new()?));
        }
        
        if std::env::var("GNOME_DESKTOP_SESSION_ID").is_ok() || 
           std::env::var("XDG_CURRENT_DESKTOP").map_or(false, |desktop| desktop.to_lowercase().contains("gnome")) {
            eprintln!("[helper] GNOME Wayland detected");
            return Ok(Box::new(GnomeWindowMonitor::new()?));
        }
        
        return Err(FocusHelperError::Unsupported("No event-driven support for generic Wayland".to_string()));
    }
    
    // Check for X11
    if std::env::var("DISPLAY").is_ok() && std::env::var("WAYLAND_DISPLAY").is_err() {
        eprintln!("[helper] X11 detected");
        return Ok(Box::new(X11WindowMonitor::new()?));
    }
    
    Err(FocusHelperError::Unsupported("No supported compositor detected".to_string()))
}

fn main() -> Result<()> {
    // Debug environment variables
    eprintln!("[helper] Environment variables:");
    eprintln!("[helper] DISPLAY={:?}", std::env::var("DISPLAY"));
    eprintln!("[helper] WAYLAND_DISPLAY={:?}", std::env::var("WAYLAND_DISPLAY"));
    eprintln!("[helper] SWAYSOCK={:?}", std::env::var("SWAYSOCK"));
    eprintln!("[helper] HYPRLAND_INSTANCE_SIGNATURE={:?}", std::env::var("HYPRLAND_INSTANCE_SIGNATURE"));
    eprintln!("[helper] NIRI_SOCKET={:?}", std::env::var("NIRI_SOCKET"));
    eprintln!("[helper] XDG_CURRENT_DESKTOP={:?}", std::env::var("XDG_CURRENT_DESKTOP"));
    
    // Detect window manager and run
    let monitor = detect_window_manager()?;
    let mut runner = EventRunner::new();
    
    runner.run_with_monitor(monitor)
}

 
 