use std::process::{Child, Command};
use std::os::unix::net::{UnixListener, UnixStream};
use std::os::unix::io::AsRawFd;
use std::os::unix::process::CommandExt;

use std::fs;
use std::collections::HashMap;
use nix::unistd::{chown, User};
use std::time::{Duration, Instant};
use libc;

// Error recovery configuration
const MAX_RESTART_ATTEMPTS: u32 = 3;
const RESTART_DELAY_SECONDS: u64 = 5;
const PROCESS_HEALTH_CHECK_INTERVAL: u64 = 30; // Check every 30 seconds

// Helper process status tracking
#[derive(Debug, Clone, PartialEq)]
pub enum ProcessStatus {
    Stopped,
    Starting,
    Running,
    Failed,
    Restarting,
}

#[derive(Debug)]
pub struct ProcessInfo {
    pub status: ProcessStatus,
    pub start_time: Option<Instant>,
    pub restart_count: u32,
    pub last_health_check: Instant,
    pub consecutive_failures: u32,
}

impl ProcessInfo {
    fn new() -> Self {
        Self {
            status: ProcessStatus::Stopped,
            start_time: None,
            restart_count: 0,
            last_health_check: Instant::now(),
            consecutive_failures: 0,
        }
    }
}

pub struct HelperManager {
    process: Option<Child>,
    listener: Option<UnixListener>,
    login_time: Option<std::time::Instant>,
    delay_start_time: Option<Instant>,
    process_info: ProcessInfo,
    auto_restart_enabled: bool,
    socket_path: String,
}

pub struct MediaPlayerHelperManager {
    process: Option<Child>,
    listener: Option<UnixListener>,
    process_info: ProcessInfo,
    auto_restart_enabled: bool,
    socket_path: String,
    window_class: Option<String>,
    window_id: Option<u64>,
    pid: Option<u32>,
}

pub struct BrowserHelperManager {
    process: Option<Child>,
    listener: Option<UnixListener>,
    process_info: ProcessInfo,
    auto_restart_enabled: bool,
    socket_path: String,
    window_class: Option<String>,
    window_id: Option<u64>,
    pid: Option<u32>,
}

impl HelperManager {
    pub fn new() -> Self {
        HelperManager {
            process: None,
            listener: None,
            login_time: None,
            delay_start_time: None,
            process_info: ProcessInfo::new(),
            auto_restart_enabled: true,
            socket_path: "/tmp/touchbar.sock".to_string(),
        }
    }

    pub fn start(&mut self, user: &str, leader_pid: u32) -> Option<i32> {
        if self.process.is_some() {
            return None;
        }

        self.process_info.status = ProcessStatus::Starting;
        self.process_info.start_time = Some(Instant::now());

        let socket_path = &self.socket_path;
        
        // Clean up old socket file if it exists
        let _ = fs::remove_file(socket_path);

        let listener = match UnixListener::bind(socket_path) {
            Ok(listener) => listener,
            Err(e) => {
                println!("[HelperManager::start] ERROR: Failed to bind socket: {}", e);
                self.process_info.status = ProcessStatus::Failed;
                self.process_info.consecutive_failures += 1;
                return None;
            }
        };

        if let Err(e) = listener.set_nonblocking(true) {
            println!("[HelperManager::start] ERROR: Failed to set socket non-blocking: {}", e);
            self.process_info.status = ProcessStatus::Failed;
            self.process_info.consecutive_failures += 1;
            return None;
        }

        // Get user info for socket ownership and process spawning
        let userinfo = match User::from_name(user) {
            Ok(Some(userinfo)) => userinfo,
            _ => {
                println!("[HelperManager::start] ERROR: Could not find user info for: {}", user);
                self.process_info.status = ProcessStatus::Failed;
                self.process_info.consecutive_failures += 1;
                return None;
            }
        };

        // Change ownership of the socket to the logged-in user
        if let Err(e) = chown(std::path::Path::new(socket_path), Some(userinfo.uid), Some(userinfo.gid)) {
            println!("[HelperManager::start] WARNING: Failed to change socket ownership: {}", e);
            // Continue anyway, this is not critical
        }

        let fd = listener.as_raw_fd();
        self.listener = Some(listener);

        // Get environment variables using the bash script approach
        let env_vars = get_env_from_session(user, leader_pid);

        let helper_path = "/usr/bin/tiny-dfr-focus-window-helper";

        // Check if helper binary exists
        if !std::path::Path::new(helper_path).exists() {
            println!("[HelperManager::start] ERROR: Helper binary not found at: {}", helper_path);
            self.process_info.status = ProcessStatus::Failed;
            self.process_info.consecutive_failures += 1;
            return None;
        }

        // Instead of using sudo, we'll run as root but set the effective user ID
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
            Ok(child) => {
                println!("[HelperManager::start] Helper process started successfully with PID: {}", child.id());
                child
            },
            Err(e) => {
                println!("[HelperManager::start] ERROR: Failed to spawn helper process: {}", e);
                self.process_info.status = ProcessStatus::Failed;
                self.process_info.consecutive_failures += 1;
                return None;
            }
        };
        
