
use cairo::{Context, Surface, Rectangle};
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
}

type FunctionLayerResult<T> = Result<T, FunctionLayerError>;

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

fn safe_cairo_render_document(renderer: &rsvg::CairoRenderer, c: &Context, rect: &Rectangle) -> FunctionLayerResult<()> {
    renderer.render_document(c, rect)
        .map_err(|e| FunctionLayerError::Cairo(format!("Failed to render document: {}", e)))
}

// Constants
pub const BUTTON_SPACING_PX: i32 = 16;
pub const APP_LAYER_KEYS2_GAP_PX: f64 = 4.0; // Custom gap for AppLayerKeys2 (Custom2)
pub const APP_LAYER_KEYS3_GAP_PX: f64 = 4.0; // Custom gap for AppLayerKeys3
pub const BUTTON_COLOR_INACTIVE: f64 = 0.172;
pub const BUTTON_COLOR_ACTIVE: f64 = 0.350;

#[derive(Default)]
pub struct FunctionLayer {
    pub buttons: Vec<Button>,
    pub split: Option<SplitLayout>,
}

pub struct SplitLayout {
    pub modules_width: f32,
    pub media: Vec<Button>,
    pub media_width: f32,
}

impl FunctionLayer {
    pub fn with_config(cfg: Vec<ButtonConfig>) -> FunctionLayer {
        if cfg.is_empty() {
            panic!("Invalid configuration, layer has 0 buttons");
        }
        FunctionLayer {
            buttons: cfg.into_iter().map(Button::with_config).collect(),
            split: None,
        }
    }

    pub fn with_split(modules_width: f32, media: Vec<ButtonConfig>, media_width: f32) -> FunctionLayer {
        FunctionLayer {
            buttons: vec![],
            split: Some(SplitLayout {
                modules_width,
                media: media.into_iter().map(Button::with_config).collect(),
                media_width,
            }),
        }
    }

    pub fn draw(&mut self, config: &Config, width: i32, height: i32, surface: &Surface, pixel_shift: (f64, f64), complete_redraw: bool, modules_only_redraw: bool, session_state: Option<&SessionState>, layer_index: Option<LayerKey>, app_layer3_slide_progress: f64, current_window_class: Option<&str>, mut app_ui_manager: Option<&mut AppUiManager>, vlc_drag_position: Option<f64>) -> FunctionLayerResult<Vec<ClipRect>> {
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
                let pixel_shift_width = if config.enable_pixel_shift { crate::display::pixel_shift::PIXEL_SHIFT_WIDTH_PX } else { 0 };
                let total_width = (width - pixel_shift_width as i32) as f64;
                let group_spacing = BUTTON_SPACING_PX as f64; // space between groups
                let modules_width = (split.modules_width as f64 * total_width).round();
                let media_width = total_width - modules_width - group_spacing;
                let media_count = split.media.len();
                let _media_spacing = if media_count > 1 { BUTTON_SPACING_PX as f64 * (media_count as f64 - 1.0) } else { 0.0 };
         
                // --- MEDIA BUTTON WIDTHS WITH FRACTION ---
                let media_spacing_px = 2.0f64; // 2px spacing for AppLayerKeys1Media
                let visible_media_count = split.media.iter().filter(|b| b.visible).count();
                let total_spacing = if visible_media_count > 1 { media_spacing_px * (visible_media_count as f64 - 1.0) } else { 0.0 };
                let button_area = media_width - total_spacing;
                let weights: Vec<f32> = split.media.iter().filter(|b| b.visible).map(|b| b.fraction.unwrap_or(1.0)).collect();
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
                    c.select_font_face(&config.font_style_cairo, if config.italic_cairo {cairo::FontSlant::Italic} else {cairo::FontSlant::Normal}, if config.bold_cairo {cairo::FontWeight::Bold} else {cairo::FontWeight::Normal});
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
                    let visible_media_count = split.media.iter().filter(|b| b.visible).count();
                    let total_spacing = if visible_media_count > 1 { media_spacing_px * (visible_media_count as f64 - 1.0) } else { 0.0 };
                    let button_area = media_width - total_spacing;
                    let weights: Vec<f32> = split.media.iter().filter(|b| b.visible).map(|b| b.fraction.unwrap_or(1.0)).collect();
                    let total_weight: f32 = weights.iter().sum();
                    let mut media_button_widths: Vec<f64> = weights.iter().map(|w| button_area * (*w as f64 / total_weight as f64)).collect();
                    let sum_widths: f64 = media_button_widths.iter().sum();
                    if let Some(last) = media_button_widths.last_mut() {
                        *last += button_area - sum_widths;
                    }
                    crate::view::media_screen::draw_media_section(
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
                let pixel_shift_width = if config.enable_pixel_shift { crate::display::pixel_shift::PIXEL_SHIFT_WIDTH_PX } else { 0 };
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
                    c.select_font_face(&config.font_style_cairo, if config.italic_cairo {cairo::FontSlant::Italic} else {cairo::FontSlant::Normal}, if config.bold_cairo {cairo::FontWeight::Bold} else {cairo::FontWeight::Normal});
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
                    if (button.action != input_linux::Key::Unknown &&
                       button.action != input_linux::Key::Macro1 &&
                       button.action != input_linux::Key::Macro2 &&
                       button.action != input_linux::Key::Macro3 &&
                       button.action != input_linux::Key::Macro4) &&
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
    pub fn hit_test_modules(&self, x: f64, width: i32, layer_index: Option<LayerKey>) -> Option<usize> {
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
    pub fn hit_test_media(&self, x: f64, width: i32, layer_index: Option<LayerKey>) -> Option<usize> {
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
            let visible_media_count = split.media.iter().filter(|b| b.visible).count();
            let media_spacing_px = 2.0f64;
            let total_spacing = if visible_media_count > 1 { media_spacing_px * (visible_media_count as f64 - 1.0) } else { 0.0 };
            let button_area = media_width - total_spacing;
            let weights: Vec<f32> = split.media.iter().filter(|b| b.visible).map(|b| b.fraction.unwrap_or(1.0)).collect();
            let total_weight: f32 = weights.iter().sum();
            let mut media_button_widths: Vec<f64> = weights.iter().map(|w| button_area * (*w as f64 / total_weight as f64)).collect();
            let sum_widths: f64 = media_button_widths.iter().sum();
            if let Some(last) = media_button_widths.last_mut() {
                *last += button_area - sum_widths;
            }
            // SIMPLIFIED: media section starts after modules_width + group_spacing
            let mut left_edge = modules_width + group_spacing;
            let mut visible_index = 0;
            for (i, button) in split.media.iter().enumerate() {
                if !button.visible {
                    continue;
                }
                let right_edge = left_edge + media_button_widths[visible_index];
                if x >= left_edge && x < right_edge {
                    return Some(i);
                }
                left_edge = right_edge;
                if visible_index < visible_media_count - 1 {
                    left_edge += media_spacing_px;
                }
                visible_index += 1;
            }
        }
        None
    }

    // Helper for flat hit test
    pub fn hit_test_flat(&self, x: f64, width: i32, layer_index: Option<LayerKey>) -> Option<usize> {
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