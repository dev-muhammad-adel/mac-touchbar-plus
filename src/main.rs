use std::collections::HashMap;
use std::fs::{File, OpenOptions};
use std::os::fd::{AsFd, OwnedFd, AsRawFd, FromRawFd, IntoRawFd};
use std::os::unix::net::UnixStream;
use std::os::unix::fs::OpenOptionsExt;
use std::cmp::min;
use std::path::Path;

use anyhow::Result;
use cairo::{Context, FontSlant, FontWeight, Format, ImageSurface, Rectangle, Surface};
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
use input_linux::{uinput::UInputHandle, EventKind, Key, SynchronizeKind};
use input_linux_sys::{input_event, input_id, timeval, uinput_setup};
use libc::{c_char, O_ACCMODE, O_RDONLY, O_RDWR, O_WRONLY};
use nix::{
    sys::eventfd::{eventfd, EfdFlags},
    sys::{
        epoll::{Epoll, EpollCreateFlags, EpollEvent, EpollFlags},
        signal::{SaFlags, SigAction, SigHandler, SigSet, Signal},
    },
};
use rsvg::CairoRenderer;
use std::io::{BufReader, Read, Write};
use std::sync::Arc;

use chrono::{Local, Timelike};
use crate::services::sessionmanager::{SessionState, monitor_sessions};
use tokio::sync::{watch, mpsc};

use view::app_ui_manager::AppUiManager;


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
            eprintln!("[main] Failed to add fd to epoll: {}", e);
            e
        })
}

