use input::{
    event::{
        Event,
        touch::{TouchEvent, TouchEventPosition, TouchEventSlot}
    }
};
use input_linux::uinput::UInputHandle;
use std::collections::HashMap;
use crate::LayerKey;
use crate::layers::FunctionLayer;
use input_linux::Key;
use crate::view::app_ui_manager::AppUiManager;

// Static HashMap for media touch slots
static mut MEDIA_TOUCHES: Option<HashMap<u32, (LayerKey, &'static str, usize)>> = None;

pub struct MediaTouchHandler;

impl MediaTouchHandler {
    fn get_touches() -> &'static mut HashMap<u32, (LayerKey, &'static str, usize)> {
        unsafe {
            if MEDIA_TOUCHES.is_none() {
                MEDIA_TOUCHES = Some(HashMap::new());
            }
            MEDIA_TOUCHES.as_mut().unwrap()
        }
    }

    /// Handles all touch events for media buttons in LayerKeys1 (App Layer 1)
    pub fn handle_touch_event(
        event: &Event,
        width: u32,
        height: u32,
        active_layer: &LayerKey,
        layers: &mut HashMap<LayerKey, FunctionLayer>,
        uinput: &mut UInputHandle<std::fs::File>,
        app_ui_manager: &mut AppUiManager,
        needs_complete_redraw: &mut bool,
    ) -> crate::MainResult<()> {
        let touches = Self::get_touches();
        
        if let Event::Touch(te) = event {
            match te {
                TouchEvent::Down(dn) => {
                    let x = dn.x_transformed(width);
                    let y = dn.y_transformed(height);
                    
                    let available_mpris_services = &app_ui_manager.generic_background_screen.available_mpris_services;
                    if let Some((group, idx)) = layers.get_mut(active_layer).ok_or(crate::MainError::LayerNotFound(*active_layer))?.hit_test(x, width as i32, Some(active_layer.clone()), available_mpris_services) {
                        if group == "media" {
                            Self::handle_touch_down(idx, active_layer, layers, touches, dn.seat_slot(), uinput, app_ui_manager, needs_complete_redraw)?;
                        }
                    }
                },
                TouchEvent::Motion(mtn) => {
                    if !touches.contains_key(&mtn.seat_slot()) {
                        return Ok(());
                    }
                    
                    let (layer, group, idx) = Self::get_touch_slot(touches, mtn.seat_slot())?;
                    if *group == "media" {
                        Self::handle_touch_motion(*idx, layer, layers, uinput)?;
                    }
                },
                TouchEvent::Up(up) => {
                    if !touches.contains_key(&up.seat_slot()) {
                        return Ok(());
                    }
                    
                    let (layer, group, idx) = Self::get_touch_slot(touches, up.seat_slot())?;
                    if *group == "media" {
                        Self::handle_touch_up(*idx, layer, layers, uinput, app_ui_manager)?;
                        touches.remove(&up.seat_slot());
                    } else {
                    }
                },
                _ => {}
            }
        }
        Ok(())
    }

    fn get_touch_slot<'a>(
        touches: &'a HashMap<u32, (LayerKey, &'static str, usize)>,
        slot: u32
    ) -> crate::MainResult<&'a (LayerKey, &'static str, usize)> {
        touches.get(&slot)
            .ok_or_else(|| crate::MainError::TouchSlotNotFound(slot))
    }

    /// Handles touch down events for media buttons in LayerKeys1 (App Layer 1)
    pub fn handle_touch_down(
        idx: usize,
        active_layer: &LayerKey,
        layers: &mut HashMap<LayerKey, FunctionLayer>,
        touches: &mut HashMap<u32, (LayerKey, &'static str, usize)>,
        seat_slot: u32,
        uinput: &mut UInputHandle<std::fs::File>,
        app_ui_manager: &mut AppUiManager,
        needs_complete_redraw: &mut bool,
    ) -> crate::MainResult<()> {
        // Ensure this is only called for LayerKeys1 (Media layer)
        if *active_layer != LayerKey::Media {
            return Ok(());
        }

        if let Some(split) = &mut layers.get_mut(active_layer).ok_or(crate::MainError::LayerNotFound(*active_layer))?.split {
            let button = &mut split.media[idx];
            
            // Check if this is the generic media toggle button (special_type = "toggle")
            if button.special_type.as_ref().map_or(false, |t| t == "toggle") {
                println!("[media_touch] Generic media toggle button {} detected", idx);
                // Toggle the generic media state
                app_ui_manager.generic_media_enabled = !app_ui_manager.generic_media_enabled;
                println!("[media_touch] Generic media enabled: {}", app_ui_manager.generic_media_enabled);
                // Set the button active state to match the toggle state
                button.set_active(uinput, true);
                // Store touch slot for proper touch up handling
                touches.insert(seat_slot, (active_layer.clone(), "media", idx));
                // Trigger a redraw since the UI state changed
                *needs_complete_redraw = true;
                return Ok(());
            }
            
            // Check if this is a macro button - if so, trigger the action but don't activate visually
            if matches!(button.action, Key::Macro1 | Key::Macro2 | Key::Macro3) {
                println!("[media_touch] Macro button {} detected, triggering action without visual activation", idx);
                // Trigger the macro action but don't store touch slot or activate visually
                button.trigger_action(uinput);
                return Ok(());
            }
            
            touches.insert(seat_slot, (active_layer.clone(), "media", idx));
            button.set_active(uinput, true);
        }
        
        Ok(())
    }

    /// Handles touch motion events for media buttons in LayerKeys1 (App Layer 1)
    pub fn handle_touch_motion(
        idx: usize,
        layer: &LayerKey,
        layers: &mut HashMap<LayerKey, FunctionLayer>,
        uinput: &mut UInputHandle<std::fs::File>,
    ) -> crate::MainResult<()> {
        // Ensure this is only called for LayerKeys1 (Media layer)
        if *layer != LayerKey::Media {
            return Ok(());
        }

        if let Some(split) = &mut layers.get_mut(layer).ok_or(crate::MainError::LayerNotFound(*layer))?.split {
            let button = &mut split.media[idx];
            if button.special_type.as_ref().map_or(false, |t| t == "toggle") {
                return Ok(());
            }
            
            // For macro buttons, we don't need motion events since they trigger on touch down
            if matches!(button.action, Key::Macro1 | Key::Macro2 | Key::Macro3) {
                println!("[media_touch] Macro button {} detected in motion, skipping", idx);
                return Ok(());
            }
            
            button.set_active(uinput, true);
        }
        
        Ok(())
    }

    /// Handles touch up events for media buttons in LayerKeys1 (App Layer 1)
    pub fn handle_touch_up(
        idx: usize,
        layer: &LayerKey,
        layers: &mut HashMap<LayerKey, FunctionLayer>,
        uinput: &mut UInputHandle<std::fs::File>,
        app_ui_manager: &AppUiManager,
    ) -> crate::MainResult<()> {
        println!("[media_touch] handle_touch_up called for idx: {}, layer: {:?}", idx, layer);
        
        // Ensure this is only called for LayerKeys1 (Media layer)
        if *layer != LayerKey::Media {
            println!("[media_touch] Layer is not Media, skipping");
            return Ok(());
        }

        if let Some(split) = &mut layers.get_mut(layer).ok_or(crate::MainError::LayerNotFound(*layer))?.split {
            let button = &mut split.media[idx];
            if button.special_type.as_ref().map_or(false, |t| t == "toggle") {
                println!("[media_touch] Button is generic media toggle, maintaining toggle state");
                // For toggle buttons, maintain the current toggle state
                button.set_active(uinput, false);
                return Ok(());
            }
            
            // For macro buttons, we don't need touch up events since they trigger on touch down
            if matches!(button.action, Key::Macro1 | Key::Macro2 | Key::Macro3) {
                println!("[media_touch] Macro button {} detected in touch up, skipping", idx);
                return Ok(());
            }
            
            button.set_active(uinput, false);
        } else {
        }
        
        Ok(())
    }
} 