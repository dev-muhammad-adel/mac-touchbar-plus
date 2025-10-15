 //! Background service helper for generic media control
//! This helper runs when user logs in and monitors background services like Spotify
//! It doesn't require window focus or specific window classes

use std::os::unix::net::UnixStream;
use std::io::{Read, Write};
use std::thread;
use std::time::Duration;
use std::sync::{Arc, Mutex};
use std::collections::HashSet;
use zbus::Connection;
use serde_json::json;
use chrono;

mod spotify {
    include!("background_services/spotify.rs");
}

mod chromium {
    include!("background_services/chromium.rs");
}

pub mod mpris_manager {
    include!("background_services/mpris_manager.rs");
}

// Import specific functions we need
use mpris_manager::MprisManager;

// Dynamic list of available MPRIS background services
static mut AVAILABLE_MPRIS_BACKGROUND: Vec<String> = Vec::new();

// Dynamic selected MPRIS name for background service
static mut SELECTED_BACKGROUND_SERVICE_MPRIS_NAME: Option<String> = None;

// Track if we've done initial auto-selection
static mut HAS_DONE_INITIAL_AUTO_SELECTION: bool = false;

// Track if MPRIS monitoring is currently enabled
static mut MPRIS_MONITORING_ENABLED: bool = false;

// Public function to check if MPRIS monitoring is enabled
pub fn is_mpris_monitoring_enabled() -> bool {
    unsafe { MPRIS_MONITORING_ENABLED }
}


// MPRIS Manager instance
static mut MPRIS_MANAGER: Option<MprisManager> = None;

// Function to query D-Bus for MPRIS services (filtered for Spotify and Chromium only)
async fn query_mpris_services() -> Result<Vec<String>, String> {
    let connection = Connection::session().await.map_err(|e| e.to_string())?;
    let proxy = zbus::Proxy::new(
        &connection,
        "org.freedesktop.DBus",
        "/org/freedesktop/DBus",
        "org.freedesktop.DBus",
    ).await.map_err(|e| e.to_string())?;
    
    let names: Vec<String> = proxy.call_method("ListNames", &()).await
        .map_err(|e| e.to_string())?
        .body::<Vec<String>>()
        .map_err(|e| e.to_string())?;
    
    let mpris_services: Vec<String> = names
        .into_iter()
        .filter(|name| {
            // Only include Spotify and Chromium MPRIS services
            name.starts_with("org.mpris.MediaPlayer2.spotify") ||
            name.starts_with("org.mpris.MediaPlayer2.chromium")
        })
        .collect();
    
    Ok(mpris_services)
}

// Function to test D-Bus connection health
async fn test_dbus_connection_health() -> bool {
    match Connection::session().await {
        Ok(connection) => {
            match connection.call_method(
                Some("org.freedesktop.DBus"),
                "/org/freedesktop/DBus",
                Some("org.freedesktop.DBus"),
                "GetId",
                &(),
            ).await {
                Ok(_) => true,
                Err(e) => {
                    eprintln!("[background-service-helper] D-Bus connection health check failed: {}", e);
                    false
                }
            }
        }
        Err(e) => {
            eprintln!("[background-service-helper] Failed to create D-Bus connection for health check: {}", e);
            false
        }
    }
}

// Function to perform periodic connection health monitoring
async fn monitor_connection_health(status_sender: Arc<Mutex<Option<UnixStream>>>) {
    let mut health_check_counter = 0;
    const HEALTH_CHECK_INTERVAL: u32 = 30; // Check every 30 iterations (60 seconds)
    
    loop {
        health_check_counter += 1;
        
        if health_check_counter >= HEALTH_CHECK_INTERVAL {
            health_check_counter = 0;
            
            if !test_dbus_connection_health().await {
                eprintln!("[background-service-helper] D-Bus connection health check failed, triggering service refresh...");
                
                // Try to refresh the available services
                match query_mpris_services().await {
                    Ok(services) => {
                        eprintln!("[background-service-helper] Successfully refreshed MPRIS services after health check failure");
                        update_available_mpris_services(services);
                        if let Err(e) = send_available_background_services(&status_sender) {
                            eprintln!("[background-service-helper] Failed to send refreshed services: {}", e);
                        }
                    }
                    Err(e) => {
                        eprintln!("[background-service-helper] Failed to refresh MPRIS services after health check failure: {}", e);
                    }
                }
            }
        }
        
        // Sleep for 2 seconds between checks
        tokio::time::sleep(Duration::from_secs(2)).await;
    }
}

