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
pub enum MediaLayerError {
    #[error("Cairo error: {0}")]
    Cairo(String),
    #[error("Button error: {0}")]
    Button(#[from] super::button::ButtonError),
    #[error("Invalid font renderer: {0}")]
    InvalidFontRenderer(String),
}

type MediaLayerResult<T> = Result<T, MediaLayerError>;

// Constants
pub const BUTTON_SPACING_PX: i32 = 16;
pub const BUTTON_COLOR_INACTIVE: f64 = 0.250;
pub const BUTTON_COLOR_ACTIVE: f64 = 0.400;

// Layout constants
const MEDIA_SPACING_PX: f64 = 2.0;
const BUTTON_RADIUS: f64 = 13.0;
const BOTTOM_MARGIN_RATIO: f64 = 0.0;
const TOP_MARGIN_RATIO: f64 = 1.0;
const FONT_SIZE: f64 = 32.0;

/// Split layout for MediaLayer (App Layer 1 / Media layer)
/// This layout divides the touch bar into two sections:
/// - Left: Modules section (apps, system functions)
/// - Right: Media section (playback controls, volume, etc.)
pub struct SplitLayout {
    /// Width ratio for the modules section (0.0 to 1.0)
    pub modules_width: f32,
    /// Media buttons (play, pause, volume, etc.)
    pub media: Vec<Button>,
}

// Layout calculation helper struct for MediaLayer split layout
#[derive(Debug, Clone)]
struct MediaLayerLayoutInfo {
    gap: f64,
    modules_width: f64,
    media_width: f64,
    button_widths: Vec<f64>,
}

impl MediaLayerLayoutInfo {
    /// Creates layout info for MediaLayer split layout
    fn new(width: i32, split: &SplitLayout, pixel_shift_width: i32, available_mpris_services: &[String]) -> Self {
        let gap = BUTTON_SPACING_PX as f64;
        let total_width = (width - pixel_shift_width) as f64;
        let modules_width = (split.modules_width as f64 * total_width).round();
        let media_width = total_width - modules_width - gap;
        
        let media_count = split.media.len();
        
        // Filter out hidden buttons (toggle button when no MPRIS services)
        let visible_buttons: Vec<(usize, &Button)> = split.media.iter().enumerate()
            .filter(|(_, button)| {
                !(button.special_type.as_ref().map_or(false, |t| t == "toggle") && available_mpris_services.is_empty())
            })
            .collect();
        
        let visible_count = visible_buttons.len();
        let total_spacing = if visible_count > 1 { MEDIA_SPACING_PX * (visible_count as f64 - 1.0) } else { 0.0 };
        let button_area = media_width - total_spacing;
        
        let weights: Vec<f32> = visible_buttons.iter().map(|(_, b)| b.fraction.unwrap_or(1.0)).collect();
        let total_weight: f32 = weights.iter().sum();
        let visible_button_widths: Vec<f64> = weights.iter().map(|w| button_area * (*w as f64 / total_weight as f64)).collect();
        
        // Create width array only for visible buttons, not including hidden ones
        let mut button_widths: Vec<f64> = vec![0.0; media_count];
        let mut visible_idx = 0;
        let last_original_idx = visible_buttons.last().map(|(idx, _)| *idx);
        
        for (original_idx, _) in visible_buttons {
            button_widths[original_idx] = visible_button_widths[visible_idx];
            visible_idx += 1;
        }
        
        // Last visible button absorbs rounding error
        if let Some(last_original_idx) = last_original_idx {
            let sum_widths: f64 = button_widths.iter().sum();
            button_widths[last_original_idx] += button_area - sum_widths;
        }
        
        Self {
            gap,
            modules_width,
            media_width,
            button_widths,
        }
    }
}

pub struct MediaLayer {
    /// Split layout for modules (left) and media buttons (right)
    pub split: SplitLayout,
}

impl MediaLayer {
    /// Creates a MediaLayer with split layout between modules and media
    pub fn with_split(modules_width: f32, media: Vec<ButtonConfig>, _media_width: f32) -> MediaLayer {
        MediaLayer {
            split: SplitLayout {
                modules_width,
                media: media.into_iter().map(Button::with_config).collect(),
            },
        }
    }

    // Helper function for safe Cairo operations
    fn safe_cairo_context(surface: &Surface) -> MediaLayerResult<Context> {
        Context::new(surface)
            .map_err(|e| MediaLayerError::Cairo(format!("Failed to create Cairo context: {}", e)))
    }