        self.process = Some(child);
        self.process_info.status = ProcessStatus::Running;
        self.process_info.consecutive_failures = 0; // Reset failure count on success
        Some(fd)
    }

    pub fn stop(&mut self) {
        if let Some(mut child) = self.process.take() {
            println!("[HelperManager::stop] Stopping helper process with PID: {}", child.id());
            
            // Kill the entire process group to handle D-Bus connections and child processes
            let result = unsafe { libc::killpg(child.id() as i32, libc::SIGTERM) };
            if result != 0 {
                let errno = std::io::Error::last_os_error();
                // Only log error if it's not "No such process" (process already dead)
                if errno.raw_os_error() != Some(3) { // ESRCH = 3
                    println!("[HelperManager::stop] WARNING: Failed to send SIGTERM: {}", errno);
                }
            }
            
            // Wait a bit for graceful shutdown
            let _ = std::thread::spawn(move || {
                std::thread::sleep(Duration::from_millis(500));
                let _ = child.wait();
            });
        }
        
        self.listener.take();
        self.process_info.status = ProcessStatus::Stopped;
        
        // Reset session state when stopping
        self.login_time = None;
        self.delay_start_time = None;
    }
    
    /// Check if the helper process is still running and clean up zombies
    pub fn check_process_status(&mut self) -> bool {
        let now = Instant::now();
        
        // Only check health periodically to avoid excessive system calls
        if now.duration_since(self.process_info.last_health_check) < Duration::from_secs(PROCESS_HEALTH_CHECK_INTERVAL) {
            return self.process.is_some();
        }
        
        self.process_info.last_health_check = now;
        
        if let Some(ref mut child) = self.process {
            // Check if process has exited
            match child.try_wait() {
                Ok(Some(exit_status)) => {
                    // Process has exited
                    println!("[HelperManager] Helper process has exited with status: {:?}", exit_status);
                    self.process = None;
                    self.process_info.status = ProcessStatus::Failed;
                    self.process_info.consecutive_failures += 1;
                    
                    // Attempt auto-restart if enabled and within limits
                    if self.auto_restart_enabled && self.process_info.restart_count < MAX_RESTART_ATTEMPTS {
                        self.schedule_restart();
                    }
                    
                    false
                }
                Ok(None) => {
                    // Process is still running
                    self.process_info.status = ProcessStatus::Running;
                    true
                }
                Err(e) => {
                    // Error checking process status
                    println!("[HelperManager] Error checking process status: {}", e);
                    self.process = None;
                    self.process_info.status = ProcessStatus::Failed;
                    self.process_info.consecutive_failures += 1;
                    
                    // Attempt auto-restart if enabled and within limits
                    if self.auto_restart_enabled && self.process_info.restart_count < MAX_RESTART_ATTEMPTS {
                        self.schedule_restart();
                    }
                    
                    false
                }
            }
        } else {
            self.process_info.status = ProcessStatus::Stopped;
            false
        }
    }

    /// Schedule a restart attempt
    fn schedule_restart(&mut self) {
        if self.process_info.restart_count >= MAX_RESTART_ATTEMPTS {
            println!("[HelperManager] Max restart attempts reached ({}), giving up", MAX_RESTART_ATTEMPTS);
            self.process_info.status = ProcessStatus::Failed;
            return;
        }
        
        self.process_info.status = ProcessStatus::Restarting;
        self.process_info.restart_count += 1;
        
        println!("[HelperManager] Scheduling restart attempt {}/{} in {} seconds", 
                self.process_info.restart_count, MAX_RESTART_ATTEMPTS, RESTART_DELAY_SECONDS);
        
        // In a real implementation, you might use a timer or async task
        // For now, we'll just mark it as restarting and let the main loop handle it
    }

    /// Attempt to restart the helper process
    pub fn attempt_restart(&mut self, user: &str, leader_pid: u32) -> bool {
        if self.process_info.status != ProcessStatus::Restarting {
            return false;
        }
        
        println!("[HelperManager] Attempting restart {}/{}", 
                self.process_info.restart_count, MAX_RESTART_ATTEMPTS);
        
        // Clean up any existing process
        self.stop();
        
        // Wait a bit before restarting
        std::thread::sleep(Duration::from_secs(RESTART_DELAY_SECONDS));
        
        // Try to start again (main helper doesn't need window info)
        if let Some(_fd) = self.start(user, leader_pid) {
            println!("[HelperManager] Restart successful");
            true
        } else {
            println!("[HelperManager] Restart failed");
            self.process_info.status = ProcessStatus::Failed;
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
                    if let Err(e) = stream.set_nonblocking(true) {
                        println!("[HelperManager] WARNING: Failed to set stream non-blocking: {}", e);
                    }
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
        self.process.is_some() && self.process_info.status == ProcessStatus::Running
    }

    /// Get process status information
    pub fn get_process_status(&self) -> &ProcessStatus {
        &self.process_info.status
    }

    /// Get restart count
    pub fn get_restart_count(&self) -> u32 {
        self.process_info.restart_count
    }

    /// Enable/disable auto-restart
    pub fn set_auto_restart(&mut self, enabled: bool) {
        self.auto_restart_enabled = enabled;
        println!("[HelperManager] Auto-restart {}", if enabled { "enabled" } else { "disabled" });
    }

    /// Force cleanup of any zombie processes
    pub fn force_cleanup(&mut self) {
        if let Some(mut child) = self.process.take() {
            println!("[HelperManager] Force cleanup of process with PID: {}", child.id());
            
            // Force kill the process
            let result = unsafe { libc::killpg(child.id() as i32, libc::SIGKILL) };
            if result != 0 {
                let errno = std::io::Error::last_os_error();
                // Only log as warning if it's not "No such process" (process already dead)
                if errno.raw_os_error() != Some(3) { // ESRCH = 3
                    println!("[HelperManager] WARNING: Failed to send SIGKILL: {}", errno);
                } else {
                    println!("[HelperManager] Process {} already terminated", child.id());
                }
            }
            
            // Wait for it to die (non-blocking)
            match child.try_wait() {
                Ok(Some(status)) => {
                    println!("[HelperManager] Process {} exited with status: {:?}", child.id(), status);
                }
                Ok(None) => {
                    // Process still running, wait a bit more
                    let _ = std::thread::spawn(move || {
                        std::thread::sleep(Duration::from_millis(100));
                        let _ = child.wait();
                    });
                }
                Err(e) => {
                    println!("[HelperManager] Error waiting for process {}: {}", child.id(), e);
                }
            }
        }
        
        self.listener.take();
        self.process_info.status = ProcessStatus::Stopped;
        self.process_info.restart_count = 0;
        self.process_info.consecutive_failures = 0;
    }
}

