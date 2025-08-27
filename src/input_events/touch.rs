use input::{
    event::{
        Event, EventTrait,
        touch::{TouchEvent, TouchEventPosition, TouchEventSlot}
    }
};
use input::Device as InputDevice;
use input_linux::{uinput::UInputHandle, Key};
use std::collections::HashMap;
use std::io::Write;
use std::os::unix::net::UnixStream;
use crate::LayerKey;
use crate::layers::FunctionLayer;
use crate::view::app_ui_manager::{AppUiManager, AppAction};
use crate::view::vlc_screen::VlcAction;
use crate::view::browser_screen::BrowserAction;
use crate::display::pixel_shift::PIXEL_SHIFT_WIDTH_PX;
use crate::layers::function_layer::BUTTON_SPACING_PX;

pub struct TouchEventHandler;

impl TouchEventHandler {
    pub fn handle_touch_event(
        event: &Event,
        digitizer: &Option<InputDevice>,
        backlight_current: u32,
        width: u32,
        height: u32,
        touches: &mut HashMap<u32, (LayerKey, &'static str, usize)>,
        active_layer: &LayerKey,
        layers: &mut HashMap<LayerKey, FunctionLayer>,
        uinput: &mut UInputHandle<std::fs::File>,
        current_window_class: &Option<String>,
        app_ui_manager: &mut AppUiManager,
        vlc_touch_active: &mut bool,
        vlc_drag_position: &mut Option<f64>,
        vlc_helper_stream: &mut Option<UnixStream>,
        browser_helper_stream: &mut Option<UnixStream>,
        needs_complete_redraw: &mut bool,
        cfg_enable_pixel_shift: bool,
    ) -> crate::MainResult<()> {
        if let Event::Touch(te) = event {
            if Some(te.device()) != *digitizer || backlight_current == 0 {
                return Ok(());
            }
            
            match te {
                TouchEvent::Down(dn) => {
                    Self::handle_touch_down(
                        dn, width, height, touches, active_layer, layers, uinput,
                        current_window_class, app_ui_manager, vlc_touch_active,
                        vlc_drag_position, vlc_helper_stream, browser_helper_stream,
                        needs_complete_redraw, cfg_enable_pixel_shift
                    )?;
                },
                TouchEvent::Motion(mtn) => {
                    Self::handle_touch_motion(
                        mtn, width, height, touches, active_layer, layers, uinput,
                        current_window_class, app_ui_manager, vlc_touch_active,
                        vlc_drag_position, vlc_helper_stream, browser_helper_stream,
                        needs_complete_redraw, cfg_enable_pixel_shift
                    )?;
                },
                TouchEvent::Up(up) => {
                    Self::handle_touch_up(
                        up, touches, active_layer, layers, uinput,
                        current_window_class, app_ui_manager, vlc_touch_active,
                        vlc_drag_position, needs_complete_redraw
                    )?;
                },
                _ => {}
            }
        }
        Ok(())
    }

    fn handle_touch_down(
        dn: &input::event::touch::TouchDownEvent,
        width: u32,
        height: u32,
        touches: &mut HashMap<u32, (LayerKey, &'static str, usize)>,
        active_layer: &LayerKey,
        layers: &mut HashMap<LayerKey, FunctionLayer>,
        uinput: &mut UInputHandle<std::fs::File>,
        current_window_class: &Option<String>,
        app_ui_manager: &mut AppUiManager,
        vlc_touch_active: &mut bool,
        vlc_drag_position: &mut Option<f64>,
        vlc_helper_stream: &mut Option<UnixStream>,
        browser_helper_stream: &mut Option<UnixStream>,
        needs_complete_redraw: &mut bool,
        cfg_enable_pixel_shift: bool,
    ) -> crate::MainResult<()> {
        let _x = dn.x_transformed(width);
        let _y = dn.y_transformed(height);
        println!("[touch] Touch down at ({}, {})", _x, _y);
        
        if let Some((group, idx)) = layers.get_mut(active_layer).ok_or(crate::MainError::LayerNotFound(*active_layer))?.hit_test(_x, width as i32, Some(active_layer.clone())) {
            match group {
                "modules" => {
                    touches.insert(dn.seat_slot(), (active_layer.clone(), group, idx));
                    println!("[touch] Touch stored for modules group, slot: {}", dn.seat_slot());
                    
                    Self::handle_modules_touch_down(
                        _x, _y, width, height, current_window_class, app_ui_manager,
                        vlc_touch_active, vlc_drag_position, vlc_helper_stream,
                        browser_helper_stream, needs_complete_redraw, cfg_enable_pixel_shift, uinput
                    )?;
                },
                "media" => {
                    if let Some(split) = &mut layers.get_mut(active_layer).ok_or(crate::MainError::LayerNotFound(*active_layer))?.split {
                        let button = &mut split.media[idx];
                        if button.action == Key::Unknown {
                            return Ok(());
                        }
                        touches.insert(dn.seat_slot(), (active_layer.clone(), group, idx));
                        button.set_active(uinput, true);
                    }
                },
                "flat" => {
                    let button = &mut layers.get_mut(active_layer).ok_or(crate::MainError::LayerNotFound(*active_layer))?.buttons[idx];
                    if button.action == Key::Unknown {
                        return Ok(());
                    }
                    touches.insert(dn.seat_slot(), (active_layer.clone(), group, idx));
                    button.set_active(uinput, true);
                },
                _ => {}
            }
        }
        Ok(())
    }

    fn handle_touch_motion(
        mtn: &input::event::touch::TouchMotionEvent,
        width: u32,
        height: u32,
        touches: &HashMap<u32, (LayerKey, &'static str, usize)>,
        _active_layer: &LayerKey,
        layers: &mut HashMap<LayerKey, FunctionLayer>,
        uinput: &mut UInputHandle<std::fs::File>,
        current_window_class: &Option<String>,
        app_ui_manager: &mut AppUiManager,
        vlc_touch_active: &bool,
        vlc_drag_position: &mut Option<f64>,
        vlc_helper_stream: &mut Option<UnixStream>,
        browser_helper_stream: &mut Option<UnixStream>,
        needs_complete_redraw: &mut bool,
        cfg_enable_pixel_shift: bool,
    ) -> crate::MainResult<()> {
        println!("[touch] Motion event received for slot: {}", mtn.seat_slot());
        if !touches.contains_key(&mtn.seat_slot()) {
            println!("[touch] Motion event ignored - slot not in touches");
            return Ok(());
        }
        
        let _x = mtn.x_transformed(width);
        let _y = mtn.y_transformed(height);
        let (layer, group, idx) = Self::get_touch_slot(touches, mtn.seat_slot())?;
        println!("[touch] Motion event: group={}, idx={}, coords=({}, {})", group, idx, _x, _y);
        
        match *group {
            "modules" => {
                Self::handle_modules_touch_motion(
                    _x, _y, width, height, current_window_class, app_ui_manager,
                    vlc_touch_active, vlc_drag_position, vlc_helper_stream,
                    browser_helper_stream, needs_complete_redraw, cfg_enable_pixel_shift
                )?;
            },
            "media" => {
                if let Some(split) = &mut layers.get_mut(layer).ok_or(crate::MainError::LayerNotFound(*layer))?.split {
                    let button = &mut split.media[*idx];
                    if button.action == Key::Unknown {
                        return Ok(());
                    }
                    button.set_active(uinput, true);
                }
            },
            "flat" => {
                let button = &mut layers.get_mut(layer).ok_or(crate::MainError::LayerNotFound(*layer))?.buttons[*idx];
                if button.action == Key::Unknown {
                    return Ok(());
                }
                button.set_active(uinput, true);
            },
            _ => {}
        }
        Ok(())
    }

    fn handle_touch_up(
        up: &input::event::touch::TouchUpEvent,
        touches: &mut HashMap<u32, (LayerKey, &'static str, usize)>,
        _active_layer: &LayerKey,
        layers: &mut HashMap<LayerKey, FunctionLayer>,
        uinput: &mut UInputHandle<std::fs::File>,
        current_window_class: &Option<String>,
        app_ui_manager: &mut AppUiManager,
        vlc_touch_active: &mut bool,
        vlc_drag_position: &mut Option<f64>,
        needs_complete_redraw: &mut bool,
    ) -> crate::MainResult<()> {
        if !touches.contains_key(&up.seat_slot()) {
            return Ok(());
        }
        
        let (layer, group, idx) = Self::get_touch_slot(touches, up.seat_slot())?;
        println!("[touch] Up: group={}, idx={}", group, idx);
        
        match *group {
            "modules" => {
                Self::handle_modules_touch_up(
                    current_window_class, app_ui_manager, vlc_touch_active,
                    vlc_drag_position, needs_complete_redraw
                )?;
            },
            "media" => {
                if let Some(split) = &mut layers.get_mut(layer).ok_or(crate::MainError::LayerNotFound(*layer))?.split {
                    let button = &mut split.media[*idx];
                    if button.action == Key::Unknown {
                        return Ok(());
                    }
                    button.set_active(uinput, false);
                }
            },
            "flat" => {
                let button = &mut layers.get_mut(layer).ok_or(crate::MainError::LayerNotFound(*layer))?.buttons[*idx];
                if button.action == Key::Unknown {
                    return Ok(());
                }
                button.set_active(uinput, false);
            },
            _ => {}
        }
        
        touches.remove(&up.seat_slot());
        Ok(())
    }

    fn handle_modules_touch_down(
        _x: f64,
        _y: f64,
        width: u32,
        height: u32,
        current_window_class: &Option<String>,
        app_ui_manager: &mut AppUiManager,
        vlc_touch_active: &mut bool,
        vlc_drag_position: &mut Option<f64>,
        vlc_helper_stream: &mut Option<UnixStream>,
        browser_helper_stream: &mut Option<UnixStream>,
        needs_complete_redraw: &mut bool,
        cfg_enable_pixel_shift: bool,
        uinput: &mut UInputHandle<std::fs::File>,
    ) -> crate::MainResult<()> {
        if let Some(window_class) = current_window_class {
            let pixel_shift_width = if cfg_enable_pixel_shift { PIXEL_SHIFT_WIDTH_PX } else { 0 };
            let total_width = (width as i32 - pixel_shift_width as i32) as f64;
            let _group_spacing = BUTTON_SPACING_PX as f64;
            let modules_width = (0.7 * total_width).round();
            let modules_x = (pixel_shift_width / 2) as f64;
            let modules_y = (height as f64) * 0.15;
            let modules_height = (height as f64) * 0.7;
            
            let adjusted_x = _x - modules_x;
            let adjusted_y = _y - modules_y;
            
            if let Some(app_action) = app_ui_manager.hit_test_app_ui(adjusted_x, adjusted_y, modules_x, modules_y, modules_width, modules_height, 8.0, window_class) {
                println!("[touch] App action detected: {:?}", app_action);
                Self::handle_app_action(
                    app_action, window_class, app_ui_manager, vlc_touch_active,
                    vlc_drag_position, vlc_helper_stream, browser_helper_stream,
                    needs_complete_redraw, uinput
                )?;
            }
        }
        Ok(())
    }

    fn handle_modules_touch_motion(
        _x: f64,
        _y: f64,
        width: u32,
        height: u32,
        current_window_class: &Option<String>,
        app_ui_manager: &mut AppUiManager,
        vlc_touch_active: &bool,
        vlc_drag_position: &mut Option<f64>,
        vlc_helper_stream: &mut Option<UnixStream>,
        browser_helper_stream: &mut Option<UnixStream>,
        needs_complete_redraw: &mut bool,
        cfg_enable_pixel_shift: bool,
    ) -> crate::MainResult<()> {
        if let Some(window_class) = current_window_class {
            let any_browser_button_active = app_ui_manager.browser_screen.buttons.iter().any(|b| b.active);
            println!("[touch] Motion - window_class: {}, vlc_touch_active: {}, browser_button_active: {}", window_class, vlc_touch_active, any_browser_button_active);
            
            let pixel_shift_width = if cfg_enable_pixel_shift { PIXEL_SHIFT_WIDTH_PX } else { 0 };
            let total_width = (width as i32 - pixel_shift_width as i32) as f64;
            let _group_spacing = BUTTON_SPACING_PX as f64;
            let modules_width = (0.7 * total_width).round();
            let modules_x = (pixel_shift_width / 2) as f64;
            let modules_y = (height as f64) * 0.15;
            let modules_height = (height as f64) * 0.7;
            
            let adjusted_x = _x - modules_x;
            let adjusted_y = _y - modules_y;
            println!("[touch] Motion - Adjusted touch coordinates: ({}, {}) relative to modules area", adjusted_x, adjusted_y);
            
            if let Some(app_action) = app_ui_manager.hit_test_app_ui(adjusted_x, adjusted_y, modules_x, modules_y, modules_width, modules_height, 8.0, window_class) {
                println!("[touch] Motion - App action detected: {:?}", app_action);
                Self::handle_app_action_motion(
                    app_action, window_class, app_ui_manager, vlc_touch_active,
                    vlc_drag_position, vlc_helper_stream, browser_helper_stream,
                    needs_complete_redraw
                )?;
            }
        }
        Ok(())
    }

    fn handle_modules_touch_up(
        current_window_class: &Option<String>,
        app_ui_manager: &mut AppUiManager,
        vlc_touch_active: &mut bool,
        vlc_drag_position: &mut Option<f64>,
        needs_complete_redraw: &mut bool,
    ) -> crate::MainResult<()> {
        if *vlc_touch_active {
            *vlc_touch_active = false;
            app_ui_manager.vlc_screen.reset_drag_state();
            *needs_complete_redraw = true;
            println!("[touch] VLC touch interaction ended, keeping drag position for smooth transition");
        }
        
        if let Some(window_class) = current_window_class {
            let window_class_lc = window_class.to_lowercase();
            if window_class_lc == "firefox" || window_class_lc == "chrome" || window_class_lc == "chromium" || window_class_lc == "brave" || window_class_lc == "brave-browser" || window_class_lc == "edge" || window_class_lc == "safari" || window_class_lc == "opera" || window_class_lc == "google-chrome" {
                let any_browser_button_active = app_ui_manager.browser_screen.buttons.iter().any(|b| b.active);
                if any_browser_button_active {
                    println!("[touch] Browser touch interaction ended, resetting button states");
                    for button in &mut app_ui_manager.browser_screen.buttons {
                        button.active = false;
                        button.changed = true;
                    }
                }
            }
        }
        Ok(())
    }

    fn handle_app_action(
        app_action: AppAction,
        window_class: &str,
        app_ui_manager: &mut AppUiManager,
        vlc_touch_active: &mut bool,
        vlc_drag_position: &mut Option<f64>,
        vlc_helper_stream: &mut Option<UnixStream>,
        browser_helper_stream: &mut Option<UnixStream>,
        needs_complete_redraw: &mut bool,
        uinput: &mut UInputHandle<std::fs::File>,
    ) -> crate::MainResult<()> {
        let window_class_lc = window_class.to_lowercase();
        
        if window_class_lc == "firefox" || window_class_lc == "chrome" || window_class_lc == "chromium" || window_class_lc == "brave" || window_class_lc == "brave-browser" || window_class_lc == "edge" || window_class_lc == "safari" || window_class_lc == "opera" || window_class_lc == "google-chrome" {
            Self::handle_browser_action(app_action, app_ui_manager, browser_helper_stream, uinput)?;
        } else if window_class_lc == "vlc" && vlc_helper_stream.is_some() {
            Self::handle_vlc_action(app_action, app_ui_manager, vlc_touch_active, vlc_drag_position, vlc_helper_stream, needs_complete_redraw)?;
        }
        Ok(())
    }

    fn handle_app_action_motion(
        app_action: AppAction,
        window_class: &str,
        app_ui_manager: &mut AppUiManager,
        vlc_touch_active: &bool,
        vlc_drag_position: &mut Option<f64>,
        vlc_helper_stream: &mut Option<UnixStream>,
        browser_helper_stream: &mut Option<UnixStream>,
        needs_complete_redraw: &mut bool,
    ) -> crate::MainResult<()> {
        let window_class_lc = window_class.to_lowercase();
        
        if window_class_lc == "firefox" || window_class_lc == "chrome" || window_class_lc == "chromium" || window_class_lc == "brave" || window_class_lc == "brave-browser" || window_class_lc == "edge" || window_class_lc == "safari" || window_class_lc == "opera" || window_class_lc == "google-chrome" {
            let any_browser_button_active = app_ui_manager.browser_screen.buttons.iter().any(|b| b.active);
            if any_browser_button_active {
                println!("[touch] Motion - Browser button active, ignoring motion");
            }
        } else if window_class_lc == "vlc" && *vlc_touch_active && vlc_helper_stream.is_some() {
            Self::handle_vlc_action_motion(app_action, vlc_drag_position, vlc_helper_stream, needs_complete_redraw)?;
        }
        Ok(())
    }

    fn handle_browser_action(
        app_action: AppAction,
        app_ui_manager: &mut AppUiManager,
        browser_helper_stream: &mut Option<UnixStream>,
        uinput: &mut UInputHandle<std::fs::File>,
    ) -> crate::MainResult<()> {
        match app_action {
            AppAction::Browser(BrowserAction::Back) => {
                println!("[touch] Executing Browser Back");
                app_ui_manager.browser_screen.buttons[0].active = true;
                app_ui_manager.browser_screen.buttons[0].changed = true;
                // Send Alt+Left directly via uinput
                crate::layers::button::toggle_key(uinput, Key::LeftAlt, 1);
                std::thread::sleep(std::time::Duration::from_millis(5));
                crate::layers::button::toggle_key(uinput, Key::Left, 1);
                std::thread::sleep(std::time::Duration::from_millis(5));
                crate::layers::button::toggle_key(uinput, Key::Left, 0);
                std::thread::sleep(std::time::Duration::from_millis(5));
                crate::layers::button::toggle_key(uinput, Key::LeftAlt, 0);
            }
            AppAction::Browser(BrowserAction::Forward) => {
                println!("[touch] Executing Browser Forward");
                app_ui_manager.browser_screen.buttons[1].active = true;
                app_ui_manager.browser_screen.buttons[1].changed = true;
                // Send Alt+Right directly via uinput
                crate::layers::button::toggle_key(uinput, Key::LeftAlt, 1);
                std::thread::sleep(std::time::Duration::from_millis(5));
                crate::layers::button::toggle_key(uinput, Key::Right, 1);
                std::thread::sleep(std::time::Duration::from_millis(5));
                crate::layers::button::toggle_key(uinput, Key::Right, 0);
                std::thread::sleep(std::time::Duration::from_millis(5));
                crate::layers::button::toggle_key(uinput, Key::LeftAlt, 0);
            }
            AppAction::Browser(BrowserAction::Refresh) => {
                println!("[touch] Executing Browser Refresh");
                app_ui_manager.browser_screen.buttons[2].active = true;
                app_ui_manager.browser_screen.buttons[2].changed = true;
                // Send Ctrl+R directly via uinput
                crate::layers::button::toggle_key(uinput, Key::LeftCtrl, 1);
                std::thread::sleep(std::time::Duration::from_millis(5));
                crate::layers::button::toggle_key(uinput, Key::R, 1);
                std::thread::sleep(std::time::Duration::from_millis(5));
                crate::layers::button::toggle_key(uinput, Key::R, 0);
                std::thread::sleep(std::time::Duration::from_millis(5));
                crate::layers::button::toggle_key(uinput, Key::LeftCtrl, 0);
            }
            AppAction::Browser(BrowserAction::Home) => {
                println!("[touch] Executing Browser Home");
                app_ui_manager.browser_screen.buttons[3].active = true;
                app_ui_manager.browser_screen.buttons[3].changed = true;
                // Send Ctrl+L directly via uinput (focus address bar, then Ctrl+A to select all)
                crate::layers::button::toggle_key(uinput, Key::LeftCtrl, 1);
                std::thread::sleep(std::time::Duration::from_millis(5));
                crate::layers::button::toggle_key(uinput, Key::L, 1);
                std::thread::sleep(std::time::Duration::from_millis(5));
                crate::layers::button::toggle_key(uinput, Key::L, 0);
                std::thread::sleep(std::time::Duration::from_millis(5));
                crate::layers::button::toggle_key(uinput, Key::LeftCtrl, 0);
                std::thread::sleep(std::time::Duration::from_millis(5));
                crate::layers::button::toggle_key(uinput, Key::LeftCtrl, 1);
                std::thread::sleep(std::time::Duration::from_millis(5));
                crate::layers::button::toggle_key(uinput, Key::A, 1);
                std::thread::sleep(std::time::Duration::from_millis(5));
                crate::layers::button::toggle_key(uinput, Key::A, 0);
                std::thread::sleep(std::time::Duration::from_millis(5));
                crate::layers::button::toggle_key(uinput, Key::LeftCtrl, 0);
            }
            AppAction::Browser(BrowserAction::AddBookmark) => {
                println!("[touch] Executing Browser Add Bookmark");
                app_ui_manager.browser_screen.buttons[4].active = true;
                app_ui_manager.browser_screen.buttons[4].changed = true;
                // Send Ctrl+D directly via uinput
                crate::layers::button::toggle_key(uinput, Key::LeftCtrl, 1);
                std::thread::sleep(std::time::Duration::from_millis(5));
                crate::layers::button::toggle_key(uinput, Key::D, 1);
                std::thread::sleep(std::time::Duration::from_millis(5));
                crate::layers::button::toggle_key(uinput, Key::D, 0);
                std::thread::sleep(std::time::Duration::from_millis(5));
                crate::layers::button::toggle_key(uinput, Key::LeftCtrl, 0);
            }
            AppAction::Browser(BrowserAction::BookmarksManager) => {
                println!("[touch] Executing Browser Bookmarks Manager");
                app_ui_manager.browser_screen.buttons[4].active = true;
                app_ui_manager.browser_screen.buttons[4].changed = true;
                // Send Ctrl+Shift+O directly via uinput
                crate::layers::button::toggle_key(uinput, Key::LeftCtrl, 1);
                std::thread::sleep(std::time::Duration::from_millis(5));
                crate::layers::button::toggle_key(uinput, Key::LeftShift, 1);
                std::thread::sleep(std::time::Duration::from_millis(5));
                crate::layers::button::toggle_key(uinput, Key::O, 1);
                std::thread::sleep(std::time::Duration::from_millis(5));
                crate::layers::button::toggle_key(uinput, Key::O, 0);
                std::thread::sleep(std::time::Duration::from_millis(5));
                crate::layers::button::toggle_key(uinput, Key::LeftShift, 0);
                std::thread::sleep(std::time::Duration::from_millis(5));
                crate::layers::button::toggle_key(uinput, Key::LeftCtrl, 0);
            }
            AppAction::Browser(BrowserAction::CloseTab) => {
                println!("[touch] Executing Browser Close Tab");
                app_ui_manager.browser_screen.buttons[4].active = true;
                app_ui_manager.browser_screen.buttons[4].changed = true;
                // Send Ctrl+W directly via uinput
                crate::layers::button::toggle_key(uinput, Key::LeftCtrl, 1);
                std::thread::sleep(std::time::Duration::from_millis(5));
                crate::layers::button::toggle_key(uinput, Key::W, 1);
                std::thread::sleep(std::time::Duration::from_millis(5));
                crate::layers::button::toggle_key(uinput, Key::W, 0);
                std::thread::sleep(std::time::Duration::from_millis(5));
                crate::layers::button::toggle_key(uinput, Key::LeftCtrl, 0);
            }
            AppAction::Browser(BrowserAction::NewTab) => {
                println!("[touch] Executing Browser New Tab");
                app_ui_manager.browser_screen.buttons[4].active = true;
                app_ui_manager.browser_screen.buttons[4].changed = true;
                // Send Ctrl+T directly via uinput
                crate::layers::button::toggle_key(uinput, Key::LeftCtrl, 1);
                std::thread::sleep(std::time::Duration::from_millis(5));
                crate::layers::button::toggle_key(uinput, Key::T, 1);
                std::thread::sleep(std::time::Duration::from_millis(5));
                crate::layers::button::toggle_key(uinput, Key::T, 0);
                std::thread::sleep(std::time::Duration::from_millis(5));
                crate::layers::button::toggle_key(uinput, Key::LeftCtrl, 0);
            }
            AppAction::Browser(BrowserAction::AddressBar) => {
                println!("[touch] Executing Browser Address Bar Focus");
                app_ui_manager.browser_screen.focus_address_bar();
                // Send Ctrl+L directly via uinput
                crate::layers::button::toggle_key(uinput, Key::LeftCtrl, 1);
                std::thread::sleep(std::time::Duration::from_millis(5));
                crate::layers::button::toggle_key(uinput, Key::L, 1);
                std::thread::sleep(std::time::Duration::from_millis(5));
                crate::layers::button::toggle_key(uinput, Key::L, 0);
                std::thread::sleep(std::time::Duration::from_millis(5));
                crate::layers::button::toggle_key(uinput, Key::LeftCtrl, 0);
            }
            _ => {
                println!("[touch] Ignoring non-browser action: {:?}", app_action);
            }
        }
        Ok(())
    }

    fn handle_vlc_action(
        app_action: AppAction,
        _app_ui_manager: &mut AppUiManager,
        vlc_touch_active: &mut bool,
        vlc_drag_position: &mut Option<f64>,
        vlc_helper_stream: &mut Option<UnixStream>,
        needs_complete_redraw: &mut bool,
    ) -> crate::MainResult<()> {
        match app_action {
            AppAction::Vlc(VlcAction::TogglePlayPause) => {
                *vlc_touch_active = true;
                if let Some(stream) = vlc_helper_stream {
                    Self::send_vlc_command(stream, "play_pause")?;
                }
            }
            AppAction::Vlc(VlcAction::Seek(position)) => {
                *vlc_touch_active = true;
                if let Some(stream) = vlc_helper_stream {
                    let seek_command = format!("seek:{}", position);
                    Self::send_vlc_command(stream, &seek_command)?;
                }
            }
            AppAction::Vlc(VlcAction::DragHead(position)) => {
                *vlc_touch_active = true;
                *vlc_drag_position = Some(position);
                *needs_complete_redraw = true;
                
                static mut LAST_SEEK_POSITION: f64 = 0.0;
                unsafe {
                    if (position - LAST_SEEK_POSITION).abs() > 0.01 {
                        LAST_SEEK_POSITION = position;
                        if let Some(stream) = vlc_helper_stream {
                            let seek_command = format!("seek:{}", position);
                            Self::send_vlc_command(stream, &seek_command)?;
                        }
                    }
                }
            }
            AppAction::Vlc(VlcAction::Next) => {
                println!("[touch] Executing VLC Next");
                *vlc_touch_active = true;
                if let Some(stream) = vlc_helper_stream {
                    Self::send_vlc_command(stream, "next")?;
                }
            }
            AppAction::Vlc(VlcAction::Previous) => {
                println!("[touch] Executing VLC Previous");
                *vlc_touch_active = true;
                if let Some(stream) = vlc_helper_stream {
                    Self::send_vlc_command(stream, "previous")?;
                }
            }
            AppAction::Vlc(VlcAction::Stop) => {
                println!("[touch] Executing VLC Stop");
                *vlc_touch_active = true;
                if let Some(stream) = vlc_helper_stream {
                    Self::send_vlc_command(stream, "stop")?;
                }
            }
            AppAction::Vlc(VlcAction::Raise) => {
                println!("[touch] Executing VLC Raise");
                *vlc_touch_active = true;
                if let Some(stream) = vlc_helper_stream {
                    Self::send_vlc_command(stream, "raise")?;
                }
            }
            AppAction::Vlc(VlcAction::Quit) => {
                println!("[touch] Executing VLC Quit");
                *vlc_touch_active = true;
                if let Some(stream) = vlc_helper_stream {
                    Self::send_vlc_command(stream, "quit")?;
                }
            }
            _ => {
                println!("[touch] Ignoring non-VLC action: {:?}", app_action);
            }
        }
        Ok(())
    }

    fn handle_vlc_action_motion(
        app_action: AppAction,
        vlc_drag_position: &mut Option<f64>,
        vlc_helper_stream: &mut Option<UnixStream>,
        needs_complete_redraw: &mut bool,
    ) -> crate::MainResult<()> {
        match app_action {
            AppAction::Vlc(VlcAction::Seek(position)) => {
                println!("[touch] VLC seek during motion to position: {}", position);
                if let Some(stream) = vlc_helper_stream {
                    let seek_command = format!("seek:{}", position);
                    Self::send_vlc_command(stream, &seek_command)?;
                }
            }
            AppAction::Vlc(VlcAction::DragHead(position)) => {
                println!("[touch] VLC drag head during motion to position: {}", position);
                *vlc_drag_position = Some(position);
                *needs_complete_redraw = true;
                if let Some(stream) = vlc_helper_stream {
                    let seek_command = format!("seek:{}", position);
                    Self::send_vlc_command(stream, &seek_command)?;
                }
            }
            _ => {
                println!("[touch] Ignoring non-seek VLC action during motion: {:?}", app_action);
            }
        }
        Ok(())
    }

    fn send_vlc_command(stream: &mut UnixStream, command: &str) -> Result<(), std::io::Error> {
        let command_with_newline = format!("{}\n", command);
        stream.write_all(command_with_newline.as_bytes())?;
        Ok(())
    }



    fn get_touch_slot<'a>(
        touches: &'a HashMap<u32, (LayerKey, &'static str, usize)>,
        slot: u32
    ) -> crate::MainResult<&'a (LayerKey, &'static str, usize)> {
        touches.get(&slot)
            .ok_or_else(|| crate::MainError::TouchSlotNotFound(slot))
    }
} 