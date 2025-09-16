use std::collections::HashMap;
use std::fs::{File, OpenOptions};
use std::os::fd::{AsFd, OwnedFd, AsRawFd, FromRawFd, IntoRawFd};
use std::os::unix::net::UnixStream;
use std::os::unix::fs::OpenOptionsExt;
use std::cmp::min;
use std::path::Path;

use anyhow::Result;
use cairo::{Format, ImageSurface};
use drm::control::ClipRect;
use input::{
    event::{
        device::DeviceEvent,
        keyboard::{KeyState, KeyboardEvent, KeyboardEventTrait},
        touch::{TouchEvent, TouchEventPosition, TouchEventSlot},
        Event, EventTrait,
    },
    Device as InputDevice, Libinput, LibinputInterface,
};
use input_linux::{uinput::UInputHandle, EventKind, Key};
use input_linux_sys::{input_id, uinput_setup};
use libc::{c_char, O_ACCMODE, O_RDONLY, O_RDWR, O_WRONLY};
use nix::{
    sys::eventfd::{eventfd, EfdFlags},
    sys::{
        epoll::{Epoll, EpollCreateFlags, EpollEvent, EpollFlags},
        signal::{SaFlags, SigAction, SigHandler, SigSet, Signal},
    },
};
use std::io::{BufReader, BufRead, Read, Write};
use std::sync::{Arc, atomic::{AtomicU64, Ordering}};

use chrono::{Local, Timelike};
use crate::services::sessionmanager::{SessionState, monitor_sessions};
use tokio::sync::{watch, mpsc};

use view::app_ui_manager::{AppUiManager, is_media_player_window_class, is_browser_window_class};


// Import the utils module
mod utils;
mod layers;
mod input_events;
use crate::layers::{Button, FunctionLayer};

use crate::input_events::{KeyboardEventHandler, TouchEventHandler};