fn safe_epoll_delete(epoll: &Epoll, fd: &dyn AsFd) -> MainResult<()> {
    epoll.delete(fd)
        .map_err(|e| MainError::Epoll(e))
        .map_err(|e| {
            eprintln!("[main] Failed to remove fd from epoll: {}", e);
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
    vlc_drag_position: Option<f64>,
    needs_complete_redraw: bool,
    any_changed: bool,
    browser_buttons_changed: bool,
    width: u32,
    height: u32,
    needs_complete_redraw_ref: &mut bool,
) -> MainResult<()> {
    // Performance optimization: Only log redraws in debug mode
    if DEBUG_LOGGING {
        println!("[main] REDRAW TRIGGERED: needs_complete_redraw={}, any_changed={}, browser_buttons_changed={}", needs_complete_redraw, any_changed, browser_buttons_changed);
    }
    
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
    // VLC drag position is handled in the drawing functions
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
        vlc_drag_position
    )?;
    
    // Performance optimization: Batch DRM operations
    let data = safe_surface_data(surface)?;
    safe_drm_map(drm)?.as_mut()[..data.len()].copy_from_slice(&data);
    safe_drm_dirty(drm, &clips)?;
    
    // Performance monitoring
    let draw_time = start_time.elapsed();
    if DEBUG_LOGGING && draw_time > std::time::Duration::from_millis(16) {
        println!("[main] SLOW DRAW: {:.2}ms (target: 16ms for 60fps)", draw_time.as_millis() as f64);
    }
    
    *needs_complete_redraw_ref = false;
    Ok(())
}

// Helper function to handle epoll events
fn handle_epoll_events(
    events: &[EpollEvent],
    n: usize,
    event_fd: &Arc<OwnedFd>,
    event_rx: &mut mpsc::UnboundedReceiver<SessionState>,
    current_session: &mut Option<SessionState>,
    current_user: &mut Option<String>,
    helper_manager: &mut HelperManager,
    helper_listener_fd: &mut Option<i32>,
    helper_stream: &mut Option<UnixStream>,
    helper_reader: &mut Option<BufReader<UnixStream>>,
    epoll: &mut Epoll,
    active_layer: &mut LayerKey,
    needs_complete_redraw: &mut bool,
    current_window_class: &mut Option<String>,
    debug_logging: bool,
) -> MainResult<()> {
    for i in 0..n {
        let event = events[i];
        match event.data() {
            0 => { /* Main input events handled in the input processing loop */ },
            1 => { /* Touch bar input events handled in the input processing loop */ },
            2 => { /* Config manager events handled by cfg_mgr.update_config() */ },
            3 => {
                // eventfd triggered: read and process session event
                let mut buf = [0u8; 8];
                let _ = nix::unistd::read(event_fd.as_raw_fd(), &mut buf);
                if let Ok(new_state) = event_rx.try_recv() {
                    // Performance optimization: Reduce logging in production
                    if debug_logging {
                        println!("[main] Received session event: {:?}", new_state);
                    }
                    
                    let session_changed = match &current_session {
                        Some(current) => current != &new_state,
                        None => {
                            // First session state update - always treat as changed
                            true
                        }
                    };
                    
                    if debug_logging {
                        println!("[main] Session changed: {} (current: {:?}, new: {:?})", session_changed, current_session, new_state);
                    }
                    
                    if session_changed {
                        if new_state.is_logged_in {
                            if debug_logging {
                                println!("[main] User logged in: {}", new_state.user);
                            }
                            *current_user = Some(new_state.user.clone());
                            
                            // Set login time and start delay
                            helper_manager.set_login_time();
                            
                            // Don't start helper immediately - wait for delay to complete
                            if debug_logging {
                                println!("[main] User logged in, starting 1 second delay before helper");
                            }
                            
                            // VLC helper will be started when VLC window gains focus
                            
                            // Switch to Media layer when user logs in
                            if *active_layer != LayerKey::Media {
                                if debug_logging {
                                    println!("[main] User logged in, switching from {:?} to Media layer", *active_layer);
                                }
                                *active_layer = LayerKey::Media;
                                *needs_complete_redraw = true;
                            }
                        } else {
                            if debug_logging {
                                println!("[main] User logged out: {:?}", current_session);
                            }
                            if let Some(fd) = helper_listener_fd.take() {
                                if debug_logging {
                                    println!("[main] Removing helper listener fd: {}", fd);
                                }
                                let listener_fd_obj = unsafe { OwnedFd::from_raw_fd(fd) };
                                if let Err(e) = safe_epoll_delete(epoll, &listener_fd_obj) {
                                    eprintln!("[main] Failed to remove helper listener from epoll: {}", e);
                                }
                            }
                            if let Some(stream) = helper_stream.take() {
                                if debug_logging {
                                    println!("[main] Removing helper stream from epoll");
                                }
                                if let Err(e) = safe_epoll_delete(epoll, &stream) {
                                    eprintln!("[main] Failed to remove helper stream from epoll: {}", e);
                                }
                                *helper_reader = None;
                            }
                            // VLC helper will be stopped when VLC window loses focus
                            if debug_logging {
                                println!("[main] Stopping main helper");
                            }
                            helper_manager.stop();
                            
                            // Reset session ready state for next login
                            // Reset login time is handled in stop() method
                            
                            // Switch to Custom2 layer when user logs out
                            if *active_layer != LayerKey::Custom2 {
                                if debug_logging {
                                    println!("[main] User logged out, switching from {:?} to Custom2 layer", *active_layer);
                                }
                                *active_layer = LayerKey::Custom2;
                                *needs_complete_redraw = true;
                            }
                            
                            // Clear current window class when user logs out
                            *current_window_class = None;
                        }
                        // No animation needed - just update session state
                        *current_session = Some(new_state);
                        *needs_complete_redraw = true;
                    } else {
                        if debug_logging {
                            println!("[main] Session state unchanged, skipping redraw");
                        }
                    }
                }
            }
            4 => { /* Helper listener event - handled in main loop */ }
            _ => {}
        }
    }
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

// Layer switching behavior:
// - When user is not logged in: Custom2 layer (AppLayerKeys2) is active
// - When user logs in: Media layer becomes active
// - When user logs out: Custom2 layer becomes active again



// Signal handler for graceful shutdown
extern "C" fn signal_handler(_signal: i32) {
    println!("[main] Received shutdown signal, exiting gracefully...");
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

use crate::helper::manager::{HelperManager, VlcHelperManager, BrowserHelperManager};


const TIMEOUT_MS: i32 = 10 * 1000;
























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
    let mut vlc_helper_manager = VlcHelperManager::new();
    let mut browser_helper_manager = BrowserHelperManager::new();
    let mut browser_helper_listener_fd: Option<i32> = None;
    let mut browser_helper_stream: Option<UnixStream> = None;
    let mut browser_helper_reader: Option<BufReader<UnixStream>> = None;
    
    // Add focus-based VLC/Dragon Player helper management
    let mut vlc_window_focused = false;
    let mut browser_window_focused = false;
    let _last_window_class: Option<String> = None;
    let mut current_user: Option<String> = None;
    let mut current_vlc_window_id: Option<u64> = None; // Track current VLC/Dragon Player window ID
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
    let mut vlc_helper_listener_fd: Option<i32> = None;
    let mut vlc_helper_stream: Option<UnixStream> = None;
    let mut vlc_helper_reader: Option<BufReader<UnixStream>> = None;
    let mut current_window_class: Option<String> = None;
    let mut current_window_id: Option<u64> = None;
    let mut needs_complete_redraw = false;
    let mut app_layer3_slide_anim = Animation::new(0.18, 16.0); // 60fps for smooth slide
    let mut app_ui_manager = AppUiManager::new();
    let mut vlc_touch_active = false; // Track if VLC/Dragon Player touch interaction is active
    let mut vlc_drag_position: Option<f64> = None; // Track current drag position for visual feedback

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
        if browser_buttons_changed {
            println!("[main] Browser buttons changed, triggering redraw");
            println!("[main] Browser button states: Back(active={}, changed={}), Forward(active={}, changed={}), Refresh(active={}, changed={}), Home(active={}, changed={})", 
                app_ui_manager.browser_screen.buttons[0].active, app_ui_manager.browser_screen.buttons[0].changed,
                app_ui_manager.browser_screen.buttons[1].active, app_ui_manager.browser_screen.buttons[1].changed,
                app_ui_manager.browser_screen.buttons[2].active, app_ui_manager.browser_screen.buttons[2].changed,
                app_ui_manager.browser_screen.buttons[3].active, app_ui_manager.browser_screen.buttons[3].changed);
        }
        if browser_buttons_active {
            println!("[main] Browser buttons active: {}", browser_buttons_active);
        }
        
        // Handle different types of redraws
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
                vlc_drag_position,
                needs_complete_redraw,
                any_changed,
                browser_buttons_changed,
                width.into(),
                height.into(),
                &mut needs_complete_redraw
            )?;
        }
        

        
        // --- epoll wait and event handling ---
        let mut events = [EpollEvent::empty(); 5];
        
        // Performance optimization: Frame rate limiting
        let frame_start = std::time::Instant::now();
        
        let n = safe_epoll_wait(&epoll, &mut events, next_timeout_ms as isize)?;

        // Handle epoll events
        for i in 0..n {
            let event = events[i];
            match event.data() {
                0 => { /* Main input events handled in the input processing loop */ },
                1 => { /* Touch bar input events handled in the input processing loop */ },
                2 => { /* Config manager events handled by cfg_mgr.update_config() */ },
                3 => {
                    // eventfd triggered: read and process session event
                    let mut buf = [0u8; 8];
                    let _ = nix::unistd::read(event_fd.as_raw_fd(), &mut buf);
                                    if let Ok(new_state) = event_rx.try_recv() {
                    // Performance optimization: Reduce logging in production
                    if DEBUG_LOGGING {
                        println!("[main] Received session event: {:?}", new_state);
                    }
                    
                    let session_changed = match &current_session {
                        Some(current) => current != &new_state,
                        None => {
                            // First session state update - always treat as changed
                            true
                        }
                    };
                    
                    if DEBUG_LOGGING {
                        println!("[main] Session changed: {} (current: {:?}, new: {:?})", session_changed, current_session, new_state);
                    }
                    
                                            if session_changed {
                            if new_state.is_logged_in {
                                if DEBUG_LOGGING {
                                    println!("[main] User logged in: {}", new_state.user);
                                }
                                current_user = Some(new_state.user.clone());
                                
                                // Set login time and start delay
                                helper_manager.set_login_time();
                                
                                // Don't start helper immediately - wait for delay to complete
                                if DEBUG_LOGGING {
                                    println!("[main] User logged in, starting 1 second delay before helper");
                                }
                                
                                // VLC helper will be started when VLC window gains focus
                                
                                // Switch to Media layer when user logs in
                                if active_layer != LayerKey::Media {
                                    if DEBUG_LOGGING {
                                        println!("[main] User logged in, switching from {:?} to Media layer", active_layer);
                                    }
                                    active_layer = LayerKey::Media;
                                    needs_complete_redraw = true;
                                }
                             } else {
                                if DEBUG_LOGGING {
                                    println!("[main] User logged out: {:?}", current_session);
                                }
                                if let Some(fd) = helper_listener_fd.take() {
                                    if DEBUG_LOGGING {
                                        println!("[main] Removing helper listener fd: {}", fd);
                                    }
                                    let listener_fd_obj = unsafe { OwnedFd::from_raw_fd(fd) };
                                    if let Err(e) = safe_epoll_delete(&epoll, &listener_fd_obj) {
                                        eprintln!("[main] Failed to remove helper listener from epoll: {}", e);
                                    }
                                }
                                if let Some(stream) = helper_stream.take() {
                                    if DEBUG_LOGGING {
                                        println!("[main] Removing helper stream from epoll");
                                    }
                                    if let Err(e) = safe_epoll_delete(&epoll, &stream) {
                                        eprintln!("[main] Failed to remove helper stream from epoll: {}", e);
                                    }
                                    helper_reader = None;
                                }
                                // VLC helper will be stopped when VLC window loses focus
                                if DEBUG_LOGGING {
                                    println!("[main] Stopping main helper");
                                }
                                helper_manager.stop();
                                
                                // Reset session ready state for next login
                                // Reset login time is handled in stop() method
                                
                                // Switch to Custom2 layer when user logs out
                                if active_layer != LayerKey::Custom2 {
                                    if DEBUG_LOGGING {
                                        println!("[main] User logged out, switching from {:?} to Custom2 layer", active_layer);
                                    }
                                    active_layer = LayerKey::Custom2;
                                    needs_complete_redraw = true;
                                }
                                
                                // Clear current window class when user logs out
                                current_window_class = None;
                                // Clear VLC and browser window IDs when user logs out
                                current_vlc_window_id = None;
                                current_browser_window_id = None;
                            }
                            // No animation needed - just update session state
                            current_session = Some(new_state);
                            needs_complete_redraw = true;
                        } else {
                            if DEBUG_LOGGING {
                                println!("[main] Session state unchanged, skipping redraw");
                            }
                        }
                    }
                }
                4 => { // Helper listener event
                    if DEBUG_LOGGING {
                        println!("[main] Helper listener event triggered");
                    }
                    if let Some(stream) = helper_manager.accept_connection() {
                        if DEBUG_LOGGING {
                            println!("[main] Helper connected to socket successfully");
                        }
                        if let Err(e) = safe_epoll_add(&epoll, &stream, EpollEvent::new(EpollFlags::EPOLLIN, 5)) {
                            eprintln!("[main] Failed to add helper stream to epoll: {}", e);
                            continue;
                        }
                        if let Ok(stream_clone) = safe_stream_try_clone(&stream) {
                            helper_reader = Some(BufReader::new(stream_clone));
                            helper_stream = Some(stream);
                            if DEBUG_LOGGING {
                                println!("[main] Helper stream added to epoll and stored");
                            }
                        } else {
                            eprintln!("[main] Failed to clone helper stream");
                            continue;
                        }
                        
                        // Stop listening for new connections
                        if let Some(fd) = helper_listener_fd.take() {
                            if DEBUG_LOGGING {
                                println!("[main] Removing helper listener fd: {} from epoll", fd);
                            }
                            let listener_fd_obj = unsafe { OwnedFd::from_raw_fd(fd) };
                            if let Err(e) = safe_epoll_delete(&epoll, &listener_fd_obj) {
                                eprintln!("[main] Failed to remove helper listener from epoll: {}", e);
                            }
                        }
                    } else {
                        if DEBUG_LOGGING {
                            println!("[main] No helper connection available to accept");
                        }
                    }
                }
                5 => { // Helper stream event
                    println!("[main] Helper stream event triggered");
                    if let Some(reader) = &mut helper_reader {
                        println!("[main] Reading from helper socket...");
                        loop {
                           let mut buf = vec![0; 1024];
                           match reader.get_mut().read(&mut buf) {
                               Ok(0) => { // EOF
                                   println!("[main] Helper disconnected (EOF)");
                                   if let Some(stream) = helper_stream.take() {
                                       println!("[main] Removing helper stream from epoll");
                                       if let Err(e) = safe_epoll_delete(&epoll, &stream) {
                                           eprintln!("[main] Failed to remove helper stream from epoll: {}", e);
                                       }
                                   }
                                   helper_reader = None;
                                   break;
                               },
                               Ok(n) => {
                                   let data = &buf[..n];
                                   println!("[main] Received {} bytes from helper: {:?}", n, data);
                                   if let Ok(text) = std::str::from_utf8(data) {
                                       println!("[main] Helper data as text: {}", text);
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
                                           
                                           println!("[main] Parsed window info - class: '{}', id: {:?}, pid: {:?}", class, window_id, pid);
                                           
                                           // Check if VLC/Dragon Player window focus changed
                                           let new_vlc_focused = class == "vlc" || class == "org.kde.dragonplayer";
                                           let vlc_focus_changed = new_vlc_focused != vlc_window_focused;
                                           let vlc_window_id_changed = if new_vlc_focused && vlc_window_focused {
                                               // VLC is still focused, check if window ID changed
                                               current_vlc_window_id != window_id
                                           } else {
                                               false
                                           };
                                           
                                           if vlc_focus_changed || vlc_window_id_changed {
                                               if vlc_focus_changed {
                                               vlc_window_focused = new_vlc_focused;
                                               }
                                               
                                               if vlc_window_id_changed {
                                                   println!("[main] VLC/Dragon Player window ID changed from {:?} to {:?}, restarting VLC helper", current_vlc_window_id, window_id);
                                                   println!("[main] Stopping existing VLC helper and clearing state...");
                                               }
                                               
                                               if new_vlc_focused {
                                                   // VLC window gained focus or ID changed - start/restart VLC helper
                                                   if vlc_window_id_changed {
                                                       // Stop existing helper first if ID changed
                                                       if let Some(stream) = vlc_helper_stream.take() {
                                                           if let Err(e) = safe_epoll_delete(&epoll, &stream) {
                                                               eprintln!("[main] Failed to remove VLC stream from epoll: {}", e);
                                                           }
                                                       }
                                                       vlc_helper_reader = None;
                                                       if let Some(fd) = vlc_helper_listener_fd.take() {
                                                           let listener_fd_obj = unsafe { OwnedFd::from_raw_fd(fd) };
                                                           if let Err(e) = safe_epoll_delete(&epoll, &listener_fd_obj) {
                                                               eprintln!("[main] Failed to remove VLC listener from epoll: {}", e);
                                                           }
                                                       }
                                                       if vlc_helper_manager.is_process_running() {
                                                           vlc_helper_manager.stop();
                                                           println!("[main] VLC helper stopped due to window ID change");
                                                       } else {
                                                           println!("[main] VLC helper was not running, no need to stop");
                                                       }
                                                       // Clear VLC drag position when switching windows
                                                       vlc_drag_position = None;
                                                   }
                                                   
                                                   if vlc_window_id_changed {
                                                       println!("[main] VLC helper restarted for new window ID: {:?}", window_id);
                                                   } else {
                                                       if current_vlc_window_id.is_none() {
                                                           println!("[main] VLC/Dragon Player window focused for the first time, starting VLC helper");
                                                       } else {
                                                           println!("[main] VLC/Dragon Player window focused, starting VLC helper");
                                                       }
                                                   }
                                                   if let Some(user) = &current_user {
                                                       if let Some(fd) = vlc_helper_manager.start(user, current_session.as_ref().and_then(|s| s.leader).unwrap_or(0), class, window_id.unwrap_or(0), pid.unwrap_or(0)) {
                                                           let listener_fd_obj = unsafe { OwnedFd::from_raw_fd(fd) };
                                                           if let Err(e) = safe_epoll_add(&epoll, &listener_fd_obj, EpollEvent::new(EpollFlags::EPOLLIN, 6)) {
                                                               eprintln!("[main] Failed to add VLC helper listener to epoll: {}", e);
                                                           } else {
                                                               vlc_helper_listener_fd = Some(listener_fd_obj.into_raw_fd());
                                                               if vlc_window_id_changed {
                                                                   println!("[main] VLC helper restarted successfully for window ID: {:?}", window_id);
                                                               } else {
                                                                   println!("[main] VLC helper started successfully for window ID: {:?}", window_id);
                                                           }
                                                           }
                                                       } else {
                                                           println!("[main] Failed to start VLC helper for user: {}", user);
                                                       }
                                                   } else {
                                                       println!("[main] No current user available for VLC helper");
                                                   }
                                               } else {
                                                   // VLC/Dragon Player window lost focus - stop VLC helper
                                                   println!("[main] VLC/Dragon Player window lost focus, stopping VLC helper");
                                                   if let Some(stream) = vlc_helper_stream.take() {
                                                       if let Err(e) = safe_epoll_delete(&epoll, &stream) {
                                                           eprintln!("[main] Failed to remove VLC stream from epoll: {}", e);
                                                       }
                                                   }
                                                   vlc_helper_reader = None;
                                                   if let Some(fd) = vlc_helper_listener_fd.take() {
                                                       let listener_fd_obj = unsafe { OwnedFd::from_raw_fd(fd) };
                                                       if let Err(e) = safe_epoll_delete(&epoll, &listener_fd_obj) {
                                                           eprintln!("[main] Failed to remove VLC listener from epoll: {}", e);
                                                       }
                                                   }
                                                   if vlc_helper_manager.is_process_running() {
                                                   vlc_helper_manager.stop();
                                                       println!("[main] VLC helper stopped due to losing focus");
                                                   } else {
                                                       println!("[main] VLC helper was not running, no need to stop");
                                                   }
                                                   // Clear VLC drag position when losing focus
                                                   vlc_drag_position = None;
                                               }
                                               
                                               // Update the current VLC/Dragon Player window ID
                                               if new_vlc_focused {
                                                   if current_vlc_window_id != window_id {
                                                       println!("[main] VLC/Dragon Player window ID updated: {:?} -> {:?}", current_vlc_window_id, window_id);
                                                   }
                                                   current_vlc_window_id = window_id;
                                               } else {
                                                   if current_vlc_window_id.is_some() {
                                                       println!("[main] VLC/Dragon Player window ID cleared (lost focus)");
                                                   }
                                                   current_vlc_window_id = None;
                                               }
                                           }
                                           
                                           // Check if browser window focus changed
                                           let class_lower = class.to_lowercase();
                                           let new_browser_focused = class_lower == "firefox" || class_lower == "chrome" || class_lower == "chromium" || class_lower == "brave" || class_lower == "brave-browser" || class_lower == "edge" || class_lower == "safari" || class_lower == "opera" || class_lower == "google-chrome";
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
                                               
                                               if browser_window_id_changed {
                                                   println!("[main] Browser window ID changed from {:?} to {:?}, restarting browser helper", current_browser_window_id, window_id);
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
                                                           println!("[main] Browser helper stopped due to window ID change");
                                                       } else {
                                                           println!("[main] Browser helper was not running, no need to stop");
                                                       }
                                                   }
                                                   
                                                   if browser_window_id_changed {
                                                       println!("[main] Browser helper restarted for new window ID: {:?}", window_id);
                                                   } else {
                                                       if current_browser_window_id.is_none() {
                                                           println!("[main] Browser window focused for the first time, starting browser helper");
                                                       } else {
                                                           println!("[main] Browser window focused, starting browser helper");
                                                       }
                                                   }
                                                   
                                                   if let Some(user) = &current_user {
                                                       if let Some(fd) = browser_helper_manager.start(user, current_session.as_ref().and_then(|s| s.leader).unwrap_or(0), class, window_id.unwrap_or(0), pid.unwrap_or(0)) {
                                                           let listener_fd_obj = unsafe { OwnedFd::from_raw_fd(fd) };
                                                           if let Err(e) = safe_epoll_add(&epoll, &listener_fd_obj, EpollEvent::new(EpollFlags::EPOLLIN, 8)) {
                                                               eprintln!("[main] Failed to add browser helper listener to epoll: {}", e);
                                                           } else {
                                                               browser_helper_listener_fd = Some(listener_fd_obj.into_raw_fd());
                                                               if browser_window_id_changed {
                                                                   println!("[main] Browser helper restarted successfully for window ID: {:?}", window_id);
                                                               } else {
                                                                   println!("[main] Browser helper started successfully for window ID: {:?}", window_id);
                                                           }
                                                           }
                                                       } else {
                                                           println!("[main] Failed to start browser helper for user: {}", user);
                                                       }
                                                   } else {
                                                       println!("[main] No current user available for browser helper");
                                                   }
                                               } else {
                                                   // Browser window lost focus - stop browser helper
                                                   println!("[main] Browser window lost focus, stopping browser helper");
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
                                                       println!("[main] Browser helper stopped due to losing focus");
                                                   } else {
                                                       println!("[main] Browser helper was not running, no need to stop");
                                                   }
                                               }
                                               
                                               // Update the current browser window ID
                                               if new_browser_focused {
                                                   if current_browser_window_id != window_id {
                                                       println!("[main] Browser window ID updated: {:?} -> {:?}", current_browser_window_id, window_id);
                                                   }
                                                   current_browser_window_id = window_id;
                                               } else {
                                                   if current_browser_window_id.is_some() {
                                                       println!("[main] Browser window ID cleared (lost focus)");
                                                   }
                                                   current_browser_window_id = None;
                                               }
                                           } else if new_browser_focused && browser_window_focused {
                                               // Browser focus state is the same, but browser type might have changed
                                               if current_window_class.as_ref() != Some(&class.to_string()) {
                                                   // Browser type changed - stop and restart helper for clean state
                                                   println!("[main] Browser type changed from '{}' to '{}', restarting browser helper", 
                                                       current_window_class.as_deref().unwrap_or("unknown"), class);
                                                   
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
                                                       if let Some(fd) = browser_helper_manager.start(user, current_session.as_ref().and_then(|s| s.leader).unwrap_or(0), class, window_id.unwrap_or(0), pid.unwrap_or(0)) {
                                                           let listener_fd_obj = unsafe { OwnedFd::from_raw_fd(fd) };
                                                           if let Err(e) = safe_epoll_add(&epoll, &listener_fd_obj, EpollEvent::new(EpollFlags::EPOLLIN, 8)) {
                                                               eprintln!("[main] Failed to add browser helper listener to epoll: {}", e);
                                                           } else {
                                                               browser_helper_listener_fd = Some(listener_fd_obj.into_raw_fd());
                                                           }
                                                       }
                                                   } else {
                                                       println!("[main] No current user available for browser helper");
                                                   }
                                               }
                                           }
                                           
                                           // Update current window class and ID AFTER all the logic
                                           current_window_class = Some(class.to_string());
                                           current_window_id = window_id;
                                           
                                           // Update app UI manager with new window class
                                           app_ui_manager.update_app(&class).await;
                                           
                                           needs_complete_redraw = true;
                                       }
                                   } else {
                                       eprintln!("[main] DEBUG: Received invalid UTF-8 data");
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
                6 => { // VLC helper listener event
                    if let Some(stream) = vlc_helper_manager.accept_connection() {
                        if let Err(e) = stream.set_nonblocking(true) {
                            eprintln!("[main] Failed to set VLC stream non-blocking: {}", e);
                            continue;
                        }
                        println!("[main] VLC helper connected to socket.");
                        if let Err(e) = epoll.add(&stream, EpollEvent::new(EpollFlags::EPOLLIN, 7)) {
                            eprintln!("[main] Failed to add VLC stream to epoll: {}", e);
                            continue;
                        }
                        if let Ok(stream_clone) = stream.try_clone() {
                            vlc_helper_reader = Some(BufReader::new(stream_clone));
                            vlc_helper_stream = Some(stream);
                            // Stop listening for new connections
                            if let Some(fd) = vlc_helper_listener_fd.take() {
                                let listener_fd_obj = unsafe { OwnedFd::from_raw_fd(fd) };
                                if let Err(e) = safe_epoll_delete(&epoll, &listener_fd_obj) {
                                    eprintln!("[main] Failed to remove VLC listener from epoll: {}", e);
                                }
                            }
                        } else {
                            eprintln!("[main] Failed to clone VLC stream");
                        }
                    }
                }
                7 => { // VLC helper stream event
                    if let Some(reader) = &mut vlc_helper_reader {
                        loop {
                           let mut buf = vec![0; 1024];
                           match reader.get_mut().read(&mut buf) {
                               Ok(0) => { // EOF
                                   println!("[main] VLC helper disconnected.");
                                   if let Some(stream) = vlc_helper_stream.take() {
                                       if let Err(e) = safe_epoll_delete(&epoll, &stream) {
                                           eprintln!("[main] Failed to remove VLC stream from epoll: {}", e);
                                       }
                                   }
                                   vlc_helper_reader = None;
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
                                           
                                           // Handle VLC status message (plain JSON format)
                                           if let Ok(vlc_status) = serde_json::from_str::<serde_json::Value>(part) {
                                               // Update VLC screen with the status
                                               if let Some(is_playing) = vlc_status.get("is_playing").and_then(|v| v.as_bool()) {
                                                   if let Some(position) = vlc_status.get("position").and_then(|v| v.as_f64()) {
                                                       if let Some(duration) = vlc_status.get("duration").and_then(|v| v.as_i64()) {
                                                                                              // Create a MediaStatus struct and update the VLC screen
                                   let status = crate::helper::MediaStatus {
                                                               is_playing,
                                                               position,
                                                               duration,
                                                           };
                                                           
                                                           // If we have a drag position and VLC has updated to a new position,
                                                           // gradually fade out the drag position
                                                           if let Some(drag_pos) = vlc_drag_position {
                                                               if (position - drag_pos).abs() < 0.01 {
                                                                   // VLC has caught up to the drag position, clear it
                                                                   vlc_drag_position = None;
                                                                   println!("[main] VLC caught up to drag position, clearing drag");
                                                               }
                                                           }
                                                           
                                                           app_ui_manager.vlc_screen.last_status = Some(status);
                                                           needs_complete_redraw = true;
                                                       }
                                                   }
                                               }
                                           }
                                       }
                                   } else {
                                       eprintln!("[main] DEBUG: Received invalid UTF-8 data from VLC helper");
                                   }
                               },
                               Err(ref e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                                   break; // No more data right now
                               },
                               Err(e) => {
                                   eprintln!("[main] VLC helper stream error: {}", e);
                                   if let Some(stream) = vlc_helper_stream.take() {
                                       if let Err(e) = safe_epoll_delete(&epoll, &stream) {
                                           eprintln!("[main] Failed to remove VLC stream from epoll: {}", e);
                                       }
                                   }
                                   vlc_helper_reader = None;
                                   break;
                               }
                           }
                        }
                    }
                }
                8 => { // Browser helper listener event
                    if let Some(mut stream) = browser_helper_manager.accept_connection() {
                        if let Err(e) = safe_stream_set_nonblocking(&stream, true) {
                            eprintln!("[main] Failed to set browser stream non-blocking: {}", e);
                            continue;
                        }
                        println!("[main] Browser helper connected to socket.");
                        
                        // Send browser type to the helper as the first message
                        if let Some(window_class) = &current_window_class {
                            // Send the EXACT window class that triggered the browser focus
                            let browser_type_msg = format!("browser_type:{}\n", window_class);
                            if let Err(e) = stream.write_all(browser_type_msg.as_bytes()) {
                                eprintln!("[main] Failed to send browser type to helper: {}", e);
                            } else {
                                println!("[main] Sent exact browser type '{}' to browser helper", window_class);
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
                9 => { // Browser helper stream event
                    if let Some(reader) = &mut browser_helper_reader {
                        loop {
                           let mut buf = vec![0; 1024];
                           match reader.get_mut().read(&mut buf) {
                               Ok(0) => { // EOF
                                   println!("[main] Browser helper disconnected.");
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
                                       eprintln!("[main] DEBUG: Received invalid UTF-8 data from browser helper");
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
                &mut vlc_touch_active,
                &mut vlc_drag_position,
                &mut vlc_helper_stream,
                &mut browser_helper_stream,
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
            vlc_helper_manager.check_process_status();
        })) {
            eprintln!("[main] Error during VLC/Dragon Player helper manager status check: {:?}", e);
        }
        if let Err(e) = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            browser_helper_manager.check_process_status();
        })) {
            eprintln!("[main] Error during browser helper manager status check: {:?}", e);
        }
        
        // Debug: Log process status periodically
        static mut PROCESS_STATUS_COUNTER: u64 = 0;
        unsafe {
            PROCESS_STATUS_COUNTER += 1;
            if PROCESS_STATUS_COUNTER % 1000 == 0 { // Log every 1000 frames
                println!("[main] Process status - Main helper: {}, VLC/Dragon Player helper: {} (window ID: {:?}), Browser helper: {} (window ID: {:?})", 
                    if helper_manager.is_process_running() { "running" } else { "stopped" },
                    if vlc_helper_manager.is_process_running() { "running" } else { "stopped" },
                    current_vlc_window_id,
                    if browser_helper_manager.is_process_running() { "running" } else { "stopped" },
                    current_browser_window_id
                );
            }
        }
        
        // Force cleanup of any zombie processes every 10000 frames (less frequent to reduce overhead)
        static mut FORCE_CLEANUP_COUNTER: u64 = 0;
        unsafe {
            FORCE_CLEANUP_COUNTER += 1;
            if FORCE_CLEANUP_COUNTER % 10000 == 0 { // Every 10000 frames (reduced frequency)
                if DEBUG_LOGGING {
                    println!("[main] Performing forced cleanup of zombie processes");
                }
                // Wrap cleanup calls in error handling to prevent crashes
                if let Err(e) = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    helper_manager.force_cleanup();
                })) {
                    eprintln!("[main] Error during helper manager cleanup: {:?}", e);
                }
                if let Err(e) = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    vlc_helper_manager.force_cleanup();
                })) {
                    eprintln!("[main] Error during VLC helper manager cleanup: {:?}", e);
                }
                if let Err(e) = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    browser_helper_manager.force_cleanup();
                })) {
                    eprintln!("[main] Error during browser helper manager cleanup: {:?}", e);
                }
            }
        }
        
        // Check if we can start the helper now that session might be ready
        if let Some(user) = &current_user {
                            if helper_manager.is_process_none() && helper_manager.check_session_ready() {
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
        }
        
        // Performance optimization: Frame rate limiting to maintain 60fps
        let frame_duration = frame_start.elapsed();
        let target_frame_time = std::time::Duration::from_millis(16); // 60fps = 16.67ms per frame
        
        if frame_duration < target_frame_time {
            let sleep_time = target_frame_time - frame_duration;
            std::thread::sleep(sleep_time);
        }
        
        // Process session events (event-driven)
    }
}

