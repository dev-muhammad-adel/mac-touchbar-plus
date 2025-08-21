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

pub struct VlcHelperManager {
    process: Option<Child>,
    listener: Option<UnixListener>,
    process_info: ProcessInfo,
    auto_restart_enabled: bool,
    socket_path: String,
}

pub struct BrowserHelperManager {
    process: Option<Child>,
    listener: Option<UnixListener>,
    process_info: ProcessInfo,
    auto_restart_enabled: bool,
    socket_path: String,
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
        
        // Try to start again
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

impl VlcHelperManager {
    pub fn new() -> Self {
        VlcHelperManager {
            process: None,
            listener: None,
            process_info: ProcessInfo::new(),
            auto_restart_enabled: true,
            socket_path: "/tmp/touchbar-vlc.sock".to_string(),
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
                println!("[VlcHelperManager::start] ERROR: Failed to bind VLC socket: {}", e);
                self.process_info.status = ProcessStatus::Failed;
                self.process_info.consecutive_failures += 1;
                return None;
            }
        };

        if let Err(e) = listener.set_nonblocking(true) {
            println!("[VlcHelperManager::start] ERROR: Failed to set VLC socket non-blocking: {}", e);
            self.process_info.status = ProcessStatus::Failed;
            self.process_info.consecutive_failures += 1;
            return None;
        }

        // Get user info for socket ownership and process spawning
        let userinfo = match User::from_name(user) {
            Ok(Some(userinfo)) => userinfo,
            _ => {
                println!("[VlcHelperManager::start] ERROR: Could not find user info for: {}", user);
                self.process_info.status = ProcessStatus::Failed;
                self.process_info.consecutive_failures += 1;
                return None;
            }
        };

        // Change ownership of the socket to the logged-in user
        if let Err(e) = chown(std::path::Path::new(socket_path), Some(userinfo.uid), Some(userinfo.gid)) {
            println!("[VlcHelperManager::start] WARNING: Failed to change VLC socket ownership: {}", e);
            // Continue anyway, this is not critical
        }

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

        // Check if helper binary exists
        if !std::path::Path::new(helper_path).exists() {
            println!("[VlcHelperManager::start] ERROR: VLC helper binary not found at: {}", helper_path);
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
        
        println!("[main] Spawning VLC helper: {} (as user {})", helper_path, user);
        let child = cmd.spawn().expect("Failed to start VLC helper");
        self.process = Some(child);
        self.process_info.status = ProcessStatus::Running;
        self.process_info.consecutive_failures = 0; // Reset failure count on success
        Some(fd)
    }