// Function to update the available MPRIS services list
fn update_available_mpris_services(services: Vec<String>) {
    unsafe {
        AVAILABLE_MPRIS_BACKGROUND = services;
        eprintln!("[background-service-helper] Updated MPRIS services: {:?}", AVAILABLE_MPRIS_BACKGROUND);
        
        // Note: Service availability checking and auto-switching is handled in send_available_background_services
        // to ensure proper coordination between service selection and MPRIS manager updates
    }
}

// Function to get current available MPRIS services
fn get_available_mpris_services() -> Vec<String> {
    unsafe {
        AVAILABLE_MPRIS_BACKGROUND.clone()
    }
}

// Function to get the currently selected MPRIS service
fn get_selected_mpris_service() -> Option<String> {
    unsafe {
        SELECTED_BACKGROUND_SERVICE_MPRIS_NAME.clone()
    }
}

// Function to set the selected MPRIS service
fn set_selected_mpris_service(service: Option<String>) {
    unsafe {
        let old_service = SELECTED_BACKGROUND_SERVICE_MPRIS_NAME.clone();
        SELECTED_BACKGROUND_SERVICE_MPRIS_NAME = service.clone();
        
        println!("[background_service_helper] ===== SELECTED_BACKGROUND_SERVICE_MPRIS_NAME UPDATE =====");
        println!("[background_service_helper] Old value: {:?}", old_service);
        println!("[background_service_helper] New value: {:?}", service);
        println!("[background_service_helper] =========================================================");
        
        if let Some(ref service_name) = SELECTED_BACKGROUND_SERVICE_MPRIS_NAME {
            eprintln!("[background-service-helper] Selected MPRIS service: {}", service_name);
        } else {
            eprintln!("[background-service-helper] No MPRIS service selected");
        }
    }
}


// Function to handle selection commands from the main app
fn handle_selection_command(command: &str, status_sender: &Arc<Mutex<Option<UnixStream>>>) {
    println!("[background_service_helper] ===== COMMAND RECEIVED =====");
    println!("[background_service_helper] Raw command: '{}'", command);
    println!("[background_service_helper] Command length: {}", command.len());
    
    if command.starts_with("select_service:") {
        let service_name = command.strip_prefix("select_service:").unwrap_or("").trim();
        println!("[background_service_helper] Parsed service name: '{}'", service_name);
        println!("[background_service_helper] Service name length: {}", service_name.len());
        
        if service_name.is_empty() {
            // Deselect current service
            println!("[background_service_helper] Action: Deselecting current service");
            set_selected_mpris_service(None);
        } else {
            // Select specific service using MPRIS manager
            println!("[background_service_helper] Action: Selecting service '{}'", service_name);
            set_selected_mpris_service(Some(service_name.to_string()));
            
            // Use MPRIS manager to select service
            unsafe {
                if let Some(ref mut manager) = MPRIS_MANAGER {
                    if let Err(e) = manager.select_service(service_name, status_sender.clone()) {
                        eprintln!("[background_service_helper] Failed to select service: {}", e);
                        return;
                    }
                    
                    // Status updates are now handled automatically by D-Bus monitoring
                    
                    // Immediately send current status from the newly selected service
                    if service_name.contains("spotify") {
                        if let Some(current_status) = spotify::get_spotify_status() {
                            spotify::send_status_update(status_sender, &current_status);
                            eprintln!("[background-service-helper] Sent immediate Spotify status after manual selection");
                        }
                    } else if service_name.contains("chromium") {
                        // For Chromium, we need to use the async function
                        let status_sender_clone = status_sender.clone();
                        tokio::spawn(async move {
                            if let Some(current_status) = chromium::get_chromium_status().await {
                                chromium::send_status_update(&status_sender_clone, &current_status);
                                eprintln!("[background-service-helper] Sent immediate Chromium status after manual selection");
                            }
                        });
                    }
                    
                    // Send service change notification
                    send_service_change_notification(status_sender, service_name);
                } else {
                    eprintln!("[background_service_helper] MPRIS manager not initialized");
                }
            }
        }
    } else {
        println!("[background_service_helper] Unknown command type, ignoring");
    }
    println!("[background_service_helper] ============================");
}