impl MediaPlayerHelperManager {
    pub fn new() -> Self {
        MediaPlayerHelperManager {
            process: None,
            listener: None,
            process_info: ProcessInfo::new(),
            auto_restart_enabled: true,
            socket_path: "/tmp/touchbar-media.sock".to_string(),
            window_class: None,
            window_id: None,
            pid: None,
        }
    }

    pub fn start(&mut self, user: &str, leader_pid: u32, window_class: &str, window_id: u64, pid: u32) -> Option<i32> {
        if self.process.is_some() {
            return None;
        }

        self.process_info.status = ProcessStatus::Starting;
        self.process_info.start_time = Some(Instant::now());

        let socket_path = &self.socket_path;
        // Clean up old socket file if it exists
        let _ = fs::remove_file(socket_path);

        let listener = match UnixListener::bind(socket_path) {
            Ok(listener) => listener,
            Err(e) => {
                println!("[MediaPlayerHelperManager::start] ERROR: Failed to bind Media Player socket: {}", e);
                self.process_info.status = ProcessStatus::Failed;
                self.process_info.consecutive_failures += 1;
                return None;
            }
        };

        if let Err(e) = listener.set_nonblocking(true) {
            println!("[MediaPlayerHelperManager::start] ERROR: Failed to set Media Player socket non-blocking: {}", e);
            self.process_info.status = ProcessStatus::Failed;
            self.process_info.consecutive_failures += 1;
            return None;
        }

        // Get user info for socket ownership and process spawning
        let userinfo = match User::from_name(user) {
            Ok(Some(userinfo)) => userinfo,
            _ => {
                println!("[MediaPlayerHelperManager::start] ERROR: Could not find user info for: {}", user);
                self.process_info.status = ProcessStatus::Failed;
                self.process_info.consecutive_failures += 1;
                return None;
            }
        };

        // Change ownership of the socket to the logged-in user
        if let Err(e) = chown(std::path::Path::new(socket_path), Some(userinfo.uid), Some(userinfo.gid)) {
            println!("[MediaPlayerHelperManager::start] WARNING: Failed to change Media Player socket ownership: {}", e);
            // Continue anyway, this is not critical
        }

        let fd = listener.as_raw_fd();
        self.listener = Some(listener);

        // Get environment variables using the bash script approach
        let env_vars = get_env_from_session(user, leader_pid);
        
        // Debug: print all environment variables being passed
        println!("[main] Environment variables for Media Player helper:");
        for (key, value) in &env_vars {
            println!("[main]   {}={}", key, value);
        }

        let helper_path = "/usr/bin/tiny-dfr-media-helper";

        // Check if helper binary exists
        if !std::path::Path::new(helper_path).exists() {
            println!("[MediaPlayerHelperManager::start] ERROR: Media Player helper binary not found at: {}", helper_path);
            self.process_info.status = ProcessStatus::Failed;
            self.process_info.consecutive_failures += 1;
            return None;
        }

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
        
        // Store window information for restart purposes
        self.window_class = Some(window_class.to_string());
        self.window_id = Some(window_id);
        self.pid = Some(pid);
        
        // Add window class, window ID, and PID as environment variables
        cmd.env("TINY_DFR_WINDOW_CLASS", window_class);
        cmd.env("TINY_DFR_WINDOW_ID", &window_id.to_string());
        cmd.env("TINY_DFR_WINDOW_PID", &pid.to_string());
        
        println!("[main] Spawning Media Player helper: {} (as user {}) for window class: {} (ID: {}, PID: {})", helper_path, user, window_class, window_id, pid);
        let child = match cmd.spawn() {
            Ok(child) => child,
            Err(e) => {
                println!("[MediaPlayerHelperManager] Failed to start Media Player helper: {}", e);
                return None;
            }
        };
        self.process = Some(child);
        self.process_info.status = ProcessStatus::Running;
        self.process_info.consecutive_failures = 0; // Reset failure count on success
        Some(fd)
    }

