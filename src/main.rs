use std::{
    fs::{File, OpenOptions, self},
    os::{
        fd::{AsRawFd, AsFd, IntoRawFd},
        unix::{io::{OwnedFd, FromRawFd}, fs::OpenOptionsExt, net::{UnixListener, UnixStream}},
    },
    path::{Path, PathBuf},
    collections::HashMap,
    cmp::min,
    // panic::self,
    process::{Command, Child},
};
use std::io::{BufReader, Read, Write};
use std::sync::Arc;
use cairo::{ImageSurface, Format, Context, Surface, Rectangle, FontSlant, FontWeight, Antialias};
use rsvg::{Loader, CairoRenderer, SvgHandle};
use drm::control::ClipRect;
use anyhow::{Result, anyhow};
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
        signal::{Signal, SigSet},
        epoll::{Epoll, EpollCreateFlags, EpollEvent, EpollFlags}
    },
    sys::eventfd::{eventfd, EfdFlags},
    unistd::{chown, User},
};
use icon_loader::{IconFileType, IconLoader};
use chrono::{Local, Timelike};
use crate::services::sessionmanager::{SessionState, monitor_sessions};
use tokio::sync::{watch, mpsc};
use view::login_screen::draw_login_screen;
use view::media_screen::draw_media_section;
use view::module_screen::draw_module_screen;
use view::app_ui_manager::{AppUiManager, AppAction};
use view::vlc_screen::VlcAction;

// Helper function to send commands to VLC helper
fn send_vlc_command(stream: &mut UnixStream, command: &str) -> Result<(), std::io::Error> {
    let command_with_newline = format!("{}\n", command);
    stream.write_all(command_with_newline.as_bytes())?;
    Ok(())
}
use crate::display::display::DrmBackend;
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

#[derive(Hash, Eq, PartialEq, Clone, Debug)]
enum LayerKey {
    Media,
    Fn,
    Custom2,
    Custom3,
}

fn get_env_from_pid(pid: u32) -> HashMap<String, String> {
    let mut env = HashMap::new();
    let path = format!("/proc/{}/environ", pid);
    if let Ok(data) = fs::read(path) {
        for entry in data.split(|&b| b == 0) {
            if let Some(eq) = entry.iter().position(|&b| b == b'=') {
                let key = String::from_utf8_lossy(&entry[..eq]).to_string();
                let value = String::from_utf8_lossy(&entry[eq+1..]).to_string();
                env.insert(key, value);
            }
        }
    }
    env
}

fn find_user_session_pid(user: &str) -> Option<u32> {
    // Look for common graphical session processes
    let session_procs = ["i3", "gnome-session", "plasmashell", "startplasma-x11", "ksmserver", "xfce4-session", "openbox", "sway"]; // add more as needed
    let output = Command::new("pgrep").arg("-u").arg(user).output().ok()?;
    let pids: Vec<u32> = String::from_utf8_lossy(&output.stdout)
        .lines()
        .filter_map(|line| line.trim().parse().ok())
        .collect();
    for pid in pids {
        if let Ok(cmdline) = fs::read_to_string(format!("/proc/{}/cmdline", pid)) {
            for proc in &session_procs {
                if cmdline.contains(proc) {
                    return Some(pid);
                }
            }
        }
    }
    None
}

struct HelperManager {
    process: Option<Child>,
    listener: Option<UnixListener>,
}

struct VlcHelperManager {
    process: Option<Child>,
    listener: Option<UnixListener>,
}

impl HelperManager {
    fn new() -> Self {
        HelperManager {
            process: None,
            listener: None,
        }
    }

    fn start(&mut self, user: &str) -> Option<i32> {
        if self.process.is_some() {
            return None;
        }

        let socket_path = "/tmp/touchbar.sock";
        // Clean up old socket file if it exists
        let _ = fs::remove_file(&socket_path);

        let listener = UnixListener::bind(socket_path).expect("Failed to bind socket");
        listener.set_nonblocking(true).expect("Failed to set socket non-blocking");

        // Change ownership of the socket to the logged-in user
        if let Some(userinfo) = User::from_name(user).unwrap() {
            chown(socket_path, Some(userinfo.uid), Some(userinfo.gid)).expect("Failed to chown socket");
        }

        let fd = listener.as_raw_fd();
        self.listener = Some(listener);

        // Find the user's session process and extract its environment
        let mut env_vars = HashMap::new();
        if let Some(pid) = find_user_session_pid(user) {
            env_vars = get_env_from_pid(pid);
        }

        let helper_path = "tiny-dfr-helper";
        let mut cmd = Command::new("sudo");
        cmd.arg("-u").arg(user)
           .arg("env");
        // Pass relevant environment variables if found
        for key in &["DISPLAY", "WAYLAND_DISPLAY", "DBUS_SESSION_BUS_ADDRESS", "XAUTHORITY"] {
            if let Some(val) = env_vars.get(*key) {
                cmd.arg(format!("{}={}", key, val));
            }
        }
        cmd.arg(helper_path);
        // (Optional) Print for debugging:
        // println!("[main] Spawning: sudo -u {} env ... {} with env {:?}", user, helper_path, env_vars);
        let child = cmd.spawn().expect("Failed to start helper");
        self.process = Some(child);
        Some(fd)
    }