// Function to handle media control commands from the main app
async fn handle_media_control_command(command: &str, status_sender: &Arc<Mutex<Option<UnixStream>>>) {
    if command.starts_with("media_action:") {
        let action = command.strip_prefix("media_action:").unwrap_or("").trim();
        println!("[background_service_helper] Received media action: {}", action);
        
        // Use MPRIS manager to execute command
        unsafe {
            if let Some(ref mut manager) = MPRIS_MANAGER {
                let success = manager.execute_command(action, status_sender.clone());
                if success {
                    println!("[background_service_helper] Command executed successfully");
                    // Note: Status updates are now sent by individual commands after completion
                    // No need to send status update here as it would be stale
                } else {
                    println!("[background_service_helper] Command execution failed");
                }
            } else {
                eprintln!("[background_service_helper] MPRIS manager not initialized");
            }
        }
    }
}


// Function to send service change notification to UI
fn send_service_change_notification(status_sender: &Arc<Mutex<Option<UnixStream>>>, service_name: &str) {
    if let Ok(mut sender_guard) = status_sender.lock() {
        if let Some(ref mut stream) = *sender_guard {
            let notification = json!({
                "type": "service_changed",
                "service": service_name,
                "timestamp": chrono::Utc::now().timestamp_millis(),
            });
            
            let message = format!("service_change:{}\n", serde_json::to_string(&notification).unwrap_or_default());
            if let Err(e) = stream.write_all(message.as_bytes()) {
                eprintln!("[background_service_helper] Failed to send service change notification: {}", e);
            } else {
                println!("[background_service_helper] Sent service change notification for: {}", service_name);
            }
        }
    }
}

// Function to monitor D-Bus for service changes and update MPRIS services
async fn monitor_dbus_services(status_sender: Arc<Mutex<Option<UnixStream>>>) {
    let mut last_services = HashSet::new();
    let mut consecutive_errors = 0;
    const MAX_CONSECUTIVE_ERRORS: u32 = 5;
    
    loop {
        match query_mpris_services().await {
            Ok(services) => {
                consecutive_errors = 0; // Reset error count on success
                let current_services: HashSet<String> = services.into_iter().collect();
                
                // Check if services have changed
                if current_services != last_services {
                    eprintln!("[background-service-helper] MPRIS services changed, updating list...");
                    
                    // Update the static list
                    update_available_mpris_services(current_services.iter().cloned().collect());
                    
                    // Send updated list to main app
                    if let Err(e) = send_available_background_services(&status_sender) {
                        eprintln!("[background-service-helper] Failed to send updated services: {}", e);
                    }
                    
                    last_services = current_services;
                }
            }
            Err(e) => {
                consecutive_errors += 1;
                eprintln!("[background-service-helper] Failed to query MPRIS services (error {}): {}", consecutive_errors, e);
                
                if consecutive_errors >= MAX_CONSECUTIVE_ERRORS {
                    eprintln!("[background-service-helper] Too many consecutive D-Bus errors, attempting recovery...");
                    
                    // Try to recover by waiting longer and clearing any cached connections
                    tokio::time::sleep(Duration::from_secs(10)).await;
                    consecutive_errors = 0; // Reset to try again
                }
            }
        }
        
        // Check every 2 seconds for service changes
        tokio::time::sleep(Duration::from_secs(2)).await;
    }
}


