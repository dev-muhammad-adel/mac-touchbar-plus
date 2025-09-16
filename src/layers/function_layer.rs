
use cairo::{Context, Surface};
use drm::control::ClipRect;
use anyhow::Result;

use crate::config::{ButtonConfig, Config};
use crate::services::sessionmanager::SessionState;
use crate::view::app_ui_manager::AppUiManager;
use super::button::Button;
use crate::LayerKey;

// Error handling types and functions
#[derive(Debug, thiserror::Error)]
pub enum FunctionLayerError {
    #[error("Cairo error: {0}")]
    Cairo(String),
    #[error("Button error: {0}")]
    Button(#[from] super::button::ButtonError),
    #[error("Invalid font renderer: {0}")]
    InvalidFontRenderer(String),
}

type FunctionLayerResult<T> = Result<T, FunctionLayerError>;

// Constants
pub const BUTTON_SPACING_PX: i32 = 16;
pub const APP_LAYER_KEYS2_GAP_PX: f64 = 4.0; // Custom gap for AppLayerKeys2 (Custom2)
pub const APP_LAYER_KEYS3_GAP_PX: f64 = 4.0; // Custom gap for AppLayerKeys3
pub const BUTTON_COLOR_INACTIVE: f64 = 0.172;
pub const BUTTON_COLOR_ACTIVE: f64 = 0.350;

// Layout constants
const MEDIA_SPACING_PX: f64 = 2.0;
const BUTTON_RADIUS: f64 = 8.0;
const BOTTOM_MARGIN_RATIO: f64 = 0.15;
const TOP_MARGIN_RATIO: f64 = 0.85;
const FONT_SIZE: f64 = 32.0;

#[derive(Default)]
pub struct FunctionLayer {
    pub buttons: Vec<Button>,
    /// Split layout is ONLY used for FunctionLayerKeys1 (App Layer 1 / Media layer)
    /// This provides the split between modules (left) and media buttons (right)
    pub split: Option<SplitLayout>,
}

/// Split layout specifically for FunctionLayerKeys1 (App Layer 1 / Media layer)
/// This layout divides the touch bar into two sections:
/// - Left: Modules section (apps, system functions)
/// - Right: Media section (playback controls, volume, etc.)
pub struct SplitLayout {
    /// Width ratio for the modules section (0.0 to 1.0)
    pub modules_width: f32,
    /// Media buttons (play, pause, volume, etc.)
    pub media: Vec<Button>,
}

// Layout calculation helper struct specifically for FunctionLayerKeys1 split layout
#[derive(Debug, Clone)]
struct FunctionLayerKeys1LayoutInfo {
    gap: f64,
    modules_width: f64,
    media_width: f64,
    button_widths: Vec<f64>,
}

impl FunctionLayerKeys1LayoutInfo {
    /// Creates layout info specifically for FunctionLayerKeys1 split layout
    fn new(width: i32, layer_index: Option<LayerKey>, split: &SplitLayout, pixel_shift_width: i32, available_mpris_services: &[String]) -> Self {
        let gap = Self::get_gap_for_layer(layer_index);
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
        
        // Create full width array with 0.0 for hidden buttons
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
    
    fn get_gap_for_layer(layer_index: Option<LayerKey>) -> f64 {
        match layer_index {
            Some(LayerKey::Custom2) => APP_LAYER_KEYS2_GAP_PX,
            Some(LayerKey::Custom3) => APP_LAYER_KEYS3_GAP_PX,
            _ => BUTTON_SPACING_PX as f64,
        }
    }
}

impl FunctionLayer {
    /// Creates a standard function layer with buttons (used for Fn keys, App Layer 2, App Layer 3)
    pub fn with_config(cfg: Vec<ButtonConfig>) -> FunctionLayer {
        if cfg.is_empty() {
            panic!("Invalid configuration, layer has 0 buttons");
        }
        FunctionLayer {
            buttons: cfg.into_iter().map(Button::with_config).collect(),
            split: None, // Standard layers don't use split layout
        }
    }

