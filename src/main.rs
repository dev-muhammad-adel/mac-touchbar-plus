use std::{
    fs::{File, OpenOptions},
    os::{
        fd::{AsRawFd, AsFd},
        unix::{io::OwnedFd, fs::OpenOptionsExt}
    },
    path::{Path, PathBuf},
    collections::HashMap,
    cmp::min,
    panic::{self, AssertUnwindSafe}
};
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
    errno::Errno
};
use privdrop::PrivDrop;
use icon_loader::{IconFileType, IconLoader};
use chrono::{Local, Locale, Timelike};

mod backlight;
mod display;
mod pixel_shift;
mod fonts;
mod config;

use backlight::BacklightManager;
use display::DrmBackend;
use pixel_shift::{PixelShiftManager, PIXEL_SHIFT_WIDTH_PX};
use config::{ButtonConfig, Config};
use crate::config::ConfigManager;

const BUTTON_SPACING_PX: i32 = 16;
const BUTTON_COLOR_INACTIVE: f64 = 0.200;
const BUTTON_COLOR_ACTIVE: f64 = 0.400;
const ICON_SIZE: i32 = 48;
const TIMEOUT_MS: i32 = 10 * 1000;

enum ButtonImage {
    Text(String),
    Svg(SvgHandle),
    Bitmap(ImageSurface),
    Time(String, String),
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
            } else if mode.to_lowercase() == "time" {
                let format = match cfg.format {
                    Some(f) => f,
                    None => "24hr".to_string()
                };
                let locale = match cfg.locale {
                    Some(l) => l,
                    None => "POSIX".to_string()
                };
                let mut btn = Button::new_time(cfg.action, format, locale, background);
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
    fn new_time(action: Key, format: String, locale: String, background: bool) -> Button {
        Button {
            action,
            active: false,
            changed: false,
            image: ButtonImage::Time(format, locale),
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
            ButtonImage::Time(format, locale) => {
                let current_time = Local::now();
                let current_locale = Locale::try_from(locale.as_str()).unwrap_or(Locale::POSIX);
                let formatted_time;
                if format == "24hr" {
                    formatted_time = format!(
                    "{}:{}    {} {} {}",
                     current_time.format_localized("%H", current_locale),
                     current_time.format_localized("%M", current_locale),
                     current_time.format_localized("%a", current_locale),
                     current_time.format_localized("%-e", current_locale),
                     current_time.format_localized("%b", current_locale)
                );
                } else {
                    formatted_time = format!(
                    "{}:{} {}    {} {} {}",
                    current_time.format_localized("%-l", current_locale),
                    current_time.format_localized("%M", current_locale),
                    current_time.format_localized("%p", current_locale),
                    current_time.format_localized("%a", current_locale),
                    current_time.format_localized("%-e", current_locale),
                    current_time.format_localized("%b", current_locale)
                );
                }
                let time_extents = c.text_extents(&formatted_time).unwrap();
                c.move_to(
                    button_left_edge + (button_width as f64 / 2.0 - time_extents.width() / 2.0).round(),
                    y_shift + (height as f64 / 2.0 + time_extents.height() / 2.0).round()
                );
                c.show_text(&formatted_time).unwrap();
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
    pub modules: Vec<Button>,
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
    fn with_split(modules: Vec<ButtonConfig>, modules_width: f32, media: Vec<ButtonConfig>, media_width: f32) -> FunctionLayer {
        FunctionLayer {
            buttons: vec![],
            split: Some(SplitLayout {
                modules: modules.into_iter().map(Button::with_config).collect(),
                modules_width,
                media: media.into_iter().map(Button::with_config).collect(),
                media_width,
            }),
        }
    }
    fn draw(&mut self, config: &Config, width: i32, height: i32, surface: &Surface, pixel_shift: (f64, f64), complete_redraw: bool) -> Vec<ClipRect> {
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
                let group_spacing = BUTTON_SPACING_PX as f64; // space between groups
                let total_width = (width - pixel_shift_width as i32) as f32;
                let usable_width = total_width as f64 - group_spacing;
                let modules_width = (split.modules_width as f64 * usable_width).round();
                let media_width = (split.media_width as f64 * usable_width).round();
                let modules_count = split.modules.len();
                let media_count = split.media.len();
                let modules_spacing = if modules_count > 1 { BUTTON_SPACING_PX as f64 * (modules_count as f64 - 1.0) } else { 0.0 };
                let media_spacing = if media_count > 1 { BUTTON_SPACING_PX as f64 * (media_count as f64 - 1.0) } else { 0.0 };
                let modules_button_width = (modules_width - modules_spacing) / modules_count as f64;
                let media_button_width = (media_width - media_spacing) / media_count as f64;
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
                // Draw modules section
                match split.modules.as_mut_slice() {
                    modules => {
                        let modules_count = modules.len();
                        let mut left_edge = pixel_shift_x + (pixel_shift_width / 2) as f64;
                        for (i, button) in modules.iter_mut().enumerate() {
                            if button.changed || complete_redraw {
                                // DEBUG: Print draw info for modules
                                println!("DRAW MODULES: idx={}, left_edge={}, width={}", i, left_edge, modules_button_width);
                                let color = if button.active {
                                    BUTTON_COLOR_ACTIVE
                                } else if config.show_button_outlines {
                                    BUTTON_COLOR_INACTIVE
                                } else {
                                    0.0
                                };
                                if !complete_redraw {
                                    c.set_source_rgb(0.0, 0.0, 0.0);
                                    c.rectangle(left_edge, bot - radius, modules_button_width, top - bot + radius * 2.0);
                                    c.fill().unwrap();
                                }
                                if (button.action != Key::Unknown && button.action != Key::Time && button.action != Key::Macro1 && button.action != Key::Macro2 && button.action != Key::Macro3 && button.action != Key::Macro4) && (button.background || button.active) {
                                    c.set_source_rgb(color, color, color);
                                    c.new_sub_path();
                                    let left = left_edge + radius;
                                    let right = (left_edge + modules_button_width.ceil()) - radius;
                                    c.arc(right, bot, radius, (-90.0f64).to_radians(), (0.0f64).to_radians());
                                    c.arc(right, top, radius, (0.0f64).to_radians(), (90.0f64).to_radians());
                                    c.arc(left, top, radius, (90.0f64).to_radians(), (180.0f64).to_radians());
                                    c.arc(left, bot, radius, (180.0f64).to_radians(), (270.0f64).to_radians());
                                    c.close_path();
                                    c.fill().unwrap();
                                }
                                c.set_source_rgb(1.0, 1.0, 1.0);
                                button.render(&c, height, left_edge, modules_button_width.ceil() as u64, pixel_shift_y);
                                button.changed = false;
                                if !complete_redraw {
                                    modified_regions.push(ClipRect::new(
                                        height as u16 - top as u16 - radius as u16,
                                        left_edge as u16,
                                        height as u16 - bot as u16 + radius as u16,
                                        left_edge as u16 + modules_button_width as u16
                                    ));
                                }
                            }
                            // Always update left_edge
                            left_edge += modules_button_width;
                            if i != modules_count - 1 {
                                left_edge += BUTTON_SPACING_PX as f64;
                            }
                        }
                        left_edge += group_spacing; // only one group spacing between modules and media
                        // Draw media section
                        let media_spacing_px = 2.0f64; // 2px spacing for AppLayerKeys1Media
                        let media_count = {
                            match split.media.as_mut_slice() {
                                media => media.len(),
                            }
                        };
                        match split.media.as_mut_slice() {
                            media => {
                                let total_spacing = if media_count > 1 { media_spacing_px * (media_count as f64 - 1.0) } else { 0.0 };
                                let button_area = media_width - total_spacing;
                                // Weight-based layout
                                let weights: Vec<f32> = media.iter().map(|b| b.fraction.unwrap_or(1.0)).collect();
                                let total_weight: f32 = weights.iter().sum();
                                let mut media_button_widths: Vec<f64> = weights.iter().map(|w| button_area * (*w as f64 / total_weight as f64)).collect();
                                // Last button absorbs rounding error
                                let sum_widths: f64 = media_button_widths.iter().sum();
                                if let Some(last) = media_button_widths.last_mut() {
                                    *last += button_area - sum_widths;
                                }
                                for (i, button) in media.iter_mut().enumerate() {
                                    if button.changed || complete_redraw {
                                        // DEBUG: Print draw info for media
                                        println!("DRAW MEDIA: idx={}, left_edge={}, width={}", i, left_edge, media_button_widths[i]);
                                        let color = if button.active {
                                            BUTTON_COLOR_ACTIVE
                                        } else if config.show_button_outlines {
                                            BUTTON_COLOR_INACTIVE
                                        } else {
                                            0.0
                                        };
                                        // Ensure macro buttons always have background
                                        if matches!(button.action, Key::Macro1 | Key::Macro2 | Key::Macro3 | Key::Macro4) {
                                            button.background = true;
                                        }
                                        let this_button_width = media_button_widths[i];
                                        let is_first = i == 0;
                                        let is_last = i == media_count - 1;
                                        let x = left_edge;
                                        let y = bot - radius;
                                        let w = this_button_width;
                                        let h = top - bot + radius * 2.0;
                                        let r = radius.min(h / 2.0);
                                        if (button.action != Key::Unknown) && (button.background || button.active) {
                                            c.set_source_rgb(color, color, color);
                                            if media_count == 1 {
                                                // زر واحد فقط: كل الزوايا دائرية
                                                c.new_sub_path();
                                                c.arc(x + w - r, y + r, r, (270.0f64).to_radians(), (360.0f64).to_radians());
                                                c.arc(x + w - r, y + h - r, r, (0.0f64).to_radians(), (90.0f64).to_radians());
                                                c.arc(x + r, y + h - r, r, (90.0f64).to_radians(), (180.0f64).to_radians());
                                                c.arc(x + r, y + r, r, (180.0f64).to_radians(), (270.0f64).to_radians());
                                                c.close_path();
                                                c.fill().unwrap();
                                            } else {
                                                if is_first && is_last {
                                                    // Single button in group: all corners rounded
                                                    c.new_sub_path();
                                                    c.arc(x + w - r, y + r, r, (270.0f64).to_radians(), (360.0f64).to_radians());
                                                    c.arc(x + w - r, y + h - r, r, (0.0f64).to_radians(), (90.0f64).to_radians());
                                                    c.arc(x + r, y + h - r, r, (90.0f64).to_radians(), (180.0f64).to_radians());
                                                    c.arc(x + r, y + r, r, (180.0f64).to_radians(), (270.0f64).to_radians());
                                                    c.close_path();
                                                    c.fill().unwrap();
                                                } else if is_first {
                                                    // First button: left corners rounded
                                                    c.new_sub_path();
                                                    c.move_to(x + r, y);
                                                    c.line_to(x + w, y);
                                                    c.line_to(x + w, y + h);
                                                    c.line_to(x + r, y + h);
                                                    c.arc(x + r, y + h - r, r, (90.0f64).to_radians(), (180.0f64).to_radians());
                                                    c.line_to(x, y + r);
                                                    c.arc(x + r, y + r, r, (180.0f64).to_radians(), (270.0f64).to_radians());
                                                    c.close_path();
                                                    c.fill().unwrap();
                                                } else if is_last {
                                                    // Last button: right corners rounded
                                                    c.new_sub_path();
                                                    c.move_to(x, y);
                                                    c.line_to(x + w - r, y);
                                                    c.arc(x + w - r, y + r, r, (270.0f64).to_radians(), (360.0f64).to_radians());
                                                    c.line_to(x + w, y + h - r);
                                                    c.arc(x + w - r, y + h - r, r, (0.0f64).to_radians(), (90.0f64).to_radians());
                                                    c.line_to(x, y + h);
                                                    c.close_path();
                                                    c.fill().unwrap();
                                                } else {
                                                    // Middle buttons: no rounded corners
                                                    c.rectangle(x, y, w, h);
                                                    c.fill().unwrap();
                                                }
                                            }
                                        }
                                        // For macro buttons, always draw background with proper rounded corners
                                        if matches!(button.action, Key::Macro1 | Key::Macro2 | Key::Macro3 | Key::Macro4) && (button.background || button.active) {
                                            c.set_source_rgb(color, color, color);
                                            if is_first {
                                                // First macro button: left corners rounded
                                                c.new_sub_path();
                                                c.move_to(x + r, y);
                                                c.line_to(x + w, y);
                                                c.line_to(x + w, y + h);
                                                c.line_to(x + r, y + h);
                                                c.arc(x + r, y + h - r, r, (90.0f64).to_radians(), (180.0f64).to_radians());
                                                c.line_to(x, y + r);
                                                c.arc(x + r, y + r, r, (180.0f64).to_radians(), (270.0f64).to_radians());
                                                c.close_path();
                                                c.fill().unwrap();
                                            } else if is_last {
                                                // Last macro button: right corners rounded
                                                c.new_sub_path();
                                                c.move_to(x, y);
                                                c.line_to(x + w - r, y);
                                                c.arc(x + w - r, y + r, r, (270.0f64).to_radians(), (360.0f64).to_radians());
                                                c.line_to(x + w, y + h - r);
                                                c.arc(x + w - r, y + h - r, r, (0.0f64).to_radians(), (90.0f64).to_radians());
                                                c.line_to(x, y + h);
                                                c.close_path();
                                                c.fill().unwrap();
                                            } else {
                                                // Middle macro buttons: no rounded corners
                                                c.rectangle(x, y, w, h);
                                                c.fill().unwrap();
                                            }
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
                                    }
                                    // Always update left_edge
                                    left_edge += media_button_widths[i];
                                    if i != media_count - 1 {
                                        left_edge += media_spacing_px;
                                    }
                                }
                            }
                        }
                    }
                }
                return modified_regions;
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
                let button_width = ((width - pixel_shift_width as i32) - (BUTTON_SPACING_PX * (self.buttons.len() - 1) as i32)) as f64 / self.buttons.len() as f64;
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
                for (i, button) in self.buttons.iter_mut().enumerate() {
                    if !button.changed && !complete_redraw {
                        continue;
                    };
                    let left_edge = (i as f64 * (button_width + BUTTON_SPACING_PX as f64)).floor() + pixel_shift_x + (pixel_shift_width / 2) as f64;
                    let color = if button.active {
                        BUTTON_COLOR_ACTIVE
                    } else if config.show_button_outlines {
                        BUTTON_COLOR_INACTIVE
                    } else {
                        0.0
                    };
                    if !complete_redraw {
                        c.set_source_rgb(0.0, 0.0, 0.0);
                        if button.action == Key::Time {
                            c.rectangle(left_edge, bot - radius, button_width * 3.0, top - bot + radius * 2.0);
                        } else {
                            c.rectangle(left_edge, bot - radius, button_width, top - bot + radius * 2.0);
                        }
                        c.fill().unwrap();
                    }
                    if (button.action != Key::Unknown &&
                       button.action != Key::Time &&
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
                    let right = (left_edge + button_width.ceil()) - radius;
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
                    if button.action == Key::Time {
                        button.render(&c, height, left_edge, button_width.ceil() as u64 * 3, pixel_shift_y);
                    } else {
                        button.render(&c, height, left_edge, button_width.ceil() as u64, pixel_shift_y);
                    }

                    button.changed = false;

                    if !complete_redraw {
                        if button.action == Key::Time {
                            modified_regions.push(ClipRect::new(
                                height as u16 - top as u16 - radius as u16,
                                left_edge as u16,
                                height as u16 - bot as u16 + radius as u16,
                                left_edge as u16 + button_width as u16 * 3
                            ));
                        } else {
                            modified_regions.push(ClipRect::new(
                                height as u16 - top as u16 - radius as u16,
                                left_edge as u16,
                                height as u16 - bot as u16 + radius as u16,
                                left_edge as u16 + button_width as u16
                            ));
                        }
                    }
                }
                modified_regions
            }
        }
    }
    /// Returns (group, index) where group is "modules" or "media" or "flat", and index is the button index in that group
    pub fn hit_test(&self, x: f64, width: i32) -> Option<(&'static str, usize)> {
        match &self.split {
            Some(split) => {
                let group_spacing = BUTTON_SPACING_PX as f64; // space between groups
                let total_width = (width - group_spacing as i32) as f64;
                let modules_width = (split.modules_width as f64 * total_width).round();
                let media_width = (split.media_width as f64 * total_width).round();
                let modules_count = split.modules.len();
                let media_count = split.media.len();
                let modules_spacing = if modules_count > 1 { BUTTON_SPACING_PX as f64 * (modules_count as f64 - 1.0) } else { 0.0 };
                let modules_button_width = (modules_width - modules_spacing) / modules_count as f64;
                let mut left_edge = 0.0;
                // Check modules
                for (i, _) in split.modules.iter().enumerate() {
                    let right_edge = left_edge + modules_button_width;
                    // DEBUG: Print hit_test info for modules
                    println!("HITTEST MODULES: idx={}, left_edge={}, right_edge={}", i, left_edge, right_edge);
                    if x >= left_edge && x < right_edge {
                        return Some(("modules", i));
                    }
                    left_edge = right_edge + BUTTON_SPACING_PX as f64;
                }
                // Add extra spacing between groups
                left_edge += group_spacing;
                // Check media (with fraction/weight logic and 2px spacing)
                let media_spacing_px = 2.0f64;
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
                for (i, _) in split.media.iter().enumerate() {
                    let right_edge = left_edge + media_button_widths[i];
                    // DEBUG: Print hit_test info for media
                    println!("HITTEST MEDIA: idx={}, left_edge={}, right_edge={}", i, left_edge, right_edge);
                    if x >= left_edge && x < right_edge {
                        return Some(("media", i));
                    }
                    left_edge = right_edge;
                    if i != media_count - 1 {
                        left_edge += media_spacing_px;
                    }
                }
                None
            }
            None => {
                let count = self.buttons.len();
                let spacing = if count > 1 { BUTTON_SPACING_PX as f64 * (count as f64 - 1.0) } else { 0.0 };
                let button_width = (width as f64 - spacing) / count as f64;
                let mut left_edge = 0.0;
                for (i, _) in self.buttons.iter().enumerate() {
                    let right_edge = left_edge + button_width;
                    if x >= left_edge && x < right_edge {
                        return Some(("flat", i));
                    }
                    left_edge = right_edge + BUTTON_SPACING_PX as f64;
                }
                None
            }
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
    let mut drm = DrmBackend::open_card().unwrap();
    let (height, width) = drm.mode().size();
    let _ = panic::catch_unwind(AssertUnwindSafe(|| {
        real_main(&mut drm)
    }));
    let crash_bitmap = include_bytes!("crash_bitmap.raw");
    let mut map = drm.map().unwrap();
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
    drm.dirty(&[ClipRect::new(0, 0, height as u16, width as u16)]).unwrap();
    let mut sigset = SigSet::empty();
    sigset.add(Signal::SIGTERM);
    sigset.wait().unwrap();
}

fn real_main(drm: &mut DrmBackend) {
    let (height, width) = drm.mode().size();
    let (db_width, db_height) = drm.fb_info().unwrap().size();
    let mut uinput = UInputHandle::new(OpenOptions::new().write(true).open("/dev/uinput").unwrap());
    let mut backlight = BacklightManager::new();
    let mut last_redraw_minute = Local::now().minute();
    let mut cfg_mgr = ConfigManager::new();
    let (mut cfg, mut layers) = cfg_mgr.load_config(width);
    let mut pixel_shift = PixelShiftManager::new();

    // drop privileges to input and video group
    let groups = ["input", "video"];

    PrivDrop::default()
        .user("nobody")
        .group_list(&groups)
        .apply()
        .unwrap_or_else(|e| { panic!("Failed to drop privileges: {}", e) });

    let mut surface = ImageSurface::create(Format::ARgb32, db_width as i32, db_height as i32).unwrap();
    let mut active_layer = 0;
    let mut needs_complete_redraw = true;

    let mut input_tb = Libinput::new_with_udev(Interface);
    let mut input_main = Libinput::new_with_udev(Interface);
    input_tb.udev_assign_seat("seat-touchbar").unwrap();
    input_main.udev_assign_seat("seat0").unwrap();
    let epoll = Epoll::new(EpollCreateFlags::empty()).unwrap();
    epoll.add(input_main.as_fd(), EpollEvent::new(EpollFlags::EPOLLIN, 0)).unwrap();
    epoll.add(input_tb.as_fd(), EpollEvent::new(EpollFlags::EPOLLIN, 1)).unwrap();
    epoll.add(cfg_mgr.fd(), EpollEvent::new(EpollFlags::EPOLLIN, 2)).unwrap();
    uinput.set_evbit(EventKind::Key).unwrap();
    for layer in &layers {
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
    loop {
        if cfg_mgr.update_config(&mut cfg, &mut layers, width) {
            active_layer = 0;
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

        let current_minute = Local::now().minute();
	for button in &mut layers[active_layer].buttons {
    	    if (button.action == Key::Time) && (current_minute != last_redraw_minute) {
                needs_complete_redraw = true;
                last_redraw_minute = current_minute;
    	    }
    	}

        let any_changed = if let Some(split) = &layers[active_layer].split {
            split.modules.iter().any(|b| b.changed) || split.media.iter().any(|b| b.changed)
        } else {
            layers[active_layer].buttons.iter().any(|b| b.changed)
        };
        if needs_complete_redraw || any_changed {
            let shift = if cfg.enable_pixel_shift {
                pixel_shift.get()
            } else {
                (0.0, 0.0)
            };
            let clips = layers[active_layer].draw(&cfg, width as i32, height as i32, &surface, shift, needs_complete_redraw);
            let data = surface.data().unwrap();
            drm.map().unwrap().as_mut()[..data.len()].copy_from_slice(&data);
            drm.dirty(&clips).unwrap();
            needs_complete_redraw = false;
        }

        match epoll.wait(&mut [EpollEvent::new(EpollFlags::EPOLLIN, 0)], next_timeout_ms as isize) {
            Err(Errno::EINTR) | Ok(_) => { 0 },
            e => e.unwrap(),
        };
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
                            KeyState::Pressed => 1,
                            KeyState::Released => 0
                        };
                        if active_layer != new_layer {
                            active_layer = new_layer;
                            needs_complete_redraw = true;
                        }
                    } else if key.key() == Key::Macro1 as u32 && key.key_state() == KeyState::Pressed {
                        if cfg.media_layer_default {active_layer = 0;} else {active_layer = 1;}
                        needs_complete_redraw = true;
                    } else if key.key() == Key::Macro2 as u32 && key.key_state() == KeyState::Pressed {
                        active_layer = 2;
                        needs_complete_redraw = true;
                    } else if key.key() == Key::Macro3 as u32 && key.key_state() == KeyState::Pressed {
                        active_layer = 3;
                        needs_complete_redraw = true;
                    }
                },
                Event::Touch(te) => {
                    if Some(te.device()) != digitizer || backlight.current_bl() == 0 {
                        continue
                    }
                    match te {
                        TouchEvent::Down(dn) => {
                            let x = dn.x_transformed(width as u32);
                            let y = dn.y_transformed(height as u32);
                            if let Some((group, idx)) = layers[active_layer].hit_test(x, width as i32) {
                                println!("Touch hit: group={}, idx={}", group, idx);
                                match group {
                                    "modules" => {
                                        if let Some(split) = &mut layers[active_layer].split {
                                            let button = &mut split.modules[idx];
                                            println!("Setting active for group=modules, idx={}, action={:?}", idx, button.action);
                                            if button.action == Key::Unknown || button.action == Key::Time {
                                                continue;
                                            }
                                            touches.insert(dn.seat_slot(), (active_layer, group, idx));
                                            button.set_active(&mut uinput, true);
                                        }
                                    }
                                    "media" => {
                                        if let Some(split) = &mut layers[active_layer].split {
                                            let button = &mut split.media[idx];
                                            println!("Setting active for group=media, idx={}, action={:?}", idx, button.action);
                                            if button.action == Key::Unknown || button.action == Key::Time {
                                                continue;
                                            }
                                            touches.insert(dn.seat_slot(), (active_layer, group, idx));
                                            button.set_active(&mut uinput, true);
                                        }
                                    }
                                    "flat" => {
                                        let button = &mut layers[active_layer].buttons[idx];
                                        println!("Setting active for group=flat, idx={}, action={:?}", idx, button.action);
                                        if button.action == Key::Unknown || button.action == Key::Time {
                                            continue;
                                        }
                                        touches.insert(dn.seat_slot(), (active_layer, group, idx));
                                        button.set_active(&mut uinput, true);
                                    }
                                    _ => {}
                                }
                            }
                        },
                        TouchEvent::Motion(mtn) => {
                            if !touches.contains_key(&mtn.seat_slot()) {
                                continue;
                            }
                            let x = mtn.x_transformed(width as u32);
                            let y = mtn.y_transformed(height as u32);
                            let (layer, group, idx) = touches.get(&mtn.seat_slot()).unwrap();
                            println!("Motion: group={}, idx={}", group, idx);
                            match *group {
                                "modules" => {
                                    if let Some(split) = &mut layers[*layer].split {
                                        let button = &mut split.modules[*idx];
                                        println!("Motion set active for group=modules, idx={}, action={:?}", idx, button.action);
                                        if button.action == Key::Unknown || button.action == Key::Time {
                                            continue;
                                        }
                                        button.set_active(&mut uinput, true);
                                    }
                                }
                                "media" => {
                                    if let Some(split) = &mut layers[*layer].split {
                                        let button = &mut split.media[*idx];
                                        println!("Motion set active for group=media, idx={}, action={:?}", idx, button.action);
                                        if button.action == Key::Unknown || button.action == Key::Time {
                                            continue;
                                        }
                                        button.set_active(&mut uinput, true);
                                    }
                                }
                                "flat" => {
                                    let button = &mut layers[*layer].buttons[*idx];
                                    println!("Motion set active for group=flat, idx={}, action={:?}", idx, button.action);
                                    if button.action == Key::Unknown || button.action == Key::Time {
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
                                    if let Some(split) = &mut layers[*layer].split {
                                        let button = &mut split.modules[*idx];
                                        println!("Up set inactive for group=modules, idx={}, action={:?}", idx, button.action);
                                        if button.action == Key::Unknown || button.action == Key::Time {
                                            continue;
                                        }
                                        button.set_active(&mut uinput, false);
                                    }
                                }
                                "media" => {
                                    if let Some(split) = &mut layers[*layer].split {
                                        let button = &mut split.media[*idx];
                                        println!("Up set inactive for group=media, idx={}, action={:?}", idx, button.action);
                                        if button.action == Key::Unknown || button.action == Key::Time {
                                            continue;
                                        }
                                        button.set_active(&mut uinput, false);
                                    }
                                }
                                "flat" => {
                                    let button = &mut layers[*layer].buttons[*idx];
                                    println!("Up set inactive for group=flat, idx={}, action={:?}", idx, button.action);
                                    if button.action == Key::Unknown || button.action == Key::Time {
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
    }
}