    pub fn stop(&mut self) {
        if let Some(mut child) = self.process.take() {
            println!("[MediaPlayerHelperManager::stop] Stopping Media Player helper process with PID: {}", child.id());
            
            // Kill the entire process group to handle D-Bus connections and child processes
            let result = unsafe { libc::killpg(child.id() as i32, libc::SIGTERM) };
            if result != 0 {
                let errno = std::io::Error::last_os_error();
                // Only log error if it's not "No such process" (process already dead)
                if errno.raw_os_error() != Some(3) { // ESRCH = 3
                    println!("[MediaPlayerHelperManager::stop] WARNING: Failed to send SIGTERM: {}", errno);
                }
            }
            
            // Wait a bit for graceful shutdown
            let _ = std::thread::spawn(move || {
                std::thread::sleep(Duration::from_millis(500));
                let _ = child.wait();
            });
        }
        
        self.listener.take();
        self.process_info.status = ProcessStatus::Stopped;
    }
    
    /// Check if the Media Player helper process is still running and clean up zombies
    pub fn check_process_status(&mut self) -> bool {
        let now = Instant::now();
        
        // Only check health periodically to avoid excessive system calls
        if now.duration_since(self.process_info.last_health_check) < Duration::from_secs(PROCESS_HEALTH_CHECK_INTERVAL) {
            return self.process.is_some();
        }
        
        self.process_info.last_health_check = now;
        
        if let Some(ref mut child) = self.process {
            // Check if process has exited
            match child.try_wait() {
                Ok(Some(exit_status)) => {
                    // Process has exited
                    println!("[MediaPlayerHelperManager] Media Player helper process has exited with status: {:?}", exit_status);
                    self.process = None;
                    self.process_info.status = ProcessStatus::Failed;
                    self.process_info.consecutive_failures += 1;
                    
                    // Attempt auto-restart if enabled and within limits
                    if self.auto_restart_enabled && self.process_info.restart_count < MAX_RESTART_ATTEMPTS {
                        self.schedule_restart();
                    }
                    
                    false
                }
                Ok(None) => {
                    // Process is still running
                    self.process_info.status = ProcessStatus::Running;
                    true
                }
                Err(e) => {
                    // Error checking process status
                    println!("[MediaPlayerHelperManager] Error checking process status: {}", e);
                    self.process = None;
                    self.process_info.status = ProcessStatus::Failed;
                    self.process_info.consecutive_failures += 1;
                    
                    // Attempt auto-restart if enabled and within limits
                    if self.auto_restart_enabled && self.process_info.restart_count < MAX_RESTART_ATTEMPTS {
                        self.schedule_restart();
                    }
                    
                    false
                }
            }
        } else {
            self.process_info.status = ProcessStatus::Stopped;
            false
        }
    }

