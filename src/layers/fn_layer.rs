use cairo::{Context, Surface};
use drm::control::ClipRect;
use anyhow::Result;

use crate::config::{ButtonConfig, Config};
use crate::services::sessionmanager::SessionState;
use crate::view::app_ui_manager::AppUiManager;
use super::button::Button;
use super::layer_trait::Layer;
use crate::LayerKey;

// Error handling types and functions
#[derive(Debug, thiserror::Error)]
pub enum FnLayerError {
    #[error("Cairo error: {0}")]
    Cairo(String),
    #[error("Button error: {0}")]
    Button(#[from] super::button::ButtonError),
    #[error("Invalid font renderer: {0}")]
    InvalidFontRenderer(String),
}

type FnLayerResult<T> = Result<T, FnLayerError>;

// Constants
pub const BUTTON_SPACING_PX: i32 = 16;
pub const BUTTON_COLOR_INACTIVE: f64 = 0.250;
pub const BUTTON_COLOR_ACTIVE: f64 = 0.400;

// Layout constants
const BUTTON_RADIUS: f64 = 13.0;
const BOTTOM_MARGIN_RATIO: f64 = 0.0;
const TOP_MARGIN_RATIO: f64 = 1.0;
const FONT_SIZE: f64 = 32.0;

pub struct FnLayer {
    pub buttons: Vec<Button>,
}

impl FnLayer {
    /// Creates a function layer with buttons
    pub fn with_config(cfg: Vec<ButtonConfig>) -> FnLayer {
        if cfg.is_empty() {
            panic!("Invalid configuration, Fn layer has 0 buttons");
        }
        FnLayer {
            buttons: cfg.into_iter().map(Button::with_config).collect(),
        }
    }

    // Helper function for safe Cairo operations
    fn safe_cairo_context(surface: &Surface) -> FnLayerResult<Context> {
        Context::new(surface)
            .map_err(|e| FnLayerError::Cairo(format!("Failed to create Cairo context: {}", e)))
    }

    fn safe_cairo_paint(c: &Context) -> FnLayerResult<()> {
        c.paint()
            .map_err(|e| FnLayerError::Cairo(format!("Failed to paint: {}", e)))
    }

    fn safe_cairo_fill(c: &Context) -> FnLayerResult<()> {
        c.fill()
            .map_err(|e| FnLayerError::Cairo(format!("Failed to fill: {}", e)))
    }

    // Helper function to setup font rendering
    fn setup_font_rendering(c: &Context, config: &Config) -> FnLayerResult<()> {
        match config.font_renderer.to_lowercase().as_str() {
            "cairo" => {
                c.select_font_face(
                    &config.font_style_cairo,
                    if config.italic_cairo { cairo::FontSlant::Italic } else { cairo::FontSlant::Normal },
                    if config.bold_cairo { cairo::FontWeight::Bold } else { cairo::FontWeight::Normal }
                );
                Ok(())
            }
            "freetype" => {
                c.set_font_face(&config.font_face);
                Ok(())
            }
            renderer => Err(FnLayerError::InvalidFontRenderer(renderer.to_string()))
        }?;
        
        c.set_font_size(FONT_SIZE);
        Ok(())
    }

    // Helper function to calculate button dimensions
    fn calculate_button_dimensions(height: i32) -> (f64, f64) {
        let height_f64 = height as f64;
        let radius_pixels = BUTTON_RADIUS;
        
        // Adjust margins to account for button radius
        let bot = (height_f64 * BOTTOM_MARGIN_RATIO) + radius_pixels;
        let top = (height_f64 * TOP_MARGIN_RATIO) - radius_pixels;
        (bot, top)
    }

    fn draw_rounded_button_static(c: &Context, left_edge: f64, bot: f64, top: f64, width: f64, radius: f64) -> FnLayerResult<()> {
        c.new_sub_path();
        let left = left_edge + radius;
        let right = (left_edge + width.ceil()) - radius;
        
        c.arc(right, bot, radius, (-90.0f64).to_radians(), (0.0f64).to_radians());
        c.arc(right, top, radius, (0.0f64).to_radians(), (90.0f64).to_radians());
        c.arc(left, top, radius, (90.0f64).to_radians(), (180.0f64).to_radians());
        c.arc(left, bot, radius, (180.0f64).to_radians(), (270.0f64).to_radians());
        c.close_path();

        Self::safe_cairo_fill(c)
    }

