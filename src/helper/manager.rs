use std::process::{Child, Command};
use std::os::unix::net::{UnixListener, UnixStream};
use std::os::unix::io::AsRawFd;
use std::fs;
use std::collections::HashMap;
use nix::unistd::{chown, User};

use crate::DEBUG_LOGGING;

// Temporarily enable debug logging for testing
const DEBUG_LOGGING_MANAGER: bool = true;

fn get_env_from_session(user: &str) -> HashMap<String, String> {
    let mut env = HashMap::new();
    
    if DEBUG_LOGGING {
        println!("[get_env_from_session] Getting environment for user: {}", user);
    }
    
    // Step 1: Get the active graphical session for the user
    let session_output = match Command::new("loginctl")
        .arg("list-sessions")
        .arg("--no-legend")
        .output() {
        Ok(output) => output,
        Err(e) => {
            println!("[get_env_from_session] Failed to get sessions: {}", e);
            return env;
        }
    };
    
    let sessions = String::from_utf8_lossy(&session_output.stdout);
    let mut session_id = None;
    
    // Find the user's session
    for line in sessions.lines() {
        let parts: Vec<&str> = line.split_whitespace().collect();
        if parts.len() >= 3 && parts[2] == user {
            session_id = Some(parts[0]);
            if DEBUG_LOGGING {
                println!("[get_env_from_session] Found session {} for user {}", parts[0], user);
            }
            break;
        }
    }
    
    let session_id = match session_id {
        Some(id) => id,
        None => {
            println!("[get_env_from_session] No session found for user {}", user);
            return env;
        }
    };
    
    // Step 2: Get the session leader PID (display manager)
    let leader_output = match Command::new("loginctl")
        .arg("show-session")
        .arg(session_id)
        .arg("-p")
        .arg("Leader")
        .arg("--value")
        .output() {
        Ok(output) => output,
        Err(e) => {
            println!("[get_env_from_session] Failed to get leader PID: {}", e);
            return env;
        }
    };
    
    let leader_output_str = String::from_utf8_lossy(&leader_output.stdout);
    let leader_pid_str = leader_output_str.trim();
    let leader_pid: u32 = match leader_pid_str.parse() {
        Ok(pid) => pid,
        Err(e) => {
            println!("[get_env_from_session] Failed to parse leader PID '{}': {}", leader_pid_str, e);
            return env;
        }
    };
    
    if DEBUG_LOGGING {
        println!("[get_env_from_session] Session leader PID: {}", leader_pid);
    }
    
    // Step 3: Find the deepest child process (GUI session) using pstree
    let pstree_output = match Command::new("pstree")
        .arg("-p")
        .arg(&leader_pid.to_string())
        .output() {
        Ok(output) => output,
        Err(e) => {
            println!("[get_env_from_session] Failed to get process tree: {}", e);
            return env;
        }
    };
    
    let pstree_str = String::from_utf8_lossy(&pstree_output.stdout);
    if DEBUG_LOGGING {
        println!("[get_env_from_session] Process tree: {}", pstree_str);
    }
    
    // Extract the last PID from the process tree (deepest child)
    let lines: Vec<&str> = pstree_str.lines().collect();
    let gui_pid = if let Some(last_line) = lines.last() {
        // Find all PIDs in the last line and take the last one
        let mut pids = Vec::new();
        for part in last_line.split_whitespace() {
            if part.contains('(') && part.contains(')') {
                if let Some(start) = part.find('(') {
                    if let Some(end) = part.find(')') {
                        if start < end {
                            let pid_str = &part[start + 1..end];
                            if let Ok(pid) = pid_str.parse::<u32>() {
                                pids.push(pid);
                            }
                        }
                    }
                }
            }
        }
        
        if let Some(&pid) = pids.last() {
            if DEBUG_LOGGING {
                println!("[get_env_from_session] Found GUI PID: {} (deepest child of leader {})", pid, leader_pid);
            }
            pid
        } else {
            if DEBUG_LOGGING {
                println!("[get_env_from_session] Using leader PID as fallback: {}", leader_pid);
            }
            leader_pid
        }
    } else {
        if DEBUG_LOGGING {
            println!("[get_env_from_session] Using leader PID as fallback: {}", leader_pid);
        }
        leader_pid
    };
    
    // Step 4: Extract environment from the GUI PID
    if DEBUG_LOGGING {
        println!("[get_env_from_session] Reading environment from /proc/{}/environ", gui_pid);
    }
    
    let path = format!("/proc/{}/environ", gui_pid);
    if let Ok(data) = fs::read(&path) {
        if DEBUG_LOGGING {
            println!("[get_env_from_session] Successfully read {} bytes from /proc/{}/environ", data.len(), gui_pid);
        }
        
        for entry in data.split(|&b| b == 0) {
            if entry.is_empty() {
                continue;
            }
            
            if let Some(eq) = entry.iter().position(|&b| b == b'=') {
                let key = String::from_utf8_lossy(&entry[..eq]).to_string();
                let value = String::from_utf8_lossy(&entry[eq+1..]).to_string();
                
                // Only log important environment variables
                if DEBUG_LOGGING && (key == "DISPLAY" || key == "WAYLAND_DISPLAY" || key == "DBUS_SESSION_BUS_ADDRESS" || key == "XDG_RUNTIME_DIR") {
                    println!("[get_env_from_session] Found key var: {}={}", key, value);
                }
                
                env.insert(key, value);
            }
        }
        
        if DEBUG_LOGGING {
            println!("[get_env_from_session] Total environment variables found: {}", env.len());
        }
    } else {
        println!("[get_env_from_session] ERROR: Failed to read environment from /proc/{}/environ", gui_pid);
    }
    
    env
}