    /// Schedule a restart attempt
    fn schedule_restart(&mut self) {
        if self.process_info.restart_count >= MAX_RESTART_ATTEMPTS {
            println!("[MediaPlayerHelperManager] Max restart attempts reached ({}), giving up", MAX_RESTART_ATTEMPTS);
            self.process_info.status = ProcessStatus::Failed;
            return;
        }
        
        self.process_info.status = ProcessStatus::Restarting;
        self.process_info.restart_count += 1;
        
        println!("[MediaPlayerHelperManager] Scheduling restart attempt {}/{} in {} seconds", 
                self.process_info.restart_count, MAX_RESTART_ATTEMPTS, RESTART_DELAY_SECONDS);
        
        // In a real implementation, you might use a timer or async task
        // For now, we'll just mark it as restarting and let the main loop handle it
    }

    /// Attempt to restart the Media Player helper process
    pub fn attempt_restart(&mut self, user: &str, leader_pid: u32) -> bool {
        if self.process_info.status != ProcessStatus::Restarting {
            return false;
        }
        
        println!("[MediaPlayerHelperManager] Attempting restart {}/{}", 
                self.process_info.restart_count, MAX_RESTART_ATTEMPTS);
        
        // Clean up any existing process
        self.stop();
        
        // Wait a bit before restarting
        std::thread::sleep(Duration::from_secs(RESTART_DELAY_SECONDS));
        
        // Try to start again with stored window information
        if let (Some(window_class), Some(window_id), Some(pid)) = (&self.window_class, &self.window_id, &self.pid) {
            let window_class = window_class.clone();
            let window_id = *window_id;
            let pid = *pid;
            if let Some(_fd) = self.start(user, leader_pid, &window_class, window_id, pid) {
                println!("[MediaPlayerHelperManager] Restart successful");
                true
            } else {
                println!("[MediaPlayerHelperManager] Restart failed");
                self.process_info.status = ProcessStatus::Failed;
                false
            }
        } else {
            println!("[MediaPlayerHelperManager] Cannot restart: no window information available");
            self.process_info.status = ProcessStatus::Failed;
            false
        }
    }

