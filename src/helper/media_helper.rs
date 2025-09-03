//! Media Player helper binary for tiny-dfr, providing VLC and Dragon Player status via DBus.
//! 
//! This helper process:
//! 1. Connects to the main process via Unix socket
//! 2. Monitors VLC or Dragon Player via DBus signals and sends status updates to main process
//! 3. Receives commands from main process and executes them on the active media player
//! 
//! Supported commands:
//! - play_pause: Toggle play/pause
//! - play: Start playback
//! - pause: Pause playback
//! - next: Next track
//! - previous: Previous track
//! - stop: Stop playback
//! - raise: Raise VLC window
//! - quit: Quit VLC
//! - seek:position: Seek to position (0.0 to 1.0)
//! - set_position:position: Set absolute position (0.0 to 1.0)

use std::os::unix::net::UnixStream;
use std::io::{Write, Read};
use std::thread;
use std::time::Duration;


use std::sync::{Arc, Mutex};
use serde_json::json;

// DBus imports for native MPRIS communication
use zbus::{Connection, MessageType, MessageStream, MatchRule, Proxy};
use zbus::fdo::DBusProxy;
use futures_lite::stream::StreamExt;
use std::collections::HashMap;
use tokio::runtime::Runtime;
use lazy_static::lazy_static;

// OOP Architecture for Media Players
// =================================

/// Enum-based Media Player for better OOP without dyn compatibility issues
#[derive(Debug)]
enum MediaPlayerType {
    Vlc(VlcPlayer),
    Dragon(DragonPlayer),
}

impl MediaPlayerType {
    /// Get the MPRIS destination for this player
    fn get_mpris_destination(&self) -> String {
        match self {
            MediaPlayerType::Vlc(player) => player.get_mpris_destination(),
            MediaPlayerType::Dragon(player) => player.get_mpris_destination(),
        }
    }
    
    /// Check if this player is currently running
    fn is_running(&self) -> bool {
        match self {
            MediaPlayerType::Vlc(player) => player.is_running(),
            MediaPlayerType::Dragon(player) => player.is_running(),
        }
    }
    
    /// Get current media status
    fn get_status(&self) -> Option<MediaStatus> {
        match self {
            MediaPlayerType::Vlc(player) => player.get_status(),
            MediaPlayerType::Dragon(player) => player.get_status(),
        }
    }
    
    /// Get current media status asynchronously (for use in async contexts)
    async fn get_status_async(&self) -> Option<MediaStatus> {
        match self {
            MediaPlayerType::Vlc(player) => player.get_status_async().await,
            MediaPlayerType::Dragon(player) => player.get_status_async().await,
        }
    }
    
    /// Execute a command on this player
    fn execute_command(&self, command: &str, args: &[&str]) -> bool {
        match self {
            MediaPlayerType::Vlc(player) => player.execute_command(command, args),
            MediaPlayerType::Dragon(player) => player.execute_command(command, args),
        }
    }
    
    /// Get player-specific status from MPRIS destination
    fn get_status_from_dest(&self, mpris_dest: &str) -> Option<MediaStatus> {
        match self {
            MediaPlayerType::Vlc(player) => player.get_status_from_dest(mpris_dest),
            MediaPlayerType::Dragon(player) => player.get_status_from_dest(mpris_dest),
        }
    }
    
    /// Handle player-specific commands
    fn handle_command(&self, command: &str, status_sender: &Arc<Mutex<Option<UnixStream>>>) {
        match self {
            MediaPlayerType::Vlc(player) => player.handle_command(command, status_sender),
            MediaPlayerType::Dragon(player) => player.handle_command(command, status_sender),
        }
    }
    
    /// Monitor player-specific events
    fn monitor_events(&self, status_sender: Arc<Mutex<Option<UnixStream>>>) {
        match self {
            MediaPlayerType::Vlc(player) => player.monitor_events(status_sender),
            MediaPlayerType::Dragon(player) => player.monitor_events(status_sender),
        }
    }
    
    /// Run player-specific D-Bus event monitor
    fn run_dbus_event_monitor(
        &self,
        status_sender: Arc<Mutex<Option<UnixStream>>>, 
        playback_state: Arc<Mutex<(bool, MediaStatus)>>
    ) -> Result<(), Box<dyn std::error::Error>> {
        match self {
            MediaPlayerType::Vlc(player) => player.run_dbus_event_monitor(status_sender, playback_state),
            MediaPlayerType::Dragon(player) => player.run_dbus_event_monitor(status_sender, playback_state),
        }
    }
    
    /// Get the player's window class name
    fn get_window_class(&self) -> &str {
        match self {
            MediaPlayerType::Vlc(player) => player.get_window_class(),
            MediaPlayerType::Dragon(player) => player.get_window_class(),
        }
    }
}

/// VLC Media Player implementation
#[derive(Debug)]
struct VlcPlayer {
    instance: Option<MediaPlayerInstance>,
}

impl VlcPlayer {
    fn new(instance: Option<MediaPlayerInstance>) -> Self {
        Self { instance }
    }
    
    fn get_mpris_destination(&self) -> String {
        if let Some(ref instance) = self.instance {
            if let Some(pid) = instance.pid {
                let instance_name = format!("org.mpris.MediaPlayer2.vlc.instance{}", pid);
                eprintln!("[VlcPlayer] Using VLC instance-specific MPRIS: {}", instance_name);
                instance_name
            } else {
                eprintln!("[VlcPlayer] No PID available, using legacy VLC MPRIS");
                "org.mpris.MediaPlayer2.vlc".to_string()
            }
        } else {
            eprintln!("[VlcPlayer] No instance available, using legacy VLC MPRIS");
            "org.mpris.MediaPlayer2.vlc".to_string()
        }
    }
    
    fn is_running(&self) -> bool {
        self.instance.as_ref()
            .map(|instance| instance.window_class == "vlc")
            .unwrap_or(false)
    }
    
    fn get_status(&self) -> Option<MediaStatus> {
        let instance = self.instance.as_ref()?;
        if instance.window_class != "vlc" {
            return None;
        }
        
        let primary_dest = &instance.mpris_name;
        let fallback_dest = "org.mpris.MediaPlayer2.vlc";
        
        let destinations = if primary_dest != fallback_dest {
            vec![primary_dest.as_str(), fallback_dest]
        } else {
            vec![primary_dest.as_str()]
        };
        
        for (i, mpris_dest) in destinations.iter().enumerate() {
            if i == 0 {
                eprintln!("[VlcPlayer] Getting VLC status from: {} (primary)", mpris_dest);
            } else {
                eprintln!("[VlcPlayer] Trying VLC fallback: {}", mpris_dest);
            }
            
            if let Some(status) = self.get_status_from_dest(mpris_dest) {
                eprintln!("[VlcPlayer] Successfully connected to: {}", mpris_dest);
                return Some(status);
            }
        }
        
        eprintln!("[VlcPlayer] All VLC MPRIS destinations failed");
        None
    }
    
    async fn get_status_async(&self) -> Option<MediaStatus> {
        let instance = self.instance.as_ref()?;
        if instance.window_class != "vlc" {
            return None;
        }
        let primary_dest = &instance.mpris_name;
        let fallback_dest = "org.mpris.MediaPlayer2.vlc";
        let destinations = if primary_dest != fallback_dest {
            vec![primary_dest.as_str(), fallback_dest]
        } else {
            vec![primary_dest.as_str()]
        };
        for (i, mpris_dest) in destinations.iter().enumerate() {
            if i == 0 {
                eprintln!("[VlcPlayer] Getting VLC status from: {} (primary, async)", mpris_dest);
            } else {
                eprintln!("[VlcPlayer] Trying VLC fallback (async): {}", mpris_dest);
            }
            if let Some(status) = get_status_from_dest_native(mpris_dest).await {
                eprintln!("[VlcPlayer] Successfully connected (async) to: {}", mpris_dest);
                return Some(status);
            }
        }
        eprintln!("[VlcPlayer] All VLC MPRIS destinations failed (async)");
        None
    }
    
    fn execute_command(&self, command: &str, args: &[&str]) -> bool {
        let instance = match &self.instance {
            Some(instance) => instance,
            None => {
                eprintln!("[VlcPlayer] No VLC instance detected");
                return false;
            }
        };
        
        let primary_dest = &instance.mpris_name;
        let fallback_dest = "org.mpris.MediaPlayer2.vlc";
        
        let destinations = if primary_dest != fallback_dest {
            vec![primary_dest.as_str(), fallback_dest]
        } else {
            vec![primary_dest.as_str()]
        };
        
        for (i, mpris_dest) in destinations.iter().enumerate() {
            if i == 0 {
                eprintln!("[VlcPlayer] Executing VLC command on: {} (primary)", mpris_dest);
            } else {
                eprintln!("[VlcPlayer] Trying VLC command fallback: {}", mpris_dest);
            }
            
            if try_execute_command_on_destination(command, args, mpris_dest) {
                eprintln!("[VlcPlayer] Command executed successfully on: {}", mpris_dest);
                return true;
            }
        }
        
        eprintln!("[VlcPlayer] All VLC command destinations failed");
        false
    }
    
    fn get_status_from_dest(&self, mpris_dest: &str) -> Option<MediaStatus> {
        // Use native D-Bus implementation with shared runtime
        eprintln!("[VlcPlayer] Getting VLC status from DBus destination: {}", mpris_dest);
        TOKIO_RT.block_on(get_status_from_dest_native(mpris_dest))
    }
    
    fn handle_command(&self, command: &str, status_sender: &Arc<Mutex<Option<UnixStream>>>) {
        // Forward to the existing VLC command handler implementation
        handle_vlc_command(command, status_sender)
    }
    
    fn monitor_events(&self, status_sender: Arc<Mutex<Option<UnixStream>>>) {
        // Forward to the existing VLC event monitoring implementation
        monitor_vlc_events(status_sender)
    }
    