    /// Creates a FunctionLayerKeys1 (App Layer 1) with split layout between modules and media
    /// This is the ONLY layer that uses split layout
    pub fn with_split(modules_width: f32, media: Vec<ButtonConfig>, _media_width: f32) -> FunctionLayer {
        FunctionLayer {
            buttons: vec![], // Split layout doesn't use regular buttons
            split: Some(SplitLayout {
                modules_width,
                media: media.into_iter().map(Button::with_config).collect(),
            }),
        }
    }

    // Helper function for safe Cairo operations
    fn safe_cairo_context(surface: &Surface) -> FunctionLayerResult<Context> {
        Context::new(surface)
            .map_err(|e| FunctionLayerError::Cairo(format!("Failed to create Cairo context: {}", e)))
    }

    fn safe_cairo_paint(c: &Context) -> FunctionLayerResult<()> {
        c.paint()
            .map_err(|e| FunctionLayerError::Cairo(format!("Failed to paint: {}", e)))
    }

    fn safe_cairo_fill(c: &Context) -> FunctionLayerResult<()> {
        c.fill()
            .map_err(|e| FunctionLayerError::Cairo(format!("Failed to fill: {}", e)))
    }

    // Helper function to setup font rendering
    fn setup_font_rendering(c: &Context, config: &Config) -> FunctionLayerResult<()> {
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
            renderer => Err(FunctionLayerError::InvalidFontRenderer(renderer.to_string()))
        }?;
        
        c.set_font_size(FONT_SIZE);
        Ok(())
    }

    // Helper function to calculate button dimensions
    fn calculate_button_dimensions(height: i32) -> (f64, f64) {
        let bot = (height as f64) * BOTTOM_MARGIN_RATIO;
        let top = (height as f64) * TOP_MARGIN_RATIO;
        (bot, top)
    }

    pub fn draw(&mut self, config: &Config, width: i32, height: i32, surface: &Surface, pixel_shift: (f64, f64), complete_redraw: bool, modules_only_redraw: bool, session_state: Option<&SessionState>, layer_index: Option<LayerKey>, app_layer3_slide_progress: f64, current_window_class: Option<&str>, app_ui_manager: Option<&mut AppUiManager>, media_player_drag_position: Option<f64>) -> FunctionLayerResult<Vec<ClipRect>> {
        // Check if this is LayerKeys1 (Media layer) and has split layout
        if let Some(LayerKey::Media) = layer_index {
            if let Some(split) = &mut self.split {
                // This is FunctionLayerKeys1 (App Layer 1) with split layout
                Self::draw_function_layer_keys1_split_static(
                    config, width, height, surface, pixel_shift, complete_redraw, modules_only_redraw,
                    session_state, layer_index, app_layer3_slide_progress, current_window_class,
                    app_ui_manager, media_player_drag_position, split
                )
            } else {
                // LayerKeys1 should have split layout, but fallback to standard if missing
                self.draw_standard_function_layer(
                    config, width, height, surface, pixel_shift, complete_redraw,
                    layer_index, app_layer3_slide_progress
                )
            }
        } else {
            // This is a standard function layer (Fn keys, App Layer 2, App Layer 3)
            self.draw_standard_function_layer(
                config, width, height, surface, pixel_shift, complete_redraw,
                layer_index, app_layer3_slide_progress
            )
        }
    }

