use input::{
    event::Event
};
use input_linux::uinput::UInputHandle;
use std::os::unix::net::UnixStream;
use crate::view::app_ui_manager::{AppUiManager, is_media_player_window_class, is_browser_window_class};
use std::collections::HashMap;
use crate::LayerKey;
use crate::layers::FunctionLayer;
use crate::input_events::applayerkeys1_touch::modules::{MediaPlayerTouchHandler, BrowserTouchHandler};

pub struct ModulesTouchHandler;

impl ModulesTouchHandler {
    /// Handles all touch events for modules section in LayerKeys1 (App Layer 1)
    pub fn handle_touch_event(
        event: &Event,
        width: u32,
        height: u32,
        active_layer: &LayerKey,
        layers: &mut HashMap<LayerKey, FunctionLayer>,
        current_window_class: &Option<String>,
        app_ui_manager: &mut AppUiManager,
        media_player_touch_active: &mut bool,
        media_player_drag_position: &mut Option<f64>,
        media_player_helper_stream: &mut Option<UnixStream>,
        browser_helper_stream: &mut Option<UnixStream>,
        needs_complete_redraw: &mut bool,
        cfg_enable_pixel_shift: bool,
        uinput: &mut UInputHandle<std::fs::File>,
    ) -> crate::MainResult<()> {
        if let Event::Touch(_te) = event {
            // Match on current_window_class to delegate to appropriate handler
        if let Some(window_class) = current_window_class {
            let window_class_lc = window_class.to_lowercase();
                
                match window_class_lc.as_str() {
                    class if is_media_player_window_class(class) => {
                        // Delegate to Media Player touch handler
                        MediaPlayerTouchHandler::handle_touch_event(
                            event, width, height, active_layer, layers,
                            current_window_class, app_ui_manager, media_player_touch_active,
                            media_player_drag_position, media_player_helper_stream, browser_helper_stream,
                            needs_complete_redraw, cfg_enable_pixel_shift, uinput
                        )?;
                    },
                    class if is_browser_window_class(class) => {
                        // Delegate to browser touch handler
                        BrowserTouchHandler::handle_touch_event(
                            event, width, height, active_layer, layers,
                            current_window_class, app_ui_manager, media_player_touch_active,
                            media_player_drag_position, media_player_helper_stream, browser_helper_stream,
                            needs_complete_redraw, cfg_enable_pixel_shift, uinput
                        )?;
                    },
                    _ => {
                        // Unknown window class, skip processing
                        println!("[modules_touch] Unknown window class: {}, skipping touch event", window_class);
                    }
                }
            } else {
                // No current window class, skip processing
                println!("[modules_touch] No current window class, skipping touch event");
            }
        }
        Ok(())
    }
} 