    pub fn accept_connection(&mut self) -> Option<UnixStream> {
        if let Some(listener) = &self.listener {
            if let Ok((stream, _)) = listener.accept() {
                if let Err(e) = stream.set_nonblocking(true) {
            println!("[MediaPlayerHelperManager] Failed to set Media Player stream non-blocking: {}", e);
            return None;
        }
                return Some(stream);
            }
        }
        None
    }
    
    /// Check if the Media Player helper process is still running
    pub fn is_process_running(&self) -> bool {
        self.process.is_some() && self.process_info.status == ProcessStatus::Running
    }

    /// Get process status information
    pub fn get_process_status(&self) -> &ProcessStatus {
        &self.process_info.status
    }

    /// Get restart count
    pub fn get_restart_count(&self) -> u32 {
        self.process_info.restart_count
    }

    /// Enable/disable auto-restart
    pub fn set_auto_restart(&mut self, enabled: bool) {
        self.auto_restart_enabled = enabled;
        println!("[MediaPlayerHelperManager] Auto-restart {}", if enabled { "enabled" } else { "disabled" });
    }

    /// Force cleanup of any zombie processes
    pub fn force_cleanup(&mut self) {
        if let Some(mut child) = self.process.take() {
            println!("[MediaPlayerHelperManager] Force cleaning up Media Player helper process");
            
            // Force kill the process
            let result = unsafe { libc::killpg(child.id() as i32, libc::SIGKILL) };
            if result != 0 {
                let errno = std::io::Error::last_os_error();
                // Only log as warning if it's not "No such process" (process already dead)
                if errno.raw_os_error() != Some(3) { // ESRCH = 3
                    println!("[MediaPlayerHelperManager] WARNING: Failed to send SIGKILL: {}", errno);
                } else {
                    println!("[MediaPlayerHelperManager] Media Player helper process {} already terminated", child.id());
                }
            }
            
            // Wait for it to die (non-blocking)
            match child.try_wait() {
                Ok(Some(status)) => {
                    println!("[MediaPlayerHelperManager] Media Player helper process {} exited with status: {:?}", child.id(), status);
                }
                Ok(None) => {
                    // Process still running, wait a bit more
                    let _ = std::thread::spawn(move || {
                        std::thread::sleep(Duration::from_millis(100));
                        let _ = child.wait();
                    });
                }
                Err(e) => {
                    println!("[MediaPlayerHelperManager] Error waiting for Media Player helper process {}: {}", child.id(), e);
                }
            }
        }
        
        self.listener.take();
        self.process_info.status = ProcessStatus::Stopped;
        self.process_info.restart_count = 0;
        self.process_info.consecutive_failures = 0;
    }
}

impl BrowserHelperManager {
    pub fn new() -> Self {
        BrowserHelperManager {
            process: None,
            listener: None,
            process_info: ProcessInfo::new(),
            auto_restart_enabled: true,
            socket_path: "/tmp/touchbar-browser.sock".to_string(),
            window_class: None,
            window_id: None,
            pid: None,
        }
    }