    /// Draws FunctionLayerKeys1 (App Layer 1) with split layout between modules and media
    /// This is a static method to avoid borrow checker issues
    fn draw_function_layer_keys1_split_static(config: &Config, width: i32, height: i32, surface: &Surface, pixel_shift: (f64, f64), complete_redraw: bool, modules_only_redraw: bool, session_state: Option<&SessionState>, layer_index: Option<LayerKey>, _app_layer3_slide_progress: f64, current_window_class: Option<&str>, app_ui_manager: Option<&mut AppUiManager>, media_player_drag_position: Option<f64>, split: &mut SplitLayout) -> FunctionLayerResult<Vec<ClipRect>> {
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
        let layout_info = FunctionLayerKeys1LayoutInfo::new(width, layer_index, split, pixel_shift_width, &available_mpris_services);
        let (bot, top) = Self::calculate_button_dimensions(height);
        let (pixel_shift_x, _) = pixel_shift;
        
        // Clear background
        if complete_redraw {
            c.set_source_rgb(0.0, 0.0, 0.0);
            Self::safe_cairo_paint(&c)?;
        } else if modules_only_redraw {
            // Only clear the modules area for modules-only redraw
            c.set_source_rgb(0.0, 0.0, 0.0);
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
        c.set_source_rgb(0.0, 0.0, 0.0);
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
            return Err(e);
        }
        
        // Skip media section if this is a modules-only redraw
        if !modules_only_redraw {
            let media_count = split.media.len();
            let media_result = Self::draw_media_section_static(
                &c, &mut split.media, &layout_info.button_widths, layout_info.media_width,
                media_count, left_edge + layout_info.modules_width + layout_info.gap,
                bot, top, BUTTON_RADIUS, height, config, complete_redraw, &mut modified_regions,
                session_state, &available_mpris_services
            );
            if let Err(e) = media_result {
                return Err(e);
            }
        }
        
        Ok(modified_regions)
    }