    pub fn stop(&mut self) {
        if let Some(mut child) = self.process.take() {
            println!("[VlcHelperManager::stop] Stopping VLC helper process with PID: {}", child.id());
            
            // Kill the entire process group to handle D-Bus connections and child processes
            let result = unsafe { libc::killpg(child.id() as i32, libc::SIGTERM) };
            if result != 0 {
                let errno = std::io::Error::last_os_error();
                // Only log error if it's not "No such process" (process already dead)
                if errno.raw_os_error() != Some(3) { // ESRCH = 3
                    println!("[VlcHelperManager::stop] WARNING: Failed to send SIGTERM: {}", errno);
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
    
    /// Check if the VLC helper process is still running and clean up zombies
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
                    println!("[VlcHelperManager] VLC helper process has exited with status: {:?}", exit_status);
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
                    println!("[VlcHelperManager] Error checking process status: {}", e);
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
            println!("[VlcHelperManager] Max restart attempts reached ({}), giving up", MAX_RESTART_ATTEMPTS);
            self.process_info.status = ProcessStatus::Failed;
            return;
        }
        
        self.process_info.status = ProcessStatus::Restarting;
        self.process_info.restart_count += 1;
        
        println!("[VlcHelperManager] Scheduling restart attempt {}/{} in {} seconds", 
                self.process_info.restart_count, MAX_RESTART_ATTEMPTS, RESTART_DELAY_SECONDS);
        
        // In a real implementation, you might use a timer or async task
        // For now, we'll just mark it as restarting and let the main loop handle it
    }

    /// Attempt to restart the VLC helper process
    pub fn attempt_restart(&mut self, user: &str, leader_pid: u32) -> bool {
        if self.process_info.status != ProcessStatus::Restarting {
            return false;
        }
        
        println!("[VlcHelperManager] Attempting restart {}/{}", 
                self.process_info.restart_count, MAX_RESTART_ATTEMPTS);
        
        // Clean up any existing process
        self.stop();
        
        // Wait a bit before restarting
        std::thread::sleep(Duration::from_secs(RESTART_DELAY_SECONDS));
        
        // Try to start again
        if let Some(_fd) = self.start(user, leader_pid) {
            println!("[VlcHelperManager] Restart successful");
            true
        } else {
            println!("[VlcHelperManager] Restart failed");
            self.process_info.status = ProcessStatus::Failed;
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
        println!("[VlcHelperManager] Auto-restart {}", if enabled { "enabled" } else { "disabled" });
    }

    /// Force cleanup of any zombie processes
    pub fn force_cleanup(&mut self) {
        if let Some(mut child) = self.process.take() {
            println!("[VlcHelperManager] Force cleaning up VLC helper process");
            
            // Force kill the process
            let result = unsafe { libc::killpg(child.id() as i32, libc::SIGKILL) };
            if result != 0 {
                let errno = std::io::Error::last_os_error();
                // Only log as warning if it's not "No such process" (process already dead)
                if errno.raw_os_error() != Some(3) { // ESRCH = 3
                    println!("[VlcHelperManager] WARNING: Failed to send SIGKILL: {}", errno);
                } else {
                    println!("[VlcHelperManager] VLC helper process {} already terminated", child.id());
                }
            }
            
            // Wait for it to die (non-blocking)
            match child.try_wait() {
                Ok(Some(status)) => {
                    println!("[VlcHelperManager] VLC helper process {} exited with status: {:?}", child.id(), status);
                }
                Ok(None) => {
                    // Process still running, wait a bit more
                    let _ = std::thread::spawn(move || {
                        std::thread::sleep(Duration::from_millis(100));
                        let _ = child.wait();
                    });
                }
                Err(e) => {
                    println!("[VlcHelperManager] Error waiting for VLC helper process {}: {}", child.id(), e);
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
        
        // Debug: print all environment variables being passed
        println!("[main] Environment variables for browser helper:");
        for (key, value) in &env_vars {
            println!("[main]   {}={}", key, value);
        }

        let helper_path = "/usr/bin/tiny-dfr-browser-helper";

        // Check if helper binary exists
        if !std::path::Path::new(helper_path).exists() {
            println!("[BrowserHelperManager::start] ERROR: Browser helper binary not found at: {}", helper_path);
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
        
        println!("[main] Spawning browser helper: {} (as user {})", helper_path, user);
        let child = cmd.spawn().expect("Failed to start browser helper");
        self.process = Some(child);
        self.process_info.status = ProcessStatus::Running;
        self.process_info.consecutive_failures = 0; // Reset failure count on success
        Some(fd)
    }

    pub fn stop(&mut self) {
        if let Some(mut child) = self.process.take() {
            println!("[BrowserHelperManager::stop] Stopping browser helper process with PID: {}", child.id());
            
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
                    println!("[BrowserHelperManager] Browser helper process has exited with status: {:?}", exit_status);
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
            println!("[BrowserHelperManager] Max restart attempts reached ({}), giving up", MAX_RESTART_ATTEMPTS);
            self.process_info.status = ProcessStatus::Failed;
            return;
        }
        
        self.process_info.status = ProcessStatus::Restarting;
        self.process_info.restart_count += 1;
        
        println!("[BrowserHelperManager] Scheduling restart attempt {}/{} in {} seconds", 
                self.process_info.restart_count, MAX_RESTART_ATTEMPTS, RESTART_DELAY_SECONDS);
        
        // In a real implementation, you might use a timer or async task
        // For now, we'll just mark it as restarting and let the main loop handle it
    }

    /// Attempt to restart the browser helper process
    pub fn attempt_restart(&mut self, user: &str, leader_pid: u32) -> bool {
        if self.process_info.status != ProcessStatus::Restarting {
            return false;
        }
        
        println!("[BrowserHelperManager] Attempting restart {}/{}", 
                self.process_info.restart_count, MAX_RESTART_ATTEMPTS);
        
        // Clean up any existing process
        self.stop();
        
        // Wait a bit before restarting
        std::thread::sleep(Duration::from_secs(RESTART_DELAY_SECONDS));
        
        // Try to start again
        if let Some(_fd) = self.start(user, leader_pid) {
            println!("[BrowserHelperManager] Restart successful");
            true
        } else {
            println!("[BrowserHelperManager] Restart failed");
            self.process_info.status = ProcessStatus::Failed;
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