    pub fn start(&mut self, user: &str, leader_pid: u32, window_class: &str, window_id: u64, pid: u32) -> Option<i32> {
        if self.process.is_some() {
            return None;
        }

        self.process_info.status = ProcessStatus::Starting;
        self.process_info.start_time = Some(Instant::now());

        let socket_path = &self.socket_path;
        // Clean up old socket file if it exists
        let _ = fs::remove_file(socket_path);

        let listener = match UnixListener::bind(socket_path) {
            Ok(listener) => listener,
            Err(e) => {
                println!("[BrowserHelperManager::start] ERROR: Failed to bind browser socket: {}", e);
                self.process_info.status = ProcessStatus::Failed;
                self.process_info.consecutive_failures += 1;
                return None;
            }
        };

        if let Err(e) = listener.set_nonblocking(true) {
            println!("[BrowserHelperManager::start] ERROR: Failed to set browser socket non-blocking: {}", e);
            self.process_info.status = ProcessStatus::Failed;
            self.process_info.consecutive_failures += 1;
            return None;
        }

        // Get user info for socket ownership and process spawning
        let userinfo = match User::from_name(user) {
            Ok(Some(userinfo)) => userinfo,
            _ => {
                println!("[BrowserHelperManager::start] ERROR: Could not find user info for: {}", user);
                self.process_info.status = ProcessStatus::Failed;
                self.process_info.consecutive_failures += 1;
                return None;
            }
        };

        // Change ownership of the socket to the logged-in user
        if let Err(e) = chown(std::path::Path::new(socket_path), Some(userinfo.uid), Some(userinfo.gid)) {
            println!("[BrowserHelperManager::start] WARNING: Failed to change browser socket ownership: {}", e);
            // Continue anyway, this is not critical
        }

        let fd = listener.as_raw_fd();
        self.listener = Some(listener);

        // Get environment variables using the bash script approach
        let env_vars = get_env_from_session(user, leader_pid);
        
      

        let helper_path = "/usr/bin/tiny-dfr-browser-helper";

        // Check if helper binary exists
        if !std::path::Path::new(helper_path).exists() {
            self.process_info.status = ProcessStatus::Failed;
            self.process_info.consecutive_failures += 1;
            return None;
        }

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
        
        // Store window information for restart purposes
        self.window_class = Some(window_class.to_string());
        self.window_id = Some(window_id);
        self.pid = Some(pid);
        
        // Add window class, window ID, and PID as environment variables
        cmd.env("TINY_DFR_WINDOW_CLASS", window_class);
        cmd.env("TINY_DFR_WINDOW_ID", &window_id.to_string());
        cmd.env("TINY_DFR_WINDOW_PID", &pid.to_string());
        
        let child = match cmd.spawn() {
            Ok(child) => child,
            Err(e) => {
                println!("[BrowserHelperManager] Failed to start browser helper: {}", e);
                return None;
            }
        };
        self.process = Some(child);
        self.process_info.status = ProcessStatus::Running;
        self.process_info.consecutive_failures = 0; // Reset failure count on success
        Some(fd)
    }

    pub fn stop(&mut self) {
        if let Some(mut child) = self.process.take() {
            
            // Kill the entire process group to handle D-Bus connections and child processes
            let result = unsafe { libc::killpg(child.id() as i32, libc::SIGTERM) };
            if result != 0 {
                let errno = std::io::Error::last_os_error();
                // Only log error if it's not "No such process" (process already dead)
                if errno.raw_os_error() != Some(3) { // ESRCH = 3
                    println!("[BrowserHelperManager::stop] WARNING: Failed to send SIGTERM: {}", errno);
                }
            }
            
            // Wait a bit for graceful shutdown
            let _ = std::thread::spawn(move || {
                std::thread::sleep(Duration::from_millis(500));
                let _ = child.wait();
            });
        }
        
        self.listener.take();
        self.process_info.status = ProcessStatus::Stopped;
    }
    
    /// Check if the browser helper process is still running and clean up zombies
    pub fn check_process_status(&mut self) -> bool {
        let now = Instant::now();
        
        // Only check health periodically to avoid excessive system calls
        if now.duration_since(self.process_info.last_health_check) < Duration::from_secs(PROCESS_HEALTH_CHECK_INTERVAL) {
            return self.process.is_some();
        }
        
        self.process_info.last_health_check = now;
        
        if let Some(ref mut child) = self.process {
            // Check if process has exited
            match child.try_wait() {
                Ok(Some(exit_status)) => {
                    // Process has exited
                    self.process = None;
                    self.process_info.status = ProcessStatus::Failed;
                    self.process_info.consecutive_failures += 1;
                    
                    // Attempt auto-restart if enabled and within limits
                    if self.auto_restart_enabled && self.process_info.restart_count < MAX_RESTART_ATTEMPTS {
                        self.schedule_restart();
                    }
                    
                    false
                }
                Ok(None) => {
                    // Process is still running
                    self.process_info.status = ProcessStatus::Running;
                    true
                }
                Err(e) => {
                    // Error checking process status
                    println!("[BrowserHelperManager] Error checking process status: {}", e);
                    self.process = None;
                    self.process_info.status = ProcessStatus::Failed;
                    self.process_info.consecutive_failures += 1;
                    
                    // Attempt auto-restart if enabled and within limits
                    if self.auto_restart_enabled && self.process_info.restart_count < MAX_RESTART_ATTEMPTS {
                        self.schedule_restart();
                    }
                    
                    false
                }
            }
        } else {
            self.process_info.status = ProcessStatus::Stopped;
            false
        }
    }