fn send_available_background_services(status_sender: &Arc<Mutex<Option<UnixStream>>>) -> std::io::Result<()> {
    let services = get_available_mpris_services();
    let services_msg = format!("list_services:{}\n", services.join(","));
    
    if let Ok(mut sender_guard) = status_sender.lock() {
        if let Some(ref mut stream) = *sender_guard {
            if let Err(e) = stream.write_all(services_msg.as_bytes()) {
                eprintln!("[background-service-helper] Failed to send available background services: {}", e);
                return Err(e);
            }
            eprintln!("[background-service-helper] Sent available background services: {:?}", services);
            
            // Handle service selection logic
            if !services.is_empty() {
                let selected_service = get_selected_mpris_service();
                if let Some(selected) = selected_service {
                    // Check if the currently selected service is still available
                    if services.contains(&selected) {
                        // Send the currently selected service
                        let selected_msg = format!("selected_service:{}\n", selected);
                        if let Err(e) = stream.write_all(selected_msg.as_bytes()) {
                            eprintln!("[background-service-helper] Failed to send selected service: {}", e);
                            return Err(e);
                        }
                        eprintln!("[background-service-helper] Sent selected service: {}", selected);
                    } else {
                        // Currently selected service is no longer available, auto-switch to another
                        let new_service = &services[0];
                        eprintln!("[background-service-helper] Selected service '{}' no longer available, auto-switching to: {}", selected, new_service);
                        
                        // Update the selected service
                        set_selected_mpris_service(Some(new_service.clone()));
                        
                        // Switch to the new service in MPRIS manager
                        unsafe {
                            if let Some(ref mut manager) = MPRIS_MANAGER {
                                if let Err(e) = manager.select_service(new_service, status_sender.clone()) {
                                    eprintln!("[background-service-helper] Failed to switch to new service: {}", e);
                                } else {
                                    eprintln!("[background-service-helper] Successfully switched to new service: {}", new_service);
                                }
                            }
                        }
                        
                        // Send the new selected service
                        let selected_msg = format!("selected_service:{}\n", new_service);
                        if let Err(e) = stream.write_all(selected_msg.as_bytes()) {
                            eprintln!("[background-service-helper] Failed to send new selected service: {}", e);
                            return Err(e);
                        }
                        eprintln!("[background-service-helper] Sent new selected service: {}", new_service);
                    }
                } else {
                    // Auto-select first available service on restart, regardless of monitoring flag
                    unsafe {
                        if let Some(first_service) = services.first() {
                            eprintln!("[background-service-helper] Auto-selecting first available service: {}", first_service);
                            set_selected_mpris_service(Some(first_service.clone()));
                            HAS_DONE_INITIAL_AUTO_SELECTION = true;
                            
                            // Also select the service in the MPRIS manager
                            unsafe {
                                if let Some(ref mut manager) = MPRIS_MANAGER {
                                    if let Err(e) = manager.select_service(first_service, status_sender.clone()) {
                                        eprintln!("[background-service-helper] Failed to select service in MPRIS manager: {}", e);
                                    } else {
                                        eprintln!("[background-service-helper] Successfully selected service in MPRIS manager: {}", first_service);
                                    }
                                }
                            }
                            
                            // Send the auto-selected service
                            let selected_msg = format!("selected_service:{}\n", first_service);
                            if let Err(e) = stream.write_all(selected_msg.as_bytes()) {
                                eprintln!("[background-service-helper] Failed to send auto-selected service: {}", e);
                                return Err(e);
                            }
                            eprintln!("[background-service-helper] Sent auto-selected service: {}", first_service);
                        } else {
                            eprintln!("[background-service-helper] No services available for auto-selection");
                        }
                    }
                }
            } else {
                eprintln!("[background-service-helper] No services available, not sending selected service");
            }
        }
    }
    Ok(())
}

