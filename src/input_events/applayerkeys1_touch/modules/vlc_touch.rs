use input::{
    event::{
        Event,
        touch::{TouchEvent, TouchEventPosition, TouchEventSlot}
    }
};
use input_linux::uinput::UInputHandle;
use std::os::unix::net::UnixStream;
use std::io::Write;
use crate::view::app_ui_manager::{AppUiManager, AppAction};
use crate::view::vlc_screen::VlcAction;
use crate::display::pixel_shift::PIXEL_SHIFT_WIDTH_PX;
use crate::layers::function_layer::BUTTON_SPACING_PX;

use std::collections::HashMap;
use crate::LayerKey;
use crate::layers::FunctionLayer;

// Static HashMap for VLC touch slots
static mut VLC_TOUCHES: Option<HashMap<u32, (LayerKey, &'static str, usize)>> = None;

pub struct VlcTouchHandler;

impl VlcTouchHandler {
    fn get_touches() -> &'static mut HashMap<u32, (LayerKey, &'static str, usize)> {
        unsafe {
            if VLC_TOUCHES.is_none() {
                VLC_TOUCHES = Some(HashMap::new());
            }
            VLC_TOUCHES.as_mut().unwrap()
        }
    }

    /// Handles all VLC touch events
    pub fn handle_touch_event(
        event: &Event,
        width: u32,
        height: u32,
        active_layer: &LayerKey,
        layers: &mut HashMap<LayerKey, FunctionLayer>,
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
        let touches = Self::get_touches();
        
        if let Event::Touch(te) = event {
            match te {
                TouchEvent::Down(dn) => {
                    let x = dn.x_transformed(width);
                    let y = dn.y_transformed(height);
                    println!("[vlc_touch] Touch down at ({}, {})", x, y);
                    
                    if let Some((group, idx)) = layers.get_mut(active_layer).ok_or(crate::MainError::LayerNotFound(*active_layer))?.hit_test(x, width as i32, Some(active_layer.clone())) {
                        if group == "modules" {
                            touches.insert(dn.seat_slot(), (active_layer.clone(), group, idx));
                            println!("[vlc_touch] Touch stored for modules group, slot: {}", dn.seat_slot());
                            
                            // Delegate to VLC touch handler
                            Self::handle_touch_down(
                                x, y, width, height, current_window_class, app_ui_manager,
                                vlc_touch_active, vlc_drag_position, vlc_helper_stream,
                                needs_complete_redraw, cfg_enable_pixel_shift
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
                        
                        // Delegate to VLC touch handler
                        Self::handle_touch_motion(
                            x, y, width, height, current_window_class, app_ui_manager,
                            vlc_touch_active, vlc_drag_position, vlc_helper_stream,
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
                        // Delegate to VLC touch handler
                        Self::handle_touch_up(
                            current_window_class, app_ui_manager, vlc_touch_active,
                            vlc_drag_position, needs_complete_redraw
                        )?;
                        
                        // Remove touch slot - VLC handler manages its own slots
                        touches.remove(&up.seat_slot());
                        println!("[vlc_touch] Touch slot {} removed by VLC handler", up.seat_slot());
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

    /// Handles VLC touch down events
    pub fn handle_touch_down(
        x: f64,
        y: f64,
        width: u32,
        height: u32,
        current_window_class: &Option<String>,
        app_ui_manager: &mut AppUiManager,
        vlc_touch_active: &mut bool,
        vlc_drag_position: &mut Option<f64>,
        vlc_helper_stream: &mut Option<UnixStream>,
        needs_complete_redraw: &mut bool,
        cfg_enable_pixel_shift: bool,
    ) -> crate::MainResult<()> {
        if vlc_helper_stream.is_some() {
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
                    println!("[vlc_touch] VLC action detected: {:?}", app_action);
                    Self::handle_vlc_action(
                        app_action, app_ui_manager, vlc_touch_active,
                        vlc_drag_position, vlc_helper_stream, needs_complete_redraw
                    )?;
                }
            }
        Ok(())
    }

    /// Handles VLC touch motion events
    pub fn handle_touch_motion(
        x: f64,
        y: f64,
        width: u32,
        height: u32,
        current_window_class: &Option<String>,
        app_ui_manager: &mut AppUiManager,
        vlc_touch_active: &bool,
        vlc_drag_position: &mut Option<f64>,
        vlc_helper_stream: &mut Option<UnixStream>,
        needs_complete_redraw: &mut bool,
        cfg_enable_pixel_shift: bool,
    ) -> crate::MainResult<()> {
        if *vlc_touch_active && vlc_helper_stream.is_some() {
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
                    println!("[vlc_touch] VLC motion action detected: {:?}", app_action);
                    Self::handle_vlc_action_motion(
                        app_action, vlc_drag_position, vlc_helper_stream, needs_complete_redraw
                    )?;
                }
            }
        Ok(())
    }

    /// Handles VLC touch up events
    pub fn handle_touch_up(
        current_window_class: &Option<String>,
        app_ui_manager: &mut AppUiManager,
        vlc_touch_active: &mut bool,
        _vlc_drag_position: &mut Option<f64>,
        needs_complete_redraw: &mut bool,
    ) -> crate::MainResult<()> {
        if *vlc_touch_active {
            println!("[vlc_touch] VLC touch interaction ended");
            *vlc_touch_active = false;
            app_ui_manager.vlc_screen.reset_drag_state();
            *needs_complete_redraw = true;
            println!("[vlc_touch] VLC touch interaction ended, keeping drag position for smooth transition");
        }
        Ok(())
    }

    /// Handles VLC-specific actions
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
                println!("[vlc_touch] Executing VLC Next");
                *vlc_touch_active = true;
                if let Some(stream) = vlc_helper_stream {
                    Self::send_vlc_command(stream, "next")?;
                }
            }
            AppAction::Vlc(VlcAction::Previous) => {
                println!("[vlc_touch] Executing VLC Previous");
                *vlc_touch_active = true;
                if let Some(stream) = vlc_helper_stream {
                    Self::send_vlc_command(stream, "previous")?;
                }
            }
            AppAction::Vlc(VlcAction::Stop) => {
                println!("[vlc_touch] Executing VLC Stop");
                *vlc_touch_active = true;
                if let Some(stream) = vlc_helper_stream {
                    Self::send_vlc_command(stream, "stop")?;
                }
            }
            AppAction::Vlc(VlcAction::Raise) => {
                println!("[vlc_touch] Executing VLC Raise");
                *vlc_touch_active = true;
                if let Some(stream) = vlc_helper_stream {
                    Self::send_vlc_command(stream, "raise")?;
                }
            }
            AppAction::Vlc(VlcAction::Quit) => {
                println!("[vlc_touch] Executing VLC Quit");
                *vlc_touch_active = true;
                if let Some(stream) = vlc_helper_stream {
                    Self::send_vlc_command(stream, "quit")?;
                }
            }
            _ => {
                println!("[vlc_touch] Ignoring non-VLC action: {:?}", app_action);
            }
        }
        Ok(())
    }

    /// Handles VLC action motion events
    fn handle_vlc_action_motion(
        app_action: AppAction,
        vlc_drag_position: &mut Option<f64>,
        vlc_helper_stream: &mut Option<UnixStream>,
        needs_complete_redraw: &mut bool,
    ) -> crate::MainResult<()> {
        match app_action {
            AppAction::Vlc(VlcAction::Seek(position)) => {
                println!("[vlc_touch] VLC seek during motion to position: {}", position);
                if let Some(stream) = vlc_helper_stream {
                    let seek_command = format!("seek:{}", position);
                    Self::send_vlc_command(stream, &seek_command)?;
                }
            }
            AppAction::Vlc(VlcAction::DragHead(position)) => {
                println!("[vlc_touch] VLC drag head during motion to position: {}", position);
                *vlc_drag_position = Some(position);
                *needs_complete_redraw = true;
                if let Some(stream) = vlc_helper_stream {
                    let seek_command = format!("seek:{}", position);
                    Self::send_vlc_command(stream, &seek_command)?;
                }
            }
            _ => {
                println!("[vlc_touch] Ignoring non-seek VLC action during motion: {:?}", app_action);
            }
        }
        Ok(())
    }

    /// Sends VLC commands via the helper stream
    fn send_vlc_command(stream: &mut UnixStream, command: &str) -> Result<(), std::io::Error> {
        let command_with_newline = format!("{}\n", command);
        stream.write_all(command_with_newline.as_bytes())?;
        Ok(())
    }
} 