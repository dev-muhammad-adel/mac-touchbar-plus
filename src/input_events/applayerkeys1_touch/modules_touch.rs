use input::{
    event::Event,
    event::touch::TouchEventPosition
};
use input_linux::uinput::UInputHandle;
use std::os::unix::net::UnixStream;
use crate::view::app_ui_manager::{AppUiManager, is_media_player_window_class, is_browser_window_class};
use crate::view::generic_background_screen::GenericBackgroundAction;
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
            // Check if generic media is enabled first
            if app_ui_manager.generic_media_enabled {
                // Handle generic background actions
                Self::handle_generic_background_touch_event(
                    event, width, height, active_layer, layers,
                    current_window_class, app_ui_manager, needs_complete_redraw, cfg_enable_pixel_shift
                )?;
            } else {
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
        }
        Ok(())
    }

    /// Handles generic background touch events
    fn handle_generic_background_touch_event(
        event: &Event,
        width: u32,
        height: u32,
        active_layer: &LayerKey,
        layers: &mut HashMap<LayerKey, FunctionLayer>,
        current_window_class: &Option<String>,
        app_ui_manager: &mut AppUiManager,
        needs_complete_redraw: &mut bool,
        cfg_enable_pixel_shift: bool,
    ) -> crate::MainResult<()> {
        if let Event::Touch(te) = event {
            match te {
                input::event::touch::TouchEvent::Down(dn) => {
                    let x = dn.x_transformed(width);
                    let y = dn.y_transformed(height);
                    println!("[modules_touch] Generic background touch down at ({}, {})", x, y);
                    
                    if let Some((group, idx)) = layers.get_mut(active_layer).ok_or(crate::MainError::LayerNotFound(*active_layer))?.hit_test(x, width as i32, Some(active_layer.clone())) {
                        if group == "modules" {
                            // Handle generic background touch down
                            Self::handle_generic_background_touch_down(
                                x, y, width, height, current_window_class, app_ui_manager, needs_complete_redraw, cfg_enable_pixel_shift
                            )?;
                        }
                    }
                },
                _ => {}
            }
        }
        Ok(())
    }

    /// Handles generic background touch down events
    fn handle_generic_background_touch_down(
        x: f64,
        y: f64,
        width: u32,
        height: u32,
        current_window_class: &Option<String>,
        app_ui_manager: &mut AppUiManager,
        needs_complete_redraw: &mut bool,
        cfg_enable_pixel_shift: bool,
    ) -> crate::MainResult<()> {
        let pixel_shift_width = if cfg_enable_pixel_shift { crate::display::pixel_shift::PIXEL_SHIFT_WIDTH_PX } else { 0 };
        let total_width = (width as i32 - pixel_shift_width as i32) as f64;
        let modules_width = (0.7 * total_width).round();
        let modules_x = (pixel_shift_width / 2) as f64;
        let modules_y = (height as f64) * 0.15;
        let modules_height = (height as f64) * 0.7;
        
        let adjusted_x = x - modules_x;
        let adjusted_y = y - modules_y;
        
        if let Some(app_action) = app_ui_manager.hit_test_app_ui(adjusted_x, adjusted_y, modules_x, modules_y, modules_width, modules_height, 8.0, current_window_class.as_ref().unwrap_or(&"".to_string())) {
            println!("[modules_touch] Generic background action detected: {:?}", app_action);
            Self::handle_generic_background_action(app_action, app_ui_manager, needs_complete_redraw)?;
        }
        Ok(())
    }

    /// Handles generic background actions
    fn handle_generic_background_action(
        app_action: crate::view::app_ui_manager::AppAction,
        app_ui_manager: &mut AppUiManager,
        needs_complete_redraw: &mut bool,
    ) -> crate::MainResult<()> {
        match app_action {
            crate::view::app_ui_manager::AppAction::GenericBackground(GenericBackgroundAction::ToggleMprisItem(index)) => {
                println!("[modules_touch] Toggling MPRIS item {}", index);
                app_ui_manager.generic_background_screen.toggle_mpris_item(index);
                *needs_complete_redraw = true;
            },
            crate::view::app_ui_manager::AppAction::GenericBackground(GenericBackgroundAction::CloseGenericMedia) => {
                println!("[modules_touch] Closing generic media - setting generic_media_enabled to false");
                app_ui_manager.close_generic_media();
                *needs_complete_redraw = true;
            },
            _ => {
                println!("[modules_touch] Unhandled generic background action: {:?}", app_action);
            }
        }
        Ok(())
    }
} 