    fn safe_cairo_paint(c: &Context) -> MediaLayerResult<()> {
        c.paint()
            .map_err(|e| MediaLayerError::Cairo(format!("Failed to paint: {}", e)))
    }

    fn safe_cairo_fill(c: &Context) -> MediaLayerResult<()> {
        c.fill()
            .map_err(|e| MediaLayerError::Cairo(format!("Failed to fill: {}", e)))
    }

    // Helper function to setup font rendering
    fn setup_font_rendering(c: &Context, config: &Config) -> MediaLayerResult<()> {
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
            renderer => Err(MediaLayerError::InvalidFontRenderer(renderer.to_string()))
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

    fn draw_modules_section_static(c: &Context, left_edge: f64, bot: f64, modules_width: f64, modules_height: f64, radius: f64, session_state: Option<&SessionState>, current_window_class: Option<&str>, mut app_ui_manager: Option<&mut AppUiManager>, media_player_drag_position: Option<f64>, modified_regions: &mut Vec<ClipRect>) -> MediaLayerResult<()> {
        match session_state {
            Some(state) if state.is_logged_in => {
                if let Some(app_ui_manager) = &mut app_ui_manager {
                    app_ui_manager.draw_app_ui(
                        c, left_edge, bot, modules_width, modules_height, radius,
                        1.0, current_window_class.as_deref(), media_player_drag_position, modified_regions
                    );
                }
            }
            Some(_) => {
                if let Some(app_ui_manager) = &mut app_ui_manager {
                    app_ui_manager.draw_app_ui(
                        c, left_edge, bot, modules_width, modules_height, radius,
                        1.0, Some("Not Logged In"), None, modified_regions
                    );
                }
            }
            _ => {}
        }
        Ok(())
    }

    fn draw_media_section_static(c: &Context, media: &mut [Button], button_widths: &[f64], media_width: f64, media_count: usize, left_edge: f64, bot: f64, top: f64, radius: f64, height: i32, config: &Config, complete_redraw: bool, modified_regions: &mut Vec<ClipRect>, session_state: Option<&SessionState>, available_mpris_services: &[String]) -> MediaLayerResult<()> {
        crate::view::media_screen::draw_media_section(
            c, media, button_widths, media_width, media_count,
            left_edge, bot, top, radius, height, config, complete_redraw,
            modified_regions, session_state, available_mpris_services
        );
        Ok(())
    }
}

impl Layer for MediaLayer {
    fn draw(
        &mut self,
        config: &Config,
        width: i32,
        height: i32,
        surface: &Surface,
        pixel_shift: (f64, f64),
        complete_redraw: bool,
        modules_only_redraw: bool,
        session_state: Option<&SessionState>,
        _layer_index: Option<LayerKey>,
        _app_layer3_slide_progress: f64,
        current_window_class: Option<&str>,
        app_ui_manager: Option<&mut AppUiManager>,
        media_player_drag_position: Option<f64>,
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
        
        let available_mpris_services: Vec<String> = app_ui_manager.as_ref().map_or(Vec::new(), |mgr| mgr.generic_background_screen.available_mpris_services.clone());
        let layout_info = MediaLayerLayoutInfo::new(width, &self.split, pixel_shift_width, &available_mpris_services);
        let (bot, top) = Self::calculate_button_dimensions(height);
        let (pixel_shift_x, _) = pixel_shift;
        
        // Clear background
        if complete_redraw {
            Self::safe_cairo_paint(&c)?;
        } else if modules_only_redraw {
            // Only clear the modules area for modules-only redraw
            c.rectangle(
                pixel_shift_x + (pixel_shift_width / 2) as f64, 
                bot - BUTTON_RADIUS, 
                layout_info.modules_width, 
                top - bot + BUTTON_RADIUS * 2.0
            );
            Self::safe_cairo_fill(&c)?;
        }
        
        // Setup font rendering
        Self::setup_font_rendering(&c, config)?;
        
        // Clear modules area first to prevent text overlap when switching between session states
        let left_edge = pixel_shift_x + (pixel_shift_width / 2) as f64;
        c.rectangle(
            left_edge, 
            bot - BUTTON_RADIUS, 
            layout_info.modules_width, 
            top - bot + BUTTON_RADIUS * 2.0
        );
        Self::safe_cairo_fill(&c)?;
        
        // Get MPRIS services before moving app_ui_manager
        let available_mpris_services: Vec<String> = app_ui_manager.as_ref().map_or(Vec::new(), |mgr| mgr.generic_background_screen.available_mpris_services.clone());
        
        // Draw modules section (left side of split layout)
        let modules_result = Self::draw_modules_section_static(
            &c, left_edge, bot, layout_info.modules_width, top - bot, BUTTON_RADIUS,
            session_state, current_window_class, app_ui_manager, media_player_drag_position, &mut modified_regions
        );
        if let Err(e) = modules_result {
            return Err(e.into());
        }
        
        // Skip media section if this is a modules-only redraw
        if !modules_only_redraw {
            let media_count = self.split.media.len();
            let media_result = Self::draw_media_section_static(
                &c, &mut self.split.media, &layout_info.button_widths, layout_info.media_width,
                media_count, left_edge + layout_info.modules_width + layout_info.gap,
                bot, top, BUTTON_RADIUS, height, config, complete_redraw, &mut modified_regions,
                session_state, &available_mpris_services
            );
            if let Err(e) = media_result {
                return Err(e.into());
            }
        }
        
        Ok(modified_regions)
    }