    /// Schedule a restart attempt
    fn schedule_restart(&mut self) {
        if self.process_info.restart_count >= MAX_RESTART_ATTEMPTS {
            self.process_info.status = ProcessStatus::Failed;
            return;
        }
        
        self.process_info.status = ProcessStatus::Restarting;
        self.process_info.restart_count += 1;
    
        // In a real implementation, you might use a timer or async task
        // For now, we'll just mark it as restarting and let the main loop handle it
    }

    /// Attempt to restart the browser helper process
    pub fn attempt_restart(&mut self, user: &str, leader_pid: u32) -> bool {
        if self.process_info.status != ProcessStatus::Restarting {
            return false;
        }
     
        
        // Clean up any existing process
        self.stop();
        
        // Wait a bit before restarting
        std::thread::sleep(Duration::from_secs(RESTART_DELAY_SECONDS));
        
        // Try to start again with stored window information
        if let (Some(window_class), Some(window_id), Some(pid)) = (&self.window_class, &self.window_id, &self.pid) {
            let window_class = window_class.clone();
            let window_id = *window_id;
            let pid = *pid;
            if let Some(_fd) = self.start(user, leader_pid, &window_class, window_id, pid) {
                println!("[BrowserHelperManager] Restart successful");
                true
            } else {
                println!("[BrowserHelperManager] Restart failed");
                self.process_info.status = ProcessStatus::Failed;
                false
            }
        } else {
            println!("[BrowserHelperManager] Cannot restart: no window information available");
            self.process_info.status = ProcessStatus::Failed;
                false
        }
    }

    pub fn accept_connection(&mut self) -> Option<UnixStream> {
        if let Some(listener) = &self.listener {
            if let Ok((stream, _)) = listener.accept() {
                if let Err(e) = stream.set_nonblocking(true) {
            println!("[BrowserHelperManager] Failed to set browser stream non-blocking: {}", e);
            return None;
        }
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
            let result = unsafe { libc::killpg(child.id() as i32, libc::SIGKILL) };
            if result != 0 {
                let errno = std::io::Error::last_os_error();
                // Only log as warning if it's not "No such process" (process already dead)
                if errno.raw_os_error() != Some(3) { // ESRCH = 3
                    println!("[BrowserHelperManager] WARNING: Failed to send SIGKILL: {}", errno);
                } else {
                    println!("[BrowserHelperManager] Browser helper process {} already terminated", child.id());
                }
            }
            
            // Wait for it to exit (non-blocking)
            match child.try_wait() {
                Ok(Some(status)) => {
                    println!("[BrowserHelperManager] Browser helper process {} exited with status: {:?}", child.id(), status);
                }
                Ok(None) => {
                    // Process still running, wait a bit more
                    let _ = std::thread::spawn(move || {
                        std::thread::sleep(Duration::from_millis(100));
                        let _ = child.wait();
                    });
                }
                Err(e) => {
                    println!("[BrowserHelperManager] Error waiting for browser helper process {}: {}", child.id(), e);
                }
            }
        }
        
        self.listener.take();
        self.process_info.status = ProcessStatus::Stopped;
        self.process_info.restart_count = 0;
        self.process_info.consecutive_failures = 0;
    }
    
    /// Check if the browser helper process is still running
    pub fn is_process_running(&self) -> bool {
        self.process.is_some() && self.process_info.status == ProcessStatus::Running
    }
} 

// Add the missing functions that were referenced

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

fn get_env_from_session(user: &str, leader_pid: u32) -> HashMap<String, String> {
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
            println!("[get_env_from_session] Process {} (PID {}): found {} env vars", i, pid, env_count);
        } else {
            println!("[get_env_from_session] Process {} (PID {}): failed to read environ", i, pid);
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
                                println!("[get_env_from_session] Found i3 socket: {}, setting I3SOCK={}", name, i3_socket_path);
                                env.insert("I3SOCK".to_string(), i3_socket_path);
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