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

mod mpris_manager {
    include!("background_services/mpris_manager.rs");
}

// Import specific functions we need
use mpris_manager::MprisManager;

// Dynamic list of available MPRIS background services
static mut AVAILABLE_MPRIS_BACKGROUND: Vec<String> = Vec::new();

// Dynamic selected MPRIS name for background service
static mut SELECTED_BACKGROUND_SERVICE_MPRIS_NAME: Option<String> = None;

// MPRIS Manager instance
static mut MPRIS_MANAGER: Option<MprisManager> = None;

// Function to query D-Bus for MPRIS services (filtered for Spotify and Chromium only)
async fn query_mpris_services() -> Result<Vec<String>, Box<dyn std::error::Error>> {
    let connection = Connection::session().await?;
    let proxy = zbus::Proxy::new(
        &connection,
        "org.freedesktop.DBus",
        "/org/freedesktop/DBus",
        "org.freedesktop.DBus",
    ).await?;
    
    let names: Vec<String> = proxy.call_method("ListNames", &()).await?
        .body::<Vec<String>>()?;
    
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

// Function to update the available MPRIS services list
fn update_available_mpris_services(services: Vec<String>) {
    unsafe {
        AVAILABLE_MPRIS_BACKGROUND = services;
        eprintln!("[background-service-helper] Updated MPRIS services: {:?}", AVAILABLE_MPRIS_BACKGROUND);
    }
    // Update selected service with fallback logic
    update_selected_service_with_fallback();
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

// Function to update selected service with fallback logic
fn update_selected_service_with_fallback() {
    unsafe {
        // Don't auto-select any service - wait for explicit selection
        if SELECTED_BACKGROUND_SERVICE_MPRIS_NAME.is_none() {
            eprintln!("[background-service-helper] No MPRIS service selected, waiting for explicit selection");
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
                    if let Err(e) = manager.select_service(service_name) {
                        eprintln!("[background_service_helper] Failed to select service: {}", e);
                        return;
                    }
                    
                    // Send status update immediately
                    manager.send_status_update(status_sender);
                    
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
                let success = manager.execute_command(action);
                if success {
                    println!("[background_service_helper] Command executed successfully");
                    // Send status update after command
                    manager.send_status_update(status_sender);
                } else {
                    println!("[background_service_helper] Command execution failed");
                }
            } else {
                eprintln!("[background_service_helper] MPRIS manager not initialized");
            }
        }
    }
}

// These functions are now handled by the MPRIS manager

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
    
    loop {
        match query_mpris_services().await {
            Ok(services) => {
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
                eprintln!("[background-service-helper] Failed to query MPRIS services: {}", e);
            }
        }
        
        // Check every 2 seconds for service changes
        tokio::time::sleep(Duration::from_secs(2)).await;
    }
}

fn send_available_services(stream: &UnixStream) -> std::io::Result<()> {
    let services = get_available_mpris_services();
    let services_msg = format!("list_services:{}\n", services.join(","));
    if let Err(e) = stream.try_clone()?.write_all(services_msg.as_bytes()) {
        eprintln!("[background-service-helper] Failed to send available background services: {}", e);
        return Err(e);
    }
    eprintln!("[background-service-helper] Sent available background services: {:?}", services);
    Ok(())
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
            
            // Only send the currently selected service if there are available services
            if !services.is_empty() {
                let selected_service = get_selected_mpris_service();
                if let Some(selected) = selected_service {
                    let selected_msg = format!("selected_service:{}\n", selected);
                    if let Err(e) = stream.write_all(selected_msg.as_bytes()) {
                        eprintln!("[background-service-helper] Failed to send selected service: {}", e);
                        return Err(e);
                    }
                    eprintln!("[background-service-helper] Sent selected service: {}", selected);
                }
            } else {
                eprintln!("[background-service-helper] No services available, not sending selected service");
            }
        }
    }
    Ok(())
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
    
    // Initialize MPRIS manager
    unsafe {
        MPRIS_MANAGER = Some(MprisManager::new());
        eprintln!("[background-service-helper] MPRIS manager initialized");
    }
    
    // Initialize MPRIS services by querying D-Bus
    match query_mpris_services().await {
        Ok(services) => {
            eprintln!("[background-service-helper] Found MPRIS services: {:?}", services);
            update_available_mpris_services(services);
            
            // Don't auto-select any service - wait for explicit selection
            eprintln!("[background-service-helper] Available MPRIS services found, waiting for explicit selection");
        }
        Err(e) => {
            eprintln!("[background-service-helper] Failed to query initial MPRIS services: {}", e);
            // Initialize with empty list
            update_available_mpris_services(Vec::new());
        }
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
    
    // Send available MPRIS services to main application first
    send_available_background_services(&status_sender)?;
    
    // Start D-Bus monitoring in a separate task
    let status_sender_clone = status_sender.clone();
    tokio::spawn(async move {
        monitor_dbus_services(status_sender_clone).await;
    });
    
    // Start event monitoring in a separate thread
    let status_sender_clone = status_sender.clone();
    thread::spawn(move || {
        // Use MPRIS manager to start monitoring
        unsafe {
            if let Some(ref mut manager) = MPRIS_MANAGER {
                manager.start_monitoring(status_sender_clone);
            } else {
                println!("[background_service_helper] MPRIS manager not initialized, not starting monitoring");
            }
        }
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
