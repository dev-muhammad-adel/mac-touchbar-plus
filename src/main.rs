use std::{
    fs::{File, OpenOptions},
    os::{
        fd::{AsRawFd, AsFd, IntoRawFd},
        unix::{io::{OwnedFd, FromRawFd}, fs::OpenOptionsExt, net::UnixStream},
    },
    path::Path,
    collections::HashMap,
    cmp::min,
};
use std::io::{BufReader, Read, Write};
use std::sync::Arc;
use cairo::{ImageSurface, Format, Context, Surface, Rectangle, FontSlant, FontWeight};
use rsvg::CairoRenderer;
use drm::control::ClipRect;
use anyhow::Result;
use input::{
    Libinput, LibinputInterface, Device as InputDevice,
    event::{
        Event, device::DeviceEvent, EventTrait,
        touch::{TouchEvent, TouchEventPosition, TouchEventSlot},
        keyboard::{KeyboardEvent, KeyboardEventTrait, KeyState}
    }
};
use libc::{O_ACCMODE, O_RDONLY, O_RDWR, O_WRONLY, c_char};
use input_linux::{uinput::UInputHandle, EventKind, Key, SynchronizeKind};
use input_linux_sys::{uinput_setup, input_id, timeval, input_event};
use nix::{
    sys::{
        signal::{Signal, SigSet, SigAction, SigHandler, SaFlags},
        epoll::{Epoll, EpollCreateFlags, EpollEvent, EpollFlags}
    },
    sys::eventfd::{eventfd, EfdFlags},
};

use chrono::{Local, Timelike};
use crate::services::sessionmanager::{SessionState, monitor_sessions};
use tokio::sync::{watch, mpsc};
use view::media_screen::draw_media_section;
use view::app_ui_manager::{AppUiManager, AppAction};
use view::vlc_screen::VlcAction;
use view::browser_screen::BrowserAction;

// Import the utils module
mod utils;
use crate::utils::button_images::{self, ICON_SIZE};

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

fn safe_cairo_context(surface: &Surface) -> MainResult<Context> {
    Context::new(surface)
        .map_err(|e| MainError::Cairo(format!("Failed to create Cairo context: {}", e)))
}

fn safe_cairo_text_extents(c: &Context, text: &str) -> MainResult<cairo::TextExtents> {
    c.text_extents(text)
        .map_err(|e| MainError::Cairo(format!("Failed to get text extents: {}", e)))
}

fn safe_cairo_show_text(c: &Context, text: &str) -> MainResult<()> {
    c.show_text(text)
        .map_err(|e| MainError::Cairo(format!("Failed to show text: {}", e)))
}

fn safe_cairo_paint(c: &Context) -> MainResult<()> {
    c.paint()
        .map_err(|e| MainError::Cairo(format!("Failed to paint: {}", e)))
}

fn safe_cairo_fill(c: &Context) -> MainResult<()> {
    c.fill()
        .map_err(|e| MainError::Cairo(format!("Failed to fill: {}", e)))
}

fn safe_cairo_set_source_surface(c: &Context, surface: &Surface, x: f64, y: f64) -> MainResult<()> {
    c.set_source_surface(surface, x, y)
        .map_err(|e| MainError::Cairo(format!("Failed to set source surface: {}", e)))
}