    fn run_dbus_event_monitor(
        &self,
        status_sender: Arc<Mutex<Option<UnixStream>>>, 
        playback_state: Arc<Mutex<(bool, MediaStatus)>>
    ) -> Result<(), Box<dyn std::error::Error>> {
        // Forward to the existing VLC D-Bus event monitor implementation
        run_vlc_dbus_event_monitor(status_sender, playback_state)
    }
    
    fn get_window_class(&self) -> &str {
        if let Some(ref instance) = self.instance {
            instance.window_class.as_str()
        } else {
            "vlc"
        }
    }
}

/// Dragon Player implementation
#[derive(Debug)]
struct DragonPlayer {
    instance: Option<MediaPlayerInstance>,
}

impl DragonPlayer {
    fn new(instance: Option<MediaPlayerInstance>) -> Self {
        Self { instance }
    }
    
    fn get_mpris_destination(&self) -> String {
        // Dragon Player typically exposes only the base MPRIS name; instance-specific names are not activatable
        eprintln!("[DragonPlayer] Using Dragon Player base MPRIS: org.mpris.MediaPlayer2.dragonplayer");
        "org.mpris.MediaPlayer2.dragonplayer".to_string()
    }
    
    fn is_running(&self) -> bool {
        self.instance.as_ref()
            .map(|instance| instance.window_class == "org.kde.dragonplayer" || instance.window_class == "dragonplayer")
            .unwrap_or(false)
    }
    
    fn get_status(&self) -> Option<MediaStatus> {
        let instance = self.instance.as_ref()?;
        if instance.window_class != "org.kde.dragonplayer" && instance.window_class != "dragonplayer" {
            return None;
        }
        
        let mpris_dest = "org.mpris.MediaPlayer2.dragonplayer";
        eprintln!("[DragonPlayer] Getting Dragon Player status from: {}", mpris_dest);
        
        if let Some(status) = self.get_status_from_dest(mpris_dest) {
            eprintln!("[DragonPlayer] Successfully connected to: {}", mpris_dest);
            Some(status)
        } else {
            eprintln!("[DragonPlayer] Failed to connect to Dragon Player MPRIS");
            None
        }
    }
    
    async fn get_status_async(&self) -> Option<MediaStatus> {
        let instance = self.instance.as_ref()?;
        if instance.window_class != "org.kde.dragonplayer" && instance.window_class != "dragonplayer" {
            return None;
        }
        
        let mpris_dest = "org.mpris.MediaPlayer2.dragonplayer";
        eprintln!("[DragonPlayer] Getting Dragon Player status from: {} (async)", mpris_dest);
        
        if let Some(status) = get_status_from_dest_native(mpris_dest).await {
            eprintln!("[DragonPlayer] Successfully connected (async) to: {}", mpris_dest);
            Some(status)
        } else {
            eprintln!("[DragonPlayer] Failed to connect to Dragon Player MPRIS (async)");
            None
        }
    }
    
    fn execute_command(&self, command: &str, args: &[&str]) -> bool {
        if self.instance.is_none() {
            eprintln!("[DragonPlayer] No Dragon Player instance detected");
            return false;
        }
        
        let mpris_dest = "org.mpris.MediaPlayer2.dragonplayer";
        eprintln!("[DragonPlayer] Executing Dragon Player command on: {}", mpris_dest);
        
        if try_execute_command_on_destination(command, args, mpris_dest) {
            eprintln!("[DragonPlayer] Command executed successfully on: {}", mpris_dest);
            true
        } else {
            eprintln!("[DragonPlayer] Dragon Player command failed");
            false
        }
    }
    
    fn get_status_from_dest(&self, mpris_dest: &str) -> Option<MediaStatus> {
        // Use native D-Bus implementation with shared runtime
        eprintln!("[DragonPlayer] Getting Dragon Player status from DBus destination: {}", mpris_dest);
        TOKIO_RT.block_on(get_status_from_dest_native(mpris_dest))
    }
    
    fn handle_command(&self, command: &str, status_sender: &Arc<Mutex<Option<UnixStream>>>) {
        // Forward to the existing Dragon Player command handler implementation
        handle_dragon_player_command(command, status_sender)
    }
    
    fn monitor_events(&self, status_sender: Arc<Mutex<Option<UnixStream>>>) {
        // Forward to the existing Dragon Player event monitoring implementation
        monitor_dragon_player_events(status_sender)
    }
    
    fn run_dbus_event_monitor(
        &self,
        status_sender: Arc<Mutex<Option<UnixStream>>>, 
        playback_state: Arc<Mutex<(bool, MediaStatus)>>
    ) -> Result<(), Box<dyn std::error::Error>> {
        // Forward to the existing Dragon Player D-Bus event monitor implementation
        run_dragon_player_dbus_event_monitor(status_sender, playback_state)
    }
    
    fn get_window_class(&self) -> &str {
        if let Some(ref instance) = self.instance {
            instance.window_class.as_str()
        } else {
            "org.kde.dragonplayer"
        }
    }
}

/// Media Player Factory for creating appropriate player instances
struct MediaPlayerFactory;

impl MediaPlayerFactory {
    fn create_player(window_class: &str, instance: Option<MediaPlayerInstance>) -> MediaPlayerType {
        match window_class {
            "vlc" => MediaPlayerType::Vlc(VlcPlayer::new(instance)),
            "org.kde.dragonplayer" | "dragonplayer" => MediaPlayerType::Dragon(DragonPlayer::new(instance)),
            _ => {
                eprintln!("[MediaPlayerFactory] Unknown player class '{}', defaulting to VLC", window_class);
                MediaPlayerType::Vlc(VlcPlayer::new(instance))
            }
        }
    }
}

/// Manager for the current active media player
struct MediaPlayerManager {
    current_player: Option<MediaPlayerType>,
}

impl MediaPlayerManager {
    fn new() -> Self {
        Self { current_player: None }
    }
    
    fn set_current_player(&mut self, window_class: &str, instance: Option<MediaPlayerInstance>) {
        self.current_player = Some(MediaPlayerFactory::create_player(window_class, instance));
    }
    
    fn get_current_player(&self) -> Option<&MediaPlayerType> {
        self.current_player.as_ref()
    }
    
    fn clear_current_player(&mut self) {
        self.current_player = None;
    }
}

// Global media player manager
static mut MEDIA_PLAYER_MANAGER: Option<MediaPlayerManager> = None;

fn get_media_player_manager() -> &'static mut MediaPlayerManager {
    unsafe {
        if MEDIA_PLAYER_MANAGER.is_none() {
            MEDIA_PLAYER_MANAGER = Some(MediaPlayerManager::new());
        }
        MEDIA_PLAYER_MANAGER.as_mut().unwrap()
    }
}

// Helper functions for native D-Bus implementation

fn extract_interface_from_method(method: &str) -> &str {
    if method.starts_with("org.mpris.MediaPlayer2.Player.") {
        "org.mpris.MediaPlayer2.Player"
    } else if method.starts_with("org.mpris.MediaPlayer2.") {
        "org.mpris.MediaPlayer2"
    } else {
        "org.mpris.MediaPlayer2.Player" // Default to Player interface
    }
}

fn extract_method_name(method: &str) -> String {
    method.split('.').last().unwrap_or(method).to_string()
}



// Global connection for reuse across async contexts
static mut SHARED_DBUS_CONNECTION: Option<Connection> = None;

// Shared Tokio runtime for all async DBus work
lazy_static! {
    static ref TOKIO_RT: Runtime = Runtime::new().expect("Failed to create shared Tokio runtime");
}

async fn get_shared_connection() -> Result<Connection, zbus::Error> {
    // Try to reuse shared connection if available, otherwise create a new one
    unsafe {
        if let Some(ref conn) = SHARED_DBUS_CONNECTION {
            // Try to clone the existing connection
            Ok(conn.clone())
        } else {
            // Create a new connection
            let conn = Connection::session().await?;
            SHARED_DBUS_CONNECTION = Some(conn.clone());
            Ok(conn)
        }
    }
}

// Native D-Bus implementations
async fn get_status_from_dest_native(mpris_dest: &str) -> Option<MediaStatus> {
    let connection = match get_shared_connection().await {
        Ok(conn) => conn,
        Err(e) => {
            eprintln!("[media-helper] Failed to get D-Bus connection: {}", e);
            return None;
        }
    };
    
    let proxy = match Proxy::new(
        &connection,
        mpris_dest,
        "/org/mpris/MediaPlayer2",
        "org.mpris.MediaPlayer2.Player",
    ).await {
        Ok(p) => p,
        Err(e) => {
            eprintln!("[media-helper] Failed to create proxy for {}: {}", mpris_dest, e);
            return None;
        }
    };
    
    // Get individual properties using get_property
    let playback_status: String = match proxy.get_property("PlaybackStatus").await {
        Ok(status) => status,
        Err(e) => {
            eprintln!("[media-helper] Failed to get PlaybackStatus: {}", e);
            return None;
        }
    };
    
    let position_raw: i64 = proxy.get_property("Position").await.unwrap_or(0);
    
    let metadata: HashMap<String, zbus::zvariant::Value> = proxy
        .get_property("Metadata")
        .await
        .unwrap_or_else(|_| HashMap::new());
    
    let length_raw = metadata.get("mpris:length")
        .and_then(|v| Some(v.clone()))
        .and_then(|v| i64::try_from(v).ok())
        .unwrap_or(0);
    
    // Dragon Player uses milliseconds, VLC uses microseconds
    let is_dragon = mpris_dest.contains("dragonplayer");
    let duration_seconds = if is_dragon { (length_raw / 1_000) } else { (length_raw / 1_000_000) };
    let position_seconds = if is_dragon { (position_raw as f64 / 1_000.0) } else { (position_raw as f64 / 1_000_000.0) };
    let is_playing = playback_status == "Playing";
    let position_ratio = if duration_seconds > 0 { position_seconds / duration_seconds as f64 } else { 0.0 };
    
    Some(MediaStatus {
        is_playing,
        position: position_ratio,
        duration: duration_seconds,
    })
}