    fn hit_test(
        &self,
        x: f64,
        width: i32,
        _layer_index: Option<LayerKey>,
        available_mpris_services: &[String],
    ) -> Option<(&'static str, usize)> {
        let gap = BUTTON_SPACING_PX as f64;
        let total_width = (width - gap as i32) as f64;
        let modules_width = (self.split.modules_width as f64 * total_width).round();
        
        // Check modules section
        if x >= 0.0 && x < modules_width {
            return Some(("modules", 0));
        }
        
        // Check media section
        let media_width = total_width - modules_width - gap;
        let media_count = self.split.media.len();
        
        // Filter out hidden buttons (toggle button when no MPRIS services)
        let visible_buttons: Vec<(usize, &Button)> = self.split.media.iter().enumerate()
            .filter(|(_, button)| {
                !(button.special_type.as_ref().map_or(false, |t| t == "toggle") && available_mpris_services.is_empty())
            })
            .collect();
        
        let visible_count = visible_buttons.len();
        let total_spacing = if visible_count > 1 { MEDIA_SPACING_PX * (visible_count as f64 - 1.0) } else { 0.0 };
        let button_area = media_width - total_spacing;
        
        let weights: Vec<f32> = visible_buttons.iter().map(|(_, b)| b.fraction.unwrap_or(1.0)).collect();
        let total_weight: f32 = weights.iter().sum();
        let visible_button_widths: Vec<f64> = weights.iter().map(|w| button_area * (*w as f64 / total_weight as f64)).collect();
        
        // Create width array only for visible buttons
        let mut media_button_widths: Vec<f64> = vec![0.0; media_count];
        let mut visible_idx = 0;
        for (original_idx, _) in visible_buttons {
            media_button_widths[original_idx] = visible_button_widths[visible_idx];
            visible_idx += 1;
        }
        
        let sum_widths: f64 = media_button_widths.iter().sum();
        if let Some(last) = media_button_widths.last_mut() {
            *last += button_area - sum_widths;
        }
        
        // Media section starts after modules_width + gap
        let mut left_edge = modules_width + gap;
        for (i, button) in self.split.media.iter().enumerate() {
            // Skip hit testing for hidden buttons (width is 0.0)
            if media_button_widths[i] == 0.0 {
                continue;
            }
            
            let right_edge = left_edge + media_button_widths[i];
            if x >= left_edge && x < right_edge {
                return Some(("media", i));
            }
            left_edge = right_edge;
            if i != media_count - 1 {
                left_edge += MEDIA_SPACING_PX;
            }
        }
        
        // Check if touch is in the gap/separator area - return None to ignore
        if x >= modules_width && x < modules_width + gap {
            return None;
        }
        
        None
    }

    fn layer_key(&self) -> LayerKey {
        LayerKey::Media
    }

    fn add_esc_button(&mut self) {
        // Media layer doesn't add ESC button as it has split layout
        // ESC button would interfere with the modules/media split
    }

    fn any_buttons_changed(&self) -> bool {
        self.split.media.iter().any(|b| b.changed)
    }

    fn get_buttons_for_time_check(&mut self) -> Option<&mut Vec<Button>> {
        Some(&mut self.split.media)
    }

    fn has_split_layout(&self) -> bool {
        true
    }

    fn get_all_buttons(&self) -> Vec<&Button> {
        self.split.media.iter().collect()
    }

    fn get_buttons_mut(&mut self) -> Option<&mut Vec<Button>> {
        None
    }

    fn get_media_buttons_mut(&mut self) -> Option<&mut Vec<Button>> {
        Some(&mut self.split.media)
    }
}