fn get_env_from_pid(pid: u32) -> HashMap<String, String> {
    let mut env = HashMap::new();
    let path = format!("/proc/{}/environ", pid);
    
    if DEBUG_LOGGING {
        println!("[get_env_from_pid] Reading environment from /proc/{}/environ", pid);
    }
    
    if let Ok(data) = fs::read(&path) {
        if DEBUG_LOGGING {
            println!("[get_env_from_pid] Successfully read {} bytes from /proc/{}/environ", data.len(), pid);
        }
        
        let mut entry_count = 0;
        for entry in data.split(|&b| b == 0) {
            if entry.is_empty() {
                continue;
            }
            
            if let Some(eq) = entry.iter().position(|&b| b == b'=') {
                let key = String::from_utf8_lossy(&entry[..eq]).to_string();
                let value = String::from_utf8_lossy(&entry[eq+1..]).to_string();
                
                // Only log important environment variables, not all 48+
                if DEBUG_LOGGING && (key == "DISPLAY" || key == "WAYLAND_DISPLAY" || key == "DBUS_SESSION_BUS_ADDRESS" || key == "XDG_RUNTIME_DIR") {
                    println!("[get_env_from_pid] Found key var: {}={}", key, value);
                }
                
                env.insert(key, value);
                entry_count += 1;
            } else if DEBUG_LOGGING {
                println!("[get_env_from_pid] Skipping malformed entry: {:?}", entry);
            }
        }
        if DEBUG_LOGGING {
            println!("[get_env_from_pid] Total environment variables found: {}", env.len());
        }
    } else {
        println!("[get_env_from_pid] ERROR: Failed to read environment from /proc/{}/environ", pid);
        // Try to get more details about why it failed
        match fs::metadata(&path) {
            Ok(metadata) => println!("[get_env_from_pid] File exists, size: {} bytes, permissions: {:?}", metadata.len(), metadata.permissions()),
            Err(e) => println!("[get_env_from_pid] File metadata error: {}", e),
        }
    }
    
    env
}



pub struct HelperManager {
    process: Option<Child>,
    listener: Option<UnixListener>,
    login_time: Option<std::time::Instant>, // Track when user logged in
}

pub struct VlcHelperManager {
    process: Option<Child>,
    listener: Option<UnixListener>,
}

pub struct BrowserHelperManager {
    process: Option<Child>,
    listener: Option<UnixListener>,
}

impl HelperManager {
    pub fn new() -> Self {
        if DEBUG_LOGGING {
            println!("[HelperManager::new] Creating new HelperManager instance");
        }
        HelperManager {
            process: None,
            listener: None,
            login_time: None, // Track when user logged in
        }
    }