// Error handling types and functions
#[derive(Debug, thiserror::Error)]
pub enum MainError {
    #[error("DRM error: {0}")]
    Drm(#[from] anyhow::Error),
    #[error("Epoll error: {0}")]
    Epoll(#[from] nix::Error),
    #[error("Input error: {0}")]
    Input(#[from] std::io::Error),
    #[error("Cairo error: {0}")]
    Cairo(String),
    #[error("Configuration error: {0}")]
    Config(String),
    #[error("Helper error: {0}")]
    Helper(String),
    #[error("Display error: {0}")]
    Display(String),
    #[error("Layer not found: {0:?}")]
    LayerNotFound(LayerKey),
    #[error("Touch slot not found: {0}")]
    TouchSlotNotFound(u32),
    #[error("Pending layer not found")]
    PendingLayerNotFound,
    #[error("Function layer error: {0}")]
    FunctionLayer(#[from] crate::layers::function_layer::FunctionLayerError),
}

type MainResult<T> = Result<T, MainError>;

// Forward declarations for types used in error handling
#[derive(Hash, Eq, PartialEq, Clone, Copy, Debug)]
pub enum LayerKey {
    Media,
    Fn,
    Custom2,
    Custom3,
}

// Error handling helper functions
fn safe_epoll_add(epoll: &Epoll, fd: &dyn AsFd, event: EpollEvent) -> MainResult<()> {
    epoll.add(fd, event)
        .map_err(|e| MainError::Epoll(e))
        .map_err(|e| {
            log_error("epoll", "add fd", &e.to_string());
            e
        })
}

fn safe_epoll_delete(epoll: &Epoll, fd: &dyn AsFd) -> MainResult<()> {
    epoll.delete(fd)
        .map_err(|e| MainError::Epoll(e))
        .map_err(|e| {
            log_error("epoll", "remove fd", &e.to_string());
            e
        })
}



fn safe_surface_data(surface: &mut ImageSurface) -> MainResult<Vec<u8>> {
    surface.data()
        .map_err(|e| MainError::Cairo(format!("Failed to get surface data: {}", e)))
        .map(|data| data.to_vec())
}

fn safe_drm_map(drm: &mut DrmBackend) -> MainResult<DumbMapping> {
    drm.map()
        .map_err(|e| MainError::Drm(e.into()))
}

fn safe_drm_dirty(drm: &mut DrmBackend, clips: &[ClipRect]) -> MainResult<()> {
    drm.dirty(clips)
        .map_err(|e| MainError::Drm(e.into()))
}

fn safe_stream_try_clone(stream: &UnixStream) -> MainResult<UnixStream> {
    stream.try_clone()
        .map_err(|e| MainError::Input(e))
}

fn safe_stream_set_nonblocking(stream: &UnixStream, nonblocking: bool) -> MainResult<()> {
    stream.set_nonblocking(nonblocking)
        .map_err(|e| MainError::Input(e))
}

fn safe_epoll_wait(epoll: &Epoll, events: &mut [EpollEvent], timeout: isize) -> MainResult<usize> {
    epoll.wait(events, timeout)
        .map_err(|e| MainError::Epoll(e))
}

fn safe_input_dispatch(input: &mut Libinput) -> MainResult<()> {
    input.dispatch()
        .map_err(|e| MainError::Input(e))
}

// Helper function for consistent error logging
fn log_error(component: &str, operation: &str, error: &str) {
    eprintln!("[main] {} {} failed: {}", component, operation, error);
}

fn log_warning(component: &str, message: &str) {
    eprintln!("[main] {} warning: {}", component, message);
}

// Helper function for safe layer access
fn get_active_layer_mut(layers: &mut HashMap<LayerKey, FunctionLayer>, active_layer: LayerKey) -> MainResult<&mut FunctionLayer> {
    layers.get_mut(&active_layer)
        .ok_or_else(|| MainError::LayerNotFound(active_layer))
}

fn get_active_layer(layers: &HashMap<LayerKey, FunctionLayer>, active_layer: LayerKey) -> MainResult<&FunctionLayer> {
    layers.get(&active_layer)
        .ok_or_else(|| MainError::LayerNotFound(active_layer))
}

// Helper function for safe touch slot access
fn get_touch_slot<'a>(touches: &'a HashMap<u32, (LayerKey, &'static str, usize)>, slot: u32) -> MainResult<&'a (LayerKey, &'static str, usize)> {
    touches.get(&slot)
        .ok_or_else(|| MainError::TouchSlotNotFound(slot))
}

// Helper function for safe pending layer access
fn get_pending_layer(pending_layer: &mut Option<LayerKey>) -> MainResult<LayerKey> {
    pending_layer.take()
        .ok_or(MainError::PendingLayerNotFound)
}

// Helper function to initialize uinput device
fn initialize_uinput(layers: &HashMap<LayerKey, FunctionLayer>) -> MainResult<UInputHandle<File>> {
    let uinput_file = OpenOptions::new()
        .write(true)
        .open("/dev/uinput")
        .map_err(|e| MainError::Input(e))?;
    let uinput = UInputHandle::new(uinput_file);
    
    uinput.set_evbit(EventKind::Key).map_err(|e| MainError::Input(std::io::Error::new(std::io::ErrorKind::Other, e)))?;
    
    // Register all button actions from layers
    for layer in layers.values() {
        // Register buttons from regular layer.buttons
        for button in &layer.buttons {
            uinput.set_keybit(button.action).map_err(|e| MainError::Input(std::io::Error::new(std::io::ErrorKind::Other, e)))?;
        }
        // Register buttons from split layout media section
        if let Some(split) = &layer.split {
            for button in &split.media {
                uinput.set_keybit(button.action).map_err(|e| MainError::Input(std::io::Error::new(std::io::ErrorKind::Other, e)))?;
            }
        }
    }
    
    // Register browser keys that we need for browser control
    let browser_keys = vec![
        Key::LeftCtrl, Key::RightCtrl, Key::LeftAlt, Key::RightAlt,
        Key::LeftShift, Key::RightShift, Key::Left, Key::Right,
        Key::R, Key::T, Key::L, Key::W, Key::N, Key::F4, Key::F6,
        Key::A, Key::B, Key::C, Key::D, Key::E, Key::F, Key::G, Key::H, Key::I, Key::J,
        Key::K, Key::M, Key::O, Key::P, Key::Q, Key::S, Key::U, Key::V, Key::X, Key::Y, Key::Z,
        Key::Num1, Key::Num2, Key::Num3, Key::Num4, Key::Num5, Key::Num6, Key::Num7, Key::Num8, Key::Num9, Key::Num0,
        Key::F1, Key::F2, Key::F3, Key::F5, Key::F7, Key::F8, Key::F9, Key::F10, Key::F11, Key::F12,
        Key::Enter, Key::Esc, Key::Backspace, Key::Tab, Key::Space
    ];
    
    for key in browser_keys {
        uinput.set_keybit(key).map_err(|e| MainError::Input(std::io::Error::new(std::io::ErrorKind::Other, e)))?;
    }
    
    // Setup device
    let mut dev_name_c = [0 as c_char; 80];
    let dev_name = "Dynamic Function Row Virtual Input Device".as_bytes();
    for i in 0..dev_name.len() {
        dev_name_c[i] = dev_name[i] as c_char;
    }
    
    uinput.dev_setup(&uinput_setup {
        id: input_id {
            bustype: 0x19,
            vendor: 0x1209,
            product: 0x316E,
            version: 1
        },
        ff_effects_max: 0,
        name: dev_name_c
    }).map_err(|e| MainError::Input(std::io::Error::new(std::io::ErrorKind::Other, e)))?;
    
    uinput.dev_create().map_err(|e| MainError::Input(std::io::Error::new(std::io::ErrorKind::Other, e)))?;
    
    Ok(uinput)
}

// Helper function to setup input devices
fn setup_input_devices() -> MainResult<(Libinput, Libinput)> {
    let mut input_tb = Libinput::new_with_udev(Interface);
    let mut input_main = Libinput::new_with_udev(Interface);
    
    // Safe seat assignment
    input_tb.udev_assign_seat("seat-touchbar")
        .map_err(|_| MainError::Input(std::io::Error::new(std::io::ErrorKind::Other, "Failed to assign touch bar seat")))?;
    input_main.udev_assign_seat("seat0")
        .map_err(|_| MainError::Input(std::io::Error::new(std::io::ErrorKind::Other, "Failed to assign main seat")))?;
    
    Ok((input_tb, input_main))
}

// Helper function to setup epoll
fn setup_epoll(input_main: &Libinput, input_tb: &Libinput, cfg_mgr: &ConfigManager, event_fd: &Arc<OwnedFd>) -> MainResult<Epoll> {
    let epoll = Epoll::new(EpollCreateFlags::empty())
        .map_err(|e| MainError::Epoll(e))?;
    
    safe_epoll_add(&epoll, &input_main.as_fd(), EpollEvent::new(EpollFlags::EPOLLIN, 0))?;
    safe_epoll_add(&epoll, &input_tb.as_fd(), EpollEvent::new(EpollFlags::EPOLLIN, 1))?;
    safe_epoll_add(&epoll, &cfg_mgr.fd(), EpollEvent::new(EpollFlags::EPOLLIN, 2))?;
    safe_epoll_add(&epoll, &*event_fd, EpollEvent::new(EpollFlags::EPOLLIN, 3))?;
    
    Ok(epoll)
}

// Helper function to create eventfd
fn create_eventfd() -> MainResult<Arc<OwnedFd>> {
    let fd = nix::sys::eventfd::eventfd(0, EfdFlags::EFD_NONBLOCK)
        .map_err(|e| MainError::Epoll(e))?;
    let event_fd = Arc::new(fd);
    Ok(event_fd)
}

// Helper function to handle layer switching and animations
fn handle_layer_switching(
    active_layer: &mut LayerKey,
    last_layer: &mut LayerKey,
    pending_layer: &mut Option<LayerKey>,
    app_layer3_slide_anim: &mut Animation,
    needs_complete_redraw: &mut bool,
) -> MainResult<()> {
    // --- Detect layer switch and trigger slide animation ---
    if *last_layer != *active_layer {
        if *active_layer == LayerKey::Custom3 {
            app_layer3_slide_anim.set_progress(0.0); // Set before animate_in
            app_layer3_slide_anim.animate_in();
        } else if *last_layer == LayerKey::Custom3 {
            // Only animate out if NOT switching to Fn keys (assume Fn keys is layer 1 or 0)
            let fn_layer_indices = [LayerKey::Fn];
            if !fn_layer_indices.contains(active_layer) {
                app_layer3_slide_anim.animate_out();
                *pending_layer = Some(*active_layer); // Remember where we want to go
                *active_layer = LayerKey::Custom3; // Stay on 3 until animation is done
            }
            // If switching to Fn keys, just switch immediately (no animation)
        }
        *last_layer = *active_layer;
        *needs_complete_redraw = true;
    }
    
    // --- Update AppLayerKeys3 slide animation ---
    if app_layer3_slide_anim.update() {
        *needs_complete_redraw = true;
    }
    
    // After slide-out animation, switch to pending layer if needed
    if !app_layer3_slide_anim.is_animating_out() && pending_layer.is_some() {
        *active_layer = get_pending_layer(pending_layer)?;
        *last_layer = *active_layer;
        *needs_complete_redraw = true;
    }
    
    Ok(())
}

// Helper function to perform redraw operations
fn perform_redraw(
    layers: &mut HashMap<LayerKey, FunctionLayer>,
    surface: &mut ImageSurface,
    drm: &mut DrmBackend,
    cfg: &Config,
    pixel_shift: &mut PixelShiftManager,
    app_layer3_slide_anim: &mut Animation,
    active_layer: LayerKey,
    last_layer: LayerKey,
    current_session: Option<&SessionState>,
    current_window_class: Option<&str>,
    app_ui_manager: &mut AppUiManager,
            media_player_drag_position: Option<f64>,
    needs_complete_redraw: bool,
    any_changed: bool,
    browser_buttons_changed: bool,
    width: u32,
    height: u32,
    needs_complete_redraw_ref: &mut bool,
) -> MainResult<()> {
    // Performance optimization: Only log redraws in debug mode
 
    
    let shift = if cfg.enable_pixel_shift {
        pixel_shift.get()
    } else {
        (0.0, 0.0)
    };
    
    // --- Pass slide progress for AppLayerKeys3 ---
    let app_layer3_slide_progress = if active_layer == LayerKey::Custom3 || last_layer == LayerKey::Custom3 {
        app_layer3_slide_anim.progress()
    } else {
        1.0
    };
    
    // Performance optimization: Batch drawing operations
    let start_time = std::time::Instant::now();
    
    // Draw only the current active layer (layer 3 during slide-out)
    // media player drag position is handled in the drawing functions
    let clips = get_active_layer_mut(layers, active_layer)?.draw(
        cfg, 
        width as i32, 
        height as i32, 
        surface, 
        shift, 
        needs_complete_redraw, 
        false, 
        current_session, 
        Some(active_layer), 
        app_layer3_slide_progress, 
        current_window_class, 
        Some(app_ui_manager), 
        media_player_drag_position
    )?;
    
    // Performance optimization: Batch DRM operations
    let data = safe_surface_data(surface)?;
    safe_drm_map(drm)?.as_mut()[..data.len()].copy_from_slice(&data);
    safe_drm_dirty(drm, &clips)?;
    
    // Performance monitoring
    let draw_time = start_time.elapsed();
    if DEBUG_LOGGING && draw_time > std::time::Duration::from_millis(FRAME_TARGET_MS as u64) {
    }
    
    *needs_complete_redraw_ref = false;
    Ok(())
}

// Helper function to setup session monitoring
fn setup_session_monitoring(event_fd: &Arc<OwnedFd>) -> MainResult<(watch::Sender<SessionState>, watch::Receiver<SessionState>, mpsc::UnboundedSender<SessionState>, mpsc::UnboundedReceiver<SessionState>)> {
    // Initialize session monitor
    let (session_tx, session_rx) = watch::channel(SessionState {
        session_type: "".to_string(),  // Empty string = no session type detected yet
        is_logged_in: false,
        user: "".to_string(),
        leader: None,
    });
    tokio::spawn(monitor_sessions(session_tx.clone()));

    // Create mpsc channel for event-driven session updates
    let (event_tx, event_rx) = mpsc::unbounded_channel();
    let mut session_rx_clone = session_rx.clone();
    let event_tx_clone = event_tx.clone();
    let event_fd_clone = Arc::clone(event_fd);
    
    tokio::spawn(async move {
        while session_rx_clone.changed().await.is_ok() {
            let new_state = session_rx_clone.borrow().clone();
            let _ = event_tx_clone.send(new_state);
            // Write to eventfd to wake up main loop
            let val: u64 = 1;
            let _ = nix::unistd::write(event_fd_clone.as_raw_fd(), &val.to_ne_bytes());
        }
    });
    
    Ok((session_tx, session_rx, event_tx, event_rx))
}

// Add log level control at the top
// Set to true to enable verbose debug logging, false for production (much less resource usage)
const DEBUG_LOGGING: bool = false; // Set to false to disable verbose logging

// Epoll event data constants
const EPOLL_DATA_MAIN_INPUT: u64 = 0;
const EPOLL_DATA_TOUCHBAR_INPUT: u64 = 1;
const EPOLL_DATA_CONFIG_MANAGER: u64 = 2;
const EPOLL_DATA_SESSION_EVENT: u64 = 3;
const EPOLL_DATA_HELPER_LISTENER: u64 = 4;
const EPOLL_DATA_HELPER_STREAM: u64 = 5;
const EPOLL_DATA_MEDIA_PLAYER_LISTENER: u64 = 6;
const EPOLL_DATA_MEDIA_PLAYER_STREAM: u64 = 7;
const EPOLL_DATA_BROWSER_LISTENER: u64 = 8;
const EPOLL_DATA_BROWSER_STREAM: u64 = 9;
const EPOLL_DATA_BACKGROUND_SERVICE_LISTENER: u64 = 10;
const EPOLL_DATA_BACKGROUND_SERVICE_STREAM: u64 = 11;

// Timeout constants
const TIMEOUT_MS: i32 = 10 * 1000;
const FRAME_TARGET_MS: u64 = 16; // 60fps = 16.67ms per frame
const FORCE_CLEANUP_INTERVAL: u64 = 10000; // Every 10000 frames
const PROCESS_STATUS_LOG_INTERVAL: u64 = 1000; // Every 1000 frames

// Buffer sizes
const SOCKET_BUFFER_SIZE: usize = 1024;
const EVENTFD_BUFFER_SIZE: usize = 8;

// Layer switching behavior:
// - When user is not logged in: Custom2 layer (AppLayerKeys2) is active
// - When user logs in: Media layer becomes active
// - When user logs out: Custom2 layer becomes active again



// Signal handler for graceful shutdown
extern "C" fn signal_handler(_signal: i32) {
    std::process::exit(0);
}

// Setup signal handlers for graceful shutdown
fn setup_signal_handlers() {
    unsafe {
        let action = SigAction::new(
            SigHandler::Handler(signal_handler),
            SaFlags::empty(),
            SigSet::empty(),
        );
        
        if let Err(e) = nix::sys::signal::sigaction(Signal::SIGTERM, &action) {
            eprintln!("[main] Failed to set SIGTERM handler: {}", e);
        }
        
        if let Err(e) = nix::sys::signal::sigaction(Signal::SIGINT, &action) {
            eprintln!("[main] Failed to set SIGINT handler: {}", e);
        }
    }
}
use crate::display::display::DrmBackend;
use drm::control::dumbbuffer::DumbMapping;
use display::animation::Animation;
use display::backlight::BacklightManager;
use display::pixel_shift::PixelShiftManager;
use config::Config;
use config::ConfigManager;

mod config;
mod fonts;
mod crash_handler;
pub mod display {
    pub mod animation;
    pub mod backlight;
    pub mod display;
    pub mod pixel_shift;
}
pub mod view;
pub mod services;
pub mod helper;

use crate::helper::manager::{HelperManager, MediaPlayerHelperManager, BrowserHelperManager, BackgroundServiceHelperManager, ProcessStatus};
























struct Interface;

impl LibinputInterface for Interface {
    fn open_restricted(&mut self, path: &Path, flags: i32) -> Result<OwnedFd, i32> {
        let mode = flags & O_ACCMODE;

        OpenOptions::new()
            .custom_flags(flags)
            .read(mode == O_RDONLY || mode == O_RDWR)
            .write(mode == O_WRONLY || mode == O_RDWR)
            .open(path)
            .map(|file| file.into())
            .map_err(|err| err.raw_os_error().unwrap_or(-1))
    }
    fn close_restricted(&mut self, fd: OwnedFd) {
        _ = File::from(fd);
    }
}






fn main() {
    if let Err(e) = run() {
        eprintln!("Error: {e}");
        std::process::exit(1);
    }
}

fn run() -> Result<(), Box<dyn std::error::Error>> {
    let mut drm = DrmBackend::open_card()?;
    let (height, width) = drm.mode().size();
    // Create a Tokio runtime for async operations
    let rt = tokio::runtime::Runtime::new()?;
    rt.block_on(async {
        real_main(&mut drm).await
    })?;
    let crash_bitmap = include_bytes!("crash_bitmap.raw");
    let mut map = drm.map()?;
    let data = map.as_mut();
    let mut wptr = 0;
    for byte in crash_bitmap {
        for i in 0..8 {
            let bit = ((byte >> i) & 0x1) == 0;
            let color = if bit { 0xFF } else { 0x0 };
            data[wptr] = color;
            data[wptr + 1] = color;
            data[wptr + 2] = color;
            data[wptr + 3] = color;
            wptr += 4;
        }
    }
    drop(map);
    drm.dirty(&[ClipRect::new(0, 0, height as u16, width as u16)])?;
    let mut sigset = SigSet::empty();
    sigset.add(Signal::SIGTERM);
    sigset.wait()?;
    Ok(())
}

async fn real_main(drm: &mut DrmBackend) -> MainResult<()> {
    // Setup signal handlers for graceful shutdown
    setup_signal_handlers();
    let (height, width) = drm.mode().size();
    
    // Safe framebuffer info retrieval
    let fb_info = drm.fb_info()
        .map_err(|e| MainError::Drm(e.into()))?;
    let (db_width, db_height) = fb_info.size();
    
    // Initialize components using helper functions
    let mut cfg_mgr = ConfigManager::new();
    let (mut cfg, mut layers) = cfg_mgr.load_config(width);
    let mut uinput = initialize_uinput(&layers)?;

      let mut backlight = BacklightManager::new();
    let mut last_redraw_minute = Local::now().minute();
    let mut pixel_shift = PixelShiftManager::new();
    let mut helper_manager = HelperManager::new();
    let mut media_player_helper_manager = MediaPlayerHelperManager::new();
    let mut browser_helper_manager = BrowserHelperManager::new();
    let mut background_service_helper_manager = BackgroundServiceHelperManager::new();
    let mut browser_helper_listener_fd: Option<i32> = None;
    let mut browser_helper_stream: Option<UnixStream> = None;
    let mut browser_helper_reader: Option<BufReader<UnixStream>> = None;
    let mut background_service_helper_listener_fd: Option<i32> = None;
    let mut background_service_helper_stream: Option<UnixStream> = None;
    let mut background_service_helper_reader: Option<BufReader<UnixStream>> = None;
    let mut available_mpris_services: Vec<String> = Vec::new();
    
    // Add focus-based Media Player helper management
    let mut media_player_window_focused = false;
    let mut browser_window_focused = false;
    let mut current_user: Option<String> = None;
    let mut current_media_player_window_id: Option<u64> = None; // Track current Media Player window ID
    let mut current_browser_window_id: Option<u64> = None; // Track current browser window ID

    // Safe surface creation
    let mut surface = ImageSurface::create(Format::ARgb32, db_width as i32, db_height as i32)
        .map_err(|e| MainError::Cairo(format!("Failed to create image surface: {}", e)))?;
    
    // Start with Custom2 layer since user starts as not logged in
    let mut active_layer = LayerKey::Custom2;
    let mut last_layer = active_layer.clone();
    let mut pending_layer: Option<LayerKey> = None;

    let (mut input_tb, mut input_main) = setup_input_devices()?;
    let event_fd = create_eventfd()?;
    let epoll = setup_epoll(&input_main, &input_tb, &cfg_mgr, &event_fd)?;

    let mut digitizer: Option<InputDevice> = None;

    // Setup session monitoring
    let (_, _, _, mut event_rx) = setup_session_monitoring(&event_fd)?;
    let mut current_session: Option<SessionState> = None;
    let mut helper_listener_fd: Option<i32> = None;
    let mut helper_stream: Option<UnixStream> = None;
    let mut helper_reader: Option<BufReader<UnixStream>> = None;
    let mut media_player_helper_listener_fd: Option<i32> = None;
    let mut media_player_helper_stream: Option<UnixStream> = None;
    let mut media_player_helper_reader: Option<BufReader<UnixStream>> = None;
    let mut current_window_class: Option<String> = None;
    let mut current_window_id: Option<u64> = None;
    let mut needs_complete_redraw = false;
    let mut app_layer3_slide_anim = Animation::new(0.18, 16.0); // 60fps for smooth slide
    let mut app_ui_manager = AppUiManager::new();
    let mut media_player_touch_active = false; // Track if Media Player touch interaction is active
    let mut media_player_drag_position: Option<f64> = None; // Track current drag position for visual feedback
    let mut background_service_drag_position: Option<f64> = None; // Track current drag position for background service visual feedback
    let mut previous_generic_media_enabled = false; // Track previous generic media state for redraw detection

    // --- main event loop ---
    loop {
        if cfg_mgr.update_config(&mut cfg, &mut layers, width) {
            // Respect current session state when updating config
            active_layer = if current_session.as_ref().map(|s| s.is_logged_in).unwrap_or(false) {
                LayerKey::Media
            } else {
                LayerKey::Custom2
            };
            needs_complete_redraw = true;
        }
        let mut next_timeout_ms = TIMEOUT_MS;
        if cfg.enable_pixel_shift {
            let (pixel_shift_needs_redraw, pixel_shift_next_timeout_ms) = pixel_shift.update();
            if pixel_shift_needs_redraw {
                needs_complete_redraw = true;
            }
            next_timeout_ms = min(next_timeout_ms, pixel_shift_next_timeout_ms);
        }
        // No login animation timeout needed
        // --- AppLayerKeys3 slide animation update ---
        if app_layer3_slide_anim.is_running() {
            next_timeout_ms = min(next_timeout_ms, 16);
        }
        let current_minute = Local::now().minute();
        for button in &mut get_active_layer_mut(&mut layers, active_layer)?.buttons {
            if (button.action == Key::Time) && (current_minute != last_redraw_minute) {
                needs_complete_redraw = true;
                last_redraw_minute = current_minute;
            }
        }
        // Handle layer switching and animations
        handle_layer_switching(
            &mut active_layer,
            &mut last_layer,
            &mut pending_layer,
            &mut app_layer3_slide_anim,
            &mut needs_complete_redraw
        )?;
        // --- Restore any_changed variable for redraw logic ---
        let any_changed = if let Some(split) = &get_active_layer(&layers, active_layer)?.split {
            split.media.iter().any(|b| b.changed)
        } else {
            get_active_layer(&layers, active_layer)?.buttons.iter().any(|b| b.changed)
        };
        
        // Check for browser screen button changes
        let browser_buttons_changed = app_ui_manager.browser_screen.buttons.iter().any(|b| b.changed);
        let browser_buttons_active = app_ui_manager.browser_screen.buttons.iter().any(|b| b.active);
        
        // Check for generic media state changes (this will be set by touch handlers)
        let generic_media_changed = app_ui_manager.generic_media_enabled != previous_generic_media_enabled;
        if generic_media_changed {
            previous_generic_media_enabled = app_ui_manager.generic_media_enabled;
            needs_complete_redraw = true;
            
            // Manage focus window helper based on generic media state
            if app_ui_manager.generic_media_enabled {
                // Generic media enabled - stop all helpers
                
                // Stop focus window helper
                if !helper_manager.is_process_none() {
                    // Stop if the helper process exists (regardless of status)
                    if let Some(fd) = helper_listener_fd.take() {
                        let listener_fd_obj = unsafe { OwnedFd::from_raw_fd(fd) };
                        if let Err(e) = safe_epoll_delete(&epoll, &listener_fd_obj) {
                            eprintln!("[main] Failed to remove focus window helper listener from epoll: {}", e);
                        }
                    }
                    if let Some(stream) = helper_stream.take() {
                        if let Err(e) = safe_epoll_delete(&epoll, &stream) {
                            eprintln!("[main] Failed to remove focus window helper stream from epoll: {}", e);
                        }
                        helper_reader = None;
                    }
                    helper_manager.stop();
                    if DEBUG_LOGGING {
                        println!("[main] Stopped focus window helper - generic media enabled");
                    }
                }
                
                // Stop browser helper
                if browser_helper_manager.is_process_running() {
                    if let Some(stream) = browser_helper_stream.take() {
                        if let Err(e) = safe_epoll_delete(&epoll, &stream) {
                            eprintln!("[main] Failed to remove browser helper stream from epoll: {}", e);
                        }
                    }
                    browser_helper_reader = None;
                    if let Some(fd) = browser_helper_listener_fd.take() {
                        let listener_fd_obj = unsafe { OwnedFd::from_raw_fd(fd) };
                        if let Err(e) = safe_epoll_delete(&epoll, &listener_fd_obj) {
                            eprintln!("[main] Failed to remove browser helper listener from epoll: {}", e);
                        }
                    }
                    browser_helper_manager.stop();
                    if DEBUG_LOGGING {
                        println!("[main] Stopped browser helper - generic media enabled");
                    }
                }
                
                // Stop main media helper
                if media_player_helper_manager.is_process_running() {
                    if let Some(stream) = media_player_helper_stream.take() {
                        if let Err(e) = safe_epoll_delete(&epoll, &stream) {
                            eprintln!("[main] Failed to remove Media Player helper stream from epoll: {}", e);
                        }
                    }
                    media_player_helper_reader = None;
                    if let Some(fd) = media_player_helper_listener_fd.take() {
                        let listener_fd_obj = unsafe { OwnedFd::from_raw_fd(fd) };
                        if let Err(e) = safe_epoll_delete(&epoll, &listener_fd_obj) {
                            eprintln!("[main] Failed to remove Media Player helper listener from epoll: {}", e);
                        }
                    }
                    media_player_helper_manager.stop();
                    if DEBUG_LOGGING {
                        println!("[main] Stopped main media helper - generic media enabled");
                    }
                }
                
                // Start MPRIS monitoring in background service helper
                if let Some(ref mut stream) = background_service_helper_stream {
                    if let Err(e) = stream.write_all(b"start_mpris_monitoring\n") {
                        eprintln!("[main] Failed to send start_mpris_monitoring command: {}", e);
                    } else {
                        if DEBUG_LOGGING {
                            println!("[main] Sent start_mpris_monitoring command - generic media enabled");
                        }
                    }
                }
            } else {
                // Generic media disabled - stop MPRIS monitoring and restart focus window helper
                
                // Stop MPRIS monitoring in background service helper
                if let Some(ref mut stream) = background_service_helper_stream {
                    if let Err(e) = stream.write_all(b"stop_mpris_monitoring\n") {
                        eprintln!("[main] Failed to send stop_mpris_monitoring command: {}", e);
                    } else {
                        if DEBUG_LOGGING {
                            println!("[main] Sent stop_mpris_monitoring command - generic media disabled");
                        }
                    }
                }
                
                // Restart focus window helper if user is logged in
                if let Some(user) = &current_user {
                    if helper_manager.is_process_none() {
                        if let Some(fd) = helper_manager.start(user, current_session.as_ref().and_then(|s| s.leader).unwrap_or(0)) {
                            let listener_fd_obj = unsafe { OwnedFd::from_raw_fd(fd) };
                            if let Err(e) = safe_epoll_add(&epoll, &listener_fd_obj, EpollEvent::new(EpollFlags::EPOLLIN, EPOLL_DATA_HELPER_LISTENER)) {
                                eprintln!("[main] Failed to add focus window helper listener to epoll: {}", e);
                            } else {
                                helper_listener_fd = Some(listener_fd_obj.into_raw_fd());
                                if DEBUG_LOGGING {
                                    println!("[main] Restarted focus window helper - generic media disabled");
                                }
                            }
                        } else {
                            eprintln!("[main] Failed to restart focus window helper for user: {}", user);
                        }
                    }
                }
            }
        }
        
        // UI drawing will happen after window focus detection logic
        

        
        // --- epoll wait and event handling ---
        let mut events = [EpollEvent::empty(); 7];
        
        // Performance optimization: Frame rate limiting
        let frame_start = std::time::Instant::now();
        
        let n = safe_epoll_wait(&epoll, &mut events, next_timeout_ms as isize)?;

        // Handle epoll events
        for i in 0..n {
            let event = events[i];
            match event.data() {
                EPOLL_DATA_MAIN_INPUT => { /* Main input events handled in the input processing loop */ },
                EPOLL_DATA_TOUCHBAR_INPUT => { /* Touch bar input events handled in the input processing loop */ },
                EPOLL_DATA_CONFIG_MANAGER => { /* Config manager events handled by cfg_mgr.update_config() */ },
                EPOLL_DATA_SESSION_EVENT => {
                    // eventfd triggered: read and process session event
                    let mut buf = [0u8; EVENTFD_BUFFER_SIZE];
                    let _ = nix::unistd::read(event_fd.as_raw_fd(), &mut buf);
                                    if let Ok(new_state) = event_rx.try_recv() {
                    // Performance optimization: Reduce logging in production
               
                    
                    let session_changed = match &current_session {
                        Some(current) => current != &new_state,
                        None => {
                            // First session state update - always treat as changed
                            true
                        }
                    };
                    
             
                    
                                            if session_changed {
                            if new_state.is_logged_in {
                          
                                current_user = Some(new_state.user.clone());
                                
                                // Set login time and start delay
                                helper_manager.set_login_time();
                                
                                // Don't start helper immediately - wait for delay to complete
                             
                                
                                // Media Player helper will be started when Media Player window gains focus
                                
                                // Switch to Media layer when user logs in
                                if active_layer != LayerKey::Media {
                            
                                    active_layer = LayerKey::Media;
                                    needs_complete_redraw = true;
                                }
                             } else {
                         
                                if let Some(fd) = helper_listener_fd.take() {
                              
                                    let listener_fd_obj = unsafe { OwnedFd::from_raw_fd(fd) };
                                    if let Err(e) = safe_epoll_delete(&epoll, &listener_fd_obj) {
                                        eprintln!("[main] Failed to remove helper listener from epoll: {}", e);
                                    }
                                }
                                if let Some(stream) = helper_stream.take() {
                                    if let Err(e) = safe_epoll_delete(&epoll, &stream) {
                                        eprintln!("[main] Failed to remove helper stream from epoll: {}", e);
                                    }
                                    helper_reader = None;
                                }
                                // Media Player helper will be stopped when Media Player window loses focus
                            
                                helper_manager.stop();
                                
                                // Reset session ready state for next login
                                helper_manager.reset_session_state();
                                
                                // Switch to Custom2 layer when user logs out
                                if active_layer != LayerKey::Custom2 {
                               
                                    active_layer = LayerKey::Custom2;
                                    needs_complete_redraw = true;
                                }
                                
                                // Clear current window class when user logs out
                                current_window_class = None;
                                // Clear Media Player and browser window IDs when user logs out
                                current_media_player_window_id = None;
                                current_browser_window_id = None;
                            }
                            // No animation needed - just update session state
                            current_session = Some(new_state);
                            needs_complete_redraw = true;
                        } else {
                            if DEBUG_LOGGING {
                            }
                        }
                    }
                }
                EPOLL_DATA_HELPER_LISTENER => { // Helper listener event
                    if DEBUG_LOGGING {
                    }
                    if let Some(stream) = helper_manager.accept_connection() {
                        if DEBUG_LOGGING {
                        }
                        if let Err(e) = safe_epoll_add(&epoll, &stream, EpollEvent::new(EpollFlags::EPOLLIN, EPOLL_DATA_HELPER_STREAM)) {
                            log_error("helper", "add stream to epoll", &e.to_string());
                            continue;
                        }
                        if let Ok(stream_clone) = safe_stream_try_clone(&stream) {
                            helper_reader = Some(BufReader::new(stream_clone));
                            helper_stream = Some(stream);
                            if DEBUG_LOGGING {
                            }
                        } else {
                            log_error("helper", "clone stream", "stream clone failed");
                            continue;
                        }
                        
                        // Stop listening for new connections
                        if let Some(fd) = helper_listener_fd.take() {
                            if DEBUG_LOGGING {
                            }
                            let listener_fd_obj = unsafe { OwnedFd::from_raw_fd(fd) };
                            if let Err(e) = safe_epoll_delete(&epoll, &listener_fd_obj) {
                                eprintln!("[main] Failed to remove helper listener from epoll: {}", e);
                            }
                        }
                    } else {
                        if DEBUG_LOGGING {
                        }
                    }
                }
                EPOLL_DATA_HELPER_STREAM => { // Helper stream event
                    if let Some(reader) = &mut helper_reader {
                        loop {
                           let mut buf = vec![0; SOCKET_BUFFER_SIZE];
                           match reader.get_mut().read(&mut buf) {
                               Ok(0) => { // EOF
                                   if let Some(stream) = helper_stream.take() {
                                       if let Err(e) = safe_epoll_delete(&epoll, &stream) {
                                           eprintln!("[main] Failed to remove helper stream from epoll: {}", e);
                                       }
                                   }
                                   helper_reader = None;
                                   break;
                               },
                               Ok(n) => {
                                   let data = &buf[..n];
                                   if let Ok(text) = std::str::from_utf8(data) {
                                       for part in text.split('\n') {
                                           let part = part.trim();
                                           if part.is_empty() {
                                               continue;
                                           }
                                           
                                           // Parse the new "class:id:pid" format
                                           let (class, window_id, pid) = if let Some(first_colon_pos) = part.find(':') {
                                               let class_part = &part[..first_colon_pos];
                                               
                                               // Look for second colon to find PID
                                               if let Some(second_colon_pos) = part[first_colon_pos + 1..].find(':') {
                                                   let id_part = &part[first_colon_pos + 1..first_colon_pos + 1 + second_colon_pos];
                                                   let pid_part = &part[first_colon_pos + 1 + second_colon_pos + 1..];
                                                   
                                                   // Try to parse the window ID and PID
                                                   let parsed_id = if id_part == "0" {
                                                       // Desktop window
                                                       Some(0)
                                                   } else {
                                                       id_part.parse::<u64>().ok()
                                                   };
                                                   
                                                   let parsed_pid = if pid_part == "0" {
                                                       // Desktop or no PID
                                                       Some(0)
                                                   } else {
                                                       pid_part.parse::<u32>().ok()
                                                   };
                                                   
                                                   (class_part, parsed_id, parsed_pid)
                                               } else {
                                                   // Fallback: no PID, just "class:id" format
                                                   let id_part = &part[first_colon_pos + 1..];
                                                   
                                                   let parsed_id = if id_part == "0" {
                                                       // Desktop window
                                                       Some(0)
                                                   } else {
                                                       id_part.parse::<u64>().ok()
                                                   };
                                                   
                                                   (class_part, parsed_id, Some(0)) // Default PID to 0
                                               }
                                           } else {
                                               // Fallback: no ID or PID, just class
                                               (part, None, Some(0)) // Default PID to 0
                                           };
                                           
                                           // Update current window class and ID BEFORE detection logic
                                           current_window_class = Some(class.to_string());
                                           current_window_id = window_id;
                                           
                                                                                        // Check if Media Player window focus changed
                                             let new_media_player_focused = is_media_player_window_class(&class.to_lowercase());
                                             let media_player_focus_changed = new_media_player_focused != media_player_window_focused;
                                             let media_player_window_id_changed = if new_media_player_focused && media_player_window_focused {
                                                 // Media Player is still focused, check if window ID changed
                                                 current_media_player_window_id != window_id
                                           } else {
                                               false
                                           };
                                           
                                                                                      if media_player_focus_changed || media_player_window_id_changed {
                                               if media_player_focus_changed {
                                                   media_player_window_focused = new_media_player_focused;
                                               }
                                               
                                               
                                               if new_media_player_focused {
                                                   // Media Player window gained focus or ID changed - start/restart Media Player helper
                                                   if media_player_window_id_changed {
                                                       // Stop existing helper first if ID changed
                                                       if let Some(stream) = media_player_helper_stream.take() {
                                                           if let Err(e) = safe_epoll_delete(&epoll, &stream) {
                                                               eprintln!("[main] Failed to remove Media Player stream from epoll: {}", e);
                                                           }
                                                       }
                                                       media_player_helper_reader = None;
                                                       if let Some(fd) = media_player_helper_listener_fd.take() {
                                                           let listener_fd_obj = unsafe { OwnedFd::from_raw_fd(fd) };
                                                           if let Err(e) = safe_epoll_delete(&epoll, &listener_fd_obj) {
                                                               eprintln!("[main] Failed to remove Media Player listener from epoll: {}", e);
                                                           }
                                                       }
                                                                               if media_player_helper_manager.is_process_running() {
                            media_player_helper_manager.stop();
                                                       }
                                                                                                          // Clear Media Player drag position when switching windows
                                                   media_player_drag_position = None;
                                                   }
                                                   
                                                   if let Some(user) = &current_user {
                                                       // Don't start the old media helper if generic media is enabled
                                                       if !app_ui_manager.generic_media_enabled {
                                                       if let Some(fd) = media_player_helper_manager.start(user, current_session.as_ref().and_then(|s| s.leader).unwrap_or(0), class, window_id.unwrap_or(0), pid.unwrap_or(0)) {
                                                           let listener_fd_obj = unsafe { OwnedFd::from_raw_fd(fd) };
                                                           if let Err(e) = safe_epoll_add(&epoll, &listener_fd_obj, EpollEvent::new(EpollFlags::EPOLLIN, 6)) {
                                                               eprintln!("[main] Failed to add Media Player helper listener to epoll: {}", e);
                                                           } else {
                                                               media_player_helper_listener_fd = Some(listener_fd_obj.into_raw_fd());
                                                           }
                                                       } else {
                                                           eprintln!("[main] Failed to start Media Player helper for user: {}", user);
                                                       }
                                                       }
                                                   } else {
                                                       eprintln!("[main] No current user available for Media Player helper");
                                                   }
                                               } else {
                                                   // Media Player window lost focus - stop Media Player helper
                                                   if let Some(stream) = media_player_helper_stream.take() {
                                                       if let Err(e) = safe_epoll_delete(&epoll, &stream) {
                                                           eprintln!("[main] Failed to remove Media Player stream from epoll: {}", e);
                                                       }
                                                   }
                                                   media_player_helper_reader = None;
                                                   if let Some(fd) = media_player_helper_listener_fd.take() {
                                                       let listener_fd_obj = unsafe { OwnedFd::from_raw_fd(fd) };
                                                       if let Err(e) = safe_epoll_delete(&epoll, &listener_fd_obj) {
                                                           eprintln!("[main] Failed to remove Media Player listener from epoll: {}", e);
                                                       }
                                                   }
                                                   if media_player_helper_manager.is_process_running() {
                                                   media_player_helper_manager.stop();
                                                   }
                                                   // Clear Media Player drag position when losing focus
                                                   media_player_drag_position = None;
                                               }
                                               
                                               // Update the current Media Player window ID
                                               if new_media_player_focused {
                                                   current_media_player_window_id = window_id;
                                               } else {
                                                   current_media_player_window_id = None;
                                               }
                                           }
                                           
                                           // Check if browser window focus changed
                                           let new_browser_focused = is_browser_window_class(&class.to_lowercase());
                                           let browser_focus_changed = new_browser_focused != browser_window_focused;
                                           let browser_window_id_changed = if new_browser_focused && browser_window_focused {
                                               // Browser is still focused, check if window ID changed
                                               current_browser_window_id != window_id
                                           } else {
                                               false
                                           };
                                           
                                           if browser_focus_changed || browser_window_id_changed {
                                               if browser_focus_changed {
                                                   browser_window_focused = new_browser_focused;
                                               }
                                               
                                               
                                               if new_browser_focused {
                                                   // Browser window gained focus or ID changed - start/restart browser helper
                                                   if browser_window_id_changed {
                                                       // Stop existing helper first if ID changed
                                                   if let Some(stream) = browser_helper_stream.take() {
                                                       if let Err(e) = safe_epoll_delete(&epoll, &stream) {
                                                           eprintln!("[main] Failed to remove browser helper stream from epoll: {}", e);
                                                       }
                                                   }
                                                   browser_helper_reader = None;
                                                   if let Some(fd) = browser_helper_listener_fd.take() {
                                                       let listener_fd_obj = unsafe { OwnedFd::from_raw_fd(fd) };
                                                       if let Err(e) = safe_epoll_delete(&epoll, &listener_fd_obj) {
                                                           eprintln!("[main] Failed to remove browser helper listener from epoll: {}", e);
                                                       }
                                                   }
                                                       if browser_helper_manager.is_process_running() {
                                                   browser_helper_manager.stop();
                                                       }
                                                   }
                                                   
                                                   
                                                   if let Some(user) = &current_user {
                                                       // Don't start browser helper if generic media is enabled
                                                       if !app_ui_manager.generic_media_enabled {
                                                       if let Some(fd) = browser_helper_manager.start(user, current_session.as_ref().and_then(|s| s.leader).unwrap_or(0), class, window_id.unwrap_or(0), pid.unwrap_or(0)) {
                                                           let listener_fd_obj = unsafe { OwnedFd::from_raw_fd(fd) };
                                                           if let Err(e) = safe_epoll_add(&epoll, &listener_fd_obj, EpollEvent::new(EpollFlags::EPOLLIN, 8)) {
                                                               eprintln!("[main] Failed to add browser helper listener to epoll: {}", e);
                                                           } else {
                                                               browser_helper_listener_fd = Some(listener_fd_obj.into_raw_fd());
                                                           }
                                                       } else {
                                                           eprintln!("[main] Failed to start browser helper for user: {}", user);
                                                       }
                                                       }
                                                   } else {
                                                       eprintln!("[main] No current user available for browser helper");
                                                   }
                                               } else {
                                                   // Browser window lost focus - stop browser helper
                                                   if let Some(stream) = browser_helper_stream.take() {
                                                       if let Err(e) = safe_epoll_delete(&epoll, &stream) {
                                                           eprintln!("[main] Failed to remove browser helper stream from epoll: {}", e);
                                                       }
                                                   }
                                                   browser_helper_reader = None;
                                                   if let Some(fd) = browser_helper_listener_fd.take() {
                                                       let listener_fd_obj = unsafe { OwnedFd::from_raw_fd(fd) };
                                                       if let Err(e) = safe_epoll_delete(&epoll, &listener_fd_obj) {
                                                           eprintln!("[main] Failed to remove browser helper listener from epoll: {}", e);
                                                       }
                                                   }
                                                   if browser_helper_manager.is_process_running() {
                                                       browser_helper_manager.stop();
                                                   }
                                               }
                                               
                                               // Update the current browser window ID
                                               if new_browser_focused {
                                                   current_browser_window_id = window_id;
                                               } else {
                                                   current_browser_window_id = None;
                                               }
                                           } else if new_browser_focused && browser_window_focused {
                                               // Browser focus state is the same, but browser type might have changed
                                               if current_window_class.as_ref() != Some(&class.to_string()) {
                                                   // Browser type changed - stop and restart helper for clean state
                                                   
                                                   // Stop existing helper
                                                   if let Some(stream) = browser_helper_stream.take() {
                                                       if let Err(e) = safe_epoll_delete(&epoll, &stream) {
                                                           eprintln!("[main] Failed to remove browser helper stream from epoll: {}", e);
                                                       }
                                                   }
                                                   browser_helper_reader = None;
                                                   if let Some(fd) = browser_helper_listener_fd.take() {
                                                       let listener_fd_obj = unsafe { OwnedFd::from_raw_fd(fd) };
                                                       if let Err(e) = safe_epoll_delete(&epoll, &listener_fd_obj) {
                                                           eprintln!("[main] Failed to remove browser helper listener from epoll: {}", e);
                                                       }
                                                   }
                                                   browser_helper_manager.stop();
                                                   
                                                   // Start new helper for the new browser type
                                                   if let Some(user) = &current_user {
                                                       // Don't start browser helper if generic media is enabled
                                                       if !app_ui_manager.generic_media_enabled {
                                                       if let Some(fd) = browser_helper_manager.start(user, current_session.as_ref().and_then(|s| s.leader).unwrap_or(0), class, window_id.unwrap_or(0), pid.unwrap_or(0)) {
                                                           let listener_fd_obj = unsafe { OwnedFd::from_raw_fd(fd) };
                                                           if let Err(e) = safe_epoll_add(&epoll, &listener_fd_obj, EpollEvent::new(EpollFlags::EPOLLIN, 8)) {
                                                               eprintln!("[main] Failed to add browser helper listener to epoll: {}", e);
                                                           } else {
                                                               browser_helper_listener_fd = Some(listener_fd_obj.into_raw_fd());
                                                           }
                                                           }
                                                       }
                                                   } else {
                                                       eprintln!("[main] No current user available for browser helper");
                                                   }
                                               }
                                           }
                                           
                                           

                                           
                                           needs_complete_redraw = true;
                                       }
                                   } else {
                                   }
                               },
                               Err(ref e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                                   break; // No more data right now
                               },
                                                               Err(_e) => {
                                    if let Some(stream) = helper_stream.take() {
                                        if let Err(e) = safe_epoll_delete(&epoll, &stream) {
                                            eprintln!("[main] Failed to remove helper stream from epoll: {}", e);
                                        }
                                    }
                                    helper_reader = None;
                                    break;
                                }
                           }
                        }
                    }
                }
                EPOLL_DATA_MEDIA_PLAYER_LISTENER => { // Media Player helper listener event
                    if let Some(stream) = media_player_helper_manager.accept_connection() {
                        if let Err(e) = stream.set_nonblocking(true) {
                            eprintln!("[main] Failed to set Media Player stream non-blocking: {}", e);
                            continue;
                        }
                        if let Err(e) = epoll.add(&stream, EpollEvent::new(EpollFlags::EPOLLIN, 7)) {
                            eprintln!("[main] Failed to add Media Player stream to epoll: {}", e);
                            continue;
                        }
                        if let Ok(stream_clone) = stream.try_clone() {
                            media_player_helper_reader = Some(BufReader::new(stream_clone));
                                                                               media_player_helper_stream = Some(stream);
                            // Stop listening for new connections
                            if let Some(fd) = media_player_helper_listener_fd.take() {
                                let listener_fd_obj = unsafe { OwnedFd::from_raw_fd(fd) };
                                if let Err(e) = safe_epoll_delete(&epoll, &listener_fd_obj) {
                                    eprintln!("[main] Failed to remove Media Player listener from epoll: {}", e);
                                }
                            }
                        } else {
                            eprintln!("[main] Failed to clone Media Player stream");
                        }
                    }
                }
                EPOLL_DATA_MEDIA_PLAYER_STREAM => { // Media Player helper stream event
                    if let Some(reader) = &mut media_player_helper_reader {
                        loop {
                           let mut buf = vec![0; SOCKET_BUFFER_SIZE];
                           match reader.get_mut().read(&mut buf) {
                               Ok(0) => { // EOF
                                   if let Some(stream) = media_player_helper_stream.take() {
                                       if let Err(e) = safe_epoll_delete(&epoll, &stream) {
                                           eprintln!("[main] Failed to remove Media Player stream from epoll: {}", e);
                                       }
                                   }
                                   media_player_helper_reader = None;
                                   break;
                               },
                               Ok(n) => {
                                   let data = &buf[..n];
                                   if let Ok(text) = std::str::from_utf8(data) {
                                       for part in text.split('\n') {
                                           let part = part.trim();
                                           if part.is_empty() {
                                               continue;
                                           }
                                           
                                           // Handle Media Player status message (plain JSON format)
                                           if let Ok(media_player_status) = serde_json::from_str::<serde_json::Value>(part) {
                                               // Update Media Player screen with the status
                                               if let Some(is_playing) = media_player_status.get("is_playing").and_then(|v| v.as_bool()) {
                                                   if let Some(position) = media_player_status.get("position").and_then(|v| v.as_f64()) {
                                                       if let Some(duration) = media_player_status.get("duration").and_then(|v| v.as_i64()) {
                                                                                              // Create a MediaStatus struct and update the Media Player screen
                                   let status = crate::helper::MediaStatus {
                                                               is_playing,
                                                               position,
                                                               duration,
                                                           };
                                                           
                                                           // If we have a drag position and Media Player has updated to a new position,
                                                           // gradually fade out the drag position
                                                           if let Some(drag_pos) = media_player_drag_position {
                                                               if (position - drag_pos).abs() < 0.01 {
                                                                   // Media Player has caught up to the drag position, clear it
                                                                   media_player_drag_position = None;
                                                               }
                                                           }
                                                           
                                                           app_ui_manager.media_player_screen.last_status = Some(status.clone());
                                                           app_ui_manager.spotify_screen.last_status = Some(status);
                                                           needs_complete_redraw = true;
                                                       }
                                                   }
                                               }
                                           }
                                       }
                                   } else {
                                   }
                               },
                               Err(ref e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                                   break; // No more data right now
                               },
                               Err(e) => {
                                   eprintln!("[main] Media Player helper stream error: {}", e);
                                   if let Some(stream) = media_player_helper_stream.take() {
                                       if let Err(e) = safe_epoll_delete(&epoll, &stream) {
                                           eprintln!("[main] Failed to remove Media Player stream from epoll: {}", e);
                                       }
                                   }
                                   media_player_helper_reader = None;
                                   break;
                               }
                           }
                        }
                    }
                }
                EPOLL_DATA_BROWSER_LISTENER => { // Browser helper listener event
                    if let Some(mut stream) = browser_helper_manager.accept_connection() {
                        if let Err(e) = safe_stream_set_nonblocking(&stream, true) {
                            eprintln!("[main] Failed to set browser stream non-blocking: {}", e);
                            continue;
                        }
                        
                        // Send browser type to the helper as the first message
                        if let Some(window_class) = &current_window_class {
                            // Send the EXACT window class that triggered the browser focus
                            let browser_type_msg = format!("browser_type:{}\n", window_class);
                            if let Err(e) = stream.write_all(browser_type_msg.as_bytes()) {
                                eprintln!("[main] Failed to send browser type to helper: {}", e);
                            }
                        }
                        
                        if let Err(e) = safe_epoll_add(&epoll, &stream, EpollEvent::new(EpollFlags::EPOLLIN, 9)) {
                            eprintln!("[main] Failed to add browser helper stream to epoll: {}", e);
                            continue;
                        }
                        if let Ok(stream_clone) = safe_stream_try_clone(&stream) {
                            browser_helper_reader = Some(BufReader::new(stream_clone));
                            browser_helper_stream = Some(stream);
                        } else {
                            eprintln!("[main] Failed to clone browser helper stream");
                            continue;
                        }
                        // Stop listening for new connections
                        if let Some(fd) = browser_helper_listener_fd.take() {
                            let listener_fd_obj = unsafe { OwnedFd::from_raw_fd(fd) };
                            if let Err(e) = safe_epoll_delete(&epoll, &listener_fd_obj) {
                                eprintln!("[main] Failed to remove browser helper listener from epoll: {}", e);
                            }
                        }
                    }
                }
                EPOLL_DATA_BROWSER_STREAM => { // Browser helper stream event
                    if let Some(reader) = &mut browser_helper_reader {
                        loop {
                           let mut buf = vec![0; SOCKET_BUFFER_SIZE];
                           match reader.get_mut().read(&mut buf) {
                               Ok(0) => { // EOF
                                   if let Some(stream) = browser_helper_stream.take() {
                                       if let Err(e) = safe_epoll_delete(&epoll, &stream) {
                                           eprintln!("[main] Failed to remove browser helper stream from epoll: {}", e);
                                       }
                                   }
                                   browser_helper_reader = None;
                                   break;
                               },
                               Ok(n) => {
                                   let data = &buf[..n];
                                   if let Ok(text) = std::str::from_utf8(data) {
                                       for part in text.split('\n') {
                                           let part = part.trim();
                                           if part.is_empty() {
                                               continue;
                                           }
                                           
                                           // Handle browser status message (plain JSON format)
                                           if let Ok(browser_status) = serde_json::from_str::<serde_json::Value>(part) {
                                               // Update browser screen with the status
                                               if let Some(url) = browser_status.get("url").and_then(|v| v.as_str()) {
                                                   if let Some(title) = browser_status.get("title").and_then(|v| v.as_str()) {
                                                       if let Some(can_go_back) = browser_status.get("can_go_back").and_then(|v| v.as_bool()) {
                                                           if let Some(can_go_forward) = browser_status.get("can_go_forward").and_then(|v| v.as_bool()) {
                                                               if let Some(is_loading) = browser_status.get("is_loading").and_then(|v| v.as_bool()) {
                                                                   let favicon_url = browser_status.get("favicon_url").and_then(|v| v.as_str()).map(|s| s.to_string());
                                                                   
                                                                   // Create a BrowserStatus struct and update the browser screen
                                                                   let status = crate::helper::BrowserStatus {
                                                                       url: url.to_string(),
                                                                       title: title.to_string(),
                                                                       favicon_url,
                                                                       can_go_back,
                                                                       can_go_forward,
                                                                       is_loading,
                                                                   };
                                                                   
                                                                   app_ui_manager.browser_screen.update_status(status);
                                                                   needs_complete_redraw = true;
                                                               }
                                                           }
                                                       }
                                                   }
                                               }
                                           }
                                       }
                                   } else {
                                   }
                               },
                               Err(ref e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                                   break; // No more data right now
                               },
                                                               Err(_e) => {
                                    if let Some(stream) = browser_helper_stream.take() {
                                        if let Err(e) = safe_epoll_delete(&epoll, &stream) {
                                            eprintln!("[main] Failed to remove browser helper stream from epoll: {}", e);
                                        }
                                    }
                                    browser_helper_reader = None;
                                    break;
                                }
                           }
                        }
                    }
                }
                EPOLL_DATA_BACKGROUND_SERVICE_LISTENER => { // Background service helper listener event
                    if let Some(mut stream) = background_service_helper_manager.accept_connection() {
                        if let Err(e) = safe_stream_set_nonblocking(&stream, true) {
                            eprintln!("[main] Failed to set background service helper stream non-blocking: {}", e);
                            continue;
                        }
                        
                        if DEBUG_LOGGING {
                        }
                        if let Err(e) = safe_epoll_add(&epoll, &stream, EpollEvent::new(EpollFlags::EPOLLIN, EPOLL_DATA_BACKGROUND_SERVICE_STREAM)) {
                            eprintln!("[main] Failed to add background service helper stream to epoll: {}", e);
                            continue;
                        }
                        if let Ok(stream_clone) = safe_stream_try_clone(&stream) {
                            background_service_helper_reader = Some(BufReader::new(stream_clone));
                            background_service_helper_stream = Some(stream);
                        
                        } else {
                            eprintln!("[main] Failed to clone background service helper stream");
                            continue;
                        }
                        
                        // Stop listening for new connections
                        if let Some(fd) = background_service_helper_listener_fd.take() {
                        
                            let listener_fd_obj = unsafe { OwnedFd::from_raw_fd(fd) };
                            if let Err(e) = safe_epoll_delete(&epoll, &listener_fd_obj) {
                                eprintln!("[main] Failed to remove background service helper listener from epoll: {}", e);
                            }
                        }
                    } else {
                        if DEBUG_LOGGING {
                        }
                    }
                }
                EPOLL_DATA_BACKGROUND_SERVICE_STREAM => { // Background service helper stream event
                    if let Some(reader) = &mut background_service_helper_reader {
                        loop {
                           let mut line = String::new();
                           match reader.read_line(&mut line) {
                               Ok(0) => { // EOF
                                   if let Some(stream) = background_service_helper_stream.take() {
                                       if let Err(e) = safe_epoll_delete(&epoll, &stream) {
                                           eprintln!("[main] Failed to remove background service helper stream from epoll: {}", e);
                                       }
                                   }
                                   background_service_helper_reader = None;
                                  
                                   break;
                               },
                               Ok(_) => {
                                   // Process the line (trim newline)
                                   let data = line.trim();
                               
                                   
                                   // Process available services message
                                   if data.starts_with("list_services:") {
                                       let services_str = data.strip_prefix("list_services:").unwrap_or("").trim();
                                       if services_str.is_empty() {
                                           available_mpris_services = Vec::new();
                                           // Disable generic media when no services are available
                                           app_ui_manager.generic_media_enabled = false;
                                        
                                       } else {
                                           available_mpris_services = services_str.split(',').map(|s| s.to_string()).collect();
                                       
                                       }
                                       app_ui_manager.update_available_services_list_with_auto_select(available_mpris_services.clone(), &mut background_service_helper_stream);
            needs_complete_redraw = true;

                                   
                                   }
                                   // Process selected service message
                                   else if data.starts_with("selected_service:") {
                                       let selected_str = data.strip_prefix("selected_service:").unwrap_or("").trim();
                                       println!("[main] ===== PROCESSING SELECTED SERVICE MESSAGE =====");
                                       println!("[main] Received selected_service: {}", selected_str);
                                       println!("[main] Current selected_service_name: {:?}", app_ui_manager.generic_background_screen.selected_service_name);
                                       
                                       app_ui_manager.generic_background_screen.selected_service_name = Some(selected_str.to_string());
                                       
                                       println!("[main] Set selected service name to: {}", selected_str);
                                       println!("[main] selected_service_name is now: {:?}", app_ui_manager.generic_background_screen.selected_service_name);
                                       println!("[main] ===== END PROCESSING SELECTED SERVICE MESSAGE =====");
                                   }
                                   // Process media status updates (JSON format)
                                   else if data.starts_with("status_update:") {
                                       let json_str = data.strip_prefix("status_update:").unwrap_or("").trim();
                                       println!("[main] Received status update: {}", json_str);
                                       if let Ok(json_data) = serde_json::from_str::<serde_json::Value>(json_str) {
                                           println!("[main] Parsed JSON successfully: {:?}", json_data);
                                           if let (Some(is_playing), Some(position), Some(duration)) = (
                                               json_data.get("is_playing").and_then(|v| v.as_bool()),
                                               json_data.get("position").and_then(|v| v.as_f64()),
                                               json_data.get("duration").and_then(|v| v.as_f64().map(|d| d as i64))
                                           ) {
                                               // Create MediaStatus and update generic background screen
                                               let media_status = crate::helper::MediaStatus {
                                                   is_playing,
                                                   position,
                                                   duration,
                                               };
                                               app_ui_manager.generic_background_screen.last_status = Some(media_status);
                                               needs_complete_redraw = true;
                                               println!("[main] Updated generic background screen with media status: playing={}, position={:.2}%", is_playing, position * 100.0);
                                           } else {
                                               println!("[main] Failed to extract fields from JSON: is_playing={:?}, position={:?}, duration={:?}", 
                                                   json_data.get("is_playing"), json_data.get("position"), json_data.get("duration"));
                                           }
                                       } else {
                                           println!("[main] Failed to parse JSON: {}", json_str);
                                       }
                                   }
                                   // Process media status updates (JSON format) - legacy format
                                   else if data.starts_with("{") && data.contains("is_playing") {
                                       println!("[main] Received JSON status update: {}", data);
                                       if let Ok(json_data) = serde_json::from_str::<serde_json::Value>(data) {
                                           println!("[main] Parsed JSON successfully: {:?}", json_data);
                                           if let (Some(is_playing), Some(position), Some(duration)) = (
                                               json_data.get("is_playing").and_then(|v| v.as_bool()),
                                               json_data.get("position").and_then(|v| v.as_f64()),
                                               json_data.get("duration").and_then(|v| v.as_f64().map(|d| d as i64))
                                           ) {
                                               // Create MediaStatus and update generic background screen
                                               let media_status = crate::helper::MediaStatus {
                                                   is_playing,
                                                   position,
                                                   duration,
                                               };
                                               app_ui_manager.generic_background_screen.last_status = Some(media_status);
                                               needs_complete_redraw = true;
                                               println!("[main] Updated generic background screen with media status: playing={}, position={:.2}%", is_playing, position * 100.0);
                                           } else {
                                               println!("[main] Failed to extract fields from JSON: is_playing={:?}, position={:?}, duration={:?}", 
                                                   json_data.get("is_playing"), json_data.get("position"), json_data.get("duration"));
                                           }
                                       } else {
                                           println!("[main] Failed to parse JSON: {}", data);
                                       }
                                   }
                               },
                               Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                                   break; // No more data right now
                               },
                               Err(_e) => {
                                    if let Some(stream) = background_service_helper_stream.take() {
                                        if let Err(e) = safe_epoll_delete(&epoll, &stream) {
                                            eprintln!("[main] Failed to remove background service helper stream from epoll: {}", e);
                                        }
                                    }
                                    background_service_helper_reader = None;
                                    break;
                                }
                           }
                        }
                    }
                }
                _ => {}
            }
        }
        // // After epoll, always process all pending input events:
        safe_input_dispatch(&mut input_tb)?;
        safe_input_dispatch(&mut input_main)?;

        for event in &mut input_tb.clone().chain(input_main.clone()) {
            backlight.process_event(&event);
            
            // Handle keyboard and device events
            if KeyboardEventHandler::handle_device_event(
                &event,
                &mut active_layer,
                &current_session,
                &mut needs_complete_redraw
            ) {
                digitizer = Some(event.device().clone());
            }
            
            // Handle touch events
            TouchEventHandler::handle_touch_event(
                &event,
                &digitizer,
                backlight.current_bl(),
                width.into(),
                height.into(),
                &active_layer,
                &mut layers,
                &mut uinput,
                &current_window_class,
                &mut app_ui_manager,
                        &mut media_player_touch_active,
        &mut media_player_drag_position,
        &mut media_player_helper_stream,
                &mut browser_helper_stream,
                &mut background_service_helper_stream,
                &mut needs_complete_redraw,
                cfg.enable_pixel_shift
            )?;
        }
        
        
         backlight.update_backlight(&cfg);
        
        // No login animation needed
        
        // Check process status and clean up zombies periodically
        if let Err(e) = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            helper_manager.check_process_status();
        })) {
            eprintln!("[main] Error during helper manager status check: {:?}", e);
        }
                  if let Err(e) = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
              media_player_helper_manager.check_process_status();
          })) {
              eprintln!("[main] Error during Media Player helper manager status check: {:?}", e);
          }
        if let Err(e) = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            browser_helper_manager.check_process_status();
        })) {
            eprintln!("[main] Error during browser helper manager status check: {:?}", e);
        }
        
        
        // Force cleanup of any zombie processes every FORCE_CLEANUP_INTERVAL frames (less frequent to reduce overhead)
        static FORCE_CLEANUP_COUNTER: AtomicU64 = AtomicU64::new(0);
        let counter = FORCE_CLEANUP_COUNTER.fetch_add(1, Ordering::Relaxed);
        if counter % FORCE_CLEANUP_INTERVAL == 0 { // Every FORCE_CLEANUP_INTERVAL frames (reduced frequency)
            
                // Wrap cleanup calls in error handling to prevent crashes
                if let Err(e) = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    helper_manager.force_cleanup();
                })) {
                    eprintln!("[main] Error during helper manager cleanup: {:?}", e);
                }
                                  if let Err(e) = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                      media_player_helper_manager.force_cleanup();
                  })) {
                      eprintln!("[main] Error during Media Player helper manager cleanup: {:?}", e);
                  }
                if let Err(e) = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    browser_helper_manager.force_cleanup();
                })) {
                    eprintln!("[main] Error during browser helper manager cleanup: {:?}", e);
                }
                if let Err(e) = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    background_service_helper_manager.force_cleanup();
                })) {
                    eprintln!("[main] Error during background service helper manager cleanup: {:?}", e);
                }
        }
        
        // Check if we can start the helper now that session might be ready
        // Only start helper if generic media is disabled (when enabled, helper should be stopped)
        if let Some(user) = &current_user {
                            if helper_manager.is_process_none() && helper_manager.check_session_ready() && !app_ui_manager.generic_media_enabled {
                if DEBUG_LOGGING {
                    println!("[main] Session is now ready, starting main helper for user: {}", user);
                }
                if let Some(fd) = helper_manager.start(user, current_session.as_ref().and_then(|s| s.leader).unwrap_or(0)) {
                    if DEBUG_LOGGING {
                        println!("[main] Main helper started successfully, fd: {}", fd);
                    }
                    let listener_fd_obj = unsafe { OwnedFd::from_raw_fd(fd) };
                    if let Err(e) = safe_epoll_add(&epoll, &listener_fd_obj, EpollEvent::new(EpollFlags::EPOLLIN, 4)) {
                        eprintln!("[main] Failed to add helper listener to epoll: {}", e);
                    } else {
                        helper_listener_fd = Some(listener_fd_obj.into_raw_fd()); // Store the raw fd
                        if DEBUG_LOGGING {
                            println!("[main] Added helper listener to epoll with fd: {}", fd);
                        }
                    }
                } else {
                    println!("[main] ERROR: Failed to start main helper for user: {}", user);
                }
            }
            
            // Start background service helper when user logs in (like focus_window_helper)
            if background_service_helper_manager.is_process_none() {
                if DEBUG_LOGGING {
                    println!("[main] Starting background service helper for user: {}", user);
                }
                if let Some(fd) = background_service_helper_manager.start(user, current_session.as_ref().and_then(|s| s.leader).unwrap_or(0)) {
                    if DEBUG_LOGGING {
                        println!("[main] Background service helper started successfully, fd: {}", fd);
                    }
                    let listener_fd_obj = unsafe { OwnedFd::from_raw_fd(fd) };
                    if let Err(e) = safe_epoll_add(&epoll, &listener_fd_obj, EpollEvent::new(EpollFlags::EPOLLIN, EPOLL_DATA_BACKGROUND_SERVICE_LISTENER)) {
                        eprintln!("[main] Failed to add background service helper listener to epoll: {}", e);
                    } else {
                        background_service_helper_listener_fd = Some(listener_fd_obj.into_raw_fd());
                        if DEBUG_LOGGING {
                            println!("[main] Added background service helper listener to epoll with fd: {}", fd);
                        }
                    }
                } else {
                    println!("[main] ERROR: Failed to start background service helper for user: {}", user);
                }
            }
        }
        
        // Performance optimization: Frame rate limiting to maintain 60fps
        let frame_duration = frame_start.elapsed();
        let target_frame_time = std::time::Duration::from_millis(FRAME_TARGET_MS as u64); // 60fps = 16.67ms per frame
        
        if frame_duration < target_frame_time {
            let sleep_time = target_frame_time - frame_duration;
            std::thread::sleep(sleep_time);
        }
        
        // Process session events (event-driven)
        
        // Handle different types of redraws AFTER window focus detection logic
        if needs_complete_redraw || any_changed || browser_buttons_changed {
            perform_redraw(
                &mut layers,
                &mut surface,
                drm,
                &cfg,
                &mut pixel_shift,
                &mut app_layer3_slide_anim,
                active_layer,
                last_layer,
                current_session.as_ref(),
                current_window_class.as_deref(),
                &mut app_ui_manager,
                media_player_drag_position,
                needs_complete_redraw,
                any_changed,
                browser_buttons_changed,
                width.into(),
                height.into(),
                &mut needs_complete_redraw
            )?;
        }
    }
}