async fn try_execute_command_on_destination_native(command: &str, args: &[&str], mpris_dest: &str) -> bool {
    let connection = match get_shared_connection().await {
        Ok(conn) => conn,
        Err(e) => {
            eprintln!("[media-helper] Failed to get D-Bus connection: {}", e);
            return false;
        }
    };
    
    let interface = extract_interface_from_method(command);
    let proxy = match Proxy::new(
        &connection,
        mpris_dest,
        "/org/mpris/MediaPlayer2",
        interface,
    ).await {
        Ok(p) => p,
        Err(e) => {
            eprintln!("[media-helper] Failed to create proxy for {}: {}", mpris_dest, e);
            return false;
        }
    };
    
    let method_name = extract_method_name(command);
    
    // Handle different method signatures
    let result = match method_name.as_str() {
        "PlayPause" | "Play" | "Pause" | "Stop" | "Next" | "Previous" => {
            proxy.call_method(method_name.as_str(), &()).await
        }
        "Seek" => {
            if let Some(arg) = args.get(0) {
                if let Some(offset_str) = arg.strip_prefix("int64:") {
                    if let Ok(offset) = offset_str.parse::<i64>() {
                        proxy.call_method(method_name.as_str(), &(offset,)).await
                    } else {
                        return false;
                    }
                } else {
                    return false;
                }
            } else {
                return false;
            }
        }
        "Raise" | "Quit" => {
            proxy.call_method(method_name.as_str(), &()).await
        }
        _ => {
            eprintln!("[media-helper] Unknown method: {}", method_name);
            return false;
        }
    };
    
    match result {
        Ok(_) => {
            eprintln!("[media-helper] Command '{}' executed successfully on {}", command, mpris_dest);
            true
        }
        Err(e) => {
            eprintln!("[media-helper] Command '{}' failed on {}: {}", command, mpris_dest, e);
            false
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
struct MediaStatus {
    is_playing: bool,
    position: f64,
    duration: i64,
}

impl MediaStatus {
    fn empty() -> Self {
        Self {
            is_playing: false,
            position: 0.0,
            duration: 0,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
struct MediaPlayerInstance {
    mpris_name: String,
    window_class: String,
    pid: Option<u32>,
    is_active: bool,
}

impl MediaPlayerInstance {
    fn new(mpris_name: String, window_class: String, pid: Option<u32>) -> Self {
        Self {
            mpris_name,
            window_class,
            pid,
            is_active: false,
        }
    }
}

// Global variable to store the current focused media player instance
static mut CURRENT_MEDIA_PLAYER_INSTANCE: Option<MediaPlayerInstance> = None;

fn set_current_media_player(class: &str, pid: Option<u32>) {
    eprintln!("[media-helper] set_current_media_player called with class: '{}', pid: {:?}", class, pid);
    
    let mpris_dest = get_mpris_destination(class, pid);
    eprintln!("[media-helper] Determined MPRIS destination: {}", mpris_dest);
    
    let instance = MediaPlayerInstance::new(mpris_dest, class.to_string(), pid);
    
    // Update the old global instance for backward compatibility
    unsafe {
        CURRENT_MEDIA_PLAYER_INSTANCE = Some(instance.clone());
        eprintln!("[media-helper] Set current media player instance to: {:?}", CURRENT_MEDIA_PLAYER_INSTANCE);
    }
    
    // Update the new OOP manager
    let manager = get_media_player_manager();
    manager.set_current_player(class, Some(instance));
}

fn get_current_media_player_instance() -> Option<MediaPlayerInstance> {
    unsafe {
        CURRENT_MEDIA_PLAYER_INSTANCE.clone()
    }
}

// OOP wrapper functions for easier migration
fn get_current_player() -> Option<&'static MediaPlayerType> {
    get_media_player_manager().get_current_player()
}

fn get_current_player_status() -> Option<MediaStatus> {
    get_current_player()?.get_status()
}

async fn get_current_player_status_async() -> Option<MediaStatus> {
    // We need to handle the async case differently since we can't hold references across await
    if let Some(instance) = get_current_media_player_instance() {
        let window_class = instance.window_class.clone();
        let player = MediaPlayerFactory::create_player(&window_class, Some(instance));
        player.get_status_async().await
    } else {
        None
    }
}

fn execute_current_player_command(command: &str, args: &[&str]) -> bool {
    match get_current_player() {
        Some(player) => player.execute_command(command, args),
        None => false,
    }
}

fn handle_current_player_command(command: &str, status_sender: &Arc<Mutex<Option<UnixStream>>>) {
    if let Some(player) = get_current_player() {
        player.handle_command(command, status_sender);
    }
}

fn monitor_current_player_events(status_sender: Arc<Mutex<Option<UnixStream>>>) {
    if let Some(player) = get_current_player() {
        player.monitor_events(status_sender);
    } else {
        eprintln!("[OOP] No current player to monitor, defaulting to VLC");
        let vlc_player = MediaPlayerType::Vlc(VlcPlayer::new(None));
        vlc_player.monitor_events(status_sender);
    }
}

// VLC-specific functions
fn get_vlc_mpris_destination(pid: Option<u32>) -> String {
    if let Some(pid) = pid {
        let instance_name = format!("org.mpris.MediaPlayer2.vlc.instance{}", pid);
        eprintln!("[vlc-helper] Using VLC instance-specific MPRIS: {}", instance_name);
        instance_name
    } else {
        eprintln!("[vlc-helper] No PID available, using legacy VLC MPRIS");
        "org.mpris.MediaPlayer2.vlc".to_string()
    }
}

fn is_vlc_running() -> bool {
    get_current_media_player_instance()
        .map(|instance| instance.window_class == "vlc")
        .unwrap_or(false)
}

fn get_vlc_status() -> Option<MediaStatus> {
    let instance = get_current_media_player_instance()?;
    if instance.window_class != "vlc" {
        return None;
    }
    
    let primary_dest = &instance.mpris_name;
    let fallback_dest = "org.mpris.MediaPlayer2.vlc";
    
    let destinations = if primary_dest != fallback_dest {
        vec![primary_dest.as_str(), fallback_dest]
        } else {
        vec![primary_dest.as_str()]
    };
    
    for (i, mpris_dest) in destinations.iter().enumerate() {
        if i == 0 {
            eprintln!("[vlc-helper] Getting VLC status from: {} (primary)", mpris_dest);
        } else {
            eprintln!("[vlc-helper] Trying VLC fallback: {}", mpris_dest);
        }
        
        if let Some(status) = get_vlc_status_from_dest(mpris_dest) {
            eprintln!("[vlc-helper] Successfully connected to: {}", mpris_dest);
            return Some(status);
        }
    }
    
    eprintln!("[vlc-helper] All VLC MPRIS destinations failed");
    None
}

// Async variant for use inside async DBus handlers (avoids block_on re-entry)
async fn get_vlc_status_async() -> Option<MediaStatus> {
    let instance = get_current_media_player_instance()?;
    if instance.window_class != "vlc" {
        return None;
    }
    let primary_dest = &instance.mpris_name;
    let fallback_dest = "org.mpris.MediaPlayer2.vlc";
    let destinations = if primary_dest != fallback_dest {
        vec![primary_dest.as_str(), fallback_dest]
    } else {
        vec![primary_dest.as_str()]
    };
    for (i, mpris_dest) in destinations.iter().enumerate() {
        if i == 0 {
            eprintln!("[vlc-helper] Getting VLC status from: {} (primary, async)", mpris_dest);
        } else {
            eprintln!("[vlc-helper] Trying VLC fallback (async): {}", mpris_dest);
        }
        if let Some(status) = get_status_from_dest_native(mpris_dest).await {
            eprintln!("[vlc-helper] Successfully connected (async) to: {}", mpris_dest);
            return Some(status);
        }
    }
    eprintln!("[vlc-helper] All VLC MPRIS destinations failed (async)");
    None
}

fn execute_vlc_command(command: &str, args: &[&str]) -> bool {
    let instance = match get_current_media_player_instance() {
        Some(instance) => instance,
        None => {
            eprintln!("[vlc-helper] No VLC instance detected");
            return false;
        }
    };
    
    if instance.window_class != "vlc" {
        eprintln!("[vlc-helper] Instance is not VLC: {}", instance.window_class);
        return false;
    }
    
    let primary_dest = &instance.mpris_name;
    let fallback_dest = "org.mpris.MediaPlayer2.vlc";
    
    let destinations = if primary_dest != fallback_dest {
        vec![primary_dest.as_str(), fallback_dest]
    } else {
        vec![primary_dest.as_str()]
    };
    
    for (i, mpris_dest) in destinations.iter().enumerate() {
        if i == 0 {
            eprintln!("[vlc-helper] Executing VLC command '{}' on {} (primary, class: {}, PID: {:?})", 
                      command, mpris_dest, instance.window_class, instance.pid);
        } else {
            eprintln!("[vlc-helper] Trying VLC fallback: {}", mpris_dest);
        }
        
        if try_execute_command_on_destination(command, args, mpris_dest) {
            return true;
        }
    }
    
    false
}

// Dragon Player-specific functions
fn get_dragon_player_mpris_destination(_pid: Option<u32>) -> String {
    // Dragon Player typically exposes only the base MPRIS name; instance-specific names are not activatable
    eprintln!("[dragon-helper] Using Dragon Player base MPRIS name");
        "org.mpris.MediaPlayer2.dragonplayer".to_string()
}

fn is_dragon_player_running() -> bool {
    get_current_media_player_instance()
        .map(|instance| instance.window_class == "org.kde.dragonplayer" || instance.window_class == "dragonplayer")
        .unwrap_or(false)
}

fn get_dragon_player_status() -> Option<MediaStatus> {
    let instance = get_current_media_player_instance()?;
    if instance.window_class != "org.kde.dragonplayer" && instance.window_class != "dragonplayer" {
        return None;
    }
    
    let primary_dest = &instance.mpris_name;
    let fallback_dest = "org.mpris.MediaPlayer2.dragonplayer";
    
    // Prefer base name first; Dragon Player instance names often aren't activatable
    let destinations = if primary_dest != fallback_dest {
        vec![fallback_dest, primary_dest.as_str()]
    } else {
        vec![fallback_dest]
    };
    
    for (i, mpris_dest) in destinations.iter().enumerate() {
        if i == 0 {
            eprintln!("[dragon-helper] Getting Dragon Player status from: {} (primary)", mpris_dest);
        } else {
            eprintln!("[dragon-helper] Trying Dragon Player fallback: {}", mpris_dest);
        }
        
        if let Some(status) = get_dragon_player_status_from_dest(mpris_dest) {
            eprintln!("[dragon-helper] Successfully connected to: {}", mpris_dest);
            return Some(status);
        }
    }
    
    eprintln!("[dragon-helper] All Dragon Player MPRIS destinations failed");
    None
}

// Async variant for use inside async DBus handlers
async fn get_dragon_player_status_async() -> Option<MediaStatus> {
    let instance = get_current_media_player_instance()?;
    if instance.window_class != "org.kde.dragonplayer" && instance.window_class != "dragonplayer" {
        return None;
    }
    let primary_dest = &instance.mpris_name;
    let fallback_dest = "org.mpris.MediaPlayer2.dragonplayer";
    // Prefer base name first in async path as well
    let destinations = if primary_dest != fallback_dest {
        vec![fallback_dest, primary_dest.as_str()]
    } else {
        vec![fallback_dest]
    };
    for (i, mpris_dest) in destinations.iter().enumerate() {
        if i == 0 {
            eprintln!("[dragon-helper] Getting Dragon Player status from: {} (primary, async)", mpris_dest);
        } else {
            eprintln!("[dragon-helper] Trying Dragon Player fallback (async): {}", mpris_dest);
        }
        if let Some(status) = get_status_from_dest_native(mpris_dest).await {
            eprintln!("[dragon-helper] Successfully connected (async) to: {}", mpris_dest);
            return Some(status);
        }
    }
    eprintln!("[dragon-helper] All Dragon Player MPRIS destinations failed (async)");
    None
}

fn execute_dragon_player_command(command: &str, args: &[&str]) -> bool {
    let instance = match get_current_media_player_instance() {
        Some(instance) => instance,
        None => {
            eprintln!("[dragon-helper] No Dragon Player instance detected");
            return false;
        }
    };
    
    if instance.window_class != "org.kde.dragonplayer" && instance.window_class != "dragonplayer" {
        eprintln!("[dragon-helper] Instance is not Dragon Player: {}", instance.window_class);
        return false;
    }
    
    let primary_dest = &instance.mpris_name;
    let fallback_dest = "org.mpris.MediaPlayer2.dragonplayer";
    
    // Prefer base name first for commands too
    let destinations = if primary_dest != fallback_dest {
        vec![fallback_dest, primary_dest.as_str()]
    } else {
        vec![fallback_dest]
    };
    
    for (i, mpris_dest) in destinations.iter().enumerate() {
        if i == 0 {
            eprintln!("[dragon-helper] Executing Dragon Player command '{}' on {} (primary, class: {}, PID: {:?})", 
                      command, mpris_dest, instance.window_class, instance.pid);
        } else {
            eprintln!("[dragon-helper] Trying Dragon Player fallback: {}", mpris_dest);
        }
        
        if try_execute_command_on_destination(command, args, mpris_dest) {
            return true;
        }
    }
    
    false
}

// Generic functions that route to the appropriate player-specific function
fn get_mpris_destination(class: &str, pid: Option<u32>) -> String {
    eprintln!("[media-helper] get_mpris_destination called with class: '{}', pid: {:?}", class, pid);
    
    match class {
        "vlc" => get_vlc_mpris_destination(pid),
        "org.kde.dragonplayer" | "dragonplayer" => get_dragon_player_mpris_destination(pid),
        _ => {
            eprintln!("[media-helper] Unknown media player class: {}, using VLC fallback", class);
            get_vlc_mpris_destination(pid)
        }
    }
}



fn get_vlc_status_from_dest(mpris_dest: &str) -> Option<MediaStatus> {
    // Use native D-Bus implementation with Handle to current runtime
    eprintln!("[vlc-helper] Getting VLC status from DBus destination: {}", mpris_dest);
    
    // Always use shared runtime
    TOKIO_RT.block_on(get_status_from_dest_native(mpris_dest))
}



fn get_dragon_player_status_from_dest(mpris_dest: &str) -> Option<MediaStatus> {
    // Use native D-Bus implementation with Handle to current runtime
    eprintln!("[dragon-helper] Getting Dragon Player status from DBus destination: {}", mpris_dest);
    
    // Always use shared runtime
    TOKIO_RT.block_on(get_status_from_dest_native(mpris_dest))
}





fn try_execute_command_on_destination(command: &str, args: &[&str], mpris_dest: &str) -> bool {
    // Use native D-Bus implementation with Handle to current runtime
    TOKIO_RT.block_on(try_execute_command_on_destination_native(command, args, mpris_dest))
}



fn handle_command(command: &str, status_sender: &Arc<Mutex<Option<UnixStream>>>) {
    // Use OOP approach for command handling
    eprintln!("[OOP] Handling command '{}' using OOP approach", command);
    handle_current_player_command(command, status_sender);
}

fn handle_vlc_command(command: &str, status_sender: &Arc<Mutex<Option<UnixStream>>>) {
    // Command debouncing to prevent spam during fast movement
    static mut LAST_SEEK_TIME: Option<std::time::Instant> = None;
    static mut PENDING_SEEK: Option<f64> = None;
    
    const MIN_SEEK_INTERVAL: u64 = 150; // Minimum 150ms between seeks
    
    match command.trim() {
        "play_pause" => {
            eprintln!("[vlc-helper] Executing play/pause command");
            execute_vlc_command("org.mpris.MediaPlayer2.Player.PlayPause", &[]);
            // Immediate UI feedback
            if let Some(status) = get_vlc_status() {
                send_status_update(status_sender, &status);
            }
        }
        "play" => {
            eprintln!("[vlc-helper] Executing play command");
            execute_vlc_command("org.mpris.MediaPlayer2.Player.Play", &[]);
            if let Some(status) = get_vlc_status() {
                send_status_update(status_sender, &status);
            }
        }
        "pause" => {
            eprintln!("[vlc-helper] Executing pause command");
            execute_vlc_command("org.mpris.MediaPlayer2.Player.Pause", &[]);
            if let Some(status) = get_vlc_status() {
                send_status_update(status_sender, &status);
            }
        }
        "next" => {
            eprintln!("[vlc-helper] Executing next command");
            execute_vlc_command("org.mpris.MediaPlayer2.Player.Next", &[]);
        }
        "previous" => {
            eprintln!("[vlc-helper] Executing previous command");
            execute_vlc_command("org.mpris.MediaPlayer2.Player.Previous", &[]);
        }
        "stop" => {
            eprintln!("[vlc-helper] Executing stop command");
            execute_vlc_command("org.mpris.MediaPlayer2.Player.Stop", &[]);
            if let Some(status) = get_vlc_status() {
                send_status_update(status_sender, &status);
            }
        }
        "raise" => {
            eprintln!("[vlc-helper] Executing raise command");
            execute_vlc_command("org.mpris.MediaPlayer2.Raise", &[]);
        }
        "quit" => {
            eprintln!("[vlc-helper] Executing quit command");
            execute_vlc_command("org.mpris.MediaPlayer2.Quit", &[]);
        }
        cmd if cmd.starts_with("seek:") => {
            if let Some(position_str) = cmd.strip_prefix("seek:") {
                if let Ok(mut position) = position_str.parse::<f64>() {
                    // Prevent seeking to exactly 0.0 or 1.0 to avoid media player closing
                    if position <= 0.001 {
                        position = 0.001;
                    } else if position >= 0.999 {
                        position = 0.999;
                    }
                    
                    let now = std::time::Instant::now();
                    let can_seek = unsafe {
                        if let Some(last_seek) = LAST_SEEK_TIME {
                            now.duration_since(last_seek).as_millis() >= MIN_SEEK_INTERVAL as u128
                        } else {
                            true // First seek, always allow
                        }
                    };
                    
                    if can_seek {
                        // Execute seek immediately
                        unsafe {
                            LAST_SEEK_TIME = Some(now);
                            PENDING_SEEK = None; // Clear any pending seek
                        }
                        
                        eprintln!("[vlc-helper] Executing seek command to position: {} (fast mode, debounced)", position);
                        
                        // Use Seek method instead of SetPosition - more reliable
                        // First get current position and duration to calculate seek offset
                        if let Some(current_status) = get_vlc_status() {
                            let duration_microseconds = current_status.duration * 1_000_000;
                            let target_position_microseconds = (position * duration_microseconds as f64) as i64;
                            let current_position_microseconds = (current_status.position * duration_microseconds as f64) as i64;
                            let seek_offset = target_position_microseconds - current_position_microseconds;
                            
                            eprintln!("[vlc-helper] Seeking (VLC): current={}μs, target={}μs, offset={}μs", 
                                     current_position_microseconds, target_position_microseconds, seek_offset);
                            
                            // Execute seek command with offset
                            let success = execute_vlc_command("org.mpris.MediaPlayer2.Player.Seek", &[&format!("int64:{}", seek_offset)]);
                            
                            if success {
                                // IMMEDIATELY send status update to move the header
                                // This prevents the delay and makes the UI responsive
                                let mut updated_status = current_status;
                                updated_status.position = position;
                                
                                // Send immediate update to move header
                                send_status_update(status_sender, &updated_status);
                                eprintln!("[vlc-helper] Header updated immediately to position: {:.2}%", position * 100.0);
                            } else {
                                eprintln!("[vlc-helper] Seek command failed");
                            }
                        } else {
                            eprintln!("[vlc-helper] Failed to get current status for seek");
                        }
                    } else {
                        // Store this seek for later execution
                        unsafe {
                            PENDING_SEEK = Some(position);
                        }
                        eprintln!("[vlc-helper] Seek throttled: position {} (too soon after last seek, will execute later)", position);
                        
                        // Still update header immediately for visual feedback
                        if let Some(current_status) = get_vlc_status() {
                            let mut updated_status = current_status;
                            updated_status.position = position;
                            send_status_update(status_sender, &updated_status);
                            eprintln!("[vlc-helper] Header updated immediately (throttled seek: {:.2}%)", position * 100.0);
                        }
                    }
                } else {
                    eprintln!("[vlc-helper] Invalid seek position: {}", position_str);
                }
            }
        }
        cmd if cmd.starts_with("set_position:") => {
            if let Some(position_str) = cmd.strip_prefix("set_position:") {
                if let Ok(position) = position_str.parse::<f64>() {
                    eprintln!("[vlc-helper] Executing set position command to: {}", position);
                    
                    // Use Seek method instead of SetPosition - more reliable
                    if let Some(current_status) = get_vlc_status() {
                        let duration_microseconds = current_status.duration * 1_000_000;
                        let target_position_microseconds = (position * duration_microseconds as f64) as i64;
                        let current_position_microseconds = (current_status.position * duration_microseconds as f64) as i64;
                        let seek_offset = target_position_microseconds - current_position_microseconds;
                        
                        eprintln!("[vlc-helper] Set position (VLC): current={}μs, target={}μs, offset={}μs", 
                                 current_position_microseconds, target_position_microseconds, seek_offset);
                        
                        // Execute seek command with offset
                        let success = execute_vlc_command("org.mpris.MediaPlayer2.Player.Seek", &[&format!("int64:{}", seek_offset)]);
                        
                        if success {
                            // IMMEDIATELY send status update to move the header
                            let mut updated_status = current_status;
                            updated_status.position = position;
                            send_status_update(status_sender, &updated_status);
                            eprintln!("[vlc-helper] Header updated immediately to position: {:.2}%", position * 100.0);
                } else {
                            eprintln!("[vlc-helper] Set position command failed");
                        }
                    } else {
                        eprintln!("[vlc-helper] Failed to get current status for set position");
                    }
                } else {
                    eprintln!("[vlc-helper] Invalid set position: {}", position_str);
                }
            }
        }
        _ => {
            eprintln!("[vlc-helper] Unknown command: {}", command);
        }
    }
    
    // Process any pending seek if enough time has passed
    unsafe {
        if let Some(pending_position) = PENDING_SEEK {
            if let Some(last_seek) = LAST_SEEK_TIME {
                let now = std::time::Instant::now();
                if now.duration_since(last_seek).as_millis() >= MIN_SEEK_INTERVAL as u128 {
                    eprintln!("[vlc-helper] Processing pending seek to position: {}", pending_position);
                    
                    // Execute the pending seek
                    if let Some(current_status) = get_vlc_status() {
                        let duration_microseconds = current_status.duration * 1_000_000;
                        let target_position_microseconds = (pending_position * duration_microseconds as f64) as i64;
                        let current_position_microseconds = (current_status.position * duration_microseconds as f64) as i64;
                        let seek_offset = target_position_microseconds - current_position_microseconds;
                        
                        eprintln!("[vlc-helper] Executing pending seek (VLC): current={}μs, target={}μs, offset={}μs", 
                                 current_position_microseconds, target_position_microseconds, seek_offset);
                        
                        let success = execute_vlc_command("org.mpris.MediaPlayer2.Player.Seek", &[&format!("int64:{}", seek_offset)]);
                        
                        if success {
                            eprintln!("[vlc-helper] Pending seek executed successfully to position: {:.2}%", pending_position * 100.0);
                            LAST_SEEK_TIME = Some(now);
                            PENDING_SEEK = None;
                        } else {
                            eprintln!("[vlc-helper] Pending seek failed");
                        }
                    }
                }
            }
        }
    }
}

fn handle_dragon_player_command(command: &str, status_sender: &Arc<Mutex<Option<UnixStream>>>) {
    // Command debouncing to prevent spam during fast movement
    static mut LAST_SEEK_TIME: Option<std::time::Instant> = None;
    static mut PENDING_SEEK: Option<f64> = None;
    
    const MIN_SEEK_INTERVAL: u64 = 150; // Minimum 150ms between seeks
    
    match command.trim() {
        "play_pause" => {
            eprintln!("[dragon-helper] Executing play/pause command");
            execute_dragon_player_command("org.mpris.MediaPlayer2.Player.PlayPause", &[]);
            if let Some(status) = get_dragon_player_status() {
                send_status_update(status_sender, &status);
            }
        }
        "play" => {
            eprintln!("[dragon-helper] Executing play command");
            execute_dragon_player_command("org.mpris.MediaPlayer2.Player.Play", &[]);
            if let Some(status) = get_dragon_player_status() {
                send_status_update(status_sender, &status);
            }
        }
        "pause" => {
            eprintln!("[dragon-helper] Executing pause command");
            execute_dragon_player_command("org.mpris.MediaPlayer2.Player.Pause", &[]);
            if let Some(status) = get_dragon_player_status() {
                send_status_update(status_sender, &status);
            }
        }
        "next" => {
            eprintln!("[dragon-helper] Executing next command");
            execute_dragon_player_command("org.mpris.MediaPlayer2.Player.Next", &[]);
        }
        "previous" => {
            eprintln!("[dragon-helper] Executing previous command");
            execute_dragon_player_command("org.mpris.MediaPlayer2.Player.Previous", &[]);
        }
        "stop" => {
            eprintln!("[dragon-helper] Executing stop command");
            execute_dragon_player_command("org.mpris.MediaPlayer2.Player.Stop", &[]);
            if let Some(status) = get_dragon_player_status() {
                send_status_update(status_sender, &status);
            }
        }
        "raise" => {
            eprintln!("[dragon-helper] Executing raise command");
            execute_dragon_player_command("org.mpris.MediaPlayer2.Raise", &[]);
        }
        "quit" => {
            eprintln!("[dragon-helper] Executing quit command");
            execute_dragon_player_command("org.mpris.MediaPlayer2.Quit", &[]);
        }
        cmd if cmd.starts_with("seek:") => {
            if let Some(position_str) = cmd.strip_prefix("seek:") {
                if let Ok(mut position) = position_str.parse::<f64>() {
                    // Prevent seeking to exactly 0.0 or 1.0 to avoid media player closing
                    if position <= 0.001 {
                        position = 0.001;
                    } else if position >= 0.999 {
                        position = 0.999;
                    }
                    
                    let now = std::time::Instant::now();
                    let can_seek = unsafe {
                        if let Some(last_seek) = LAST_SEEK_TIME {
                            now.duration_since(last_seek).as_millis() >= MIN_SEEK_INTERVAL as u128
                        } else {
                            true // First seek, always allow
                        }
                    };
                    
                    if can_seek {
                        // Execute seek immediately
                        unsafe {
                            LAST_SEEK_TIME = Some(now);
                            PENDING_SEEK = None; // Clear any pending seek
                        }
                        
                        eprintln!("[dragon-helper] Executing seek command to position: {} (fast mode, debounced)", position);
                        
                        // Use Seek method instead of SetPosition - more reliable
                        // First get current position and duration to calculate seek offset
                        if let Some(current_status) = get_dragon_player_status() {
                            let duration_milliseconds = current_status.duration * 1_000;
                            let target_position_milliseconds = (position * duration_milliseconds as f64) as i64;
                            let current_position_milliseconds = (current_status.position * duration_milliseconds as f64) as i64;
                            let seek_offset = target_position_milliseconds - current_position_milliseconds;
                            
                            eprintln!("[dragon-helper] Seeking (Dragon Player): current={}ms, target={}ms, offset={}ms", 
                                     current_position_milliseconds, target_position_milliseconds, seek_offset);
                            
                            // Execute seek command with offset
                            let success = execute_dragon_player_command("org.mpris.MediaPlayer2.Player.Seek", &[&format!("int64:{}", seek_offset)]);
                            
                            if success {
                                // IMMEDIATELY send status update to move the header
                                // This prevents the delay and makes the UI responsive
                                let mut updated_status = current_status;
                                updated_status.position = position;
                                
                                // Send immediate update to move header
                                send_status_update(status_sender, &updated_status);
                                eprintln!("[dragon-helper] Header updated immediately to position: {:.2}%", position * 100.0);
                            } else {
                                eprintln!("[dragon-helper] Seek command failed");
                            }
                        } else {
                            eprintln!("[dragon-helper] Failed to get current status for seek");
                        }
                    } else {
                        // Store this seek for later execution
                        unsafe {
                            PENDING_SEEK = Some(position);
                        }
                        eprintln!("[dragon-helper] Seek throttled: position {} (too soon after last seek, will execute later)", position);
                        
                        // Still update header immediately for visual feedback
                        if let Some(current_status) = get_dragon_player_status() {
                            let mut updated_status = current_status;
                            updated_status.position = position;
                            send_status_update(status_sender, &updated_status);
                            eprintln!("[dragon-helper] Header updated immediately (throttled seek: {:.2}%)", position * 100.0);
                        }
                    }
                        } else {
                    eprintln!("[dragon-helper] Invalid seek position: {}", position_str);
                }
            }
        }
        cmd if cmd.starts_with("set_position:") => {
            if let Some(position_str) = cmd.strip_prefix("set_position:") {
                if let Ok(position) = position_str.parse::<f64>() {
                    eprintln!("[dragon-helper] Executing set position command to: {}", position);
                    
                    // Use Seek method instead of SetPosition - more reliable
                    if let Some(current_status) = get_dragon_player_status() {
                        let duration_milliseconds = current_status.duration * 1_000;
                        let target_position_milliseconds = (position * duration_milliseconds as f64) as i64;
                        let current_position_milliseconds = (current_status.position * duration_milliseconds as f64) as i64;
                        let seek_offset = target_position_milliseconds - current_position_milliseconds;
                        
                        eprintln!("[dragon-helper] Set position (Dragon Player): current={}ms, target={}ms, offset={}ms", 
                                 current_position_milliseconds, target_position_milliseconds, seek_offset);
                        
                        // Execute seek command with offset
                        let success = execute_dragon_player_command("org.mpris.MediaPlayer2.Player.Seek", &[&format!("int64:{}", seek_offset)]);
                        
                        if success {
                            // IMMEDIATELY send status update to move the header
                            let mut updated_status = current_status;
                            updated_status.position = position;
                            send_status_update(status_sender, &updated_status);
                            eprintln!("[dragon-helper] Header updated immediately to position: {:.2}%", position * 100.0);
                        } else {
                            eprintln!("[dragon-helper] Set position command failed");
                        }
                    } else {
                        eprintln!("[dragon-helper] Failed to get current status for set position");
                    }
                } else {
                    eprintln!("[dragon-helper] Invalid set position: {}", position_str);
                }
            }
        }
        _ => {
            eprintln!("[dragon-helper] Unknown command: {}", command);
        }
    }
    
    // Process any pending seek if enough time has passed
    unsafe {
        if let Some(pending_position) = PENDING_SEEK {
            if let Some(last_seek) = LAST_SEEK_TIME {
                let now = std::time::Instant::now();
                if now.duration_since(last_seek).as_millis() >= MIN_SEEK_INTERVAL as u128 {
                    eprintln!("[dragon-helper] Processing pending seek to position: {}", pending_position);
                    
                    // Execute the pending seek
                    if let Some(current_status) = get_dragon_player_status() {
                        let duration_milliseconds = current_status.duration * 1_000;
                        let target_position_milliseconds = (pending_position * duration_milliseconds as f64) as i64;
                        let current_position_milliseconds = (current_status.position * duration_milliseconds as f64) as i64;
                        let seek_offset = target_position_milliseconds - current_position_milliseconds;
                        
                        eprintln!("[dragon-helper] Executing pending seek (Dragon Player): current={}ms, target={}ms, offset={}ms", 
                                 current_position_milliseconds, target_position_milliseconds, seek_offset);
                        
                        let success = execute_dragon_player_command("org.mpris.MediaPlayer2.Player.Seek", &[&format!("int64:{}", seek_offset)]);
                        
                        if success {
                            eprintln!("[dragon-helper] Pending seek executed successfully to position: {:.2}%", pending_position * 100.0);
                            LAST_SEEK_TIME = Some(now);
                            PENDING_SEEK = None;
                        } else {
                            eprintln!("[dragon-helper] Pending seek failed");
                        }
                    }
                }
            }
        }
    }
}

fn monitor_vlc_events(status_sender: Arc<Mutex<Option<UnixStream>>>) {
    // VLC-specific event monitoring with position polling
    
    // Check initial VLC status
    if is_vlc_running() {
        if let Some(initial_status) = get_vlc_status() {
            send_status_update(&status_sender, &initial_status);
            eprintln!("[vlc] Initial status detected: playing={}, position={:.2}%", 
                     initial_status.is_playing, initial_status.position * 100.0);
        }
    }
    
    // Create a shared state for playback status to coordinate between threads
    let playback_state = Arc::new(Mutex::new((false, MediaStatus::empty())));
    let playback_state_clone = playback_state.clone();
    
    // Initialize shared state with current VLC status if available
    if let Some(current_status) = get_vlc_status() {
        if let Ok(mut state) = playback_state.lock() {
            state.0 = current_status.is_playing;
            state.1 = current_status.clone();
        }
        
        eprintln!("[vlc] Shared state initialized: playing={}, position={:.2}%", 
                 current_status.is_playing, current_status.position * 100.0);
    }
    
    // Clone status_sender for position updates
    let position_sender = status_sender.clone();
    
    // Start VLC position polling thread - VLC needs polling for smooth updates
    thread::spawn(move || {
        loop {
            // Check if currently playing from shared state
            let is_playing = {
                if let Ok(state) = playback_state_clone.lock() {
                    state.0
            } else {
                    false
                }
            };
            
            if is_playing {
                // Get VLC position using polling
                if let Some(status) = get_vlc_status() {
                    if status.is_playing && status.duration > 0 {
                        send_status_update(&position_sender, &status);
                        eprintln!("[vlc] Position polling update: {:.2}%", status.position * 100.0);
                    }
                }
                
                // Poll every 100ms for VLC smooth progress updates
                thread::sleep(Duration::from_millis(100));
            } else {
                // Not playing - sleep longer and wait for events
            thread::sleep(Duration::from_millis(500));
            }
        }
    });
    
    // Start VLC-specific DBus event monitoring
    let status_sender_clone = status_sender.clone();
    let playback_state_clone = playback_state.clone();
    thread::spawn(move || {
        let result = run_vlc_dbus_event_monitor(status_sender_clone.clone(), playback_state_clone.clone());
        
        if let Err(e) = result {
            eprintln!("[vlc-helper] VLC DBus event monitor failed: {}, restarting...", e);
            thread::sleep(Duration::from_millis(1000));
            monitor_vlc_events(status_sender);
        }
    });
}

fn monitor_dragon_player_events(status_sender: Arc<Mutex<Option<UnixStream>>>) {
    // Dragon Player-specific event monitoring - fully event-driven, no polling needed
    
    // Check initial Dragon Player status
    if is_dragon_player_running() {
        if let Some(initial_status) = get_dragon_player_status() {
            send_status_update(&status_sender, &initial_status);
            eprintln!("[dragon-helper] Initial status detected: playing={}, position={:.2}%", 
                     initial_status.is_playing, initial_status.position * 100.0);
        }
    }
    
    // Create a shared state for playback status to coordinate between threads
    let playback_state = Arc::new(Mutex::new((false, MediaStatus::empty())));
    
    // Initialize shared state with current Dragon Player status if available
    if let Some(current_status) = get_dragon_player_status() {
        if let Ok(mut state) = playback_state.lock() {
            state.0 = current_status.is_playing;
            state.1 = current_status.clone();
        }
        
        eprintln!("[dragon-helper] Shared state initialized: playing={}, position={:.2}%", 
                 current_status.is_playing, current_status.position * 100.0);
    }
    
    // Start Dragon Player-specific DBus event monitoring (no polling thread needed)
    let status_sender_clone = status_sender.clone();
    let playback_state_clone = playback_state.clone();
    thread::spawn(move || {
        let result = run_dragon_player_dbus_event_monitor(status_sender_clone.clone(), playback_state_clone.clone());
        
        if let Err(e) = result {
            eprintln!("[dragon-helper] Dragon Player DBus event monitor failed: {}, restarting...", e);
            thread::sleep(Duration::from_millis(1000));
            monitor_dragon_player_events(status_sender);
        }
    });
}

fn monitor_media_player_events(status_sender: Arc<Mutex<Option<UnixStream>>>) {
    // Use OOP approach for monitoring events
    eprintln!("[OOP] Starting media player event monitoring using OOP approach");
    monitor_current_player_events(status_sender);
}



fn run_vlc_dbus_event_monitor(
    status_sender: Arc<Mutex<Option<UnixStream>>>, 
    playback_state: Arc<Mutex<(bool, MediaStatus)>>
) -> Result<(), Box<dyn std::error::Error>> {
    // Use shared async runtime for zbus
    TOKIO_RT.block_on(async {
        let connection = Connection::session().await?;
        
        // Initialize shared connection for native D-Bus calls
        unsafe {
            SHARED_DBUS_CONNECTION = Some(connection.clone());
        }
        let mut stream = MessageStream::from(&connection);
        let dbus_proxy = DBusProxy::new(&connection).await?;
        
        // Subscribe to MPRIS signals specifically for VLC
        let rules = vec![
            // PropertiesChanged signals for playback status, position, metadata
            MatchRule::builder()
                .msg_type(MessageType::Signal)
                .interface("org.freedesktop.DBus.Properties")?
                .path_namespace("/org/mpris/MediaPlayer2")?
                .member("PropertiesChanged")?
                .build(),
            // Seeked signals for position changes
            MatchRule::builder()
                .msg_type(MessageType::Signal)
                .interface("org.mpris.MediaPlayer2.Player")?
                .member("Seeked")?
                .build(),
            // NameOwnerChanged signals for VLC service appearance/disappearance
            MatchRule::builder()
                .msg_type(MessageType::Signal)
                .interface("org.freedesktop.DBus")?
                .member("NameOwnerChanged")?
                .arg0ns("org.mpris.MediaPlayer2.vlc")?
                .build(),
        ];
        
        // Add all match rules
        for rule in rules {
            if let Err(e) = dbus_proxy.add_match_rule(rule).await {
                eprintln!("[vlc-helper] Failed to add VLC match rule: {}", e);
            }
        }
        
        eprintln!("[vlc-helper] VLC-specific DBus signal subscription active");
        
        // Process incoming DBus messages
        while let Some(msg) = stream.next().await {
            let msg = msg?;
            let header = msg.header()?;
            
            // Only process signal messages
            if msg.message_type() != MessageType::Signal {
                continue;
            }
            
            if let (Some(interface), Some(member)) = (header.interface()?, header.member()?) {
                let interface_str = interface.as_str();
                let member_str = member.as_str();
                
                eprintln!("[vlc-helper] Received VLC DBus signal: {}.{}", interface_str, member_str);
                
                match (interface_str, member_str) {
                    ("org.freedesktop.DBus.Properties", "PropertiesChanged") => {
                        // Handle PropertiesChanged signal for VLC
                        if let Ok((interface_name, changed_props, _invalidated_props)) = 
                            msg.body::<(String, std::collections::HashMap<String, zbus::zvariant::Value>, Vec<String>)>() {
                            
                            if interface_name == "org.mpris.MediaPlayer2.Player" {
                                eprintln!("[vlc-helper] VLC player properties changed: {:?}", changed_props);
                                // Process the changed properties for VLC
                                process_vlc_properties_changed_signal_dbus(changed_props, &status_sender, &playback_state).await;
                            }
                        }
                    }
                    ("org.mpris.MediaPlayer2.Player", "Seeked") => {
                        // Handle Seeked signal for VLC
                        if let Ok(position) = msg.body::<i64>() {
                            eprintln!("[vlc-helper] VLC seeked to position: {} microseconds", position);
                            process_vlc_seeked_signal_dbus(position, &status_sender, &playback_state).await;
                        }
                    }
                    ("org.freedesktop.DBus", "NameOwnerChanged") => {
                        // Handle NameOwnerChanged signal for VLC
                        if let Ok((name, old_owner, new_owner)) = 
                            msg.body::<(String, String, String)>() {
                            
                            if name.starts_with("org.mpris.MediaPlayer2.vlc") {
                                eprintln!("[vlc-helper] VLC service changed: {} (old: {}, new: {})", name, old_owner, new_owner);
                                process_vlc_name_owner_changed_signal_dbus(&name, &old_owner, &new_owner, &status_sender, &playback_state).await;
                            }
                        }
                    }
                    _ => {
                        // Other signals - log for debugging
                        if let Ok(body) = msg.body::<String>() {
                            eprintln!("[vlc-helper] Unhandled VLC signal: {}.{} - {}", interface_str, member_str, body);
                        }
                    }
                }
            }
        }
        
        Ok::<(), zbus::Error>(())
    })?;
    
    Ok(())
}

fn run_dragon_player_dbus_event_monitor(
    status_sender: Arc<Mutex<Option<UnixStream>>>, 
    playback_state: Arc<Mutex<(bool, MediaStatus)>>
) -> Result<(), Box<dyn std::error::Error>> {
    // Use shared async runtime for zbus
    TOKIO_RT.block_on(async {
        let connection = Connection::session().await?;
        
        // Initialize shared connection for native D-Bus calls
        unsafe {
            if SHARED_DBUS_CONNECTION.is_none() {
                SHARED_DBUS_CONNECTION = Some(connection.clone());
            }
        }
        let mut stream = MessageStream::from(&connection);
        let dbus_proxy = DBusProxy::new(&connection).await?;
        
        // Subscribe to MPRIS signals specifically for Dragon Player
        let rules = vec![
            // PropertiesChanged signals for playback status, position, metadata
            MatchRule::builder()
                .msg_type(MessageType::Signal)
                .interface("org.freedesktop.DBus.Properties")?
                .path_namespace("/org/mpris/MediaPlayer2")?
                .member("PropertiesChanged")?
                .build(),
            // Seeked signals for position changes
            MatchRule::builder()
                .msg_type(MessageType::Signal)
                .interface("org.mpris.MediaPlayer2.Player")?
                .member("Seeked")?
                .build(),
            // NameOwnerChanged signals for Dragon Player service appearance/disappearance
            MatchRule::builder()
                .msg_type(MessageType::Signal)
                .interface("org.freedesktop.DBus")?
                .member("NameOwnerChanged")?
                .arg0ns("org.mpris.MediaPlayer2.dragonplayer")?
                .build(),
        ];
        
        // Add all match rules
        for rule in rules {
            if let Err(e) = dbus_proxy.add_match_rule(rule).await {
                eprintln!("[dragon-helper] Failed to add Dragon Player match rule: {}", e);
            }
        }
        
        eprintln!("[dragon-helper] Dragon Player-specific DBus signal subscription active");
        
        // Process incoming DBus messages
        while let Some(msg) = stream.next().await {
            let msg = msg?;
            let header = msg.header()?;
            
            // Only process signal messages
            if msg.message_type() != MessageType::Signal {
                continue;
            }
            
            if let (Some(interface), Some(member)) = (header.interface()?, header.member()?) {
                let interface_str = interface.as_str();
                let member_str = member.as_str();
                
                eprintln!("[dragon-helper] Received Dragon Player DBus signal: {}.{}", interface_str, member_str);
                
                match (interface_str, member_str) {
                    ("org.freedesktop.DBus.Properties", "PropertiesChanged") => {
                        // Handle PropertiesChanged signal for Dragon Player
                        if let Ok((interface_name, changed_props, _invalidated_props)) = 
                            msg.body::<(String, std::collections::HashMap<String, zbus::zvariant::Value>, Vec<String>)>() {
                            
                            if interface_name == "org.mpris.MediaPlayer2.Player" {
                                eprintln!("[dragon-helper] Dragon Player properties changed: {:?}", changed_props);
                                // Process the changed properties for Dragon Player
                                process_dragon_player_properties_changed_signal_dbus(changed_props, &status_sender, &playback_state).await;
                            }
                        }
                    }
                    ("org.mpris.MediaPlayer2.Player", "Seeked") => {
                        // Handle Seeked signal for Dragon Player
                        if let Ok(position) = msg.body::<i64>() {
                            eprintln!("[dragon-helper] Dragon Player seeked to position: {} microseconds", position);
                            process_dragon_player_seeked_signal_dbus(position, &status_sender, &playback_state).await;
                        }
                    }
                    ("org.freedesktop.DBus", "NameOwnerChanged") => {
                        // Handle NameOwnerChanged signal for Dragon Player
                        if let Ok((name, old_owner, new_owner)) = 
                            msg.body::<(String, String, String)>() {
                            
                            if name.starts_with("org.mpris.MediaPlayer2.dragonplayer") {
                                eprintln!("[dragon-helper] Dragon Player service changed: {} (old: {}, new: {})", name, old_owner, new_owner);
                                process_dragon_player_name_owner_changed_signal_dbus(&name, &old_owner, &new_owner, &status_sender, &playback_state).await;
                            }
                        }
                    }
                    _ => {
                        // Other signals - log for debugging
                        if let Ok(body) = msg.body::<String>() {
                            eprintln!("[dragon-helper] Unhandled Dragon Player signal: {}.{} - {}", interface_str, member_str, body);
                        }
                    }
                }
            }
        }
        
        Ok::<(), zbus::Error>(())
    })?;
    
    Ok(())
}

async fn process_vlc_properties_changed_signal_dbus(
    changed_props: std::collections::HashMap<String, zbus::zvariant::Value<'_>>, 
    status_sender: &Arc<Mutex<Option<UnixStream>>>,
    playback_state: &Arc<Mutex<(bool, MediaStatus)>>
) {
    // Process changed properties from DBus signal for VLC
    for (prop_name, prop_value) in changed_props {
        match prop_name.as_str() {
            "PlaybackStatus" => {
                if let Some(status_str) = prop_value.downcast::<String>() {
                    let is_playing = status_str == "Playing";
                    eprintln!("[vlc-helper] VLC playback status changed to: {}", status_str);
                    
                    // Get current VLC status and update
                    if let Some(mut status) = get_vlc_status_async().await {
                        status.is_playing = is_playing;
                        
                        // Update shared playback state
                        if let Ok(mut state) = playback_state.lock() {
                            state.0 = is_playing;
                            state.1 = status.clone();
                        }
                        
                        send_status_update(status_sender, &status);
                        
                        if is_playing {
                            eprintln!("[vlc-helper] VLC playback started - position polling activated");
                        } else {
                            eprintln!("[vlc-helper] VLC playback stopped - position polling deactivated");
                        }
                    }
                }
            }
            "Position" => {
                if let Some(position) = prop_value.downcast::<i64>() {
                    eprintln!("[vlc-helper] VLC position changed to: {} microseconds", position);
                    
                    // Get current VLC status and update position immediately
                    if let Some(mut status) = get_vlc_status_async().await {
                        let duration = status.duration * 1_000_000; // Convert to microseconds
                        status.position = if duration > 0 { position as f64 / duration as f64 } else { 0.0 };
                        
                        // Update shared playback state
                        if let Ok(mut state) = playback_state.lock() {
                            state.1 = status.clone();
                        }
                        
                        // Send immediate update for instant header movement
                        send_status_update(status_sender, &status);
                        eprintln!("[vlc-helper] VLC position updated via DBus signal: {:.2}% (immediate)", status.position * 100.0);
                    }
                }
            }
            "Metadata" => {
                eprintln!("[vlc-helper] VLC metadata changed");
                
                // Get updated VLC status with new metadata
                if let Some(status) = get_vlc_status_async().await {
                    // Update shared playback state
                    if let Ok(mut state) = playback_state.lock() {
                        state.1 = status.clone();
                    }
                    
                    send_status_update(status_sender, &status);
                }
            }
            _ => {
                eprintln!("[vlc-helper] VLC property changed: {} = {:?}", prop_name, prop_value);
            }
        }
    }
}

async fn process_vlc_seeked_signal_dbus(
    position: i64, 
    status_sender: &Arc<Mutex<Option<UnixStream>>>,
    playback_state: &Arc<Mutex<(bool, MediaStatus)>>
) {
    // Process seeked signal from DBus for VLC - use the position directly from the signal
    // This avoids an extra DBus call and makes the response immediate
    
    // Get current VLC status to get duration and other metadata
    if let Some(mut status) = get_vlc_status_async().await {
        let duration = status.duration * 1_000_000; // Convert to microseconds
        status.position = if duration > 0 { position as f64 / duration as f64 } else { 0.0 };
        
        // Update shared playback state
        if let Ok(mut state) = playback_state.lock() {
            state.1 = status.clone();
        }
        
        eprintln!("[vlc-helper] VLC seeked to position: {:.2}% (from DBus signal)", status.position * 100.0);
        send_status_update(status_sender, &status);
    }
}

async fn process_vlc_name_owner_changed_signal_dbus(
    name: &str, 
    _old_owner: &str, 
    new_owner: &str, 
    status_sender: &Arc<Mutex<Option<UnixStream>>>,
    playback_state: &Arc<Mutex<(bool, MediaStatus)>>
) {
    // Process name owner changed signal from DBus for VLC
    let vlc_running = !new_owner.is_empty();
    
    if vlc_running {
        eprintln!("[vlc-helper] VLC service appeared: {}", name);
        // Get initial VLC status
        if let Some(status) = get_vlc_status_async().await {
            // Update shared playback state
            if let Ok(mut state) = playback_state.lock() {
                state.0 = status.is_playing;
                state.1 = status.clone();
            }
            
            send_status_update(status_sender, &status);
        }
    } else {
        eprintln!("[vlc-helper] VLC service disappeared: {}", name);
        // Send empty status and update shared state
        let empty_status = MediaStatus::empty();
        
        if let Ok(mut state) = playback_state.lock() {
            state.0 = false;
            state.1 = empty_status.clone();
        }
        
        send_status_update(status_sender, &empty_status);
    }
}

async fn process_dragon_player_properties_changed_signal_dbus(
    changed_props: std::collections::HashMap<String, zbus::zvariant::Value<'_>>, 
    status_sender: &Arc<Mutex<Option<UnixStream>>>,
    playback_state: &Arc<Mutex<(bool, MediaStatus)>>
) {
    // Process changed properties from DBus signal for Dragon Player
    for (prop_name, prop_value) in changed_props {
        match prop_name.as_str() {
            "PlaybackStatus" => {
                if let Some(status_str) = prop_value.downcast::<String>() {
                    let is_playing = status_str == "Playing";
                    eprintln!("[dragon-helper] Dragon Player playback status changed to: {}", status_str);
                    
                    // Get current Dragon Player status and update
                    if let Some(mut status) = get_dragon_player_status_async().await {
                        status.is_playing = is_playing;
                        
                        // Update shared playback state
                        if let Ok(mut state) = playback_state.lock() {
                            state.0 = is_playing;
                            state.1 = status.clone();
                        }
                        
                        send_status_update(status_sender, &status);
                        
                        if is_playing {
                            eprintln!("[dragon-helper] Dragon Player playback started - position polling activated");
                        } else {
                            eprintln!("[dragon-helper] Dragon Player playback stopped - position polling deactivated");
                        }
                    }
                }
            }
            "Position" => {
                if let Some(position) = prop_value.downcast::<i64>() {
                    eprintln!("[dragon-helper] Dragon Player position changed to: {} milliseconds", position);
                    
                    // Get current Dragon Player status and update position immediately
                    if let Some(mut status) = get_dragon_player_status() {
                        let duration = status.duration * 1_000; // Convert to milliseconds
                        status.position = if duration > 0 { position as f64 / duration as f64 } else { 0.0 };
                        
                        // Update shared playback state
                        if let Ok(mut state) = playback_state.lock() {
                            state.1 = status.clone();
                        }
                        
                        // Send immediate update for instant header movement
                        send_status_update(status_sender, &status);
                        eprintln!("[dragon-helper] Dragon Player position updated via DBus signal: {:.2}% (immediate)", status.position * 100.0);
                    }
                }
            }
            "Metadata" => {
                eprintln!("[dragon-helper] Dragon Player metadata changed");
                
                // Get updated Dragon Player status with new metadata
                if let Some(status) = get_dragon_player_status_async().await {
                    // Update shared playback state
                    if let Ok(mut state) = playback_state.lock() {
                        state.1 = status.clone();
                    }
                    
                    send_status_update(status_sender, &status);
                }
            }
            _ => {
                eprintln!("[dragon-helper] Dragon Player property changed: {} = {:?}", prop_name, prop_value);
            }
        }
    }
}

async fn process_dragon_player_seeked_signal_dbus(
    position: i64, 
    status_sender: &Arc<Mutex<Option<UnixStream>>>,
    playback_state: &Arc<Mutex<(bool, MediaStatus)>>
) {
    // Process seeked signal from DBus for Dragon Player - use the position directly from the signal
    // This avoids an extra DBus call and makes the response immediate
    
    // Get current Dragon Player status to get duration and other metadata
    if let Some(mut status) = get_dragon_player_status_async().await {
        let duration = status.duration * 1_000; // Dragon Player uses milliseconds, convert to milliseconds
        status.position = if duration > 0 { position as f64 / duration as f64 } else { 0.0 };
        
        // Update shared playback state
        if let Ok(mut state) = playback_state.lock() {
            state.1 = status.clone();
        }
        
        eprintln!("[dragon-helper] Dragon Player seeked to position: {:.2}% (from DBus signal)", status.position * 100.0);
        send_status_update(status_sender, &status);
    }
}

async fn process_dragon_player_name_owner_changed_signal_dbus(
    name: &str, 
    _old_owner: &str, 
    new_owner: &str, 
    status_sender: &Arc<Mutex<Option<UnixStream>>>,
    playback_state: &Arc<Mutex<(bool, MediaStatus)>>
) {
    // Process name owner changed signal from DBus for Dragon Player
    let dragon_player_running = !new_owner.is_empty();
    
    if dragon_player_running {
        eprintln!("[dragon-helper] Dragon Player service appeared: {}", name);
        // Get initial Dragon Player status
        if let Some(status) = get_dragon_player_status_async().await {
            // Update shared playback state
            if let Ok(mut state) = playback_state.lock() {
                state.0 = status.is_playing;
                state.1 = status.clone();
            }
            
            send_status_update(status_sender, &status);
        }
    } else {
        eprintln!("[dragon-helper] Dragon Player service disappeared: {}", name);
        // Send empty status and update shared state
        let empty_status = MediaStatus::empty();
        
        if let Ok(mut state) = playback_state.lock() {
            state.0 = false;
            state.1 = empty_status.clone();
        }
        
        send_status_update(status_sender, &empty_status);
    }
}

fn send_status_update(status_sender: &Arc<Mutex<Option<UnixStream>>>, status: &MediaStatus) {
    if let Ok(mut sender_guard) = status_sender.lock() {
        if let Some(ref mut stream) = *sender_guard {
            let status_json = json!({
                "is_playing": status.is_playing,
                "position": status.position,
                "duration": status.duration
            });
            
            if let Err(e) = stream.write_all(format!("{}\n", status_json.to_string()).as_bytes()) {
                eprintln!("[vlc-helper] Failed to send status update: {}", e);
            }
        }
    }
}

fn main() -> std::io::Result<()> {
    let socket_path = "/tmp/touchbar-media.sock";
    
    // Print environment info for debugging
    if let Ok(addr) = std::env::var("DBUS_SESSION_BUS_ADDRESS") {
        eprintln!("[vlc-helper] DBUS_SESSION_BUS_ADDRESS={}", addr);
    } else {
        eprintln!("[vlc-helper] DBUS_SESSION_BUS_ADDRESS is not set");
    }
    
    // Get the window class, ID, and PID from environment variables
    if let Ok(window_class) = std::env::var("TINY_DFR_WINDOW_CLASS") {
        eprintln!("[vlc-helper] Window class: {}", window_class);
        
        // Get window PID for instance matching
        let window_pid = std::env::var("TINY_DFR_WINDOW_PID")
            .ok()
            .and_then(|pid_str| pid_str.parse::<u32>().ok());
        
        if let Some(pid) = window_pid {
            eprintln!("[vlc-helper] Window PID: {}", pid);
        }
        
        set_current_media_player(&window_class, window_pid);
        
        // Also read window ID for future use
        if let Ok(window_id_str) = std::env::var("TINY_DFR_WINDOW_ID") {
            if let Ok(window_id) = window_id_str.parse::<u64>() {
                eprintln!("[vlc-helper] Window ID: {}", window_id);
                // Store window ID for future use (you can add a global variable here if needed)
            } else {
                eprintln!("[vlc-helper] Invalid window ID format: {}", window_id_str);
            }
        } else {
            eprintln!("[vlc-helper] TINY_DFR_WINDOW_ID is not set");
        }
    } else {
        eprintln!("[vlc-helper] TINY_DFR_WINDOW_CLASS is not set");
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
    
    eprintln!("[vlc-helper] Connected to socket, starting VLC monitoring...");
    
    // Create a reader for incoming commands
    let mut stream_clone = stream.try_clone()?;
    let mut buffer = Vec::new();
    
    // Create a shared sender for status updates
    let status_sender = Arc::new(Mutex::new(Some(stream)));
    
    // Start event monitoring in a separate thread
    let status_sender_clone = status_sender.clone();
    thread::spawn(move || {
        monitor_media_player_events(status_sender_clone);
    });
    
    loop {
        // Event-driven command processing (non-blocking)
        let mut temp_buffer = [0u8; 1024];
        match stream_clone.read(&mut temp_buffer) {
            Ok(0) => {
                // EOF - connection closed
                eprintln!("[vlc-helper] Connection closed by main process");
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
                        eprintln!("[vlc-helper] Received command: {}", command);
                        // Execute command immediately (event-driven)
                        handle_command(command, &status_sender);
                    }
                }
            }
            Err(e) => {
                if e.kind() == std::io::ErrorKind::WouldBlock {
                    // No data available - add small sleep to prevent busy-waiting
                    // This prevents 100% CPU usage and freezing when switching modules
                    thread::sleep(Duration::from_millis(1));
                    continue;
                } else {
                    eprintln!("[vlc-helper] Error reading from socket: {}", e);
                    break;
                }
            }
        }
    }
    
    Ok(())
} 