// Handle starting MPRIS monitoring
async fn handle_start_mpris_monitoring(status_sender: &Arc<Mutex<Option<UnixStream>>>) {
    eprintln!("[background-service-helper] Starting MPRIS monitoring");
    
    unsafe {
        MPRIS_MONITORING_ENABLED = true;
        
        if let Some(ref mut mpris_manager) = MPRIS_MANAGER {
            // Start monitoring if we have a selected service
            if let Some(ref selected_service) = SELECTED_BACKGROUND_SERVICE_MPRIS_NAME {
                eprintln!("[background-service-helper] Starting monitoring for service: {}", selected_service);
                mpris_manager.start_monitoring(status_sender.clone());
            } else {
                eprintln!("[background-service-helper] No service selected, triggering auto-selection");
                // Trigger auto-selection by sending available services
                if let Err(e) = send_available_background_services(status_sender) {
                    eprintln!("[background-service-helper] Failed to trigger auto-selection: {}", e);
                }
            }
        }
    }
}

// Handle stopping MPRIS monitoring
async fn handle_stop_mpris_monitoring(status_sender: &Arc<Mutex<Option<UnixStream>>>) {
    eprintln!("[background-service-helper] Stopping MPRIS monitoring");
    
    unsafe {
        MPRIS_MONITORING_ENABLED = false;
        
        if let Some(ref mut mpris_manager) = MPRIS_MANAGER {
            // Stop all monitoring
            mpris_manager.stop_monitoring();
            eprintln!("[background-service-helper] MPRIS monitoring stopped");
        }
    }
}

