use input::{
    event::{
        Event, device::DeviceEvent, EventTrait,
        keyboard::{KeyboardEvent, KeyboardEventTrait, KeyState}
    }
};
use input_linux::Key;
use cairo::ImageSurface;
use crate::LayerKey;
use crate::utils::screenshot::ScreenshotManager;

pub struct KeyboardEventHandler {
    ctrl_pressed: bool,
    shift_pressed: bool,
    super_pressed: bool,
}

impl KeyboardEventHandler {
    pub fn new() -> Self {
        Self {
            ctrl_pressed: false,
            shift_pressed: false,
            super_pressed: false,
        }
    }

    pub fn handle_device_event(
        &mut self,
        event: &Event,
        active_layer: &mut LayerKey,
        current_session: &Option<crate::services::sessionmanager::SessionState>,
        needs_complete_redraw: &mut bool,
        surface: &mut ImageSurface,
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
                // Track modifier keys
                match key.key() {
                    k if k == Key::LeftCtrl as u32 || k == Key::RightCtrl as u32 => {
                        self.ctrl_pressed = key.key_state() == KeyState::Pressed;
                    },
                    k if k == Key::LeftShift as u32 || k == Key::RightShift as u32 => {
                        self.shift_pressed = key.key_state() == KeyState::Pressed;
                    },
                    k if k == Key::LeftMeta as u32 || k == Key::RightMeta as u32 => {
                        self.super_pressed = key.key_state() == KeyState::Pressed;
                    },
                    k if k == Key::Num6 as u32 && key.key_state() == KeyState::Pressed => {
                        // Check for Ctrl+Shift+6 combination
                        if self.ctrl_pressed && self.shift_pressed {
                            println!("[keyboard] Screenshot shortcut detected: Ctrl+Shift+6");
                            // Capture screenshot directly
                            let filename = ScreenshotManager::generate_screenshot_filename(current_session);
                            if let Err(e) = ScreenshotManager::capture_touchbar_screenshot(surface, &filename, current_session) {
                                eprintln!("[keyboard] Failed to capture screenshot: {}", e);
                            }
                            return true;
                        }
                    },
                    _ => {}
                }
                
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