use input::{
    event::{
        Event, device::DeviceEvent, EventTrait,
        keyboard::{KeyboardEvent, KeyboardEventTrait, KeyState}
    }
};
use input_linux::Key;
use crate::LayerKey;

pub struct KeyboardEventHandler;

impl KeyboardEventHandler {
    pub fn handle_device_event(
        event: &Event,
        active_layer: &mut LayerKey,
        current_session: &Option<crate::services::sessionmanager::SessionState>,
        needs_complete_redraw: &mut bool,
    ) -> bool {
        match event {
            Event::Device(DeviceEvent::Added(evt)) => {
                let dev = evt.device();
                if dev.name().contains(" Touch Bar") {
                    // Return true to indicate we found the digitizer
                    return true;
                }
            },
            Event::Keyboard(KeyboardEvent::Key(key)) => {
                if key.key() == Key::Fn as u32 {
                    let new_layer = match key.key_state() {
                        KeyState::Pressed => LayerKey::Fn,
                        KeyState::Released => {
                            // Return to appropriate layer based on session state
                            if current_session.as_ref().map(|s| s.is_logged_in).unwrap_or(false) {
                                LayerKey::Media
                            } else {
                                LayerKey::Custom2
                            }
                        },
                    };
                    if *active_layer != new_layer {
                        *active_layer = new_layer;
                        *needs_complete_redraw = true;
                    }
                } else if key.key() == Key::Macro1 as u32 && key.key_state() == KeyState::Pressed {
                    // Switch to appropriate layer based on session state
                    *active_layer = if current_session.as_ref().map(|s| s.is_logged_in).unwrap_or(false) {
                        LayerKey::Media
                    } else {
                        LayerKey::Custom2
                    };
                    *needs_complete_redraw = true;
                } else if key.key() == Key::Macro2 as u32 && key.key_state() == KeyState::Pressed {
                    *active_layer = LayerKey::Custom2;
                    *needs_complete_redraw = true;
                } else if key.key() == Key::Macro3 as u32 && key.key_state() == KeyState::Pressed {
                    *active_layer = LayerKey::Custom3;
                    *needs_complete_redraw = true;
                }
            },
            _ => {}
        }
        false
    }
} 