    /// Draws standard function layers (Fn keys, App Layer 2, App Layer 3) without split layout
    fn draw_standard_function_layer(&mut self, config: &Config, width: i32, height: i32, surface: &Surface, pixel_shift: (f64, f64), complete_redraw: bool, layer_index: Option<LayerKey>, app_layer3_slide_progress: f64) -> FunctionLayerResult<Vec<ClipRect>> {
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
        
        // Handle AppLayerKeys3 slide animation
        if let Some(LayerKey::Custom3) = layer_index {
            if app_layer3_slide_progress == 0.0 {
                return Ok(modified_regions);
            }
            
            let slide_offset = if app_layer3_slide_progress < 1.0 {
                if app_layer3_slide_progress > 0.0 {
                    (1.0 - app_layer3_slide_progress) * width as f64
                } else {
                    -app_layer3_slide_progress * width as f64
                }
            } else {
                0.0
            };
            c.translate(slide_offset, 0.0);
        }
        
        let gap = FunctionLayerKeys1LayoutInfo::get_gap_for_layer(layer_index);
        let count = self.buttons.len();
        let (_, _, button_widths) = self.get_flat_layout_info(count, width, gap);
        
        let (bot, top) = Self::calculate_button_dimensions(height);
        let (pixel_shift_x, pixel_shift_y) = pixel_shift;
        
        if complete_redraw {
            c.set_source_rgb(0.0, 0.0, 0.0);
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
                c.set_source_rgb(0.0, 0.0, 0.0);
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
                modified_regions.push(ClipRect::new(
                    height as u16 - top as u16 - BUTTON_RADIUS as u16,
                    left_edge as u16,
                    height as u16 - bot as u16 + BUTTON_RADIUS as u16,
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

    fn draw_modules_section_static(c: &Context, left_edge: f64, bot: f64, modules_width: f64, modules_height: f64, radius: f64, session_state: Option<&SessionState>, current_window_class: Option<&str>, mut app_ui_manager: Option<&mut AppUiManager>, media_player_drag_position: Option<f64>, modified_regions: &mut Vec<ClipRect>) -> FunctionLayerResult<()> {
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

    fn draw_media_section_static(c: &Context, media: &mut [Button], button_widths: &[f64], media_width: f64, media_count: usize, left_edge: f64, bot: f64, top: f64, radius: f64, height: i32, config: &Config, complete_redraw: bool, modified_regions: &mut Vec<ClipRect>, session_state: Option<&SessionState>, available_mpris_services: &[String]) -> FunctionLayerResult<()> {
        crate::view::media_screen::draw_media_section(
            c, media, button_widths, media_width, media_count,
            left_edge, bot, top, radius, height, config, complete_redraw,
            modified_regions, session_state, available_mpris_services
        );
        Ok(())
    }

    fn draw_rounded_button_static(c: &Context, left_edge: f64, bot: f64, top: f64, width: f64, radius: f64) -> FunctionLayerResult<()> {
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

    // Helper for modules hit test (FunctionLayerKeys1 split layout only)
    pub fn hit_test_modules(&self, x: f64, width: i32, layer_index: Option<LayerKey>) -> Option<usize> {
        // Only allow modules hit test for LayerKeys1 (Media layer) with split layout
        if let Some(LayerKey::Media) = layer_index {
            if let Some(split) = &self.split {
                let gap = FunctionLayerKeys1LayoutInfo::get_gap_for_layer(layer_index);
                let total_width = (width - gap as i32) as f64;
                let modules_width = (split.modules_width as f64 * total_width).round();
                if x >= 0.0 && x < modules_width {
                    return Some(0);
                }
            }
        }
        None
    }

    // Helper for media hit test (FunctionLayerKeys1 split layout only)
    pub fn hit_test_media(&self, x: f64, width: i32, layer_index: Option<LayerKey>, available_mpris_services: &[String]) -> Option<usize> {
        // Only allow media hit test for LayerKeys1 (Media layer) with split layout
        if let Some(LayerKey::Media) = layer_index {
            if let Some(split) = &self.split {
                let gap = FunctionLayerKeys1LayoutInfo::get_gap_for_layer(layer_index);
                let total_width = (width - gap as i32) as f64;
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
                
                // Create full width array with 0.0 for hidden buttons
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
                
                // SIMPLIFIED: media section starts after modules_width + gap
                let mut left_edge = modules_width + gap;
                for (i, button) in split.media.iter().enumerate() {
                    // Skip hit testing for hidden buttons (width is 0.0)
                    if media_button_widths[i] == 0.0 {
                        continue;
                    }
                    
                    let right_edge = left_edge + media_button_widths[i];
                    if x >= left_edge && x < right_edge {
                        return Some(i);
                    }
                    left_edge = right_edge;
                    if i != media_count - 1 {
                        left_edge += MEDIA_SPACING_PX;
                    }
                }
            }
        }
        None
    }

    // Helper for flat hit test (standard function layers)
    pub fn hit_test_flat(&self, x: f64, width: i32, layer_index: Option<LayerKey>) -> Option<usize> {
        // Only allow flat hit test for non-LayerKeys1 layers (Fn keys, App Layer 2, App Layer 3)
        if let Some(layer_key) = layer_index {
            if layer_key != LayerKey::Media {
                let count = self.buttons.len();
                let gap = FunctionLayerKeys1LayoutInfo::get_gap_for_layer(layer_index);
                let (_, _, button_widths) = self.get_flat_layout_info(count, width, gap);
                
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
        }
        None
    }

    /// Returns (group, index) where group is "modules" or "media" or "flat", and index is the button index in that group
    /// - "modules" and "media": FunctionLayerKeys1 (App Layer 1) with split layout
    /// - "flat": Standard function layers (Fn keys, App Layer 2, App Layer 3)
    pub fn hit_test(&self, x: f64, width: i32, layer_index: Option<LayerKey>, available_mpris_services: &[String]) -> Option<(&'static str, usize)> {
        if let Some(LayerKey::Media) = layer_index {
            // This is FunctionLayerKeys1 (App Layer 1) - check for split layout
            if let Some(_split) = &self.split {
                // Has split layout, check modules and media
                if let Some(idx) = self.hit_test_modules(x, width, layer_index) {
                    return Some(("modules", idx));
                }
                if let Some(idx) = self.hit_test_media(x, width, layer_index, available_mpris_services) {
                    return Some(("media", idx));
                }
            }
            // LayerKeys1 without split layout - no hit possible
            None
        } else {
            // This is a standard function layer (Fn keys, App Layer 2, App Layer 3)
            if let Some(idx) = self.hit_test_flat(x, width, layer_index) {
                return Some(("flat", idx));
            }
            None
        }
    }
} 