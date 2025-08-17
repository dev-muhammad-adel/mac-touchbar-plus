use std::process::{Child, Command};
use std::os::unix::net::{UnixListener, UnixStream};
use std::os::unix::io::AsRawFd;
use std::os::unix::process::CommandExt;

use std::fs;
use std::path;
use std::collections::HashMap;
use nix::unistd::{chown, User};
use std::time::{Duration, Instant};
use libc;



fn get_env_from_session(user: &str, leader_pid: u32) -> HashMap<String, String> {
    let mut env = HashMap::new();
    
        // NEW APPROACH: Use the exact same bash command that works
    
    let mut main_level_pids: Vec<u32> = Vec::new();
    
    // Run the exact same command that you tested
    let bash_output = match Command::new("bash")
        .arg("-c")
        .arg(format!("pstree -p {} | grep -oP '\\(\\d+\\)' | grep -oP '\\d+' | head -n 10", leader_pid))
        .output() {
        Ok(output) => output,
        Err(e) => {
            println!("[get_env_from_session] Failed to run bash command: {}", e);
            return env;
            }
    };
    
    let bash_str = String::from_utf8_lossy(&bash_output.stdout);
    
    // Parse the PIDs from bash output
    for line in bash_str.lines() {
        if let Ok(pid) = line.trim().parse::<u32>() {
            main_level_pids.push(pid);
        }
    }
    
    // Step 4: Accumulate environment from each main tree level, with later levels overriding earlier ones
    for (_, &pid) in main_level_pids.iter().enumerate() {
        let path = format!("/proc/{}/environ", pid);
        if let Ok(data) = fs::read(&path) {
            for entry in data.split(|&b| b == 0) {
                if entry.is_empty() {
                    continue;
                }
                
                if let Some(eq) = entry.iter().position(|&b| b == b'=') {
                    let key = String::from_utf8_lossy(&entry[..eq]).to_string();
                    let value = String::from_utf8_lossy(&entry[eq+1..]).to_string();
                    
                    // Insert or override (later levels take precedence)
                    env.insert(key, value);
                }
            }
        }
    }
        

    
    // Fallback: If we have XDG_RUNTIME_DIR but no WAYLAND_DISPLAY, check for wayland socket
    if !env.contains_key("WAYLAND_DISPLAY") && env.contains_key("XDG_RUNTIME_DIR") {
        if let Some(xdg_runtime) = env.get("XDG_RUNTIME_DIR") {
            // Look for any wayland socket files (wayland-0, wayland-1, etc.)
            if let Ok(entries) = fs::read_dir(xdg_runtime) {
                for entry in entries {
                    if let Ok(entry) = entry {
                        let file_name = entry.file_name();
                        if let Some(name) = file_name.to_str() {
                            if name.starts_with("wayland-") && !name.ends_with(".lock") {
                                println!("[get_env_from_session] Found wayland socket: {}, setting WAYLAND_DISPLAY={}", name, name);
                                env.insert("WAYLAND_DISPLAY".to_string(), name.to_string());
                                break;
                            }
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
                println!("[get_env_from_session] Detected GNOME from DESKTOP_SESSION, setting GNOME_DESKTOP_SESSION_ID=gnome");
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



pub struct HelperManager {
    process: Option<Child>,
    listener: Option<UnixListener>,
    login_time: Option<std::time::Instant>, // Track when user logged in
    delay_start_time: Option<Instant>, // Track when delay started
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
        HelperManager {
            process: None,
            listener: None,
            login_time: None, // Track when user logged in
            delay_start_time: None, // Track when delay started
        }
    }

    pub fn start(&mut self, user: &str, leader_pid: u32) -> Option<i32> {
        if self.process.is_some() {
            return None;
        }

        // Delay start time is already set in set_login_time()

        let socket_path = "/tmp/touchbar.sock";
        
        // Clean up old socket file if it exists
        let _ = fs::remove_file(&socket_path);

        let listener = UnixListener::bind(socket_path).expect("Failed to bind socket");
        listener.set_nonblocking(true).expect("Failed to set socket non-blocking");

        // Get user info for socket ownership and process spawning
        let userinfo = match User::from_name(user) {
            Ok(Some(userinfo)) => userinfo,
            _ => {
                println!("[HelperManager::start] ERROR: Could not find user info for: {}", user);
                panic!("Failed to get user info");
            }
        };

        // Change ownership of the socket to the logged-in user
            let _ = chown(socket_path, Some(userinfo.uid), Some(userinfo.gid));

        let fd = listener.as_raw_fd();
        self.listener = Some(listener);

        // Get environment variables using the bash script approach
        let env_vars = get_env_from_session(user, leader_pid);

        let helper_path = "/usr/bin/tiny-dfr-focus-window-helper";

        // Instead of using sudo, we'll run as root but set the effective user ID
        // This gives us direct control over the process without sudo wrapper
        let mut cmd = Command::new(helper_path);
        
        // Set the user ID and group ID for the process
        cmd.uid(userinfo.uid.into());
        cmd.gid(userinfo.gid.into());
        
        // Set the working directory to the user's home
        if let Some(home) = env_vars.get("HOME") {
            cmd.current_dir(home);
        }
        
        // Set all the environment variables
        for (key, value) in &env_vars {
            cmd.env(key, value);
        }
        
        let child = match cmd.spawn() {
            Ok(child) => child,
            Err(e) => {
                println!("[HelperManager::start] ERROR: Failed to spawn helper process: {}", e);
                panic!("Failed to start helper");
            }
        };
        
        self.process = Some(child);
        Some(fd)
    }

    pub fn stop(&mut self) {
        if let Some(mut child) = self.process.take() {
            // Kill the entire process group to handle D-Bus connections and child processes
            let _ = unsafe { libc::killpg(child.id() as i32, libc::SIGTERM) };
            let _ = child.wait();
        }
        self.listener.take();
        
        // Reset session state when stopping
        self.login_time = None; // Reset login time
        self.delay_start_time = None; // Reset delay time
    }
    
    /// Check if the helper process is still running and clean up zombies
    pub fn check_process_status(&mut self) -> bool {
        if let Some(ref mut child) = self.process {
            // Check if process has exited
            match child.try_wait() {
                Ok(Some(_)) => {
                    // Process has exited, clean it up
                    println!("[HelperManager] Helper process has exited, cleaning up");
                    self.process = None;
                    false
                }
                Ok(None) => {
                    // Process is still running
                    true
                }
                Err(e) => {
                    // Error checking process status
                    println!("[HelperManager] Error checking process status: {}", e);
                    self.process = None;
                    false
                }
            }
        } else {
            false
        }
    }

    pub fn check_session_ready(&mut self) -> bool {
        // Check if 1 second has passed since delay started
        if let Some(delay_start) = self.delay_start_time {
            let elapsed = delay_start.elapsed();
            if elapsed >= Duration::from_secs(1) {
                println!("[HelperManager::check_session_ready] 1 second delay completed, session is ready");
                return true;
            } else {
                let remaining = Duration::from_secs(1) - elapsed;
                println!("[HelperManager::check_session_ready] Waiting for session to be ready, {:.1} seconds remaining", remaining.as_secs_f64());
                return false;
            }
        }
        
        // If no delay has been set, session is not ready
        println!("[HelperManager::check_session_ready] No delay timer set, session not ready");
        false
    }

    pub fn accept_connection(&mut self) -> Option<UnixStream> {
        if let Some(listener) = &self.listener {
            match listener.accept() {
                Ok((stream, _)) => {
                    let _ = stream.set_nonblocking(true);
                    Some(stream)
                },
                Err(_) => None
            }
        } else {
            None
        }
    }

    pub fn set_login_time(&mut self) {
        self.login_time = Some(std::time::Instant::now());
        // Start the delay timer when login time is set
        self.delay_start_time = Some(Instant::now());
        println!("[HelperManager::set_login_time] Started 1 second delay timer");
    }

    pub fn is_process_none(&self) -> bool {
        self.process.is_none()
    }
    
    /// Check if any helper process is currently running
    pub fn is_process_running(&self) -> bool {
        self.process.is_some()
    }
    
    /// Force cleanup of zombie processes
    pub fn force_cleanup(&mut self) {
        if let Some(mut child) = self.process.take() {
            println!("[HelperManager] Force cleaning up helper process");
            
            // Kill the entire process group to handle D-Bus connections and child processes
            let _ = unsafe { libc::killpg(child.id() as i32, libc::SIGTERM) };
            
            // Wait for it to exit
            let _ = child.wait();
            
            // Reset state
            self.login_time = None;
            self.delay_start_time = None;
        }
    }
}

impl VlcHelperManager {
    pub fn new() -> Self {
        VlcHelperManager {
            process: None,
            listener: None,
        }
    }

    pub fn start(&mut self, user: &str, leader_pid: u32) -> Option<i32> {
        if self.process.is_some() {
            return None;
        }

        let socket_path = "/tmp/touchbar-vlc.sock";
        // Clean up old socket file if it exists
        let _ = fs::remove_file(&socket_path);

        let listener = UnixListener::bind(socket_path).expect("Failed to bind VLC socket");
        listener.set_nonblocking(true).expect("Failed to set VLC socket non-blocking");

        // Get user info for socket ownership and process spawning
        let userinfo = match User::from_name(user) {
            Ok(Some(userinfo)) => userinfo,
            _ => {
                println!("[VlcHelperManager::start] ERROR: Could not find user info for: {}", user);
                panic!("Failed to get user info");
            }
        };

        // Change ownership of the socket to the logged-in user
        let _ = chown(socket_path, Some(userinfo.uid), Some(userinfo.gid));

        let fd = listener.as_raw_fd();
        self.listener = Some(listener);

        // Get environment variables using the bash script approach
        let env_vars = get_env_from_session(user, leader_pid);
        
        // Debug: print all environment variables being passed
        println!("[main] Environment variables for VLC helper:");
        for (key, value) in &env_vars {
            println!("[main]   {}={}", key, value);
        }

        let helper_path = "/usr/bin/tiny-dfr-vlc-helper";

        // Instead of using sudo, we'll run as root but set the effective user ID
        // This gives us direct control over the process without sudo wrapper
        let mut cmd = Command::new(helper_path);
        
        // Set the user ID and group ID for the process
        cmd.uid(userinfo.uid.into());
        cmd.gid(userinfo.gid.into());
        
        // Set the working directory to the user's home
        if let Some(home) = env_vars.get("HOME") {
            cmd.current_dir(home);
        }
        
        // Set all the environment variables
        for (key, value) in &env_vars {
            cmd.env(key, value);
        }
        
        println!("[main] Spawning VLC helper: {} (as user {})", helper_path, user);
        let child = cmd.spawn().expect("Failed to start VLC helper");
        self.process = Some(child);
        Some(fd)
    }

    pub fn stop(&mut self) {
        if let Some(mut child) = self.process.take() {
            // Kill the entire process group to handle D-Bus connections and child processes
            let _ = unsafe { libc::killpg(child.id() as i32, libc::SIGTERM) };
            let _ = child.wait();
        }
        self.listener.take();
    }
    
    /// Check if the VLC helper process is still running and clean up zombies
    pub fn check_process_status(&mut self) -> bool {
        if let Some(ref mut child) = self.process {
            // Check if process has exited
            match child.try_wait() {
                Ok(Some(_)) => {
                    // Process has exited, clean it up
                    println!("[VlcHelperManager] VLC helper process has exited, cleaning up");
                    self.process = None;
                    false
                }
                Ok(None) => {
                    // Process is still running
                    true
                }
                Err(e) => {
                    // Error checking process status
                    println!("[VlcHelperManager] Error checking process status: {}", e);
                    self.process = None;
                    false
                }
            }
        } else {
            false
        }
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
    
    /// Check if the VLC helper process is still running
    pub fn is_process_running(&self) -> bool {
        self.process.is_some()
    }
    
    /// Force cleanup of zombie processes
    pub fn force_cleanup(&mut self) {
        if let Some(mut child) = self.process.take() {
            println!("[VlcHelperManager] Force cleaning up VLC helper process");
            
            // Kill the entire process group to handle D-Bus connections and child processes
            let _ = unsafe { libc::killpg(child.id() as i32, libc::SIGTERM) };
            
            // Wait for it to exit
            let _ = child.wait();
        }
    }
}

impl BrowserHelperManager {
    pub fn new() -> Self {
        BrowserHelperManager {
            process: None,
            listener: None,
        }
    }

    pub fn start(&mut self, user: &str, leader_pid: u32) -> Option<i32> {
        if self.process.is_some() {
            return None;
        }

        let socket_path = "/tmp/touchbar-browser.sock";
        // Clean up old socket file if it exists
        let _ = fs::remove_file(&socket_path);

        let listener = UnixListener::bind(socket_path).expect("Failed to bind browser socket");
        listener.set_nonblocking(true).expect("Failed to set browser socket non-blocking");

        // Get user info for socket ownership and process spawning
        let userinfo = match User::from_name(user) {
            Ok(Some(userinfo)) => userinfo,
            _ => {
                println!("[BrowserHelperManager::start] ERROR: Could not find user info for: {}", user);
                panic!("Failed to get user info");
            }
        };

        // Change ownership of the socket to the logged-in user
        let _ = chown(socket_path, Some(userinfo.uid), Some(userinfo.gid));

        let fd = listener.as_raw_fd();
        self.listener = Some(listener);

        // Get environment variables using the bash script approach
        let env_vars = get_env_from_session(user, leader_pid);
        
        // Debug: print all environment variables being passed
        println!("[main] Environment variables for browser helper:");
        for (key, value) in &env_vars {
            println!("[main]   {}={}", key, value);
        }

        let helper_path = "/usr/bin/tiny-dfr-browser-helper";

        // Instead of using sudo, we'll run as root but set the effective control over the process without sudo wrapper
        let mut cmd = Command::new(helper_path);
        
        // Set the user ID and group ID for the process
        cmd.uid(userinfo.uid.into());
        cmd.gid(userinfo.gid.into());
        
        // Set the working directory to the user's home
        if let Some(home) = env_vars.get("HOME") {
            cmd.current_dir(home);
        }
        
        // Set all the environment variables
        for (key, value) in &env_vars {
            cmd.env(key, value);
        }
        
        println!("[main] Spawning browser helper: {} (as user {})", helper_path, user);
        let child = cmd.spawn().expect("Failed to start browser helper");
        self.process = Some(child);
        Some(fd)
    }

    pub fn stop(&mut self) {
        if let Some(mut child) = self.process.take() {
            // Kill the entire process group to handle D-Bus connections and child processes
            let _ = unsafe { libc::killpg(child.id() as i32, libc::SIGTERM) };
            let _ = child.wait();
        }
        self.listener.take();
    }
    
    /// Check if the browser helper process is still running and clean up zombies
    pub fn check_process_status(&mut self) -> bool {
        if let Some(ref mut child) = self.process {
            // Check if process has exited
            match child.try_wait() {
                Ok(Some(_)) => {
                    // Process has exited, clean it up
                    println!("[BrowserHelperManager] Browser helper process has exited, cleaning up");
                    self.process = None;
                    false
                }
                Ok(None) => {
                    // Process is still running
                    true
                }
                Err(e) => {
                    // Error checking process status
                    println!("[BrowserHelperManager] Error checking process status: {}", e);
                    self.process = None;
                    false
                }
            }
        } else {
            false
        }
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
    
    /// Force cleanup of zombie processes
    pub fn force_cleanup(&mut self) {
        if let Some(mut child) = self.process.take() {
            println!("[BrowserHelperManager] Force cleaning up browser helper process");
            
            // Kill the entire process group to handle D-Bus connections and child processes
            let _ = unsafe { libc::killpg(child.id() as i32, libc::SIGTERM) };
            
            // Wait for it to exit
            let _ = child.wait();
        }
    }
    
    /// Check if the browser helper process is still running
    pub fn is_process_running(&self) -> bool {
        self.process.is_some()
    }
} 