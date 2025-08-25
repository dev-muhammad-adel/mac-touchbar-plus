
use cairo::{Context, Surface, Rectangle};
use rsvg::CairoRenderer;
use input_linux::{uinput::UInputHandle, EventKind, Key, SynchronizeKind};
use input_linux_sys::{input_event, timeval};
use anyhow::Result;

use crate::utils::button_images::{self, ICON_SIZE};

// Error handling types and functions
#[derive(Debug, thiserror::Error)]
pub enum ButtonError {
    #[error("Cairo error: {0}")]
    Cairo(String),
}

type ButtonResult<T> = Result<T, ButtonError>;

// Helper function for safe Cairo operations
fn safe_cairo_text_extents(c: &Context, text: &str) -> ButtonResult<cairo::TextExtents> {
    c.text_extents(text)
        .map_err(|e| ButtonError::Cairo(format!("Failed to get text extents: {}", e)))
}

fn safe_cairo_show_text(c: &Context, text: &str) -> ButtonResult<()> {
    c.show_text(text)
        .map_err(|e| ButtonError::Cairo(format!("Failed to show text: {}", e)))
}

fn safe_cairo_fill(c: &Context) -> ButtonResult<()> {
    c.fill()
        .map_err(|e| ButtonError::Cairo(format!("Failed to fill: {}", e)))
}

fn safe_cairo_set_source_surface(c: &Context, surface: &Surface, x: f64, y: f64) -> ButtonResult<()> {
    c.set_source_surface(surface, x, y)
        .map_err(|e| ButtonError::Cairo(format!("Failed to set source surface: {}", e)))
}

fn safe_cairo_render_document(renderer: &CairoRenderer, c: &Context, rect: &Rectangle) -> ButtonResult<()> {
    renderer.render_document(c, rect)
        .map_err(|e| ButtonError::Cairo(format!("Failed to render document: {}", e)))
}

// Helper function to emit uinput events
fn emit<F>(uinput: &mut UInputHandle<F>, ty: EventKind, code: u16, value: i32) where F: std::os::fd::AsRawFd {
    if let Err(e) = uinput.write(&[input_event {
        value: value,
        type_: ty as u16,
        code: code,
        time: timeval {
            tv_sec: 0,
            tv_usec: 0
        }
    }]) {
        eprintln!("[button] Failed to emit uinput event: {}", e);
    }
}

pub fn toggle_key<F>(uinput: &mut UInputHandle<F>, code: Key, value: i32) where F: std::os::fd::AsRawFd {
    emit(uinput, EventKind::Key, code as u16, value);
    emit(uinput, EventKind::Synchronize, SynchronizeKind::Report as u16, 0);
}

pub struct Button {
    pub image: button_images::ButtonImage,
    pub changed: bool,
    pub active: bool,
    pub action: Key,
    pub background: bool,
    pub fraction: Option<f32>,
}

impl Button {
    pub fn with_config(cfg: crate::config::ButtonConfig) -> Button {
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

    pub fn new_text(text: String, action: Key, background: bool) -> Button {
        Button {
            action,
            active: false,
            changed: false,
            image: button_images::ButtonImage::Text(text),
            background,
            fraction: None,
        }
    }

    pub fn new_icon(icon_name: &str, action: Key, mode: Option<String>, path: &str, background: bool) -> Button {
        let theme = crate::config::ConfigManager::new().load_theme();
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

    pub fn new_blank(action: Key, background: bool) -> Button {
        Button {
            action,
            active: false,
            changed: false,
            image: button_images::ButtonImage::Blank,
            background,
            fraction: None,
        }
    }

    pub fn render(&self, c: &Context, height: i32, button_left_edge: f64, button_width: u64, y_shift: f64) -> ButtonResult<()> {
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
                // Handle other image types if needed
            }
        }
        Ok(())
    }

    pub fn set_active<F>(&mut self, uinput: &mut UInputHandle<F>, active: bool) where F: std::os::fd::AsRawFd {
        if self.active != active {
            self.active = active;
            self.changed = true;

            toggle_key(uinput, self.action, active as i32);
        }
    }
} 