    pub fn start(&mut self, user: &str) -> Option<i32> {
        if DEBUG_LOGGING {
            println!("[HelperManager::start] Starting helper for user: {}", user);
        }
        
        if self.process.is_some() {
            if DEBUG_LOGGING {
                println!("[HelperManager::start] Helper process already exists, returning None");
            }
            return None;
        }

        let socket_path = "/tmp/touchbar.sock";
        if DEBUG_LOGGING {
            println!("[HelperManager::start] Using socket path: {}", socket_path);
        }
        
        // Clean up old socket file if it exists
        match fs::remove_file(&socket_path) {
            Ok(_) => if DEBUG_LOGGING { println!("[HelperManager::start] Removed old socket file") },
            Err(e) => if DEBUG_LOGGING { println!("[HelperManager::start] No old socket file to remove: {}", e) },
        }

        if DEBUG_LOGGING {
            println!("[HelperManager::start] Binding Unix listener to socket");
        }
        let listener = UnixListener::bind(socket_path).expect("Failed to bind socket");
        if DEBUG_LOGGING {
            println!("[HelperManager::start] Successfully bound socket");
        }
        
        listener.set_nonblocking(true).expect("Failed to set socket non-blocking");
        if DEBUG_LOGGING {
            println!("[HelperManager::start] Set socket to non-blocking mode");
        }

        // Change ownership of the socket to the logged-in user
        if DEBUG_LOGGING {
            println!("[HelperManager::start] Looking up user info for: {}", user);
        }
        if let Some(userinfo) = User::from_name(user).unwrap() {
            if DEBUG_LOGGING {
                println!("[HelperManager::start] Found user info - UID: {}, GID: {}", userinfo.uid, userinfo.gid);
            }
            match chown(socket_path, Some(userinfo.uid), Some(userinfo.gid)) {
                Ok(_) => if DEBUG_LOGGING { println!("[HelperManager::start] Successfully changed socket ownership") },
                Err(e) => if DEBUG_LOGGING { println!("[HelperManager::start] Failed to change socket ownership: {}", e) },
            }
        } else {
            println!("[HelperManager::start] WARNING: Could not find user info for: {}", user);
        }

        let fd = listener.as_raw_fd();
        if DEBUG_LOGGING {
            println!("[HelperManager::start] Got listener file descriptor: {}", fd);
        }
        self.listener = Some(listener);

        // Get environment variables using the bash script approach
        let env_vars = get_env_from_session(user);
        
        // Debug: print only important environment variables
        if DEBUG_LOGGING {
            println!("[HelperManager::start] Important environment variables found:");
            for key in &["DISPLAY", "WAYLAND_DISPLAY", "DBUS_SESSION_BUS_ADDRESS", "XDG_RUNTIME_DIR"] {
                if let Some(val) = env_vars.get(*key) {
                    println!("[HelperManager::start]   {}={}", key, val);
                } else {
                    println!("[HelperManager::start]   Missing: {}", key);
                }
            }
        }

        let helper_path = "/usr/bin/tiny-dfr-focus-window-helper";
        if DEBUG_LOGGING {
            println!("[HelperManager::start] Helper path: {}", helper_path);
        }
        
        // Check if helper binary exists
        match fs::metadata(helper_path) {
            Ok(metadata) => if DEBUG_LOGGING { println!("[HelperManager::start] Helper binary exists, size: {} bytes", metadata.len()) },
            Err(e) => if DEBUG_LOGGING { println!("[HelperManager::start] WARNING: Helper binary not found or not accessible: {}", e) },
        }

        if DEBUG_LOGGING {
            println!("[HelperManager::start] Building command: sudo -u {} env ...", user);
        }
        let mut cmd = Command::new("sudo");
        cmd.arg("-u").arg(user)
           .arg("env");
        
        // Pass relevant environment variables if found
        let relevant_keys = ["DISPLAY", "WAYLAND_DISPLAY", "DBUS_SESSION_BUS_ADDRESS", "XAUTHORITY", "SWAYSOCK", "XDG_RUNTIME_DIR", "HOME", "HYPRLAND_INSTANCE_SIGNATURE", "XDG_CURRENT_DESKTOP", "GNOME_DESKTOP_SESSION_ID"];
        if DEBUG_LOGGING {
            println!("[HelperManager::start] Checking for relevant environment variables:");
            for key in &relevant_keys {
                if let Some(val) = env_vars.get(*key) {
                    println!("[HelperManager::start]   Found {}={}", key, val);
                } else {
                    println!("[HelperManager::start]   Missing: {}", key);
                }
            }
        }
        
        for key in &relevant_keys {
            if let Some(val) = env_vars.get(*key) {
                cmd.arg(format!("{}={}", key, val));
            }
        }
        
        cmd.arg(helper_path);
        
        // Debug: print the final command being executed
        if DEBUG_LOGGING {
            println!("[HelperManager::start] Final command: {:?}", cmd);
        }
        
        if DEBUG_LOGGING {
            println!("[HelperManager::start] Spawning helper process");
        }
        let child = match cmd.spawn() {
            Ok(child) => {
                if DEBUG_LOGGING {
                    println!("[HelperManager::start] Successfully spawned helper process with PID: {}", child.id());
                }
                child
            },
            Err(e) => {
                println!("[HelperManager::start] ERROR: Failed to spawn helper process: {}", e);
                panic!("Failed to start helper");
            }
        };
        
        self.process = Some(child);
        if DEBUG_LOGGING {
            println!("[HelperManager::start] Helper process stored, returning fd: {}", fd);
        }
        Some(fd)
    }