    fn stop(&mut self) {
        if let Some(mut child) = self.process.take() {
            child.kill().expect("Failed to kill helper");
        }
        self.listener.take();
    }

    fn accept_connection(&mut self) -> Option<UnixStream> {
        if let Some(listener) = &self.listener {
            if let Ok((stream, _)) = listener.accept() {
                stream.set_nonblocking(true).expect("Failed to set stream non-blocking");
                return Some(stream);
            }
        }
        None
    }
}

impl VlcHelperManager {
    fn new() -> Self {
        VlcHelperManager {
            process: None,
            listener: None,
        }
    }

    fn start(&mut self, user: &str) -> Option<i32> {
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

        // Find the user's session process and extract its environment
        let mut env_vars = HashMap::new();
        if let Some(pid) = find_user_session_pid(user) {
            env_vars = get_env_from_pid(pid);
        }

        let helper_path = "tiny-dfr-vlc-helper";
        let mut cmd = Command::new("sudo");
        cmd.arg("-u").arg(user)
           .arg("env");
        // Pass relevant environment variables if found
        for key in &["DISPLAY", "WAYLAND_DISPLAY", "DBUS_SESSION_BUS_ADDRESS", "XAUTHORITY"] {
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

    fn stop(&mut self) {
        if let Some(mut child) = self.process.take() {
            child.kill().expect("Failed to kill VLC helper");
        }
        self.listener.take();
    }

    fn accept_connection(&mut self) -> Option<UnixStream> {
        if let Some(listener) = &self.listener {
            if let Ok((stream, _)) = listener.accept() {
                stream.set_nonblocking(true).expect("Failed to set VLC stream non-blocking");
                return Some(stream);
            }
        }
        None
    }
}

const BUTTON_SPACING_PX: i32 = 16;
const APP_LAYER_KEYS3_GAP_PX: f64 = 4.0; // Custom gap for AppLayerKeys3
const BUTTON_COLOR_INACTIVE: f64 = 0.172;
const BUTTON_COLOR_ACTIVE: f64 = 0.350;
const ICON_SIZE: i32 = 48;
const TIMEOUT_MS: i32 = 10 * 1000;

enum ButtonImage {
    Text(String),
    Svg(SvgHandle),
    Bitmap(ImageSurface),
    Blank
}

struct Button {
    image: ButtonImage,
    changed: bool,
    active: bool,
    action: Key,
    background: bool,
    fraction: Option<f32>,
}

fn load_image(icon_name: &str, mode: Option<String>, path: &str) -> Result<ButtonImage> {
    if path != "use_default" {
        return Err(anyhow!("Custom path defined, using that"));
    }
    let theme = ConfigManager::new().load_theme();
    let icon_theme = match mode {
        Some(mode_val) => {
            if mode_val == "App" {theme.app_icon_theme} else {theme.media_icon_theme}
        }
        None => {
            panic!("No mode specified")
        }
    };
    let mut search_paths: Vec<PathBuf> = vec![
        PathBuf::from("/etc/tiny-dfr/icons"),
        PathBuf::from("/usr/share/tiny-dfr/icons/"),
        PathBuf::from("/usr/share/icons/"),
    ];
    let mut loader = IconLoader::new();
    search_paths.extend(loader.search_paths().into_owned());
    loader.set_search_paths(search_paths);
    loader.set_theme_name_provider(icon_theme);
    loader.update_theme_name().unwrap();
    let icon_loader;
    match loader.load_icon(icon_name) {
        Some(icon) => {
            icon_loader = icon;
        }
        None => {
            match loader.load_icon(format!("{}.svg", icon_name)) {
                Some(icon) => {
                    icon_loader = icon;
                }
                None => {
                    match loader.load_icon(format!("{}.png", icon_name)) {
                        Some(icon) => {
                            icon_loader = icon;
                        }
                        None => {
                            return Err(anyhow!("Icon not found: {}, trying /usr/share/pixmaps", icon_name));
                        }
                    }
                }
            }
        }
    };
    let icon = icon_loader.file_for_size(256);
    match icon.icon_type() {
        IconFileType::SVG => {
            let handle = Loader::new().read_path(icon.path())?;
            Ok(ButtonImage::Svg(handle))
        }
        IconFileType::PNG => {
            let mut file = File::open(icon.path())?;
            let surf = ImageSurface::create_from_png(&mut file)?;
            if surf.height() == ICON_SIZE && surf.width() == ICON_SIZE {
                return Ok(ButtonImage::Bitmap(surf));
            }
            let resized = ImageSurface::create(Format::ARgb32, ICON_SIZE, ICON_SIZE).unwrap();
            let c = Context::new(&resized).unwrap();
            c.scale(ICON_SIZE as f64 / surf.width() as f64, ICON_SIZE as f64 / surf.height() as f64);
            c.set_source_surface(surf, 0.0, 0.0).unwrap();
            c.set_antialias(Antialias::Best);
            c.paint().unwrap();
            return Ok(ButtonImage::Bitmap(resized));
        }
        IconFileType::XPM => {
            panic!("Legacy XPM icons are not supported")
        }
    }
}

fn try_load_svg_path(icon_name: &str, path: &str) -> Result<ButtonImage> {
    let handle = Loader::new().read_path(format!("{}", path)).or_else(|_| {
        Loader::new().read_path(format!("/usr/share/pixmaps/{}.svg", icon_name))
    })?;
    Ok(ButtonImage::Svg(handle))
}

fn try_load_png_path(icon_name: &str, path: &str) -> Result<ButtonImage> {
    let mut file = File::open(format!("{}", path)).or_else(|_| {
        File::open(format!("/usr/share/pixmaps/{}.png", icon_name))
    })?;
    let surf = ImageSurface::create_from_png(&mut file)?;
    if surf.height() == ICON_SIZE && surf.width() == ICON_SIZE {
        return Ok(ButtonImage::Bitmap(surf));
    }
    let resized = ImageSurface::create(Format::ARgb32, ICON_SIZE, ICON_SIZE).unwrap();
    let c = Context::new(&resized).unwrap();
    c.scale(ICON_SIZE as f64 / surf.width() as f64, ICON_SIZE as f64 / surf.height() as f64);
    c.set_source_surface(surf, 0.0, 0.0).unwrap();
    c.set_antialias(Antialias::Best);
    c.paint().unwrap();
    return Ok(ButtonImage::Bitmap(resized));
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
            image: ButtonImage::Text(text),
            background,
            fraction: None,
        }
    }
    fn new_icon(icon_name: &str, action: Key, mode: Option<String>, path: &str, background: bool) -> Button {
        let image = load_image(icon_name, mode, path)
            .or_else(|_| try_load_svg_path(icon_name, path))
            .or_else(|_| try_load_png_path(icon_name, path))
            .unwrap_or_else(|_| ButtonImage::Text(icon_name.to_string()));
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
            image: ButtonImage::Blank,
            background,
            fraction: None,
        }
    }
    fn render(&self, c: &Context, height: i32, button_left_edge: f64, button_width: u64, y_shift: f64) {
        match &self.image {
            ButtonImage::Text(text) => {
                let extents = c.text_extents(text).unwrap();
                c.move_to(
                    button_left_edge + (button_width as f64 / 2.0 - extents.width() / 2.0).round(),
                    y_shift + (height as f64 / 2.0 + extents.height() / 2.0).round()
                );
                c.show_text(text).unwrap();
            },
            ButtonImage::Svg(svg) => {
                let renderer = CairoRenderer::new(&svg);
                let x = button_left_edge + (button_width as f64 / 2.0 - (ICON_SIZE / 2) as f64).round();
                let y = y_shift + ((height as f64 - ICON_SIZE as f64) / 2.0).round();

                renderer.render_document(c,
                    &Rectangle::new(x, y, ICON_SIZE as f64, ICON_SIZE as f64)
                ).unwrap();
            }
            ButtonImage::Bitmap(surf) => {
                let x = button_left_edge + (button_width as f64 / 2.0 - (ICON_SIZE / 2) as f64).round();
                let y = y_shift + ((height as f64 - ICON_SIZE as f64) / 2.0).round();
                c.set_source_surface(surf, x, y).unwrap();
                c.rectangle(x, y, ICON_SIZE as f64, ICON_SIZE as f64);
                c.fill().unwrap();
            }
            _ => {
            }
        }
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
    fn draw(&mut self, config: &Config, width: i32, height: i32, surface: &Surface, pixel_shift: (f64, f64), complete_redraw: bool, modules_only_redraw: bool, session_state: Option<&SessionState>, layer_index: Option<LayerKey>, login_anim_progress: f64, app_layer3_slide_progress: f64, current_window_class: Option<&str>, app_ui_manager: Option<&AppUiManager>, vlc_drag_position: Option<f64>) -> Vec<ClipRect> {
        match &mut self.split {
            Some(split) => {
                let c = Context::new(&surface).unwrap();
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
                    c.paint().unwrap();
                } else if modules_only_redraw {
                    // Only clear the modules area for modules-only redraw
                    c.set_source_rgb(0.0, 0.0, 0.0);
                    c.rectangle(pixel_shift_x + (pixel_shift_width / 2) as f64, bot - radius, modules_width, top - bot + radius * 2.0);
                    c.fill().unwrap();
                }
                if config.font_renderer.to_lowercase() == "cairo" {
                    c.select_font_face(&config.font_style_cairo, if config.italic_cairo {FontSlant::Italic} else {FontSlant::Normal}, if config.bold_cairo {FontWeight::Bold} else {FontWeight::Normal});
                } else if config.font_renderer.to_lowercase() == "freetype" {
                    c.set_font_face(&config.font_face);
                } else { panic!("Invalid font renderer chosen. Choose between \"Cairo\" and \"FreeType\""); }
                c.set_font_size(32.0);
                
                // Use new session state
                match session_state {
                    Some(state) if state.session_type == "desktop-logged" => {
                        // User is logged in - show normal modules
                        let left_edge = pixel_shift_x + (pixel_shift_width / 2) as f64;
                        if let Some(window_class) = current_window_class {
                            
                            // Use app-specific UI if available, otherwise fall back to default
                            if let Some(app_ui_manager) = &app_ui_manager {
                                app_ui_manager.draw_app_ui(
                                    &c,
                                    left_edge,
                                    bot,
                                    modules_width,
                                    top - bot,
                                    radius,
                                    login_anim_progress,
                                    window_class,
                                    vlc_drag_position, // Pass drag position for visual feedback
                                );
                            } else {
                                draw_module_screen(
                                    &c,
                                    left_edge,
                                    bot,
                                    modules_width,
                                    top - bot,
                                    radius,
                                    height,
                                    complete_redraw,
                                    window_class,
                                    login_anim_progress, // Use the same animation progress as login screen
                                );
                            }
                        }
                    }
                    Some(state) if state.session_type == "login-screen" => {
                        let left_edge = pixel_shift_x + (pixel_shift_width / 2) as f64;

                    draw_login_screen(
                        &c,
                        left_edge,
                        bot,
                        modules_width,
                        top - bot,
                        top,
                        bot,
                        radius,
                        height,
                        complete_redraw,
                        &mut modified_regions,
                        session_state,
                        login_anim_progress,
                    );
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
                
                modified_regions
            }
        
            None => {
                let c = Context::new(&surface).unwrap();
                let mut modified_regions = if complete_redraw {
                    vec![ClipRect::new(0, 0, height as u16, width as u16)]
                } else {
                    Vec::new()
                };
                c.translate(height as f64, 0.0);
                c.rotate((90.0f64).to_radians());
                let pixel_shift_width = if config.enable_pixel_shift { PIXEL_SHIFT_WIDTH_PX } else { 0 };
                // Use custom gap for AppLayerKeys3 (layer index 3), else default
                let gap = if let Some(LayerKey::Custom3) = layer_index { APP_LAYER_KEYS3_GAP_PX } else { BUTTON_SPACING_PX as f64 };
                // --- AppLayerKeys3 slide animation translation ---
                if let Some(LayerKey::Custom3) = layer_index {
                    // If progress is 0.0, skip drawing (prevents flicker)
                    if app_layer3_slide_progress == 0.0 {
                        return modified_regions;
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
                    c.paint().unwrap();
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
                        c.fill().unwrap();
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

                    c.fill().unwrap();
                    }
                    c.set_source_rgb(1.0, 1.0, 1.0);
                    button.render(&c, height, left_edge, this_button_width.ceil() as u64, pixel_shift_y);

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
                modified_regions
            }


        }
        }
    
    // Helper for modules hit test
    fn hit_test_modules(&self, x: f64, width: i32) -> Option<usize> {
        if let Some(split) = &self.split {
            let group_spacing = BUTTON_SPACING_PX as f64;
            let total_width = (width - group_spacing as i32) as f64;
            let modules_width = (split.modules_width as f64 * total_width).round();
            if x >= 0.0 && x < modules_width {
                return Some(0);
            }
        }
        None
    }
    // Helper for media hit test
    fn hit_test_media(&self, x: f64, width: i32) -> Option<usize> {
        if let Some(split) = &self.split {
            let group_spacing = BUTTON_SPACING_PX as f64;
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
    fn hit_test_flat(&self, x: f64, width: i32) -> Option<usize> {
        if self.split.is_none() {
            let count = self.buttons.len();
            let gap = BUTTON_SPACING_PX as f64;
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
    pub fn hit_test(&self, x: f64, width: i32) -> Option<(&'static str, usize)> {
        if self.split.is_some() {
            if let Some(idx) = self.hit_test_modules(x, width) {
                return Some(("modules", idx));
            }
            if let Some(idx) = self.hit_test_media(x, width) {
                return Some(("media", idx));
            }
            None
        } else {
            if let Some(idx) = self.hit_test_flat(x, width) {
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
            .map_err(|err| err.raw_os_error().unwrap())
    }
    fn close_restricted(&mut self, fd: OwnedFd) {
        _ = File::from(fd);
    }
}


fn button_hit(num: u32, idx: u32, width: u16, height: u16, x: f64, y: f64) -> bool {
    let button_width = (width as i32 - (BUTTON_SPACING_PX * (num - 1) as i32)) as f64 / num as f64;
    let left_edge = idx as f64 * (button_width + BUTTON_SPACING_PX as f64);
    if x < left_edge || x > (left_edge + button_width) {
        return false
    }
    y > 0.1 * height as f64 && y < 0.9 * height as f64
}

fn emit<F>(uinput: &mut UInputHandle<F>, ty: EventKind, code: u16, value: i32) where F: AsRawFd {
    uinput.write(&[input_event {
        value: value,
        type_: ty as u16,
        code: code,
        time: timeval {
            tv_sec: 0,
            tv_usec: 0
        }
    }]).unwrap();
}

fn toggle_key<F>(uinput: &mut UInputHandle<F>, code: Key, value: i32) where F: AsRawFd {
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

async fn real_main(drm: &mut DrmBackend) -> Result<()> {
    let (height, width) = drm.mode().size();
    let (db_width, db_height) = drm.fb_info().unwrap().size();
    let mut uinput = UInputHandle::new(OpenOptions::new().write(true).open("/dev/uinput").unwrap());
    let mut backlight = BacklightManager::new();
    let mut last_redraw_minute = Local::now().minute();
    let mut cfg_mgr = ConfigManager::new();
    let (mut cfg, mut layers) = cfg_mgr.load_config(width);
    let mut pixel_shift = PixelShiftManager::new();
    let mut helper_manager = HelperManager::new();
    let mut vlc_helper_manager = VlcHelperManager::new();
    let mut vlc_helper_listener_fd: Option<i32> = None;
    let mut vlc_helper_stream: Option<UnixStream> = None;
    let mut vlc_helper_reader: Option<BufReader<UnixStream>> = None;
    
    // Add focus-based VLC helper management
    let mut vlc_window_focused = false;
    let mut last_window_class: Option<String> = None;
    let mut current_user: Option<String> = None;

    // Privilege dropping removed - run with appropriate permissions

    let mut surface = ImageSurface::create(Format::ARgb32, db_width as i32, db_height as i32).unwrap();
    let mut active_layer = LayerKey::Media;
    let mut last_layer = active_layer.clone();
    let mut pending_layer: Option<LayerKey> = None;

    let mut input_tb = Libinput::new_with_udev(Interface);
    let mut input_main = Libinput::new_with_udev(Interface);
    input_tb.udev_assign_seat("seat-touchbar").unwrap();
    input_main.udev_assign_seat("seat0").unwrap();
    let epoll = Epoll::new(EpollCreateFlags::empty()).unwrap();
    epoll.add(input_main.as_fd(), EpollEvent::new(EpollFlags::EPOLLIN, 0)).unwrap();
    epoll.add(input_tb.as_fd(), EpollEvent::new(EpollFlags::EPOLLIN, 1)).unwrap();
    epoll.add(cfg_mgr.fd(), EpollEvent::new(EpollFlags::EPOLLIN, 2)).unwrap();
    // --- eventfd integration ---
    let event_fd = Arc::new(eventfd(0, EfdFlags::EFD_NONBLOCK).unwrap());
    epoll.add(&*event_fd, EpollEvent::new(EpollFlags::EPOLLIN, 3)).unwrap();
    // --- end eventfd integration ---
    uinput.set_evbit(EventKind::Key).unwrap();
    for layer in layers.values() {
        for button in &layer.buttons {
            uinput.set_keybit(button.action).unwrap();
        }
    }
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
    }).unwrap();
    uinput.dev_create().unwrap();

    let mut digitizer: Option<InputDevice> = None;
    let mut touches = HashMap::new();

    // Initialize session monitor
    let (session_tx, session_rx) = watch::channel(SessionState {
        session_type: "none".to_string(),
        is_logged_in: false,
        user: "".to_string(),
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
    let mut last_login_session_state: Option<SessionState> = None;
    let mut helper_listener_fd: Option<i32> = None;
    let mut helper_stream: Option<UnixStream> = None;
    let mut helper_reader: Option<BufReader<UnixStream>> = None;
    let mut vlc_helper_listener_fd: Option<i32> = None;
    let mut vlc_helper_stream: Option<UnixStream> = None;
    let mut vlc_helper_reader: Option<BufReader<UnixStream>> = None;
    let mut current_window_class: Option<String> = None;
    let mut needs_complete_redraw = false;
    let mut animation = Animation::new(0.05, 50.0); // step, interval_ms
    let mut app_layer3_slide_anim = Animation::new(0.18, 16.0); // 60fps for smooth slide
    let mut app_ui_manager = AppUiManager::new();
    let mut vlc_touch_active = false; // Track if VLC touch interaction is active
    let mut vlc_drag_position: Option<f64> = None; // Track current drag position for visual feedback

    // --- main event loop ---
    loop {
        if cfg_mgr.update_config(&mut cfg, &mut layers, width) {
            active_layer = LayerKey::Media;
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
        // Add animation timeout - ensure animation runs even without input events
        if animation.is_running() {
            next_timeout_ms = min(next_timeout_ms, 16); // 16ms = ~60fps for smooth animation
        }
        // --- AppLayerKeys3 slide animation update ---
        if app_layer3_slide_anim.is_running() {
            next_timeout_ms = min(next_timeout_ms, 16);
        }
        let current_minute = Local::now().minute();
        for button in &mut layers.get_mut(&active_layer).unwrap().buttons {
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
            active_layer = pending_layer.take().unwrap().clone();
            last_layer = active_layer.clone();
            needs_complete_redraw = true;
        }
        // --- Restore any_changed variable for redraw logic ---
        let any_changed = if let Some(split) = &layers.get(&active_layer).unwrap().split {
            split.media.iter().any(|b| b.changed)
        } else {
            layers.get(&active_layer).unwrap().buttons.iter().any(|b| b.changed)
        };
        
        // Handle different types of redraws
        if needs_complete_redraw || any_changed {
            let shift = if cfg.enable_pixel_shift {
                pixel_shift.get()
            } else {
                (0.0, 0.0)
            };
            // Use last_login_session_state for fade-out
            let session_for_draw = if animation.is_animating_out() {
                last_login_session_state.as_ref()
            } else {
                current_session.as_ref()
            };
            // --- Pass slide progress for AppLayerKeys3 ---
            let app_layer3_slide_progress = if active_layer == LayerKey::Custom3 || last_layer == LayerKey::Custom3 {
                app_layer3_slide_anim.progress()
            } else {
                1.0
            };
            // Draw only the current active layer (layer 3 during slide-out)
            if vlc_drag_position.is_some() {

            }
            let mut clips = layers.get_mut(&active_layer).unwrap().draw(&cfg, width as i32, height as i32, &surface, shift, needs_complete_redraw, false, session_for_draw, Some(active_layer.clone()), animation.progress(), app_layer3_slide_progress, current_window_class.as_deref(), Some(&app_ui_manager), vlc_drag_position);
            let data = surface.data().unwrap();
            drm.map().unwrap().as_mut()[..data.len()].copy_from_slice(&data);
            drm.dirty(&clips).unwrap();
            needs_complete_redraw = false;
        }
        
        // --- epoll wait and event handling ---
        let mut events = [EpollEvent::empty(); 5];
        let n = epoll.wait(&mut events, next_timeout_ms as isize).unwrap();

        for i in 0..n {
            let event = events[i];
            match event.data() {
                0 => {  },
                1 => {  },
                2 => { /* handle cfg_mgr.fd() if needed */ },
                3 => {
                    // eventfd triggered: read and process session event
                    let mut buf = [0u8; 8];
                    let _ = nix::unistd::read(event_fd.as_raw_fd(), &mut buf);
                    if let Ok(new_state) = event_rx.try_recv() {
                        let session_changed = match &current_session {
                            Some(current) => current != &new_state,
                            None => true,
                        };
                        if session_changed {
                            if new_state.is_logged_in {
                                current_user = Some(new_state.user.clone());
                                if let Some(fd) = helper_manager.start(&new_state.user) {
                                    let listener_fd_obj = unsafe { OwnedFd::from_raw_fd(fd) };
                                    epoll.add(listener_fd_obj.as_fd(), EpollEvent::new(EpollFlags::EPOLLIN, 4)).unwrap();
                                    helper_listener_fd = Some(listener_fd_obj.into_raw_fd()); // Store the raw fd
                                }
                                
                                // VLC helper will be started when VLC window gains focus
                            } else {
                                if let Some(fd) = helper_listener_fd.take() {
                                    let listener_fd_obj = unsafe { OwnedFd::from_raw_fd(fd) };
                                    epoll.delete(listener_fd_obj.as_fd()).unwrap();
                                }
                                if let Some(stream) = helper_stream.take() {
                                    epoll.delete(&stream).unwrap();
                                    helper_reader = None;
                                }
                                // VLC helper will be stopped when VLC window loses focus
                                helper_manager.stop();
                            }
                            // Step 2: Trigger animation when login screen or desktop-logged becomes visible
                            if (new_state.session_type == "login-screen" || new_state.session_type == "desktop-logged") && !animation.is_animating_in() {
                                animation.animate_in();
                                needs_complete_redraw = true;
                            } else if new_state.session_type != "login-screen" && new_state.session_type != "desktop-logged" && !animation.is_animating_out() && current_session.as_ref().map(|s| {
    let t = s.session_type.as_str();
    t == "login-screen" || t == "desktop-logged"
}) == Some(true) {
                                animation.animate_out();
                                needs_complete_redraw = true;
                            }
                            // Store last login session state for fade-out
                            if new_state.session_type == "login-screen" {
                                last_login_session_state = Some(new_state.clone());
                            }
                            current_session = Some(new_state);
                            needs_complete_redraw = true;
                        } else {
                            println!("[main] Session state unchanged, skipping redraw");
                        }
                    }
                }
                4 => { // Helper listener event
                    if let Some(stream) = helper_manager.accept_connection() {
                        println!("[main] Helper connected to socket.");
                        epoll.add(&stream, EpollEvent::new(EpollFlags::EPOLLIN, 5)).unwrap();
                        helper_reader = Some(BufReader::new(stream.try_clone().unwrap()));
                        helper_stream = Some(stream);
                        // Stop listening for new connections
                        if let Some(fd) = helper_listener_fd.take() {
                            let listener_fd_obj = unsafe { OwnedFd::from_raw_fd(fd) };
                            epoll.delete(listener_fd_obj.as_fd()).unwrap();
                        }
                    }
                }
                5 => { // Helper stream event
                    if let Some(reader) = &mut helper_reader {
                        eprintln!("[main] DEBUG: Reading from helper socket...");
                        loop {
                           let mut buf = vec![0; 1024];
                           match reader.get_mut().read(&mut buf) {
                               Ok(0) => { // EOF
                                   println!("[main] Helper disconnected.");
                                   if let Some(stream) = helper_stream.take() {
                                       epoll.delete(&stream).unwrap();
                                   }
                                   helper_reader = None;
                                   break;
                               },
                               Ok(n) => {
                                   let data = &buf[..n];
                                   if let Ok(text) = std::str::from_utf8(data) {
                                       for part in text.split('\n') {
                                           let class = part.trim();
                                           if class.is_empty() {
                                               continue;
                                           }
                                           current_window_class = Some(class.to_string());
                                           
                                           // Check if VLC window focus changed
                                           let new_vlc_focused = class == "vlc";
                                           if new_vlc_focused != vlc_window_focused {
                                               vlc_window_focused = new_vlc_focused;
                                               if vlc_window_focused {
                                                   // VLC window gained focus - start VLC helper
                                                   println!("[main] VLC window focused, starting VLC helper");
                                                   if let Some(user) = &current_user {
                                                       if let Some(fd) = vlc_helper_manager.start(user) {
                                                           let listener_fd_obj = unsafe { OwnedFd::from_raw_fd(fd) };
                                                           epoll.add(listener_fd_obj.as_fd(), EpollEvent::new(EpollFlags::EPOLLIN, 6)).unwrap();
                                                           vlc_helper_listener_fd = Some(listener_fd_obj.into_raw_fd());
                                                       }
                                                   } else {
                                                       println!("[main] No current user available for VLC helper");
                                                   }
                                               } else {
                                                   // VLC window lost focus - stop VLC helper
                                                   println!("[main] VLC window lost focus, stopping VLC helper");
                                                   if let Some(stream) = vlc_helper_stream.take() {
                                                       epoll.delete(&stream).unwrap();
                                                   }
                                                   vlc_helper_reader = None;
                                                   if let Some(fd) = vlc_helper_listener_fd.take() {
                                                       let listener_fd_obj = unsafe { OwnedFd::from_raw_fd(fd) };
                                                       epoll.delete(listener_fd_obj.as_fd()).unwrap();
                                                   }
                                                   vlc_helper_manager.stop();
                                                   // Clear VLC drag position when losing focus
                                                   vlc_drag_position = None;
                                               }
                                           }
                                           
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
                                        epoll.delete(&stream).unwrap();
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
                    stream.set_nonblocking(true).expect("Failed to set VLC stream non-blocking");
                    println!("[main] VLC helper connected to socket.");
                    epoll.add(&stream, EpollEvent::new(EpollFlags::EPOLLIN, 7)).unwrap();
                    vlc_helper_reader = Some(BufReader::new(stream.try_clone().unwrap()));
                    vlc_helper_stream = Some(stream);
                    // Stop listening for new connections
                    if let Some(fd) = vlc_helper_listener_fd.take() {
                        let listener_fd_obj = unsafe { OwnedFd::from_raw_fd(fd) };
                        epoll.delete(listener_fd_obj.as_fd()).unwrap();
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
                                       epoll.delete(&stream).unwrap();
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
                                                               Err(_e) => {
                                    if let Some(stream) = vlc_helper_stream.take() {
                                        epoll.delete(&stream).unwrap();
                                    }
                                    vlc_helper_reader = None;
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
        input_tb.dispatch().unwrap();
        input_main.dispatch().unwrap();

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
                            KeyState::Released => LayerKey::Media,
                        };
                        if active_layer != new_layer {
                            active_layer = new_layer;
                            needs_complete_redraw = true;
                        }
                    } else if key.key() == Key::Macro1 as u32 && key.key_state() == KeyState::Pressed {
                        active_layer = LayerKey::Media;
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
                            if let Some((group, idx)) = layers.get_mut(&active_layer).unwrap().hit_test(_x, width as i32) {
                                match group {
                                    "modules" => {
                                        // Store touch for modules group
                                        touches.insert(dn.seat_slot(), (active_layer.clone(), group, idx));
                                        println!("[main] Touch stored for modules group, slot: {}", dn.seat_slot());
                                        
                                        // Check for app-specific UI interactions
                                        if let Some(window_class) = &current_window_class {
                                            // Only handle VLC interactions when VLC is focused
                                            if window_class == "vlc" {
                                                println!("[main] VLC detected as focused window, processing touch events");
                                                // Calculate modules area coordinates
                                                let pixel_shift_width = if cfg.enable_pixel_shift { PIXEL_SHIFT_WIDTH_PX } else { 0 };
                                                let total_width = (width as i32 - pixel_shift_width as i32) as f64;
                                                let _group_spacing = BUTTON_SPACING_PX as f64;
                                                let modules_width = (0.7 * total_width).round(); // Assuming 70% for modules
                                                let modules_x = (pixel_shift_width / 2) as f64;
                                                let modules_y = (height as f64) * 0.15;
                                                let modules_height = (height as f64) * 0.7;
                                                
                                                // Reduced debug logging
                                                
                                                // Adjust touch coordinates relative to modules area
                                                let adjusted_x = _x - modules_x;
                                                let adjusted_y = _y - modules_y;
                                                // Reduced debug logging
                                                
                                                if let Some(app_action) = app_ui_manager.hit_test_app_ui(adjusted_x, adjusted_y, modules_x, modules_y, modules_width, modules_height, 8.0, window_class) {
                                                    // Only process VLC actions if VLC helper stream is available AND VLC window is focused
                                                    if vlc_helper_stream.is_some() && vlc_window_focused {
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
                                }
                                    "media" => {
                                        if let Some(split) = &mut layers.get_mut(&active_layer).unwrap().split {
                                            let button = &mut split.media[idx];
                                            if button.action == Key::Unknown {
                                                continue;
                                            }
                                            touches.insert(dn.seat_slot(), (active_layer.clone(), group, idx));
                                            button.set_active(&mut uinput, true);
                                        }
                                    }
                                    "flat" => {
                                        let button = &mut layers.get_mut(&active_layer).unwrap().buttons[idx];
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
                            let (layer, group, idx) = touches.get(&mtn.seat_slot()).unwrap();
                            println!("[main] Motion event: group={}, idx={}, coords=({}, {})", group, idx, _x, _y);
                            match *group {
                                "modules" => {
                                    // Check for VLC touch interaction during motion
                                    if let Some(window_class) = &current_window_class {
                                        println!("[main] Motion - window_class: {}, vlc_touch_active: {}", window_class, vlc_touch_active);
                                        if window_class == "vlc" && vlc_touch_active {
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
                                                println!("[main] Motion - VLC action detected: {:?}", app_action);
                                                // Only process VLC seek during motion if VLC helper stream is available
                                                if vlc_helper_stream.is_some() {
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
                                                    println!("[main] VLC helper stream not available during motion, ignoring seek");
                                                }
                                            }
                                        }
                                    }
                                    continue;
                                }
                                "media" => {
                                    if let Some(split) = &mut layers.get_mut(layer).unwrap().split {
                                        let button = &mut split.media[*idx];
                                        if button.action == Key::Unknown {
                                            continue;
                                        }
                                        button.set_active(&mut uinput, true);
                                    }
                                }
                                "flat" => {
                                    let button = &mut layers.get_mut(layer).unwrap().buttons[*idx];
                                    if button.action == Key::Unknown {
                                        continue;
                                    }
                                    button.set_active(&mut uinput, true);
                                }
                                _ => {}
                            }
                        },
                        TouchEvent::Up(up) => {
                            if !touches.contains_key(&up.seat_slot()) {
                                continue;
                            }
                            let (layer, group, idx) = touches.get(&up.seat_slot()).unwrap();
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
                                    continue;
                                }
                                "media" => {
                                    if let Some(split) = &mut layers.get_mut(layer).unwrap().split {
                                        let button = &mut split.media[*idx];
                                        if button.action == Key::Unknown {
                                            continue;
                                        }
                                        button.set_active(&mut uinput, false);
                                    }
                                }
                                "flat" => {
                                    let button = &mut layers.get_mut(layer).unwrap().buttons[*idx];
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
        
        backlight.update_backlight(&cfg);
        
        // Step 3: Increment animation in main loop (time-based)
        if animation.update() {
            needs_complete_redraw = true;
        }
        
        // Process session events (event-driven)
    }
}
