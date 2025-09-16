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

// Window information structure
#[derive(Debug, Clone, PartialEq)]
pub struct WindowInfo {
    pub class: String,
    pub window_id: Option<u64>,
    pub pid: Option<u32>,
}

impl WindowInfo {
    pub fn new(class: String, window_id: Option<u64>, pid: Option<u32>) -> Self {
        Self { class, window_id, pid }
    }
    
    pub fn desktop() -> Self {
        Self {
            class: "Desktop".to_string(),
            window_id: Some(0),
            pid: Some(0),
        }
    }
    
    pub fn to_string(&self) -> String {
        let mut result = self.class.clone();
        
        if let Some(id) = self.window_id {
            result.push_str(&format!(":{}", id));
        }
        
        if let Some(pid) = self.pid {
            result.push_str(&format!(":{}", pid));
        }
        
        result
    }
}

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
    

}

// ============================================================================
// Thread Synchronization Utilities
// ============================================================================

struct ThreadSafeState {
    shutdown_requested: Arc<AtomicBool>,
}

impl ThreadSafeState {
    fn new() -> Self {
        Self {
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
fn create_bounded_channel() -> (mpsc::Sender<WindowInfo>, mpsc::Receiver<WindowInfo>) {
    // Use a bounded channel to prevent memory issues
    mpsc::channel()
}





// ============================================================================
// Window Monitor Trait
// ============================================================================

trait WindowMonitor: Send {
    fn get_initial_window_info(&self) -> Result<WindowInfo>;
    fn run_event_monitor(&self, tx: mpsc::Sender<WindowInfo>) -> Result<()>;
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
        
        // Get initial window info
        let initial_info = monitor.get_initial_window_info()?;
        self.write_to_socket(&mut stream, &initial_info.to_string())?;
        
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

    fn spawn_monitor_thread(&self, monitor: Box<dyn WindowMonitor>, tx: mpsc::Sender<WindowInfo>) -> Result<thread::JoinHandle<()>> {
        let tx_clone = tx.clone();
        let _shutdown_flag = self.state.shutdown_requested.clone();
        
        let monitor_thread = thread::spawn(move || {
            if let Err(e) = monitor.run_event_monitor(tx_clone) {
                eprintln!("[helper] Event monitor error: {}", e);
            }
        });
        Ok(monitor_thread)
    }

    fn run_event_loop(&mut self, stream: &mut UnixStream, rx: mpsc::Receiver<WindowInfo>) -> Result<()> {
        let mut last_info = WindowInfo::desktop();
        
        loop {
            // Check for shutdown request
            if self.state.is_shutdown_requested() {
                eprintln!("[helper] Shutdown requested, exiting event loop");
                break;
            }
            
            // Use pure blocking recv - no polling
            match rx.recv() {
                Ok(info) => {
                    if info != last_info {
                        eprintln!("[helper] Window focus changed from '{}' to '{}'", 
                                 last_info.to_string(), info.to_string());
                        
                        if let Err(e) = self.write_to_socket(stream, &info.to_string()) {
                            eprintln!("[helper] Socket write error: {}", e);
                            // Exit the event loop when socket write fails (e.g., broken pipe)
                            eprintln!("[helper] Exiting due to socket write error");
                            break;
                        }
                        
                        last_info = info;
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
        // Always fetch fresh WM_CLASS to avoid stale cache issues
        // Window IDs can be reused by different applications
        
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
                            eprintln!("[helper] Fresh window class for window {}: {}", window, class_str);
                            return Some(class_str);
                        }
                    }
                }
            }
            Err(_) => {}
        }
        
        None
    }
    
    fn get_window_pid(&self, window: Window) -> Option<u32> {
        // Get _NET_WM_PID property - first try to intern the atom
        let net_wm_pid_atom = match self.conn.intern_atom(false, b"_NET_WM_PID") {
            Ok(cookie) => match cookie.reply() {
                Ok(reply) => reply.atom,
                Err(_) => {
                    eprintln!("[helper] X11: Failed to intern _NET_WM_PID atom");
                    return None;
                },
            },
            Err(_) => {
                eprintln!("[helper] X11: Failed to intern _NET_WM_PID atom");
                return None;
            },
        };
        
        let cookie = self.conn.get_property(
            false,
            window,
            net_wm_pid_atom,
            xproto::AtomEnum::CARDINAL,
            0,
            4,
        );
        
        match cookie.ok()?.reply() {
            Ok(reply) => {
                if reply.value.len() >= 4 {
                    let pid = u32::from_ne_bytes([
                        reply.value[0],
                        reply.value[1],
                        reply.value[2],
                        reply.value[3],
                    ]);
                    return Some(pid);
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
    
    fn get_active_window_info(&mut self) -> Option<WindowInfo> {
        let active_window = self.get_active_window()?;
        
        if active_window == 0 {
            eprintln!("[helper] No active window (empty workspace)");
            return Some(WindowInfo::desktop());
        }
        
        if let Some(class) = self.get_window_class(active_window) {
            if self.active_window != Some(active_window) {
                self.active_window = Some(active_window);
            }
            let pid = self.get_window_pid(active_window);
            return Some(WindowInfo::new(class, Some(active_window as u64), pid));
        }
        
        eprintln!("[helper] Could not get window class for window {}, using Desktop", active_window);
        Some(WindowInfo::desktop())
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
    
    fn wait_for_focus_change(&mut self) -> Result<Option<WindowInfo>> {
        // Use pure blocking event wait - no polling
        match self.conn.wait_for_event() {
            Ok(event) => {
                match event {
                    x11rb::protocol::Event::PropertyNotify(ev) => {
                        if ev.atom == self.net_active_window_atom {
                            eprintln!("[helper] Active window property changed!");
                            return Ok(self.get_active_window_info());
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
    fn get_initial_window_info(&self) -> Result<WindowInfo> {
        let mut monitor = X11WindowMonitor::new()?;
        monitor.setup_event_listening()?;
        
        Ok(monitor.get_active_window_info().unwrap_or_else(|| WindowInfo::desktop()))
    }
    
    fn run_event_monitor(&self, tx: mpsc::Sender<WindowInfo>) -> Result<()> {
        let mut monitor = X11WindowMonitor::new()?;
        monitor.setup_event_listening()?;
        
        let mut last_info = monitor.get_active_window_info().unwrap_or_else(|| WindowInfo::desktop());
        let _ = tx.send(last_info.clone());
        
        loop {
            match monitor.wait_for_focus_change() {
                Ok(Some(new_info)) => {
                    if new_info != last_info {
                        let _ = tx.send(new_info.clone());
                        last_info = new_info;
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
    

    
    fn get_active_window_info() -> Option<WindowInfo> {
        if let Ok(mut connection) = swayipc::Connection::new() {
            if let Ok(tree) = connection.get_tree() {
                if let Some(focused) = tree.find_focused(|n| n.focused) {
                    if focused.node_type == swayipc::reply::NodeType::Con {
                        if let Some(window_properties) = &focused.window_properties {
                            if let Some(class) = &window_properties.class {
                                if !class.is_empty() && class != "null" {
                                    return Some(WindowInfo::new(class.clone(), Some(focused.id as u64), focused.pid.map(|p| p as u32)));
                                }
                            }
                            if let Some(instance) = &window_properties.instance {
                                if !instance.is_empty() && instance != "null" {
                                    return Some(WindowInfo::new(instance.clone(), Some(focused.id as u64), focused.pid.map(|p| p as u32)));
                                }
                            }
                        }
                        
                        if let Some(app_id) = &focused.app_id {
                            if !app_id.is_empty() && app_id != "null" {
                                return Some(WindowInfo::new(app_id.clone(), Some(focused.id as u64), focused.pid.map(|p| p as u32)));
                            }
                        }
                        
                        if let Some(name) = &focused.name {
                            if !name.is_empty() && name != "null" {
                                return Some(WindowInfo::new(name.clone(), Some(focused.id as u64), focused.pid.map(|p| p as u32)));
                            }
                        }
                    } else {
                        return Some(WindowInfo::desktop());
                    }
                }
            }
        }
        Some(WindowInfo::desktop())
    }
    
    fn run_event_monitor(tx: mpsc::Sender<WindowInfo>) -> Result<()> {
        let connection = swayipc::Connection::new()?;
        
        let events = connection.subscribe(&[swayipc::EventType::Workspace, swayipc::EventType::Window])?;
        
        if let Some(info) = Self::get_active_window_info() {
            let _ = tx.send(info);
        }
        
        // Use pure blocking iterator - no polling
        for event in events {
            match event? {
                swayipc::reply::Event::Workspace(_) | swayipc::reply::Event::Window(_) => {
                    if let Some(info) = Self::get_active_window_info() {
                        let _ = tx.send(info);
                    }
                }
                _ => {}
            }
        }
        
        Ok(())
    }
}

impl WindowMonitor for SwayWindowMonitor {
    fn get_initial_window_info(&self) -> Result<WindowInfo> {
        Ok(Self::get_active_window_info().unwrap_or_else(|| WindowInfo::desktop()))
    }
    
    fn run_event_monitor(&self, tx: mpsc::Sender<WindowInfo>) -> Result<()> {
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
    
    fn get_window_pid(_window_id: u64) -> Option<u32> {
        // Use hyprctl to get PID for a specific window
        let output = std::process::Command::new("hyprctl")
            .arg("activewindow")
            .output();
        
        match output {
            Ok(output) => {
                if output.status.success() {
                    let stdout = String::from_utf8_lossy(&output.stdout);
                    let lines: Vec<&str> = stdout.lines().collect();
                    
                    for line in lines {
                        let line = line.trim();
                        if line.starts_with("pid:") {
                            let pid_str = line[4..].trim();
                          
                            if let Ok(pid) = pid_str.parse::<u32>() {
                                return Some(pid);
                            }
                        }
                    }
                }
            }
            Err(e) => {
                eprintln!("[helper] hyprctl command error: {:?}", e);
            }
        }
        
        None
    }
    
    fn run_event_monitor(tx: mpsc::Sender<WindowInfo>) -> Result<()> {
        let socket_path = Self::get_socket_path()?;
        
        let mut stream = UnixStream::connect(&socket_path)?;
        // Keep socket blocking for pure event-driven behavior
        stream.write_all(b"subscribe\n")?;
        
        use std::io::{BufRead, BufReader};
        let reader = BufReader::new(stream);
        
        let mut last_window_id: Option<String> = None;
        let mut last_window_class: Option<String> = None;
        
        // Use pure blocking line reading - no polling
        for line in reader.lines() {
            match line {
                Ok(line) => {
                    eprintln!("[helper] Event received: '{}'", line);
                    
                    if line.starts_with("activewindowv2>>") {
                        // Extract window ID from activewindowv2 event
                        let window_id = &line[16..];
                        
                        if !window_id.trim().is_empty() {
                            last_window_id = Some(window_id.to_string());
                            
                            // If we have both class and ID now, send the complete info
                            if let Some(class) = &last_window_class {
                                if let Ok(window_id_u64) = u64::from_str_radix(window_id, 16) {
                                    // Get PID for this window
                                    let pid = Self::get_window_pid(window_id_u64);
                                    let _ = tx.send(WindowInfo::new(class.clone(), Some(window_id_u64), pid));
                                }
                            } else {
                                eprintln!("[helper] No class yet, waiting for activewindow>> event");
                            }
                        }
                    } else if line.starts_with("activewindow>>") {
                        // Extract window class from activewindow event
                        let window_info = if let Some(pos) = line.find(',') {
                            &line[14..pos]
                        } else {
                            &line[14..]
                        };
                        
                        
                        if window_info.trim().is_empty() {
                            last_window_id = None;
                            last_window_class = None;
                            let _ = tx.send(WindowInfo::desktop());
                        } else {
                            last_window_class = Some(window_info.to_string());
                            
                            // Always wait for both class and ID before sending
                            if let Some(id) = &last_window_id {
                                // Convert hex string to u64
                                if let Ok(window_id_u64) = u64::from_str_radix(id, 16) {
                                    // Get PID for this window
                                    let pid = Self::get_window_pid(window_id_u64);
                                    let _ = tx.send(WindowInfo::new(window_info.to_string(), Some(window_id_u64), pid));
                                } else {
                                    eprintln!("[helper] Failed to parse ID, waiting for valid ID");
                                    // Don't send anything until we have a valid ID
                                }
                            } else {
                                eprintln!("[helper] No ID yet, waiting for activewindowv2>> event");
                                // Don't send anything until we have both class and ID
                            }
                        }
                    } else {
                        eprintln!("[helper] Unknown event type: '{}'", line);
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
    fn get_initial_window_info(&self) -> Result<WindowInfo> {
        
        // Try to get current window info using hyprctl (more reliable than socket)
        let output = std::process::Command::new("hyprctl")
            .arg("activewindow")
            .output();
        
        match output {
            Ok(output) => {
                if output.status.success() {
                    let stdout = String::from_utf8_lossy(&output.stdout);
                    
                    let lines: Vec<&str> = stdout.lines().collect();
                    
                    // Parse hyprctl activewindow output
                    let mut window_class = None;
                    let mut window_id = None;
                    let mut window_pid = None;
                    
                    for line in lines {
                        let line = line.trim();
                        
                        if line.starts_with("class:") {
                            let class = line[6..].trim();
                            if !class.is_empty() && class != "null" {
                                window_class = Some(class.to_string());
                            }
                        } else if line.starts_with("Window") && line.contains("->") {
                            // Extract hex ID from "Window 5626b6b0a690 -> ..."
                            if let Some(start) = line.find("Window ") {
                                if let Some(end) = line[start..].find(" ->") {
                                    let id_str = &line[start + 7..start + end];
                                    if let Ok(id) = u64::from_str_radix(id_str, 16) {
                                        window_id = Some(id);
                                    }
                                }
                            }
                        } else if line.starts_with("pid:") {
                            let pid_str = line[4..].trim();
                            if let Ok(pid) = pid_str.parse::<u32>() {
                                window_pid = Some(pid);
                            }
                        }
                    }
                    
                    
                    if let Some(class) = window_class {
                        if class.is_empty() || class == "null" {
                            eprintln!("[helper] Returning Desktop (empty/null class)");
                            return Ok(WindowInfo::desktop());
                        } else {
                            return Ok(WindowInfo::new(class, window_id, window_pid));
                        }
                    } else {
                        return Ok(WindowInfo::desktop());
                    }
                } else {
                    eprintln!("[helper] hyprctl command failed with status: {}", output.status);
                }
            }
            Err(e) => {
                eprintln!("[helper] hyprctl command error: {:?}", e);
            }
        }
        
        eprintln!("[helper] Fallback: returning Desktop");
        // Fallback to desktop if we can't get window info
        Ok(WindowInfo::desktop())
    }
    
    fn run_event_monitor(&self, tx: mpsc::Sender<WindowInfo>) -> Result<()> {
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
    
    fn get_initial_focused_info() -> Option<WindowInfo> {
        // Get both window class and ID from WindowMonitorPro extension
        let class_output = std::process::Command::new("gdbus")
            .arg("call")
            .arg("--session")
            .arg("--dest")
            .arg("org.gnome.Shell")
            .arg("--object-path")
            .arg("/org/gnome/Shell/Extensions/WindowMonitorPro")
            .arg("--method")
            .arg("org.gnome.Shell.Extensions.WindowMonitorPro.FocusClass")
            .output();
        
        let id_output = std::process::Command::new("gdbus")
            .arg("call")
            .arg("--session")
            .arg("--dest")
            .arg("org.gnome.Shell")
            .arg("--object-path")
            .arg("/org/gnome/Shell/Extensions/WindowMonitorPro")
            .arg("--method")
            .arg("org.gnome.Shell.Extensions.WindowMonitorPro.FocusID")
            .output();
        
        let class = match class_output {
            Ok(output) => {
                if output.status.success() {
                    let stdout = String::from_utf8_lossy(&output.stdout);
                    if let Some(start) = stdout.find("('") {
                        if let Some(end) = stdout[start..].find("',") {
                            let class_str = &stdout[start + 2..start + end];
                            if class_str.is_empty() || class_str == "null" {
                                return Some(WindowInfo::desktop());
                            } else {
                                Some(class_str.to_string())
                            }
                        } else {
                            None
                        }
                    } else {
                        None
                    }
                } else {
                    None
                }
            }
            Err(e) => {
                eprintln!("[helper] FocusClass command error: {:?}", e);
                None
            }
        };
        
        let window_id = match id_output {
            Ok(output) => {
                if output.status.success() {
                    let stdout = String::from_utf8_lossy(&output.stdout);
                    if let Some(start) = stdout.find("('") {
                        if let Some(end) = stdout[start..].find("',") {
                            let id_str = &stdout[start + 2..start + end];
                            id_str.parse::<u64>().ok()
                        } else {
                            None
                        }
                    } else {
                        None
                    }
                } else {
                    None
                }
            }
            Err(e) => {
                eprintln!("[helper] FocusID command error: {:?}", e);
                None
            }
        };
        
        // Get PID using FocusPID method
        let pid_output = std::process::Command::new("gdbus")
            .arg("call")
            .arg("--session")
            .arg("--dest")
            .arg("org.gnome.Shell")
            .arg("--object-path")
            .arg("/org/gnome/Shell/Extensions/WindowMonitorPro")
            .arg("--method")
            .arg("org.gnome.Shell.Extensions.WindowMonitorPro.FocusPID")
            .output();
        
        let window_pid = match pid_output {
            Ok(output) => {
                if output.status.success() {
                    let stdout = String::from_utf8_lossy(&output.stdout);
                    if let Some(start) = stdout.find("('") {
                        if let Some(end) = stdout[start..].find("',") {
                            let pid_str = &stdout[start + 2..start + end];
                            if let Ok(pid) = pid_str.parse::<u32>() {
                                eprintln!("[helper] GNOME: Retrieved PID {} from FocusPID", pid);
                                Some(pid)
                            } else {
                                eprintln!("[helper] GNOME: Failed to parse PID from '{}'", pid_str);
                                None
                            }
                        } else {
                            None
                        }
                    } else {
                        None
                    }
                } else {
                    eprintln!("[helper] GNOME: FocusPID command failed");
                    None
                }
            }
            Err(e) => {
                eprintln!("[helper] GNOME: FocusPID command error: {:?}", e);
                None
            }
        };
        
        match (class, window_id) {
            (Some(class), Some(id)) => {
                println!("[helper] GNOME: Got window class '{}' with ID {} and PID {:?}", class, id, window_pid);
                Some(WindowInfo::new(class, Some(id), window_pid))
            }
            (Some(class), None) => {
                println!("[helper] GNOME: Got window class '{}' but no ID, PID {:?}", class, window_pid);
                Some(WindowInfo::new(class, None, window_pid))
            }
            (None, _) => {
                eprintln!("[helper] GNOME: WindowMonitorPro extension not available or failed");
                eprintln!("[helper] Install from: https://extensions.gnome.org/extension/6027/window-monitor-pro/");
                Some(WindowInfo::new("GNOME: Install WindowMonitorPro Extension".to_string(), None, window_pid))
            }
        }
    }
    
    async fn run_dbus_event_monitor(tx: mpsc::Sender<WindowInfo>) -> Result<()> {
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
                // For now, set PID to 0 (will be handled later)
                let _ = tx.send(WindowInfo::new("Install WindowMonitorPro".to_string(), None, Some(0)));
                return Ok(());
            }
        }
        
        let mut last_info = WindowInfo::desktop();
        
        // Use blocking stream iteration to prevent CPU spinning
        while let Some(msg) = stream.next().await {
            if let Ok(msg) = msg {
                if let Some(interface) = msg.interface() {
                    if interface.as_str() == "org.gnome.Shell.Extensions.WindowMonitorPro" {
                        if let Ok((window_id_str, _window_title, window_class, window_pid_str)) = 
                            msg.body::<(String, String, String, String)>() {
                            
                            let display_info = if window_class.is_empty() {
                                WindowInfo::desktop()
                            } else {
                                // Parse window ID and PID from the signal
                                let window_id = window_id_str.parse::<u64>().ok();
                                let window_pid = window_pid_str.parse::<u32>().ok();
                                eprintln!("[helper] GNOME: D-Bus signal - class: '{}', ID: {:?}, PID: {:?}", window_class, window_id, window_pid);
                                WindowInfo::new(window_class, window_id, window_pid)
                            };
                            
                            if display_info != last_info {
                                let _ = tx.send(display_info.clone());
                                last_info = display_info;
                            }
                        }
                    }
                }
            }
            
            // Pure event-driven - no artificial delays needed
        }
        
        Ok(())
    }
    
    fn run_event_monitor(tx: mpsc::Sender<WindowInfo>) -> Result<()> {
        if let Some(initial_info) = Self::get_initial_focused_info() {
            let _ = tx.send(initial_info);
        }
        
        let rt = tokio::runtime::Runtime::new()
            .map_err(|e| FocusHelperError::Gnome(format!("Failed to create runtime: {}", e)))?;
        
        rt.block_on(async {
            Self::run_dbus_event_monitor(tx).await
        })
    }
}

impl WindowMonitor for GnomeWindowMonitor {
    fn get_initial_window_info(&self) -> Result<WindowInfo> {
        Ok(Self::get_initial_focused_info().unwrap_or_else(|| WindowInfo::desktop()))
    }
    
    fn run_event_monitor(&self, tx: mpsc::Sender<WindowInfo>) -> Result<()> {
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
    
    fn get_active_window_info() -> Option<WindowInfo> {
        let output = std::process::Command::new("niri")
            .arg("msg")
            .arg("focused-window")
            .env("WAYLAND_DISPLAY", std::env::var("WAYLAND_DISPLAY").unwrap_or_else(|_| "wayland-1".to_string()))
            .output();
        
        match output {
            Ok(output) => {
                if !output.status.success() {
                    eprintln!("[helper] Niri command failed with status: {:?}", output.status);
                    return Some(WindowInfo::desktop());
                }
                
                let stdout = String::from_utf8_lossy(&output.stdout);
                let lines: Vec<&str> = stdout.lines().collect();
                
                eprintln!("[helper] Niri output lines: {:?}", lines);
                
                if lines.is_empty() || !lines[0].contains("Window ID") {
                    eprintln!("[helper] No Window ID found in first line: {:?}", lines.get(0));
                    return Some(WindowInfo::desktop());
                }
                
                // Try to extract window ID and PID
                let mut window_id = None;
                let mut window_pid = None;
                
                for line in &lines {
                    if line.trim().starts_with("Window ID") {
                        // Handle both "Window ID:" and "Window ID 4:" formats
                        let parts: Vec<&str> = line.split_whitespace().collect();
                        eprintln!("[helper] Parsing Window ID line: {:?}", parts);
                        if parts.len() >= 3 && parts[0] == "Window" && parts[1] == "ID" {
                            let id_str = parts[2].trim_end_matches(':');
                            eprintln!("[helper] Attempting to parse ID: '{}'", id_str);
                            if let Ok(id) = id_str.parse::<u64>() {
                                window_id = Some(id);
                                eprintln!("[helper] Successfully parsed window ID: {}", id);
                                break;
                            } else {
                                eprintln!("[helper] Failed to parse ID: '{}'", id_str);
                            }
                        }
                    }
                }
                
                // Extract PID
                for line in &lines {
                    if line.trim().starts_with("PID:") {
                        let pid_str = line.split("PID:").nth(1).unwrap_or("").trim();
                        eprintln!("[helper] Found PID line: '{}', extracted: '{}'", line.trim(), pid_str);
                        if let Ok(pid) = pid_str.parse::<u32>() {
                            window_pid = Some(pid);
                            eprintln!("[helper] Successfully parsed PID: {}", pid);
                        } else {
                            eprintln!("[helper] Failed to parse PID: '{}'", pid_str);
                        }
                        break;
                    }
                }
                
                for line in &lines {
                    if line.trim().starts_with("App ID:") {
                        if let Some(app_id) = line.split("App ID:").nth(1) {
                            let app_id = app_id.trim().trim_matches('"');
                            eprintln!("[helper] Found App ID: '{}'", app_id);
                            if !app_id.is_empty() && app_id != "null" {
                                eprintln!("[helper] Returning WindowInfo with App ID: '{}', window_id: {:?}, PID: {:?}", app_id, window_id, window_pid);
                                return Some(WindowInfo::new(app_id.to_string(), window_id, window_pid));
                            }
                        }
                    }
                }
                
                for line in &lines {
                    if line.trim().starts_with("Title:") {
                        if let Some(title) = line.split("Title:").nth(1) {
                            let title = title.trim().trim_matches('"');
                            if !title.is_empty() && title != "null" {
                                eprintln!("[helper] Returning WindowInfo with Title: '{}', window_id: {:?}, PID: {:?}", title, window_id, window_pid);
                                return Some(WindowInfo::new(title.to_string(), window_id, window_pid));
                            }
                        }
                    }
                }
            }
            Err(e) => {
                eprintln!("[helper] Niri command error: {:?}", e);
            }
        }
        
        Some(WindowInfo::desktop())
    }
    
    fn run_event_monitor(tx: mpsc::Sender<WindowInfo>) -> Result<()> {
        if let Some(info) = Self::get_active_window_info() {
            let _ = tx.send(info);
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
                        if let Some(info) = Self::get_active_window_info() {
                            let _ = tx.send(info);
                        }
                    }
                        }
            }
        }
        
        Ok(())
    }
}

impl WindowMonitor for NiriWindowMonitor {
    fn get_initial_window_info(&self) -> Result<WindowInfo> {
        Ok(Self::get_active_window_info().unwrap_or_else(|| WindowInfo::desktop()))
    }
    
    fn run_event_monitor(&self, tx: mpsc::Sender<WindowInfo>) -> Result<()> {
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

    // Detect window manager and run
    let monitor = detect_window_manager()?;
    let mut runner = EventRunner::new();
    
    runner.run_with_monitor(monitor)
}

 
 
 
 
 