#[tokio::main]
async fn main() -> std::io::Result<()> {
    let socket_path = "/tmp/touchbar-background-service.sock";
    
    // Print environment info for debugging
    if let Ok(addr) = std::env::var("DBUS_SESSION_BUS_ADDRESS") {
        eprintln!("[background-service-helper] DBUS_SESSION_BUS_ADDRESS={}", addr);
    } else {
        eprintln!("[background-service-helper] DBUS_SESSION_BUS_ADDRESS is not set");
    }
    
    eprintln!("[background-service-helper] Starting background service monitoring...");
    
    // Reset auto-selection flag on restart
    unsafe {
        HAS_DONE_INITIAL_AUTO_SELECTION = false;
        eprintln!("[background-service-helper] Reset auto-selection flag for restart");
    }
    
    // Initialize MPRIS manager
    unsafe {
        MPRIS_MANAGER = Some(MprisManager::new());
        eprintln!("[background-service-helper] MPRIS manager initialized");
    }
    
    let stream = loop {
        match UnixStream::connect(socket_path) {
            Ok(stream) => {
                let stream = stream;
                stream.set_nonblocking(true)?;
                break stream;
            }
            Err(_) => {
                // Add small delay to prevent busy-waiting during connection attempts
                // This prevents the helper from consuming 100% CPU when the main app is not ready
                thread::sleep(Duration::from_millis(10));
                continue;
            }
        }
    };
    
    eprintln!("[background-service-helper] Connected to socket, starting background service monitoring...");
    
    // Create a reader for incoming commands
    let mut stream_clone = stream.try_clone()?;
    let mut buffer = Vec::new();
    
    // Create a shared sender for status updates
    let status_sender = Arc::new(Mutex::new(Some(stream)));
    
    // Initialize MPRIS services by querying D-Bus
    match query_mpris_services().await {
        Ok(services) => {
            eprintln!("[background-service-helper] Found MPRIS services: {:?}", services);
            
            // Auto-select first available service on restart
            if let Some(first_service) = services.first() {
                eprintln!("[background-service-helper] Auto-selecting first available service on restart: {}", first_service);
                set_selected_mpris_service(Some(first_service.clone()));
                
                // Also select the service in the MPRIS manager
                unsafe {
                    if let Some(ref mut manager) = MPRIS_MANAGER {
                        if let Err(e) = manager.select_service(first_service, status_sender.clone()) {
                            eprintln!("[background-service-helper] Failed to select service in MPRIS manager: {}", e);
                        } else {
                            eprintln!("[background-service-helper] Successfully selected service in MPRIS manager: {}", first_service);
                        }
                    }
                }
            } else {
                eprintln!("[background-service-helper] No MPRIS services available for auto-selection");
            }
            
            // Update available services after auto-selection
            update_available_mpris_services(services);
        }
        Err(e) => {
            eprintln!("[background-service-helper] Failed to query initial MPRIS services: {}", e);
            // Initialize with empty list
            update_available_mpris_services(Vec::new());
        }
    }
    
    // Send available MPRIS services to main application first
    send_available_background_services(&status_sender)?;
    
    // Start D-Bus monitoring in a separate task
    let status_sender_clone = status_sender.clone();
    tokio::spawn(async move {
        monitor_dbus_services(status_sender_clone).await;
    });
    
    // Start connection health monitoring in a separate task
    let status_sender_clone = status_sender.clone();
    tokio::spawn(async move {
        monitor_connection_health(status_sender_clone).await;
    });
    
    
    let mut last_logged_selection = None;
    let mut selection_check_counter = 0;
    
    loop {
        // Event-driven command processing (non-blocking)
        let mut temp_buffer = [0u8; 1024];
        match stream_clone.read(&mut temp_buffer) {
            Ok(0) => {
                // EOF - connection closed
                eprintln!("[background-service-helper] Connection closed by main process");
                break;
            }
            Ok(n) => {
                // Process incoming data immediately (event-driven)
                buffer.extend_from_slice(&temp_buffer[..n]);
                
                // Process complete lines as they arrive
                while let Some(newline_pos) = buffer.iter().position(|&b| b == b'\n') {
                    let line_data = buffer.drain(..=newline_pos).collect::<Vec<_>>();
                    let line = String::from_utf8_lossy(&line_data[..line_data.len()-1]); // Remove newline
                    let command = line.trim();
                    if !command.is_empty() {
                        eprintln!("[background-service-helper] Received command: {}", command);
                        // Handle selection commands
                        if command.starts_with("select_service:") {
                            handle_selection_command(command, &status_sender);
                        }
                        // Handle media control commands
                        else if command.starts_with("media_action:") {
                            handle_media_control_command(command, &status_sender).await;
                        }
                        // Handle MPRIS monitoring control commands
                        else if command == "start_mpris_monitoring" {
                            handle_start_mpris_monitoring(&status_sender).await;
                        }
                        else if command == "stop_mpris_monitoring" {
                            handle_stop_mpris_monitoring(&status_sender).await;
                        }
                        // Handle other commands
                        else {
                            eprintln!("[background-service-helper] Unknown command type: {}", command);
                        }
                    }
                }
            }
            Err(e) => {
                if e.kind() == std::io::ErrorKind::WouldBlock {
                    // No data available, continue to next iteration
                    thread::sleep(Duration::from_millis(1));
                    continue;
                } else {
                    eprintln!("[background-service-helper] Read error: {}", e);
                    break;
                }
            }
        }
        
        // Periodically log the current selected service (every 1000 iterations = ~1 second)
        selection_check_counter += 1;
        if selection_check_counter >= 1000 {
            let current_selection = get_selected_mpris_service();
            if current_selection != last_logged_selection {
                if let Some(ref service) = current_selection {
                    println!("[background_service_helper] Currently monitoring MPRIS service: {}", service);
                } else {
                    println!("[background_service_helper] Currently monitoring: No service selected");
                }
                last_logged_selection = current_selection;
            }
            selection_check_counter = 0;
        }
    }
    
    eprintln!("[background-service-helper] Background service helper shutting down");
    Ok(())
}