    pub fn stop(&mut self) {
        if let Some(mut child) = self.process.take() {
            child.kill().expect("Failed to kill helper");
        }
        self.listener.take();
        
        // Reset session state when stopping
        self.login_time = None; // Reset login time
        if DEBUG_LOGGING {
            println!("[HelperManager::stop] Helper stopped, session state reset");
        }
    }

    pub fn check_session_ready(&mut self) -> bool {
        // Simple approach: wait 20 seconds after login, then start helper
        if let Some(login_time) = self.login_time {
            let elapsed = login_time.elapsed();
            if elapsed >= std::time::Duration::from_secs(20) {
                if DEBUG_LOGGING {
                    println!("[HelperManager::check_session_ready] 20 seconds passed since login, session should be ready");
                }
                return true;
            } else {
                if DEBUG_LOGGING {
                    println!("[HelperManager::check_session_ready] Waiting for session to be ready... {}s remaining", 20 - elapsed.as_secs());
                }
                return false;
            }
        }
        
        // No login time set yet
        false
    }

    pub fn accept_connection(&mut self) -> Option<UnixStream> {
        if DEBUG_LOGGING {
            println!("[HelperManager::accept_connection] Attempting to accept connection");
        }
        if let Some(listener) = &self.listener {
            if DEBUG_LOGGING {
                println!("[HelperManager::accept_connection] Listener exists, trying to accept");
            }
            match listener.accept() {
                Ok((stream, addr)) => {
                    if DEBUG_LOGGING {
                        println!("[HelperManager::accept_connection] Connection accepted from {:?}", addr);
                    }
                    if let Err(e) = stream.set_nonblocking(true) {
                        if DEBUG_LOGGING {
                            println!("[HelperManager::accept_connection] WARNING: Failed to set stream non-blocking: {}", e);
                        }
                    } else if DEBUG_LOGGING {
                        println!("[HelperManager::accept_connection] Stream set to non-blocking mode");
                    }
                    Some(stream)
                },
                Err(e) => {
                    if DEBUG_LOGGING {
                        println!("[HelperManager::accept_connection] Failed to accept connection: {}", e);
                    }
                    None
                }
            }
        } else {
            if DEBUG_LOGGING {
                println!("[HelperManager::accept_connection] No listener available");
            }
            None
        }
    }

    pub fn set_login_time(&mut self) {
        self.login_time = Some(std::time::Instant::now());
        if DEBUG_LOGGING {
            println!("[HelperManager::set_login_time] Login time set");
        }
    }

    pub fn is_process_none(&self) -> bool {
        self.process.is_none()
    }
}

impl VlcHelperManager {
    pub fn new() -> Self {
        VlcHelperManager {
            process: None,
            listener: None,
        }
    }

