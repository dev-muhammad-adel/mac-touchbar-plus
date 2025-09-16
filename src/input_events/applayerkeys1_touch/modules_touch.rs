use input::{
    event::Event,
    event::touch::{TouchEventPosition, TouchEventSlot}
};
use input_linux::uinput::UInputHandle;
use std::os::unix::net::UnixStream;
use std::io::Write;
use crate::view::app_ui_manager::{AppUiManager, is_media_player_window_class, is_browser_window_class};
use crate::view::generic_background_screen::GenericBackgroundAction;
use std::collections::HashMap;
use crate::LayerKey;
use crate::layers::FunctionLayer;
use crate::input_events::applayerkeys1_touch::modules::{MediaPlayerTouchHandler, BrowserTouchHandler};

pub struct ModulesTouchHandler;

// Touch tracking for generic background
static mut GENERIC_BACKGROUND_TOUCHES: Option<HashMap<u32, (LayerKey, &'static str, usize)>> = None;

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
        background_service_helper_stream: &mut Option<UnixStream>,
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
                    current_window_class, app_ui_manager, background_service_helper_stream, needs_complete_redraw, cfg_enable_pixel_shift
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
        background_service_helper_stream: &mut Option<UnixStream>,
        needs_complete_redraw: &mut bool,
        cfg_enable_pixel_shift: bool,
    ) -> crate::MainResult<()> {
        let touches = Self::get_touches();
        
        if let Event::Touch(te) = event {
            match te {
                input::event::touch::TouchEvent::Down(dn) => {
                    let x = dn.x_transformed(width);
                    let y = dn.y_transformed(height);
                    println!("[modules_touch] Generic background touch down at ({}, {})", x, y);
                    
                    let available_mpris_services = &app_ui_manager.generic_background_screen.available_mpris_services;
                    if let Some((group, idx)) = layers.get_mut(active_layer).ok_or(crate::MainError::LayerNotFound(*active_layer))?.hit_test(x, width as i32, Some(active_layer.clone()), available_mpris_services) {
                        if group == "modules" {
                            // Store touch for motion tracking
                            touches.insert(dn.seat_slot(), (active_layer.clone(), group, idx));
                            
                            // Handle generic background touch down
                            Self::handle_generic_background_touch_down(
                                x, y, width, height, current_window_class, app_ui_manager, background_service_helper_stream, needs_complete_redraw, cfg_enable_pixel_shift
                            )?;
                        }
                    }
                },
                input::event::touch::TouchEvent::Motion(mtn) => {
                    if !touches.contains_key(&mtn.seat_slot()) {
                        return Ok(());
                    }
                    
                    let (_layer, group, _idx) = Self::get_touch_slot(touches, mtn.seat_slot())?;
                    if *group == "modules" {
                        let x = mtn.x_transformed(width);
                        let y = mtn.y_transformed(height);
                        
                        // Handle generic background touch motion
                        Self::handle_generic_background_touch_motion(
                            x, y, width, height, current_window_class, app_ui_manager, background_service_helper_stream, needs_complete_redraw, cfg_enable_pixel_shift
                        )?;
                    }
                },
                input::event::touch::TouchEvent::Up(up) => {
                    if !touches.contains_key(&up.seat_slot()) {
                        return Ok(());
                    }
                    
                    let (_layer, group, _idx) = Self::get_touch_slot(touches, up.seat_slot())?;
                    if *group == "modules" {
                        // Handle generic background touch up
                        Self::handle_generic_background_touch_up(
                            current_window_class, app_ui_manager, needs_complete_redraw
                        )?;
                        
                        // Remove touch slot
                        touches.remove(&up.seat_slot());
                        println!("[modules_touch] Generic background touch slot {} removed", up.seat_slot());
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
        background_service_helper_stream: &mut Option<UnixStream>,
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
            Self::handle_generic_background_action(app_action, app_ui_manager, background_service_helper_stream, needs_complete_redraw)?;
        }
        Ok(())
    }

    /// Handles generic background actions
    fn handle_generic_background_action(
        app_action: crate::view::app_ui_manager::AppAction,
        app_ui_manager: &mut AppUiManager,
        background_service_helper_stream: &mut Option<UnixStream>,
        needs_complete_redraw: &mut bool,
    ) -> crate::MainResult<()> {
        match app_action {
            crate::view::app_ui_manager::AppAction::GenericBackground(action) => {
                println!("[modules_touch] Handling generic background action: {:?}", action);
                // Handle actions directly like media player actions
                Self::handle_generic_background_action_direct(action, app_ui_manager, background_service_helper_stream, needs_complete_redraw)?;
            },
            _ => {
                println!("[modules_touch] Unhandled action: {:?}", app_action);
            }
        }
        Ok(())
    }

    /// Handles generic background actions directly (like media player actions)
    fn handle_generic_background_action_direct(
        action: crate::view::generic_background_screen::GenericBackgroundAction,
        app_ui_manager: &mut AppUiManager,
        background_service_helper_stream: &mut Option<UnixStream>,
        needs_complete_redraw: &mut bool,
    ) -> crate::MainResult<()> {
        match action {
            crate::view::generic_background_screen::GenericBackgroundAction::ToggleMprisItem(index) => {
                app_ui_manager.generic_background_screen.toggle_mpris_item(index);
                // Send selection command to background service helper using the correct format
                if let Some(service_name) = app_ui_manager.generic_background_screen.available_mpris_services.get(index) {
                    let command = format!("select_service:{}\n", service_name);
                    if let Some(stream) = background_service_helper_stream {
                        stream.write_all(command.as_bytes())?;
                        println!("[modules_touch] Sent service selection command: {}", command.trim());
                    }
                }
            }
            crate::view::generic_background_screen::GenericBackgroundAction::CloseGenericMedia => {
                app_ui_manager.close_generic_media();
            }
            crate::view::generic_background_screen::GenericBackgroundAction::BackgroundServicePlayerPlayPause => {
                Self::send_background_service_command(background_service_helper_stream, "play_pause")?;
            }
            crate::view::generic_background_screen::GenericBackgroundAction::BackgroundServicePlayerNext => {
                Self::send_background_service_command(background_service_helper_stream, "next")?;
            }
            crate::view::generic_background_screen::GenericBackgroundAction::BackgroundServicePlayerPrevious => {
                Self::send_background_service_command(background_service_helper_stream, "previous")?;
            }
            crate::view::generic_background_screen::GenericBackgroundAction::BackgroundServicePlayerSeek(ratio) => {
                Self::send_background_service_command(background_service_helper_stream, &format!("seek:{}", ratio))?;
            }
            crate::view::generic_background_screen::GenericBackgroundAction::BackgroundServicePlayerDragHead(ratio) => {
                Self::send_background_service_command(background_service_helper_stream, &format!("seek:{}", ratio))?;
            }
            _ => {
                println!("[modules_touch] Unhandled generic background action: {:?}", action);
            }
        }
        *needs_complete_redraw = true;
        Ok(())
    }

    /// Sends background service commands via the helper stream
    fn send_background_service_command(stream: &mut Option<UnixStream>, command: &str) -> Result<(), std::io::Error> {
        if let Some(stream) = stream {
            let command_with_newline = format!("media_action:{}\n", command);
            stream.write_all(command_with_newline.as_bytes())?;
            println!("[modules_touch] Sent background service command: {}", command);
            Ok(())
        } else {
            println!("[modules_touch] No background service helper stream available for command: {}", command);
            Err(std::io::Error::new(std::io::ErrorKind::NotConnected, "No background service helper stream available"))
        }
    }

    /// Gets the touch tracking HashMap
    fn get_touches() -> &'static mut HashMap<u32, (LayerKey, &'static str, usize)> {
        unsafe {
            if GENERIC_BACKGROUND_TOUCHES.is_none() {
                GENERIC_BACKGROUND_TOUCHES = Some(HashMap::new());
            }
            GENERIC_BACKGROUND_TOUCHES.as_mut().unwrap()
        }
    }

    /// Gets touch slot information
    fn get_touch_slot<'a>(
        touches: &'a mut HashMap<u32, (LayerKey, &'static str, usize)>,
        seat_slot: u32,
    ) -> Result<&'a (LayerKey, &'static str, usize), crate::MainError> {
        touches.get(&seat_slot)
            .ok_or(crate::MainError::TouchSlotNotFound(seat_slot))
    }

    /// Handles generic background touch motion events
    fn handle_generic_background_touch_motion(
        x: f64,
        y: f64,
        width: u32,
        height: u32,
        current_window_class: &Option<String>,
        app_ui_manager: &mut AppUiManager,
        background_service_helper_stream: &mut Option<UnixStream>,
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
            println!("[modules_touch] Generic background motion action detected: {:?}", app_action);
            Self::handle_generic_background_action_motion(
                app_action, app_ui_manager, background_service_helper_stream, needs_complete_redraw
            )?;
        }
        Ok(())
    }

    /// Handles generic background touch up events
    fn handle_generic_background_touch_up(
        current_window_class: &Option<String>,
        app_ui_manager: &mut AppUiManager,
        needs_complete_redraw: &mut bool,
    ) -> crate::MainResult<()> {
        // Reset any dragging state in the background service player
        app_ui_manager.generic_background_screen.background_service_player.stop_dragging();
        *needs_complete_redraw = true;
        println!("[modules_touch] Generic background touch up - dragging stopped");
        Ok(())
    }

    /// Handles generic background action motion events
    fn handle_generic_background_action_motion(
        app_action: crate::view::app_ui_manager::AppAction,
        app_ui_manager: &mut AppUiManager,
        background_service_helper_stream: &mut Option<UnixStream>,
        needs_complete_redraw: &mut bool,
    ) -> crate::MainResult<()> {
        match app_action {
            crate::view::app_ui_manager::AppAction::GenericBackground(action) => {
                println!("[modules_touch] Handling generic background motion action: {:?}", action);
                // Handle actions directly like media player actions
                Self::handle_generic_background_action_direct(action, app_ui_manager, background_service_helper_stream, needs_complete_redraw)?;
            },
            _ => {
                println!("[modules_touch] Unhandled motion action: {:?}", app_action);
            }
        }
        Ok(())
    }
} 