fn safe_cairo_render_document(renderer: &CairoRenderer, c: &Context, rect: &Rectangle) -> MainResult<()> {
    renderer.render_document(c, rect)
        .map_err(|e| MainError::Cairo(format!("Failed to render document: {}", e)))
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

// Add log level control at the top
// Set to true to enable verbose debug logging, false for production (much less resource usage)
const DEBUG_LOGGING: bool = false; // Set to false to disable verbose logging

// Layer switching behavior:
// - When user is not logged in: Custom2 layer (AppLayerKeys2) is active
// - When user logs in: Media layer becomes active
// - When user logs out: Custom2 layer becomes active again

// Helper function to send commands to VLC helper
fn send_vlc_command(stream: &mut UnixStream, command: &str) -> Result<(), std::io::Error> {
    let command_with_newline = format!("{}\n", command);
    stream.write_all(command_with_newline.as_bytes())?;
    Ok(())
}

// Helper function to send commands to browser helper
fn send_browser_command(stream: &mut UnixStream, command: &str) -> Result<(), std::io::Error> {
    let command_with_newline = format!("{}\n", command);
    stream.write_all(command_with_newline.as_bytes())?;
    Ok(())
}

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
use display::pixel_shift::{PixelShiftManager, PIXEL_SHIFT_WIDTH_PX};
use config::{ButtonConfig, Config};
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

const BUTTON_SPACING_PX: i32 = 16;
const APP_LAYER_KEYS2_GAP_PX: f64 = 4.0; // Custom gap for AppLayerKeys2 (Custom2)
const APP_LAYER_KEYS3_GAP_PX: f64 = 4.0; // Custom gap for AppLayerKeys3
const BUTTON_COLOR_INACTIVE: f64 = 0.172;
const BUTTON_COLOR_ACTIVE: f64 = 0.350;
const TIMEOUT_MS: i32 = 10 * 1000;

pub struct Button {
    image: button_images::ButtonImage,
    changed: bool,
    active: bool,
    action: Key,
    background: bool,
    fraction: Option<f32>,
}









impl Button {
    fn with_config(cfg: ButtonConfig) -> Button {
        let background;
        if let Some(text) = cfg.text {
            if let Some(bg) = cfg.background {
                background = bg;
            } else {
                background = true;
            }
            Button::new_text(text, cfg.action, background)
        } else if let Some(icon) = cfg.icon {
            let path = match cfg.path {
                Some(p) => p,
                None => "use_default".to_string()
            };
            if let Some(bg) = cfg.background {
                background = bg;
            } else {
                if let Some(ref mode) = cfg.mode {
                    if mode.to_lowercase() == "app" {
                        background = false;
                    } else {
                        background = true;
                    }
                } else {
                    panic!("Invalid config, a button must have either Text, Icon or be Blank")
                }
            }
            let mut btn = Button::new_icon(&icon, cfg.action, cfg.mode, &path, background);
            btn.fraction = cfg.fraction;
            btn
        } else if let Some(mode) = cfg.mode {
            if let Some(bg) = cfg.background {
                background = bg;
            } else {
                background = false;
            }
            if mode.to_lowercase() == "blank" {
                let mut btn = Button::new_blank(cfg.action, background);
                btn.fraction = cfg.fraction;
                btn
            } else {
                panic!("Invalid config, a button must have either Text, Icon or be Blank")
            }
        } else {
            panic!("Invalid config, a button must have either Text, Icon or be Blank")
        }
    }
    fn new_text(text: String, action: Key, background: bool) -> Button {
        Button {
            action,
            active: false,
            changed: false,
            image: button_images::ButtonImage::Text(text),
            background,
            fraction: None,
        }
    }
    fn new_icon(icon_name: &str, action: Key, mode: Option<String>, path: &str, background: bool) -> Button {
        let theme = ConfigManager::new().load_theme();
        let icon_theme = match &mode {
            Some(mode_val) => {
                if mode_val == "App" {theme.app_icon_theme} else {theme.media_icon_theme}
            }
            None => {
                panic!("No mode specified")
            }
        };
        
                    let image = button_images::load_image(icon_name, mode, path, &icon_theme)
                .or_else(|_| button_images::try_load_svg_path(icon_name, path))
                .or_else(|_| button_images::try_load_png_path(icon_name, path))
                .unwrap_or_else(|_| button_images::ButtonImage::Text(icon_name.to_string()));
        Button {
            action, image,
            active: false,
            changed: false,
            background,
            fraction: None,
        }
    }
    fn new_blank(action: Key, background: bool) -> Button {
        Button {
            action,
            active: false,
            changed: false,
            image: button_images::ButtonImage::Blank,
            background,
            fraction: None,
        }
    }
    fn render(&self, c: &Context, height: i32, button_left_edge: f64, button_width: u64, y_shift: f64) -> MainResult<()> {
        match &self.image {
            button_images::ButtonImage::Text(text) => {
                let extents = safe_cairo_text_extents(c, text)?;
                c.move_to(
                    button_left_edge + (button_width as f64 / 2.0 - extents.width() / 2.0).round(),
                    y_shift + (height as f64 / 2.0 + extents.height() / 2.0).round()
                );
                safe_cairo_show_text(c, text)?;
            },
            button_images::ButtonImage::Svg(svg) => {
                let renderer = CairoRenderer::new(&svg);
                let x = button_left_edge + (button_width as f64 / 2.0 - (ICON_SIZE / 2) as f64).round();
                let y = y_shift + ((height as f64 - ICON_SIZE as f64) / 2.0).round();

                safe_cairo_render_document(&renderer, c, &Rectangle::new(x, y, ICON_SIZE as f64, ICON_SIZE as f64))?;
            }
            button_images::ButtonImage::Bitmap(surf) => {
                let x = button_left_edge + (button_width as f64 / 2.0 - (ICON_SIZE / 2) as f64).round();
                let y = y_shift + ((height as f64 - ICON_SIZE as f64) / 2.0).round();
                safe_cairo_set_source_surface(c, surf, x, y)?;
                c.rectangle(x, y, ICON_SIZE as f64, ICON_SIZE as f64);
                safe_cairo_fill(c)?;
            }
            _ => {
            }
        }
        Ok(())
    }
    fn set_active<F>(&mut self, uinput: &mut UInputHandle<F>, active: bool) where F: AsRawFd {
        if self.active != active {
            self.active = active;
            self.changed = true;

            toggle_key(uinput, self.action, active as i32);
        }
    }
}

#[derive(Default)]
pub struct FunctionLayer {
    buttons: Vec<Button>,
    split: Option<SplitLayout>,
}

pub struct SplitLayout {
    pub modules_width: f32,
    pub media: Vec<Button>,
    pub media_width: f32,
}

impl FunctionLayer {
    fn with_config(cfg: Vec<ButtonConfig>) -> FunctionLayer {
        if cfg.is_empty() {
            panic!("Invalid configuration, layer has 0 buttons");
        }
        FunctionLayer {
            buttons: cfg.into_iter().map(Button::with_config).collect(),
            split: None,
        }
    }
    fn with_split(modules_width: f32, media: Vec<ButtonConfig>, media_width: f32) -> FunctionLayer {
        FunctionLayer {
            buttons: vec![],
            split: Some(SplitLayout {
                modules_width,
                media: media.into_iter().map(Button::with_config).collect(),
                media_width,
            }),
        }
    }
    fn draw(&mut self, config: &Config, width: i32, height: i32, surface: &Surface, pixel_shift: (f64, f64), complete_redraw: bool, modules_only_redraw: bool, session_state: Option<&SessionState>, layer_index: Option<LayerKey>, app_layer3_slide_progress: f64, current_window_class: Option<&str>, mut app_ui_manager: Option<&mut AppUiManager>, vlc_drag_position: Option<f64>) -> MainResult<Vec<ClipRect>> {
        match &mut self.split {
            Some(split) => {
                let c = safe_cairo_context(&surface)?;
                let mut modified_regions = if complete_redraw {
                    vec![ClipRect::new(0, 0, height as u16, width as u16)]
                } else {
                    Vec::new()
                };
                c.translate(height as f64, 0.0);
                c.rotate((90.0f64).to_radians());
                let pixel_shift_width = if config.enable_pixel_shift { PIXEL_SHIFT_WIDTH_PX } else { 0 };
                let total_width = (width - pixel_shift_width as i32) as f64;
                let group_spacing = BUTTON_SPACING_PX as f64; // space between groups
                let modules_width = (split.modules_width as f64 * total_width).round();
                let media_width = total_width - modules_width - group_spacing;
                let media_count = split.media.len();
                let _media_spacing = if media_count > 1 { BUTTON_SPACING_PX as f64 * (media_count as f64 - 1.0) } else { 0.0 };
         
                // --- MEDIA BUTTON WIDTHS WITH FRACTION ---
                let media_spacing_px = 2.0f64; // 2px spacing for AppLayerKeys1Media
                let total_spacing = if media_count > 1 { media_spacing_px * (media_count as f64 - 1.0) } else { 0.0 };
                let button_area = media_width - total_spacing;
                let weights: Vec<f32> = split.media.iter().map(|b| b.fraction.unwrap_or(1.0)).collect();
                let total_weight: f32 = weights.iter().sum();
                let mut media_button_widths: Vec<f64> = weights.iter().map(|w| button_area * (*w as f64 / total_weight as f64)).collect();
                // Last button absorbs rounding error
                let sum_widths: f64 = media_button_widths.iter().sum();
                if let Some(last) = media_button_widths.last_mut() {
                    *last += button_area - sum_widths;
                }
                let radius = 8.0f64;
                let bot = (height as f64) * 0.15;
                let top = (height as f64) * 0.85;
                let (pixel_shift_x, _pixel_shift_y) = pixel_shift;
                if complete_redraw {
                    c.set_source_rgb(0.0, 0.0, 0.0);
                    safe_cairo_paint(&c)?;
                } else if modules_only_redraw {
                    // Only clear the modules area for modules-only redraw
                    c.set_source_rgb(0.0, 0.0, 0.0);
                    c.rectangle(pixel_shift_x + (pixel_shift_width / 2) as f64, bot - radius, modules_width, top - bot + radius * 2.0);
                    safe_cairo_fill(&c)?;
                }
                if config.font_renderer.to_lowercase() == "cairo" {
                    c.select_font_face(&config.font_style_cairo, if config.italic_cairo {FontSlant::Italic} else {FontSlant::Normal}, if config.bold_cairo {FontWeight::Bold} else {FontWeight::Normal});
                } else if config.font_renderer.to_lowercase() == "freetype" {
                    c.set_font_face(&config.font_face);
                } else { panic!("Invalid font renderer chosen. Choose between \"Cairo\" and \"FreeType\""); }
                c.set_font_size(32.0);
                
                // Clear modules area first to prevent text overlap when switching between session states
                // This ensures old text doesn't remain visible when drawing new content
                let left_edge = pixel_shift_x + (pixel_shift_width / 2) as f64;
                c.set_source_rgb(0.0, 0.0, 0.0);
                c.rectangle(left_edge, bot - radius, modules_width, top - bot + radius * 2.0);
                safe_cairo_fill(&c)?;
                
                // Use new session state
                match session_state {
                    Some(state) if state.is_logged_in => {
                        // User is logged in - show normal modules
                        // Always use app UI manager for consistent module screen drawing
                            if let Some(app_ui_manager) = &mut app_ui_manager {
                                app_ui_manager.draw_app_ui(
                                    &c,
                                    left_edge,
                                    bot,
                                    modules_width,
                                    top - bot,
                                    radius,
                                    1.0, // Always fully visible
                                current_window_class.as_deref(), // Pass Option<&str> to handle None case
                                    vlc_drag_position, // Pass drag position for visual feedback
                                    &mut modified_regions,
                                );
                        }
                    }
                    Some(state) if !state.is_logged_in => {
                        // Show simple module screen when not logged in
                        if let Some(app_ui_manager) = &mut app_ui_manager {
                            app_ui_manager.draw_app_ui(
                            &c,
                            left_edge,
                            bot,
                            modules_width,
                            top - bot,
                            radius,
                            1.0, // Always fully visible
                                Some("Not Logged In"), // Pass as Some for consistent handling
                                None, // No drag position
                                &mut modified_regions,
                        );
                        }
                    }
                    _ => {
                        // Handle None or unknown status if needed
                    }
                }
                
                // Add spacing between modules and media sections
                let left_edge = pixel_shift_x + (pixel_shift_width / 2) as f64 + modules_width + group_spacing;
            
                // Skip media section if this is a modules-only redraw
                if !modules_only_redraw {
                    // Draw media section
                    let media_spacing_px = 2.0f64; // 2px spacing for AppLayerKeys1Media
                    let total_spacing = if media_count > 1 { media_spacing_px * (media_count as f64 - 1.0) } else { 0.0 };
                    let button_area = media_width - total_spacing;
                    let weights: Vec<f32> = split.media.iter().map(|b| b.fraction.unwrap_or(1.0)).collect();
                    let total_weight: f32 = weights.iter().sum();
                    let mut media_button_widths: Vec<f64> = weights.iter().map(|w| button_area * (*w as f64 / total_weight as f64)).collect();
                    let sum_widths: f64 = media_button_widths.iter().sum();
                    if let Some(last) = media_button_widths.last_mut() {
                        *last += button_area - sum_widths;
                    }
                    draw_media_section(
                        &c,
                        &mut split.media,
                        &media_button_widths,
                        media_width,
                        media_count,
                        left_edge,
                        bot,
                        top,
                        radius,
                        height,
                        config,
                        complete_redraw,
                        &mut modified_regions,
                        session_state,
                    );
                }
                
                Ok(modified_regions)
            }
        
            None => {
                let c = safe_cairo_context(&surface)?;
                let mut modified_regions = if complete_redraw {
                    vec![ClipRect::new(0, 0, height as u16, width as u16)]
                } else {
                    Vec::new()
                };
                c.translate(height as f64, 0.0);
                c.rotate((90.0f64).to_radians());
                let pixel_shift_width = if config.enable_pixel_shift { PIXEL_SHIFT_WIDTH_PX } else { 0 };
                // Use custom gap for AppLayerKeys2/3, else default
                let gap = if let Some(layer) = layer_index {
                    match layer {
                        LayerKey::Custom2 => APP_LAYER_KEYS2_GAP_PX,
                        LayerKey::Custom3 => APP_LAYER_KEYS3_GAP_PX,
                        _ => BUTTON_SPACING_PX as f64,
                    }
                } else {
                    BUTTON_SPACING_PX as f64
                };
                // --- AppLayerKeys3 slide animation translation ---
                if let Some(LayerKey::Custom3) = layer_index {
                    // If progress is 0.0, skip drawing (prevents flicker)
                    if app_layer3_slide_progress == 0.0 {
                        return Ok(modified_regions);
                    }
                    // Slide in: progress 0.0 (off right) to 1.0 (onscreen)
                    // Slide out: progress 1.0 (onscreen) to 0.0 (off left)
                    let slide_offset = if app_layer3_slide_progress < 1.0 {
                        let direction = if app_layer3_slide_progress > 0.0 { 1.0 } else { -1.0 };
                        if direction > 0.0 {
                            (1.0 - app_layer3_slide_progress) * width as f64
                        } else {
                            -app_layer3_slide_progress * width as f64
                        }
                    } else {
                        0.0
                    };
                    c.translate(slide_offset, 0.0);
                }
                // --- FLAT BUTTON WIDTHS WITH FRACTION ---
                let count = self.buttons.len();
                let spacing = if count > 1 { gap * (count as f64 - 1.0) } else { 0.0 };
                let button_area = (width as f64 - pixel_shift_width as f64) - spacing;
                let weights: Vec<f32> = self.buttons.iter().map(|b| b.fraction.unwrap_or(1.0)).collect();
                let total_weight: f32 = weights.iter().sum();
                let mut button_widths: Vec<f64> = weights.iter().map(|w| button_area * (*w as f64 / total_weight as f64)).collect();
                // Last button absorbs rounding error
                let sum_widths: f64 = button_widths.iter().sum();
                if let Some(last) = button_widths.last_mut() {
                    *last += button_area - sum_widths;
                }
                let radius = 8.0f64;
                let bot = (height as f64) * 0.15;
                let top = (height as f64) * 0.85;
                let (pixel_shift_x, pixel_shift_y) = pixel_shift;
                if complete_redraw {
                    c.set_source_rgb(0.0, 0.0, 0.0);
                    safe_cairo_paint(&c)?;
                }
                if config.font_renderer.to_lowercase() == "cairo" {
                    c.select_font_face(&config.font_style_cairo, if config.italic_cairo {FontSlant::Italic} else {FontSlant::Normal}, if config.bold_cairo {FontWeight::Bold} else {FontWeight::Normal});
                } else if config.font_renderer.to_lowercase() == "freetype" {
                    c.set_font_face(&config.font_face);
                } else { panic!("Invalid font renderer chosen. Choose between \"Cairo\" and \"FreeType\""); }
                c.set_font_size(32.0);
                let mut left_edge = pixel_shift_x + (pixel_shift_width / 2) as f64;
                for (i, button) in self.buttons.iter_mut().enumerate() {
                    let this_button_width = button_widths[i];
                    if !button.changed && !complete_redraw {
                        left_edge += this_button_width;
                        if i != count - 1 {
                            left_edge += gap;
                        }
                        continue;
                    };
                    let color = if button.active {
                        BUTTON_COLOR_ACTIVE
                    } else if config.show_button_outlines {
                        BUTTON_COLOR_INACTIVE
                    } else {
                        0.0
                    };
                    if !complete_redraw {
                        c.set_source_rgb(0.0, 0.0, 0.0);
                        c.rectangle(left_edge, bot - radius, this_button_width, top - bot + radius * 2.0);
                        safe_cairo_fill(&c)?;
                    }
                    if (button.action != Key::Unknown &&
                       button.action != Key::Macro1 &&
                       button.action != Key::Macro2 &&
                       button.action != Key::Macro3 &&
                       button.action != Key::Macro4) &&
                       ((button.background) ||
                        button.active) {
                    c.set_source_rgb(color, color, color);
                    // draw box with rounded corners
                    c.new_sub_path();
                    let left = left_edge + radius;
                    let right = (left_edge + this_button_width.ceil()) - radius;
                    c.arc(
                        right,
                        bot,
                        radius,
                        (-90.0f64).to_radians(),
                        (0.0f64).to_radians(),
                    );
                    c.arc(
                        right,
                        top,
                        radius,
                        (0.0f64).to_radians(),
                        (90.0f64).to_radians(),
                    );
                    c.arc(
                        left,
                        top,
                        radius,
                        (90.0f64).to_radians(),
                        (180.0f64).to_radians(),
                    );
                    c.arc(
                        left,
                        bot,
                        radius,
                        (180.0f64).to_radians(),
                        (270.0f64).to_radians(),
                    );
                    c.close_path();

                    safe_cairo_fill(&c)?;
                    }
                    c.set_source_rgb(1.0, 1.0, 1.0);
                    button.render(&c, height, left_edge, this_button_width.ceil() as u64, pixel_shift_y)?;

                    button.changed = false;

                    if !complete_redraw {
                        modified_regions.push(ClipRect::new(
                            height as u16 - top as u16 - radius as u16,
                            left_edge as u16,
                            height as u16 - bot as u16 + radius as u16,
                            left_edge as u16 + this_button_width as u16
                        ));
                    }
                    left_edge += this_button_width;
                    if i != count - 1 {
                        left_edge += gap;
                    }
                }
                Ok(modified_regions)
            }


        }
        }
    
    // Helper for modules hit test
    fn hit_test_modules(&self, x: f64, width: i32, layer_index: Option<LayerKey>) -> Option<usize> {
        if let Some(split) = &self.split {
            let group_spacing = if let Some(layer) = layer_index {
                match layer {
                    LayerKey::Custom2 => APP_LAYER_KEYS2_GAP_PX,
                    LayerKey::Custom3 => APP_LAYER_KEYS3_GAP_PX,
                    _ => BUTTON_SPACING_PX as f64,
                }
            } else {
                BUTTON_SPACING_PX as f64
            };
            let total_width = (width - group_spacing as i32) as f64;
            let modules_width = (split.modules_width as f64 * total_width).round();
            if x >= 0.0 && x < modules_width {
                return Some(0);
            }
        }
        None
    }
    // Helper for media hit test
    fn hit_test_media(&self, x: f64, width: i32, layer_index: Option<LayerKey>) -> Option<usize> {
        if let Some(split) = &self.split {
            let group_spacing = if let Some(layer) = layer_index {
                match layer {
                    LayerKey::Custom2 => APP_LAYER_KEYS2_GAP_PX,
                    LayerKey::Custom3 => APP_LAYER_KEYS3_GAP_PX,
                    _ => BUTTON_SPACING_PX as f64,
                }
            } else {
                BUTTON_SPACING_PX as f64
            };
            let total_width = (width - group_spacing as i32) as f64;
            let modules_width = (split.modules_width as f64 * total_width).round();
            let media_width = total_width - modules_width - group_spacing;
            let media_count = split.media.len();
            let media_spacing_px = 2.0f64;
            let total_spacing = if media_count > 1 { media_spacing_px * (media_count as f64 - 1.0) } else { 0.0 };
            let button_area = media_width - total_spacing;
            let weights: Vec<f32> = split.media.iter().map(|b| b.fraction.unwrap_or(1.0)).collect();
            let total_weight: f32 = weights.iter().sum();
            let mut media_button_widths: Vec<f64> = weights.iter().map(|w| button_area * (*w as f64 / total_weight as f64)).collect();
            let sum_widths: f64 = media_button_widths.iter().sum();
            if let Some(last) = media_button_widths.last_mut() {
                *last += button_area - sum_widths;
            }
            // SIMPLIFIED: media section starts after modules_width + group_spacing
            let mut left_edge = modules_width + group_spacing;
            for (i, _) in split.media.iter().enumerate() {
                let right_edge = left_edge + media_button_widths[i];
                if x >= left_edge && x < right_edge {
                    return Some(i);
                }
                left_edge = right_edge;
                if i != media_count - 1 {
                    left_edge += media_spacing_px;
                }
            }
        }
        None
    }
    // Helper for flat hit test
    fn hit_test_flat(&self, x: f64, width: i32, layer_index: Option<LayerKey>) -> Option<usize> {
        if self.split.is_none() {
            let count = self.buttons.len();
            let gap = if let Some(layer) = layer_index {
                match layer {
                    LayerKey::Custom2 => APP_LAYER_KEYS2_GAP_PX,
                    LayerKey::Custom3 => APP_LAYER_KEYS3_GAP_PX,
                    _ => BUTTON_SPACING_PX as f64,
                }
            } else {
                BUTTON_SPACING_PX as f64
            };
            let spacing = if count > 1 { gap * (count as f64 - 1.0) } else { 0.0 };
            let button_area = (width as f64) - spacing;
            let weights: Vec<f32> = self.buttons.iter().map(|b| b.fraction.unwrap_or(1.0)).collect();
            let total_weight: f32 = weights.iter().sum();
            let mut button_widths: Vec<f64> = weights.iter().map(|w| button_area * (*w as f64 / total_weight as f64)).collect();
            let sum_widths: f64 = button_widths.iter().sum();
            if let Some(last) = button_widths.last_mut() {
                *last += button_area - sum_widths;
            }
            let mut left_edge = 0.0;
            for (i, _) in self.buttons.iter().enumerate() {
                let right_edge = left_edge + button_widths[i];
                if x >= left_edge && x < right_edge {
                    return Some(i);
                }
                left_edge = right_edge;
                if i != count - 1 {
                    left_edge += gap;
                }
            }
        }
        None
    }
    /// Returns (group, index) where group is "modules" or "media" or "flat", and index is the button index in that group
    pub fn hit_test(&self, x: f64, width: i32, layer_index: Option<LayerKey>) -> Option<(&'static str, usize)> {
        if self.split.is_some() {
            if let Some(idx) = self.hit_test_modules(x, width, layer_index) {
                return Some(("modules", idx));
            }
            if let Some(idx) = self.hit_test_media(x, width, layer_index) {
                return Some(("media", idx));
            }
            None
        } else {
            if let Some(idx) = self.hit_test_flat(x, width, layer_index) {
                return Some(("flat", idx));
            }
            None
        }
    }
}

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




fn emit<F>(uinput: &mut UInputHandle<F>, ty: EventKind, code: u16, value: i32) where F: AsRawFd {
    if let Err(e) = uinput.write(&[input_event {
        value: value,
        type_: ty as u16,
        code: code,
        time: timeval {
            tv_sec: 0,
            tv_usec: 0
        }
    }]) {
        eprintln!("[main] Failed to emit uinput event: {}", e);
    }
}

pub fn toggle_key<F>(uinput: &mut UInputHandle<F>, code: Key, value: i32) where F: AsRawFd {
    emit(uinput, EventKind::Key, code as u16, value);
    emit(uinput, EventKind::Synchronize, SynchronizeKind::Report as u16, 0);
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
    
    // Safe uinput device creation
    let uinput_file = OpenOptions::new()
        .write(true)
        .open("/dev/uinput")
        .map_err(|e| MainError::Input(e))?;
    let mut uinput = UInputHandle::new(uinput_file);
    
    let mut backlight = BacklightManager::new()
        .map_err(|e| MainError::Display(format!("Failed to initialize backlight manager: {}", e)))?;
    let mut last_redraw_minute = Local::now().minute();
    let mut cfg_mgr = ConfigManager::new();
    let (mut cfg, mut layers) = cfg_mgr.load_config(width);
    let mut pixel_shift = PixelShiftManager::new();
    let mut helper_manager = HelperManager::new();
    let mut vlc_helper_manager = VlcHelperManager::new();
    let mut browser_helper_manager = BrowserHelperManager::new();
    let mut browser_helper_listener_fd: Option<i32> = None;
    let mut browser_helper_stream: Option<UnixStream> = None;
    let mut browser_helper_reader: Option<BufReader<UnixStream>> = None;
    
    // Add focus-based VLC helper management
    let mut vlc_window_focused = false;
    let mut browser_window_focused = false;
    let _last_window_class: Option<String> = None;
    let mut current_user: Option<String> = None;

    // Privilege dropping removed - run with appropriate permissions

    // Safe surface creation
    let mut surface = ImageSurface::create(Format::ARgb32, db_width as i32, db_height as i32)
        .map_err(|e| MainError::Cairo(format!("Failed to create image surface: {}", e)))?;
    
    // Start with Custom2 layer since user starts as not logged in
    let mut active_layer = LayerKey::Custom2;
    let mut last_layer = active_layer.clone();
    let mut pending_layer: Option<LayerKey> = None;

    let mut input_tb = Libinput::new_with_udev(Interface);
    let mut input_main = Libinput::new_with_udev(Interface);
    
    // Safe seat assignment
    input_tb.udev_assign_seat("seat-touchbar")
        .map_err(|_| MainError::Input(std::io::Error::new(std::io::ErrorKind::Other, "Failed to assign touch bar seat")))?;
    input_main.udev_assign_seat("seat0")
        .map_err(|_| MainError::Input(std::io::Error::new(std::io::ErrorKind::Other, "Failed to assign main seat")))?;
    
    // Safe epoll creation
    let epoll = Epoll::new(EpollCreateFlags::empty())
        .map_err(|e| MainError::Epoll(e))?;
    
    safe_epoll_add(&epoll, &input_main.as_fd(), EpollEvent::new(EpollFlags::EPOLLIN, 0))?;
    safe_epoll_add(&epoll, &input_tb.as_fd(), EpollEvent::new(EpollFlags::EPOLLIN, 1))?;
    safe_epoll_add(&epoll, &cfg_mgr.fd(), EpollEvent::new(EpollFlags::EPOLLIN, 2))?;
    
    // --- eventfd integration ---
    let event_fd = Arc::new(eventfd(0, EfdFlags::EFD_NONBLOCK)
        .map_err(|e| MainError::Epoll(e))?);
    safe_epoll_add(&epoll, &*event_fd, EpollEvent::new(EpollFlags::EPOLLIN, 3))?;
    // --- end eventfd integration ---
    
    uinput.set_evbit(EventKind::Key).map_err(|e| MainError::Input(std::io::Error::new(std::io::ErrorKind::Other, e)))?;
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
    // Browser buttons use Key::Unknown (no uinput events needed)
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

    let mut digitizer: Option<InputDevice> = None;
    let mut touches = HashMap::new();

    // Initialize session monitor
    let (session_tx, session_rx) = watch::channel(SessionState {
        session_type: "".to_string(),  // Empty string = no session type detected yet
        is_logged_in: false,
        user: "".to_string(),
        leader: None,
    });
    tokio::spawn(monitor_sessions(session_tx));

    // Create mpsc channel for event-driven session updates
    let (event_tx, mut event_rx) = mpsc::unbounded_channel();
    let mut session_rx_clone = session_rx.clone();
    let event_tx_clone = event_tx.clone();
    // --- eventfd integration for tokio task ---
    let event_fd_clone = Arc::clone(&event_fd);
    tokio::spawn(async move {
        while session_rx_clone.changed().await.is_ok() {
            let new_state = session_rx_clone.borrow().clone();
            let _ = event_tx_clone.send(new_state);
            // Write to eventfd to wake up main loop
            let val: u64 = 1;
            let _ = nix::unistd::write(event_fd_clone.as_raw_fd(), &val.to_ne_bytes());
        }
    });
    // --- end eventfd integration ---
    let mut current_session: Option<SessionState> = None;
    let mut helper_listener_fd: Option<i32> = None;
    let mut helper_stream: Option<UnixStream> = None;
    let mut helper_reader: Option<BufReader<UnixStream>> = None;
    let mut vlc_helper_listener_fd: Option<i32> = None;
    let mut vlc_helper_stream: Option<UnixStream> = None;
    let mut vlc_helper_reader: Option<BufReader<UnixStream>> = None;
    let mut current_window_class: Option<String> = None;
    let mut needs_complete_redraw = false;
    let mut app_layer3_slide_anim = Animation::new(0.18, 16.0); // 60fps for smooth slide
    let mut app_ui_manager = AppUiManager::new();
    let mut vlc_touch_active = false; // Track if VLC touch interaction is active
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
        // --- Detect layer switch and trigger slide animation ---
        if last_layer != active_layer {
            if active_layer == LayerKey::Custom3 {
                app_layer3_slide_anim.set_progress(0.0); // Set before animate_in
                app_layer3_slide_anim.animate_in();
            } else if last_layer == LayerKey::Custom3 {
                // Only animate out if NOT switching to Fn keys (assume Fn keys is layer 1 or 0)
                let fn_layer_indices = [LayerKey::Fn];
                if !fn_layer_indices.contains(&active_layer) {
                    app_layer3_slide_anim.animate_out();
                    pending_layer = Some(active_layer.clone()); // Remember where we want to go
                    active_layer = LayerKey::Custom3; // Stay on 3 until animation is done
                }
                // If switching to Fn keys, just switch immediately (no animation)
            }
            last_layer = active_layer.clone();
            needs_complete_redraw = true;
        }
        // --- Update AppLayerKeys3 slide animation ---
        if app_layer3_slide_anim.update() {
            needs_complete_redraw = true;
        }
        // After slide-out animation, switch to pending layer if needed
        if !app_layer3_slide_anim.is_animating_out() && pending_layer.is_some() {
            active_layer = get_pending_layer(&mut pending_layer)?;
            last_layer = active_layer.clone();
            needs_complete_redraw = true;
        }
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

            
            // Performance optimization: Only log redraws in debug mode
            if DEBUG_LOGGING {
                println!("[main] REDRAW TRIGGERED: needs_complete_redraw={}, any_changed={}, browser_buttons_changed={}", needs_complete_redraw, any_changed, browser_buttons_changed);
            }
            
            let shift = if cfg.enable_pixel_shift {
                pixel_shift.get()
            } else {
                (0.0, 0.0)
            };
            
            // Use current session state directly
            let session_for_draw = current_session.as_ref();
            
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
            let clips = get_active_layer_mut(&mut layers, active_layer)?.draw(&cfg, width as i32, height as i32, &surface, shift, needs_complete_redraw, false, session_for_draw, Some(active_layer.clone()), app_layer3_slide_progress, current_window_class.as_deref(), Some(&mut app_ui_manager), vlc_drag_position)?;
            
            // Performance optimization: Batch DRM operations
            let data = safe_surface_data(&mut surface)?;
            safe_drm_map(drm)?.as_mut()[..data.len()].copy_from_slice(&data);
            safe_drm_dirty(drm, &clips)?;
            
            // Performance monitoring
            let draw_time = start_time.elapsed();
            if DEBUG_LOGGING && draw_time > std::time::Duration::from_millis(16) {
                println!("[main] SLOW DRAW: {:.2}ms (target: 16ms for 60fps)", draw_time.as_millis() as f64);
            }
            
            needs_complete_redraw = false;
        }
        

        
        // --- epoll wait and event handling ---
        let mut events = [EpollEvent::empty(); 5];
        
        // Performance optimization: Frame rate limiting
        let frame_start = std::time::Instant::now();
        
        let n = safe_epoll_wait(&epoll, &mut events, next_timeout_ms as isize)?;

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
                                           let class = part.trim();
                                           if class.is_empty() {
                                               continue;
                                           }
                                           
                                           // Check if VLC window focus changed
                                           let new_vlc_focused = class == "vlc";
                                           if new_vlc_focused != vlc_window_focused {
                                               vlc_window_focused = new_vlc_focused;
                                               if vlc_window_focused {
                                                   // VLC window gained focus - start VLC helper
                                                   println!("[main] VLC window focused, starting VLC helper");
                                                   if let Some(user) = &current_user {
                                                       if let Some(fd) = vlc_helper_manager.start(user, current_session.as_ref().and_then(|s| s.leader).unwrap_or(0)) {
                                                           let listener_fd_obj = unsafe { OwnedFd::from_raw_fd(fd) };
                                                           if let Err(e) = safe_epoll_add(&epoll, &listener_fd_obj, EpollEvent::new(EpollFlags::EPOLLIN, 6)) {
                                                               eprintln!("[main] Failed to add VLC helper listener to epoll: {}", e);
                                                           } else {
                                                               vlc_helper_listener_fd = Some(listener_fd_obj.into_raw_fd());
                                                           }
                                                       }
                                                   } else {
                                                       println!("[main] No current user available for VLC helper");
                                                   }
                                               } else {
                                                   // VLC window lost focus - stop VLC helper
                                                   println!("[main] VLC window lost focus, stopping VLC helper");
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
                                                   vlc_helper_manager.stop();
                                                   // Clear VLC drag position when losing focus
                                                   vlc_drag_position = None;
                                               }
                                           }
                                           
                                           // Check if browser window focus changed
                                           let class_lower = class.to_lowercase();
                                           let new_browser_focused = class_lower == "firefox" || class_lower == "chrome" || class_lower == "chromium" || class_lower == "brave" || class_lower == "brave-browser" || class_lower == "edge" || class_lower == "safari" || class_lower == "opera" || class_lower == "google-chrome";
                                           
                                           if new_browser_focused != browser_window_focused {
                                               // Browser focus state changed
                                               if browser_window_focused && !new_browser_focused {
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
                                                   browser_helper_manager.stop();
                                               } else if !browser_window_focused && new_browser_focused {
                                                   // Browser window gained focus - start browser helper
                                                   println!("[main] Browser window focused: '{}', starting browser helper", class);
                                                   if let Some(user) = &current_user {
                                                       if let Some(fd) = browser_helper_manager.start(user, current_session.as_ref().and_then(|s| s.leader).unwrap_or(0)) {
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
                                               
                                               // Update focus state
                                               browser_window_focused = new_browser_focused;
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
                                                       if let Some(fd) = browser_helper_manager.start(user, current_session.as_ref().and_then(|s| s.leader).unwrap_or(0)) {
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
                                           
                                           // Update current window class AFTER all the logic
                                           current_window_class = Some(class.to_string());
                                           
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
                                                       if let Some(title) = vlc_status.get("title").and_then(|v| v.as_str()) {
                                                           if let Some(artist) = vlc_status.get("artist").and_then(|v| v.as_str()) {
                                                               if let Some(duration) = vlc_status.get("duration").and_then(|v| v.as_i64()) {
                                                                   // Create a VlcStatus struct and update the VLC screen
                                                                   let status = crate::helper::VlcStatus {
                                                                       is_playing,
                                                                       position,
                                                                       duration,
                                                                       title: title.to_string(),
                                                                       artist: artist.to_string(),
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
            match event {
                Event::Device(DeviceEvent::Added(evt)) => {
                    let dev = evt.device();
                    if dev.name().contains(" Touch Bar") {
                        digitizer = Some(dev);
                    }
                },
                Event::Keyboard(KeyboardEvent::Key(key)) => {
                    if key.key() == Key::Fn as u32 {
                        let new_layer = match key.key_state() {
                            KeyState::Pressed => LayerKey::Fn,
                            KeyState::Released => {
                                // Return to appropriate layer based on session state
                                if current_session.as_ref().map(|s| s.is_logged_in).unwrap_or(false) {
                                    LayerKey::Media
                                } else {
                                    LayerKey::Custom2
                                }
                            },
                        };
                        if active_layer != new_layer {
                            active_layer = new_layer;
                            needs_complete_redraw = true;
                        }
                    } else if key.key() == Key::Macro1 as u32 && key.key_state() == KeyState::Pressed {
                        // Switch to appropriate layer based on session state
                        active_layer = if current_session.as_ref().map(|s| s.is_logged_in).unwrap_or(false) {
                            LayerKey::Media
                        } else {
                            LayerKey::Custom2
                        };
                        needs_complete_redraw = true;
                    } else if key.key() == Key::Macro2 as u32 && key.key_state() == KeyState::Pressed {
                        active_layer = LayerKey::Custom2;
                        needs_complete_redraw = true;
                    } else if key.key() == Key::Macro3 as u32 && key.key_state() == KeyState::Pressed {
                        active_layer = LayerKey::Custom3;
                        needs_complete_redraw = true;
                    }

                },
                Event::Touch(te) => {
                    if Some(te.device()) != digitizer || backlight.current_bl() == 0 {
                        continue
                    }
                    match te {
                        TouchEvent::Down(dn) => {
                            let _x = dn.x_transformed(width as u32);
                            let _y = dn.y_transformed(height as u32);
                            println!("[main] Touch down at ({}, {})", _x, _y);
                            if let Some((group, idx)) = get_active_layer_mut(&mut layers, active_layer)?.hit_test(_x, width as i32, Some(active_layer.clone())) {
                                match group {
                                    "modules" => {
                                        // Store touch for modules group
                                        touches.insert(dn.seat_slot(), (active_layer.clone(), group, idx));
                                        println!("[main] Touch stored for modules group, slot: {}", dn.seat_slot());
                                        
                                        // Check for app-specific UI interactions
                                        if let Some(window_class) = &current_window_class {
                                                // Calculate modules area coordinates
                                                let pixel_shift_width = if cfg.enable_pixel_shift { PIXEL_SHIFT_WIDTH_PX } else { 0 };
                                                let total_width = (width as i32 - pixel_shift_width as i32) as f64;
                                                let _group_spacing = BUTTON_SPACING_PX as f64;
                                                let modules_width = (0.7 * total_width).round(); // Assuming 70% for modules
                                                let modules_x = (pixel_shift_width / 2) as f64;
                                                let modules_y = (height as f64) * 0.15;
                                                let modules_height = (height as f64) * 0.7;
                                                
                                                // Adjust touch coordinates relative to modules area
                                                let adjusted_x = _x - modules_x;
                                                let adjusted_y = _y - modules_y;
                                                
                                            // Compare window_class in lowercase for browser detection
                                            let window_class_lc = window_class.to_lowercase();
                                                if let Some(app_action) = app_ui_manager.hit_test_app_ui(adjusted_x, adjusted_y, modules_x, modules_y, modules_width, modules_height, 8.0, window_class) {
                                                println!("[main] App action detected: {:?}", app_action);
                                                println!("[main] Checking browser condition for window_class: {}", window_class);
                                                if window_class_lc == "firefox" || window_class_lc == "chrome" || window_class_lc == "chromium" || window_class_lc == "brave" || window_class_lc == "brave-browser" || window_class_lc == "edge" || window_class_lc == "safari" || window_class_lc == "opera" || window_class_lc == "google-chrome" {
                                                    println!("[main] Browser condition met, processing browser action");
                                                    match app_action {
                                                        AppAction::Browser(BrowserAction::Back) => {
                                                            // Send browser back command
                                                            println!("[main] Executing Browser Back");
                                                            app_ui_manager.browser_screen.buttons[0].active = true;
                                                            app_ui_manager.browser_screen.buttons[0].changed = true;
                                                            if let Some(stream) = &mut browser_helper_stream {
                                                                if let Err(e) = send_browser_command(stream, "back") {
                                                                    eprintln!("[main] Failed to send back command to browser helper: {}", e);
                                                                }
                                                            } else {
                                                                eprintln!("[main] Browser helper stream not available!");
                                                            }
                                                        }
                                                        AppAction::Browser(BrowserAction::Forward) => {
                                                            // Send browser forward command
                                                            println!("[main] Executing Browser Forward");
                                                            app_ui_manager.browser_screen.buttons[1].active = true;
                                                            app_ui_manager.browser_screen.buttons[1].changed = true;
                                                            if let Some(stream) = &mut browser_helper_stream {
                                                                if let Err(e) = send_browser_command(stream, "forward") {
                                                                    eprintln!("[main] Failed to send forward command to browser helper: {}", e);
                                                                }
                                                            } else {
                                                                eprintln!("[main] Browser helper stream not available!");
                                                            }
                                                        }
                                                        AppAction::Browser(BrowserAction::Refresh) => {
                                                            // Send browser refresh command
                                                            println!("[main] Executing Browser Refresh");
                                                            app_ui_manager.browser_screen.buttons[2].active = true;
                                                            app_ui_manager.browser_screen.buttons[2].changed = true;
                                                            if let Some(stream) = &mut browser_helper_stream {
                                                                if let Err(e) = send_browser_command(stream, "refresh") {
                                                                    eprintln!("[main] Failed to send refresh command to browser helper: {}", e);
                                                                }
                                                            } else {
                                                                eprintln!("[main] Browser helper stream not available!");
                                                            }
                                                        }
                                                        AppAction::Browser(BrowserAction::Home) => {
                                                            // Send browser home command
                                                            println!("[main] Executing Browser Home");
                                                            app_ui_manager.browser_screen.buttons[3].active = true;
                                                            app_ui_manager.browser_screen.buttons[3].changed = true;
                                                            if let Some(stream) = &mut browser_helper_stream {
                                                                if let Err(e) = send_browser_command(stream, "home") {
                                                                    eprintln!("[main] Failed to send home command to browser helper: {}", e);
                                                                }
                                                            } else {
                                                                eprintln!("[main] Browser helper stream not available!");
                                                            }
                                                        }
                                                        AppAction::Browser(BrowserAction::AddBookmark) => {
                                                            // Send browser add bookmark command
                                                            println!("[main] Executing Browser Add Bookmark");
                                                            app_ui_manager.browser_screen.buttons[4].active = true;
                                                            app_ui_manager.browser_screen.buttons[4].changed = true;
                                                            if let Some(stream) = &mut browser_helper_stream {
                                                                if let Err(e) = send_browser_command(stream, "add_bookmark") {
                                                                    eprintln!("[main] Failed to send add_bookmark command to browser helper: {}", e);
                                                                }
                                                            } else {
                                                                eprintln!("[main] Browser helper stream not available!");
                                                            }
                                                        }
                                                        AppAction::Browser(BrowserAction::BookmarksManager) => {
                                                            // Send browser bookmarks manager command
                                                            println!("[main] Executing Browser Bookmarks Manager");
                                                            app_ui_manager.browser_screen.buttons[4].active = true;
                                                            app_ui_manager.browser_screen.buttons[4].changed = true;
                                                            if let Some(stream) = &mut browser_helper_stream {
                                                                if let Err(e) = send_browser_command(stream, "bookmarks_manager") {
                                                                    eprintln!("[main] Failed to send bookmarks_manager command to browser helper: {}", e);
                                                                }
                                                            } else {
                                                                eprintln!("[main] Browser helper stream not available!");
                                                            }
                                                        }
                                                        AppAction::Browser(BrowserAction::CloseTab) => {
                                                            // Send browser close tab command
                                                            println!("[main] Executing Browser Close Tab");
                                                            app_ui_manager.browser_screen.buttons[4].active = true;
                                                            app_ui_manager.browser_screen.buttons[4].changed = true;
                                                            if let Some(stream) = &mut browser_helper_stream {
                                                                if let Err(e) = send_browser_command(stream, "close_tab") {
                                                                    eprintln!("[main] Failed to send close_tab command to browser helper: {}", e);
                                                                }
                                                            } else {
                                                                eprintln!("[main] Browser helper stream not available!");
                                                            }
                                                        }
                                                        AppAction::Browser(BrowserAction::NewTab) => {
                                                            // Send browser new tab command
                                                            println!("[main] Executing Browser New Tab");
                                                            app_ui_manager.browser_screen.buttons[5].active = true;
                                                            app_ui_manager.browser_screen.buttons[5].changed = true;
                                                            if let Some(stream) = &mut browser_helper_stream {
                                                                if let Err(e) = send_browser_command(stream, "new_tab") {
                                                                    eprintln!("[main] Failed to send new_tab command to browser helper: {}", e);
                                                                }
                                                            } else {
                                                                eprintln!("[main] Browser helper stream not available!");
                                                            }
                                                        }
                                                        AppAction::Browser(BrowserAction::AddressBar) => {
                                                            // Focus on address bar
                                                            println!("[main] Executing Browser Address Bar Focus");
                                                            app_ui_manager.browser_screen.focus_address_bar();
                                                            if let Some(stream) = &mut browser_helper_stream {
                                                                if let Err(e) = send_browser_command(stream, "focus_address_bar") {
                                                                    eprintln!("[main] Failed to send focus_address_bar command to browser helper: {}", e);
                                                                }
                                                            } else {
                                                                eprintln!("[main] Browser helper stream not available!");
                                                            }
                                                        }
                                                        _ => {
                                                            // Ignore non-browser actions
                                                            println!("[main] Ignoring non-browser action: {:?}", app_action);
                                                        }
                                                    }
                                                } else if window_class_lc == "vlc" && vlc_helper_stream.is_some() && vlc_window_focused {
                                                    // Only process VLC actions if VLC helper stream is available AND VLC window is focused
                                                        match app_action {
                                                        AppAction::Vlc(VlcAction::TogglePlayPause) => {
                                                            // Send play/pause command to VLC helper
                                                            // Reduced debug logging
                                                            vlc_touch_active = true; // Mark VLC touch as active
                                                            if let Some(stream) = &mut vlc_helper_stream {
                                                                if let Err(e) = send_vlc_command(stream, "play_pause") {
                                                                    eprintln!("[main] Failed to send play/pause command to VLC helper: {}", e);
                                                                }
                                                            } else {
                                                                eprintln!("[main] VLC helper stream not available!");
                                                            }
                                                        }
                                                        AppAction::Vlc(VlcAction::Seek(position)) => {
                                                            // Send seek command to VLC helper
                                                            // Reduced debug logging
                                                            vlc_touch_active = true; // Mark VLC touch as active
                                                            if let Some(stream) = &mut vlc_helper_stream {
                                                                let seek_command = format!("seek:{}", position);
                                                                if let Err(e) = send_vlc_command(stream, &seek_command) {
                                                                    eprintln!("[main] Failed to send seek command to VLC helper: {}", e);
                                                                }
                                                            }
                                                        }
                                                        AppAction::Vlc(VlcAction::DragHead(position)) => {
                                                            // Send seek command to VLC helper for head dragging
                                                            vlc_touch_active = true; // Mark VLC touch as active
                                                            vlc_drag_position = Some(position); // Update drag position for visual feedback
                                                            needs_complete_redraw = true; // Force redraw for visual feedback
                                                            // Only send seek command if position changed significantly (avoid spam)
                                                            static mut LAST_SEEK_POSITION: f64 = 0.0;
                                                            unsafe {
                                                                if (position - LAST_SEEK_POSITION).abs() > 0.01 {
                                                                    LAST_SEEK_POSITION = position;
                                                            if let Some(stream) = &mut vlc_helper_stream {
                                                                let seek_command = format!("seek:{}", position);
                                                                if let Err(e) = send_vlc_command(stream, &seek_command) {
                                                                    eprintln!("[main] Failed to send seek command to VLC helper: {}", e);
                                                                    }
                                                                }
                                                                }
                                                            }
                                                        }
                                                        AppAction::Vlc(VlcAction::Next) => {
                                                            // Send next command to VLC helper
                                                            println!("[main] Executing VLC Next");
                                                            vlc_touch_active = true; // Mark VLC touch as active
                                                            if let Some(stream) = &mut vlc_helper_stream {
                                                                if let Err(e) = send_vlc_command(stream, "next") {
                                                                    eprintln!("[main] Failed to send next command to VLC helper: {}", e);
                                                                }
                                                            }
                                                        }
                                                        AppAction::Vlc(VlcAction::Previous) => {
                                                            // Send previous command to VLC helper
                                                            println!("[main] Executing VLC Previous");
                                                            vlc_touch_active = true; // Mark VLC touch as active
                                                            if let Some(stream) = &mut vlc_helper_stream {
                                                                if let Err(e) = send_vlc_command(stream, "previous") {
                                                                    eprintln!("[main] Failed to send previous command to VLC helper: {}", e);
                                                                }
                                                            }
                                                        }
                                                        AppAction::Vlc(VlcAction::Stop) => {
                                                            // Send stop command to VLC helper
                                                            println!("[main] Executing VLC Stop");
                                                            vlc_touch_active = true; // Mark VLC touch as active
                                                            if let Some(stream) = &mut vlc_helper_stream {
                                                                if let Err(e) = send_vlc_command(stream, "stop") {
                                                                    eprintln!("[main] Failed to send stop command to VLC helper: {}", e);
                                                                }
                                                            }
                                                        }
                                                        AppAction::Vlc(VlcAction::Raise) => {
                                                            // Send raise command to VLC helper
                                                            println!("[main] Executing VLC Raise");
                                                            vlc_touch_active = true; // Mark VLC touch as active
                                                            if let Some(stream) = &mut vlc_helper_stream {
                                                                if let Err(e) = send_vlc_command(stream, "raise") {
                                                                    eprintln!("[main] Failed to send raise command to VLC helper: {}", e);
                                                                }
                                                            }
                                                        }
                                                        AppAction::Vlc(VlcAction::Quit) => {
                                                            // Send quit command to VLC helper
                                                            println!("[main] Executing VLC Quit");
                                                            vlc_touch_active = true; // Mark VLC touch as active
                                                            if let Some(stream) = &mut vlc_helper_stream {
                                                                if let Err(e) = send_vlc_command(stream, "quit") {
                                                                    eprintln!("[main] Failed to send quit command to VLC helper: {}", e);
                                                                }
                                                            }
                                                        }
                                                        
                                                        _ => {
                                                            // Ignore non-VLC actions
                                                            println!("[main] Ignoring non-VLC action: {:?}", app_action);
                                                        }
                                                    }
                                                } else {
                                                    println!("[main] VLC helper stream not available, ignoring VLC action");
                                                }
                                                } else {
                                                    println!("[main] No VLC action detected for touch at ({}, {})", _x, _y);
                                                }
                                        }
                                    }
                                
                                    "media" => {
                                        if let Some(split) = &mut get_active_layer_mut(&mut layers, active_layer)?.split {
                                            let button = &mut split.media[idx];
                                            if button.action == Key::Unknown {
                                                continue;
                                            }
                                            touches.insert(dn.seat_slot(), (active_layer.clone(), group, idx));
                                            button.set_active(&mut uinput, true);
                                        }
                                    }
                                    "flat" => {
                                        let button = &mut get_active_layer_mut(&mut layers, active_layer)?.buttons[idx];
                                        if button.action == Key::Unknown {
                                            continue;
                                        }
                                        touches.insert(dn.seat_slot(), (active_layer.clone(), group, idx));
                                        button.set_active(&mut uinput, true);
                                    }
                                    _ => {}
                                }
                            }
                            
                        
                        },
                        TouchEvent::Motion(mtn) => {
                            println!("[main] Motion event received for slot: {}", mtn.seat_slot());
                            if !touches.contains_key(&mtn.seat_slot()) {
                                println!("[main] Motion event ignored - slot not in touches");
                                continue;
                            }
                            let _x = mtn.x_transformed(width as u32);
                            let _y = mtn.y_transformed(height as u32);
                            let (layer, group, idx) = get_touch_slot(&touches, mtn.seat_slot())?;
                            println!("[main] Motion event: group={}, idx={}, coords=({}, {})", group, idx, _x, _y);
                            match *group {
                                "modules" => {
                                    // Check for app-specific touch interaction during motion
                                    if let Some(window_class) = &current_window_class {
                                        let any_browser_button_active = app_ui_manager.browser_screen.buttons.iter().any(|b| b.active);
                                        println!("[main] Motion - window_class: {}, vlc_touch_active: {}, browser_button_active: {}", window_class, vlc_touch_active, any_browser_button_active);
                                        
                                            // Calculate modules area coordinates
                                            let pixel_shift_width = if cfg.enable_pixel_shift { PIXEL_SHIFT_WIDTH_PX } else { 0 };
                                            let total_width = (width as i32 - pixel_shift_width as i32) as f64;
                                            let _group_spacing = BUTTON_SPACING_PX as f64;
                                            let modules_width = (0.7 * total_width).round(); // Assuming 70% for modules
                                            let modules_x = (pixel_shift_width / 2) as f64;
                                            let modules_y = (height as f64) * 0.15;
                                            let modules_height = (height as f64) * 0.7;
                                            
                                            // Adjust touch coordinates relative to modules area (same as in TouchEvent::Down)
                                            let adjusted_x = _x - modules_x;
                                            let adjusted_y = _y - modules_y;
                                            println!("[main] Motion - Adjusted touch coordinates: ({}, {}) relative to modules area", adjusted_x, adjusted_y);
                                            
                                            if let Some(app_action) = app_ui_manager.hit_test_app_ui(adjusted_x, adjusted_y, modules_x, modules_y, modules_width, modules_height, 8.0, window_class) {
                                            println!("[main] Motion - App action detected: {:?}", app_action);
                                            
                                            // Handle browser actions during motion
                                            let any_browser_button_active = app_ui_manager.browser_screen.buttons.iter().any(|b| b.active);
                                            if (window_class == "firefox" || window_class == "chrome" || window_class == "chromium" || window_class == "brave" || window_class == "brave-browser" || window_class == "edge" || window_class == "safari" || window_class == "opera" || window_class == "google-chrome") && any_browser_button_active {
                                                // Browser buttons don't need motion handling - they're simple press/release
                                                println!("[main] Motion - Browser button active, ignoring motion");
                                            }
                                            // Handle VLC actions during motion
                                            else if window_class == "vlc" && vlc_touch_active && vlc_helper_stream.is_some() {
                                                    match app_action {
                                                        AppAction::Vlc(VlcAction::Seek(position)) => {
                                                            // Send seek command to VLC helper during motion
                                                            println!("[main] VLC seek during motion to position: {}", position);
                                                            if let Some(stream) = &mut vlc_helper_stream {
                                                                let seek_command = format!("seek:{}", position);
                                                                if let Err(e) = send_vlc_command(stream, &seek_command) {
                                                                    eprintln!("[main] Failed to send seek command to VLC helper during motion: {}", e);
                                                                }
                                                            }
                                                        }
                                                        AppAction::Vlc(VlcAction::DragHead(position)) => {
                                                            // Send seek command to VLC helper during head dragging
                                                            println!("[main] VLC drag head during motion to position: {}", position);
                                                            vlc_drag_position = Some(position); // Update drag position for visual feedback
                                                            needs_complete_redraw = true; // Force redraw for visual feedback
                                                            println!("[main] Set vlc_drag_position during motion to: {:?}", vlc_drag_position);
                                                            if let Some(stream) = &mut vlc_helper_stream {
                                                                let seek_command = format!("seek:{}", position);
                                                                if let Err(e) = send_vlc_command(stream, &seek_command) {
                                                                    eprintln!("[main] Failed to send seek command to VLC helper during motion: {}", e);
                                                                }
                                                            }
                                                        }
                                                        _ => {
                                                            // For other VLC actions, only respond to initial touch, not motion
                                                            println!("[main] Ignoring non-seek VLC action during motion: {:?}", app_action);
                                                        }
                                                    }
                                                } else {
                                                println!("[main] No active touch interaction for motion");
                                            }
                                        }
                                    }
                                    continue;
                                }
                                "media" => {
                                    if let Some(split) = &mut get_active_layer_mut(&mut layers, *layer)?.split {
                                        let button = &mut split.media[*idx];
                                        if button.action == Key::Unknown {
                                            continue;
                                        }
                                        button.set_active(&mut uinput, true);
                                    }
                                }
                                "flat" => {
                                    let button = &mut get_active_layer_mut(&mut layers, *layer)?.buttons[*idx];
                                    if button.action == Key::Unknown {
                                        continue;
                                    }
                                    button.set_active(&mut uinput, true);
                                }
                                _ => {}
                            }
                        }
                        TouchEvent::Up(up) => {
                            if !touches.contains_key(&up.seat_slot()) {
                                continue;
                            }
                            let (layer, group, idx) = get_touch_slot(&touches, up.seat_slot())?;
                            println!("Up: group={}, idx={}", group, idx);
                            match *group {
                                "modules" => {
                                    // Reset VLC touch state when touch ends
                                    if vlc_touch_active {
                                        vlc_touch_active = false;
                                        // Don't immediately reset drag position - let VLC update naturally
                                        // vlc_drag_position = None; // Reset drag position
                                        // Reset VLC screen drag state
                                        app_ui_manager.vlc_screen.reset_drag_state();
                                        needs_complete_redraw = true; // Force redraw to reset visual feedback
                                        println!("[main] VLC touch interaction ended, keeping drag position for smooth transition");
                                    }
                                    // Reset browser button states when touch ends
                                    // Only reset if we were actually handling a browser touch
                                    if let Some(window_class) = &current_window_class {
                                        let window_class_lc = window_class.to_lowercase();
                                        if window_class_lc == "firefox" || window_class_lc == "chrome" || window_class_lc == "chromium" || window_class_lc == "brave" || window_class_lc == "brave-browser" || window_class_lc == "edge" || window_class_lc == "safari" || window_class_lc == "opera" || window_class_lc == "google-chrome" {
                                            let any_browser_button_active = app_ui_manager.browser_screen.buttons.iter().any(|b| b.active);
                                            if any_browser_button_active {
                                                println!("[main] Browser touch interaction ended, resetting button states");
                                                println!("[main] Before reset: Back(active={}, changed={}), Forward(active={}, changed={}), Refresh(active={}, changed={}), Home(active={}, changed={})", 
                                                    app_ui_manager.browser_screen.buttons[0].active, app_ui_manager.browser_screen.buttons[0].changed,
                                                    app_ui_manager.browser_screen.buttons[1].active, app_ui_manager.browser_screen.buttons[1].changed,
                                                    app_ui_manager.browser_screen.buttons[2].active, app_ui_manager.browser_screen.buttons[2].changed,
                                                    app_ui_manager.browser_screen.buttons[3].active, app_ui_manager.browser_screen.buttons[3].changed);
                                                // Reset browser screen button states
                                                for button in &mut app_ui_manager.browser_screen.buttons {
                                                    button.active = false;
                                                    button.changed = true;
                                                }
                                                println!("[main] After reset: Back(active={}, changed={}), Forward(active={}, changed={}), Refresh(active={}, changed={}), Home(active={}, changed={})", 
                                                    app_ui_manager.browser_screen.buttons[0].active, app_ui_manager.browser_screen.buttons[0].changed,
                                                    app_ui_manager.browser_screen.buttons[1].active, app_ui_manager.browser_screen.buttons[1].changed,
                                                    app_ui_manager.browser_screen.buttons[2].active, app_ui_manager.browser_screen.buttons[2].changed,
                                                    app_ui_manager.browser_screen.buttons[3].active, app_ui_manager.browser_screen.buttons[3].changed);
                                            }
                                        }
                                    }
                                    continue;
                                }
                                "media" => {
                                    if let Some(split) = &mut get_active_layer_mut(&mut layers, *layer)?.split {
                                        let button = &mut split.media[*idx];
                                        if button.action == Key::Unknown {
                                            continue;
                                        }
                                        button.set_active(&mut uinput, false);
                                    }
                                }
                                "flat" => {
                                    let button = &mut get_active_layer_mut(&mut layers, *layer)?.buttons[*idx];
                                    if button.action == Key::Unknown {
                                        continue;
                                    }
                                    button.set_active(&mut uinput, false);
                                }
                                _ => {}
                            }
                        }
                        _ => {}
                    }
                },
                _ => {}
            }
        }
        
        let _ = backlight.update_backlight(&cfg);
        
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
            eprintln!("[main] Error during VLC helper manager status check: {:?}", e);
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
                println!("[main] Process status - Main helper: {}, VLC helper: {}, Browser helper: {}", 
                    if helper_manager.is_process_running() { "running" } else { "stopped" },
                    if vlc_helper_manager.is_process_running() { "running" } else { "stopped" },
                    if browser_helper_manager.is_process_running() { "running" } else { "stopped" }
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