    pub fn start(&mut self, user: &str) -> Option<i32> {
        if self.process.is_some() {
            return None;
        }

        let socket_path = "/tmp/touchbar-vlc.sock";
        // Clean up old socket file if it exists
        let _ = fs::remove_file(&socket_path);

        let listener = UnixListener::bind(socket_path).expect("Failed to bind VLC socket");
        listener.set_nonblocking(true).expect("Failed to set VLC socket non-blocking");

        // Change ownership of the socket to the logged-in user
        if let Some(userinfo) = User::from_name(user).unwrap() {
            chown(socket_path, Some(userinfo.uid), Some(userinfo.gid)).expect("Failed to chown VLC socket");
        }

        let fd = listener.as_raw_fd();
        self.listener = Some(listener);

        // Get environment variables using the bash script approach
        let env_vars = get_env_from_session(user);
        
        // Debug: print all environment variables being passed
        println!("[main] Environment variables for VLC helper:");
        for (key, value) in &env_vars {
            println!("[main]   {}={}", key, value);
        }

        let helper_path = "tiny-dfr-vlc-helper";
        let mut cmd = Command::new("sudo");
        cmd.arg("-u").arg(user)
           .arg("env");
        // Pass relevant environment variables if found
        for key in &["DISPLAY", "WAYLAND_DISPLAY", "DBUS_SESSION_BUS_ADDRESS", "XAUTHORITY", "SWAYSOCK", "XDG_RUNTIME_DIR", "HOME", "HYPRLAND_INSTANCE_SIGNATURE"] {
            if let Some(val) = env_vars.get(*key) {
                cmd.arg(format!("{}={}", key, val));
            }
        }
        cmd.arg(helper_path);
        println!("[main] Spawning VLC helper: sudo -u {} env ... {}", user, helper_path);
        let child = cmd.spawn().expect("Failed to start VLC helper");
        self.process = Some(child);
        Some(fd)
    }

    pub fn stop(&mut self) {
        if let Some(mut child) = self.process.take() {
            child.kill().expect("Failed to kill VLC helper");
        }
        self.listener.take();
    }

    pub fn accept_connection(&mut self) -> Option<UnixStream> {
        if let Some(listener) = &self.listener {
            if let Ok((stream, _)) = listener.accept() {
                stream.set_nonblocking(true).expect("Failed to set VLC stream non-blocking");
                return Some(stream);
            }
        }
        None
    }
}

impl BrowserHelperManager {
    pub fn new() -> Self {
        BrowserHelperManager {
            process: None,
            listener: None,
        }
    }

    pub fn start(&mut self, user: &str) -> Option<i32> {
        if self.process.is_some() {
            return None;
        }

        let socket_path = "/tmp/touchbar-browser.sock";
        // Clean up old socket file if it exists
        let _ = fs::remove_file(&socket_path);

        let listener = UnixListener::bind(socket_path).expect("Failed to bind browser socket");
        listener.set_nonblocking(true).expect("Failed to set browser socket non-blocking");

        // Change ownership of the socket to the logged-in user
        if let Some(userinfo) = User::from_name(user).unwrap() {
            chown(socket_path, Some(userinfo.uid), Some(userinfo.gid)).expect("Failed to chown browser socket");
        }

        let fd = listener.as_raw_fd();
        self.listener = Some(listener);

        // Get environment variables using the bash script approach
        let env_vars = get_env_from_session(user);
        
        // Debug: print all environment variables being passed
        println!("[main] Environment variables for browser helper:");
        for (key, value) in &env_vars {
            println!("[main]   {}={}", key, value);
        }

        let helper_path = "tiny-dfr-browser-helper";
        let mut cmd = Command::new("sudo");
        cmd.arg("-u").arg(user)
           .arg("env");
        // Pass relevant environment variables if found
        for key in &["DISPLAY", "WAYLAND_DISPLAY", "DBUS_SESSION_BUS_ADDRESS", "XAUTHORITY", "SWAYSOCK", "XDG_RUNTIME_DIR", "HOME", "HYPRLAND_INSTANCE_SIGNATURE"] {
            if let Some(val) = env_vars.get(*key) {
                cmd.arg(format!("{}={}", key, val));
            }
        }
        cmd.arg(helper_path);
        println!("[main] Spawning browser helper: sudo -u {} env ... {}", user, helper_path);
        let child = cmd.spawn().expect("Failed to start browser helper");
        self.process = Some(child);
        Some(fd)
    }

    pub fn stop(&mut self) {
        if let Some(mut child) = self.process.take() {
            child.kill().expect("Failed to kill browser helper");
        }
        self.listener.take();
    }

    pub fn accept_connection(&mut self) -> Option<UnixStream> {
        if let Some(listener) = &self.listener {
            if let Ok((stream, _)) = listener.accept() {
                stream.set_nonblocking(true).expect("Failed to set browser stream non-blocking");
                return Some(stream);
            }
        }
        None
    }
} 