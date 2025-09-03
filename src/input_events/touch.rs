use input::{
    event::{
        Event, EventTrait
    }
};
use input::Device as InputDevice;
use input_linux::uinput::UInputHandle;
use std::collections::HashMap;
use std::os::unix::net::UnixStream;
use crate::LayerKey;
use crate::layers::FunctionLayer;
use crate::view::app_ui_manager::AppUiManager;
use crate::input_events::applayerkeys1_touch::{MediaTouchHandler, ModulesTouchHandler};
use crate::input_events::FlatTouchHandler;

pub struct TouchEventHandler;

impl TouchEventHandler {
    pub fn handle_touch_event(
        event: &Event,
        digitizer: &Option<InputDevice>,
        backlight_current: u32,
        width: u32,
        height: u32,
        active_layer: &LayerKey,
        layers: &mut HashMap<LayerKey, FunctionLayer>,
        uinput: &mut UInputHandle<std::fs::File>,
        current_window_class: &Option<String>,
        app_ui_manager: &mut AppUiManager,
            media_player_touch_active: &mut bool,
    media_player_drag_position: &mut Option<f64>,
    media_player_helper_stream: &mut Option<UnixStream>,
        browser_helper_stream: &mut Option<UnixStream>,
        needs_complete_redraw: &mut bool,
        cfg_enable_pixel_shift: bool,
    ) -> crate::MainResult<()> {
        if let Event::Touch(te) = event {
            if Some(te.device()) != *digitizer || backlight_current == 0 {
                return Ok(());
            }
            
            // Check if we're in Media layer (App Layer 1)
            if *active_layer == LayerKey::Media {
                // For Media layer, delegate to all handlers and let them filter internally
                ModulesTouchHandler::handle_touch_event(
                    event, width, height, active_layer, layers,
                            current_window_class, app_ui_manager, media_player_touch_active,
        media_player_drag_position, media_player_helper_stream, browser_helper_stream,
                    needs_complete_redraw, cfg_enable_pixel_shift, uinput
                )?;
                
                MediaTouchHandler::handle_touch_event(
                    event, width, height, active_layer, layers, uinput
                )?;
            } else {
                // For other layers, use flat touch handler
                FlatTouchHandler::handle_touch_event(
                    event, width, height, active_layer, layers, uinput
                )?;
                
            }
        }
        Ok(())
    }
} 