    fn get_flat_layout_info(&self, count: usize, width: i32, gap: f64) -> (f64, f64, Vec<f64>) {
        let spacing = if count > 1 { gap * (count as f64 - 1.0) } else { 0.0 };
        let button_area = (width as f64) - spacing;
        
        let weights: Vec<f32> = self.buttons.iter().map(|b| b.fraction.unwrap_or(1.0)).collect();
        let total_weight: f32 = weights.iter().sum();
        let mut button_widths: Vec<f64> = weights.iter().map(|w| button_area * (*w as f64 / total_weight as f64)).collect();
        
        // Last button absorbs rounding error
        let sum_widths: f64 = button_widths.iter().sum();
        if let Some(last) = button_widths.last_mut() {
            *last += button_area - sum_widths;
        }
        
        (spacing, button_area, button_widths)
    }
}

impl Layer for FnLayer {
    fn draw(
        &mut self,
        config: &Config,
        width: i32,
        height: i32,
        surface: &Surface,
        pixel_shift: (f64, f64),
        complete_redraw: bool,
        _modules_only_redraw: bool,
        _session_state: Option<&SessionState>,
        _layer_index: Option<LayerKey>,
        _app_layer3_slide_progress: f64,
        _current_window_class: Option<&str>,
        _app_ui_manager: Option<&mut AppUiManager>,
        _media_player_drag_position: Option<f64>,
    ) -> Result<Vec<ClipRect>> {
        let c = Self::safe_cairo_context(surface)?;
        let mut modified_regions = if complete_redraw {
            vec![ClipRect::new(0, 0, height as u16, width as u16)]
        } else {
            Vec::new()
        };
        
        c.translate(height as f64, 0.0);
        c.rotate((90.0f64).to_radians());
        
        let pixel_shift_width = if config.enable_pixel_shift { 
            crate::display::pixel_shift::PIXEL_SHIFT_WIDTH_PX as i32
        } else { 
            0 
        };
        
        let gap = BUTTON_SPACING_PX as f64;
        let count = self.buttons.len();
        let (_, _, button_widths) = self.get_flat_layout_info(count, width - pixel_shift_width, gap);
        
        let (bot, top) = Self::calculate_button_dimensions(height);
        let (pixel_shift_x, pixel_shift_y) = pixel_shift;
        
        if complete_redraw {
            Self::safe_cairo_paint(&c)?;
        }
        
        // Setup font rendering
        Self::setup_font_rendering(&c, config)?;
        
        let mut left_edge = pixel_shift_x + (pixel_shift_width / 2) as f64;
        
        for (i, button) in self.buttons.iter_mut().enumerate() {
            let this_button_width = button_widths[i];
            
            if !button.changed && !complete_redraw {
                left_edge += this_button_width;
                if i != count - 1 {
                    left_edge += gap;
                }
                continue;
            }
            
            let color = if button.active {
                BUTTON_COLOR_ACTIVE
            } else if config.show_button_outlines {
                BUTTON_COLOR_INACTIVE
            } else {
                0.0
            };
            
            if !complete_redraw {
                c.rectangle(
                    left_edge, 
                    bot - BUTTON_RADIUS, 
                    this_button_width, 
                    top - bot + BUTTON_RADIUS * 2.0
                );
                Self::safe_cairo_fill(&c)?;
            }
            
            if (button.action != input_linux::Key::Macro1 &&
               button.action != input_linux::Key::Macro2 &&
               button.action != input_linux::Key::Macro3 &&
               button.action != input_linux::Key::Macro4) &&
               ((button.background) || button.active) {
                
                c.set_source_rgb(color, color, color);
                Self::draw_rounded_button_static(&c, left_edge, bot, top, this_button_width, BUTTON_RADIUS)?;
            }
            
            c.set_source_rgb(1.0, 1.0, 1.0);
            button.render(&c, height, left_edge, this_button_width.ceil() as u64, pixel_shift_y)?;

            button.changed = false;

            if !complete_redraw {
                let clip_rect = ClipRect::new(
                    height as u16 - top as u16 - BUTTON_RADIUS as u16,
                    left_edge as u16,
                    height as u16 - bot as u16 + BUTTON_RADIUS as u16,
                    (left_edge + this_button_width) as u16
                );
                modified_regions.push(clip_rect);
            }
            
            left_edge += this_button_width;
            if i != count - 1 {
                left_edge += gap;
            }
        }
        
        Ok(modified_regions)
    }

    fn hit_test(
        &self,
        x: f64,
        width: i32,
        _layer_index: Option<LayerKey>,
        _available_mpris_services: &[String],
    ) -> Option<(&'static str, usize)> {
        let count = self.buttons.len();
        let gap = BUTTON_SPACING_PX as f64;
        let (_, _, button_widths) = self.get_flat_layout_info(count, width, gap);
        
        let mut left_edge = 0.0;
        for (i, _) in self.buttons.iter().enumerate() {
            let right_edge = left_edge + button_widths[i];
            if x >= left_edge && x < right_edge {
                return Some(("flat", i));
            }
            left_edge = right_edge;
            if i != count - 1 {
                left_edge += gap;
            }
        }
        None
    }

    fn layer_key(&self) -> LayerKey {
        LayerKey::Fn
    }

    fn add_esc_button(&mut self) {
        self.buttons.insert(0, Button::new_text("esc".to_string(), input_linux::Key::Esc, true));
    }

    fn any_buttons_changed(&self) -> bool {
        self.buttons.iter().any(|b| b.changed)
    }

    fn get_buttons_for_time_check(&mut self) -> Option<&mut Vec<Button>> {
        Some(&mut self.buttons)
    }

    fn has_split_layout(&self) -> bool {
        false
    }

    fn get_all_buttons(&self) -> Vec<&Button> {
        self.buttons.iter().collect()
    }

    fn get_buttons_mut(&mut self) -> Option<&mut Vec<Button>> {
        Some(&mut self.buttons)
    }

    fn get_media_buttons_mut(&mut self) -> Option<&mut Vec<Button>> {
        None
    }
}
