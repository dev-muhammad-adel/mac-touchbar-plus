use cairo::Surface;
use drm::control::ClipRect;
use anyhow::Result;

use crate::config::Config;
use crate::services::sessionmanager::SessionState;
use crate::view::app_ui_manager::AppUiManager;
use crate::LayerKey;

use crate::layers::Button;

/// Common trait for all layer types
pub trait Layer {
    /// Draw the layer to the surface
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
        layer_index: Option<LayerKey>,
        app_layer3_slide_progress: f64,
        current_window_class: Option<&str>,
        app_ui_manager: Option<&mut AppUiManager>,
        media_player_drag_position: Option<f64>,
    ) -> Result<Vec<ClipRect>>;

    /// Handle hit testing for touch events
    fn hit_test(
        &self,
        x: f64,
        width: i32,
        layer_index: Option<LayerKey>,
        available_mpris_services: &[String],
    ) -> Option<(&'static str, usize)>;

    /// Get the layer key for this layer type
    fn layer_key(&self) -> LayerKey;

    /// Add ESC button to the layer (for wide displays)
    fn add_esc_button(&mut self);

    /// Check if any buttons have changed (for standard layers)
    fn any_buttons_changed(&self) -> bool;

    /// Get buttons for time-based redraws (for standard layers)
    fn get_buttons_for_time_check(&mut self) -> Option<&mut Vec<Button>>;

    /// Check if layer has split layout (for media layer)
    fn has_split_layout(&self) -> bool;

    /// Get all buttons for uinput registration
    fn get_all_buttons(&self) -> Vec<&Button>;

    /// Get mutable reference to buttons for touch handling (for standard layers)
    fn get_buttons_mut(&mut self) -> Option<&mut Vec<Button>>;

    /// Get mutable reference to media buttons for touch handling (for media layer)
    fn get_media_buttons_mut(&mut self) -> Option<&mut Vec<Button>>;
}
