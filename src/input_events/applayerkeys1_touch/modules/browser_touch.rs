use input::{
    event::{
        Event,
        touch::{TouchEvent, TouchEventPosition, TouchEventSlot}
    }
};
use input_linux::uinput::UInputHandle;
use std::os::unix::net::UnixStream;
use crate::view::app_ui_manager::{AppUiManager, AppAction};
use crate::view::browser_screen::BrowserAction;
use crate::display::pixel_shift::PIXEL_SHIFT_WIDTH_PX;
use crate::layers::function_layer::BUTTON_SPACING_PX;
use input_linux::Key;
use std::collections::HashMap;
use crate::LayerKey;
use crate::layers::FunctionLayer;

// Static HashMap for browser touch slots
static mut BROWSER_TOUCHES: Option<HashMap<u32, (LayerKey, &'static str, usize)>> = None;

pub struct BrowserTouchHandler;

impl BrowserTouchHandler {
    fn get_touches() -> &'static mut HashMap<u32, (LayerKey, &'static str, usize)> {
        unsafe {
            if BROWSER_TOUCHES.is_none() {
                BROWSER_TOUCHES = Some(HashMap::new());
            }
            BROWSER_TOUCHES.as_mut().unwrap()
        }
    }

    /// Handles all browser touch events
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
        let touches = Self::get_touches();
        
        println!("[browser_touch] handle_touch_event called");
        if let Event::Touch(te) = event {
            match te {
                TouchEvent::Down(dn) => {
                    let x = dn.x_transformed(width);
                    let y = dn.y_transformed(height);
                    
                    let available_mpris_services = &app_ui_manager.generic_background_screen.available_mpris_services;
                    if let Some((group, idx)) = layers.get_mut(active_layer).ok_or(crate::MainError::LayerNotFound(*active_layer))?.hit_test(x, width as i32, Some(active_layer.clone()), available_mpris_services) {
                        if group == "modules" {
                            touches.insert(dn.seat_slot(), (active_layer.clone(), group, idx));
                            
                            // Delegate to browser touch handler
                            Self::handle_touch_down(
                                x, y, width, height, current_window_class, app_ui_manager,
                                browser_helper_stream, needs_complete_redraw, cfg_enable_pixel_shift, uinput
                            )?;
                        }
                    }
                },
                TouchEvent::Motion(mtn) => {
                    if !touches.contains_key(&mtn.seat_slot()) {
                        return Ok(());
                    }
                    
                    let (_layer, group, _idx) = Self::get_touch_slot(touches, mtn.seat_slot())?;
                    if *group == "modules" {
                        let x = mtn.x_transformed(width);
                        let y = mtn.y_transformed(height);
                        
                        // Delegate to browser touch handler
                        Self::handle_touch_motion(
                            x, y, width, height, current_window_class, app_ui_manager,
                            needs_complete_redraw, cfg_enable_pixel_shift
                        )?;
                    }
                },
                TouchEvent::Up(up) => {
                    if !touches.contains_key(&up.seat_slot()) {
                        return Ok(());
                    }
                    
                    let (_layer, group, _idx) = Self::get_touch_slot(touches, up.seat_slot())?;
                    if *group == "modules" {
             
                        // Delegate to browser touch handler
                        Self::handle_touch_up(
                            current_window_class, app_ui_manager, needs_complete_redraw, uinput
                        )?;
                        
                        // Remove touch slot - Browser handler manages its own slots
                        touches.remove(&up.seat_slot());
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

    /// Handles browser touch down events
    pub fn handle_touch_down(
        x: f64,
        y: f64,
        width: u32,
        height: u32,
        current_window_class: &Option<String>,
        app_ui_manager: &mut AppUiManager,
        _browser_helper_stream: &mut Option<UnixStream>,
        _needs_complete_redraw: &mut bool,
        cfg_enable_pixel_shift: bool,
        uinput: &mut UInputHandle<std::fs::File>,
    ) -> crate::MainResult<()> {
        let pixel_shift_width = if cfg_enable_pixel_shift { PIXEL_SHIFT_WIDTH_PX } else { 0 };
        let total_width = (width as i32 - pixel_shift_width as i32) as f64;
        let _group_spacing = BUTTON_SPACING_PX as f64;
        let modules_width = (0.7 * total_width).round();
        let modules_x = (pixel_shift_width / 2) as f64;
        let modules_y = (height as f64) * 0.15;
        let modules_height = (height as f64) * 0.7;
        
        let adjusted_x = x - modules_x;
        let adjusted_y = y - modules_y;
        
        if let Some(app_action) = app_ui_manager.hit_test_app_ui(adjusted_x, adjusted_y, modules_x, modules_y, modules_width, modules_height, 8.0, current_window_class.as_ref().unwrap()) {
            println!("[browser_touch] Browser action detected: {:?}", app_action);
            Self::handle_browser_action(app_action, app_ui_manager, _browser_helper_stream, uinput)?;
        }
        Ok(())
    }

    /// Handles browser touch motion events
    pub fn handle_touch_motion(
        x: f64,
        y: f64,
        width: u32,
        height: u32,
        current_window_class: &Option<String>,
        app_ui_manager: &mut AppUiManager,
        _needs_complete_redraw: &mut bool,
        cfg_enable_pixel_shift: bool,
    ) -> crate::MainResult<()> {
        let any_browser_button_active = app_ui_manager.browser_screen.buttons.iter().any(|b| b.active);
        println!("[browser_touch] Motion - window_class: {}, browser_button_active: {}", current_window_class.as_ref().unwrap(), any_browser_button_active);
                
                let pixel_shift_width = if cfg_enable_pixel_shift { PIXEL_SHIFT_WIDTH_PX } else { 0 };
                let total_width = (width as i32 - pixel_shift_width as i32) as f64;
                let _group_spacing = BUTTON_SPACING_PX as f64;
                let modules_width = (0.7 * total_width).round();
                let modules_x = (pixel_shift_width / 2) as f64;
                let modules_y = (height as f64) * 0.15;
                let modules_height = (height as f64) * 0.7;
                
                let adjusted_x = x - modules_x;
                let adjusted_y = y - modules_y;
                
        if let Some(app_action) = app_ui_manager.hit_test_app_ui(adjusted_x, adjusted_y, modules_x, modules_y, modules_width, modules_height, 8.0, current_window_class.as_ref().unwrap()) {
            println!("[browser_touch] Motion browser action detected: {:?}", app_action);
            Self::handle_browser_action_motion(app_action, app_ui_manager, _needs_complete_redraw)?;
        }
        Ok(())
    }

    /// Handles browser touch up events
    pub fn handle_touch_up(
        current_window_class: &Option<String>,
        app_ui_manager: &mut AppUiManager,
        needs_complete_redraw: &mut bool,
        uinput: &mut UInputHandle<std::fs::File>,
    ) -> crate::MainResult<()> {
        println!("[browser_touch] handle_touch_up called");
        let any_browser_button_active = app_ui_manager.browser_screen.buttons.iter().any(|b| b.active);
        println!("[browser_touch] Any browser button active: {}", any_browser_button_active);
        if any_browser_button_active {
            println!("[browser_touch] Browser touch interaction ended, resetting button states");
            for button in &mut app_ui_manager.browser_screen.buttons {
                button.set_active(uinput, false);
            }
            *needs_complete_redraw = true;
            println!("[browser_touch] Button states reset successfully");
        } else {
            println!("[browser_touch] No browser buttons were active, nothing to reset");
        }
        Ok(())
    }

    /// Handles browser-specific actions
    fn handle_browser_action(
        app_action: AppAction,
        app_ui_manager: &mut AppUiManager,
        _browser_helper_stream: &mut Option<UnixStream>,
        uinput: &mut UInputHandle<std::fs::File>,
    ) -> crate::MainResult<()> {
        match app_action {
            AppAction::Browser(BrowserAction::Back) => {
                println!("[browser_touch] Executing Browser Back");
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
                println!("[browser_touch] Executing Browser Forward");
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
                println!("[browser_touch] Executing Browser Refresh");
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
                println!("[browser_touch] Executing Browser Home");
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
                println!("[browser_touch] Executing Browser Add Bookmark");
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
                println!("[browser_touch] Executing Browser Bookmarks Manager");
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
                println!("[browser_touch] Executing Browser Close Tab");
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
                println!("[browser_touch] Executing Browser New Tab");
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
                println!("[browser_touch] Executing Browser Address Bar Focus");
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
                println!("[browser_touch] Ignoring non-browser action: {:?}", app_action);
            }
        }
        Ok(())
    }

    /// Handles browser action motion events
    fn handle_browser_action_motion(
        _app_action: AppAction,
        app_ui_manager: &mut AppUiManager,
        _needs_complete_redraw: &mut bool,
    ) -> crate::MainResult<()> {
        let any_browser_button_active = app_ui_manager.browser_screen.buttons.iter().any(|b| b.active);
        if any_browser_button_active {
            println!("[browser_touch] Motion - Browser button active, ignoring motion");
        }
        Ok(())
    }
} 