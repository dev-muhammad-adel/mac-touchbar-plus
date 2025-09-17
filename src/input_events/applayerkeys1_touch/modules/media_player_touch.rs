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
use crate::view::media_player_screen::MediaPlayerAction;
use crate::view::spotify_screen::SpotifyAction;
use crate::display::pixel_shift::PIXEL_SHIFT_WIDTH_PX;
use crate::layers::function_layer::BUTTON_SPACING_PX;

use std::collections::HashMap;
use crate::LayerKey;
use crate::layers::FunctionLayer;

// Static HashMap for Media Player touch slots
static mut MEDIA_PLAYER_TOUCHES: Option<HashMap<u32, (LayerKey, &'static str, usize)>> = None;

pub struct MediaPlayerTouchHandler;

impl MediaPlayerTouchHandler {
    fn get_touches() -> &'static mut HashMap<u32, (LayerKey, &'static str, usize)> {
        unsafe {
            if MEDIA_PLAYER_TOUCHES.is_none() {
                MEDIA_PLAYER_TOUCHES = Some(HashMap::new());
            }
            MEDIA_PLAYER_TOUCHES.as_mut().unwrap()
        }
    }

    /// Handles all Media Player touch events
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
        
        if let Event::Touch(te) = event {
            match te {
                TouchEvent::Down(dn) => {
                    let x = dn.x_transformed(width);
                    let y = dn.y_transformed(height);
                    
                    let available_mpris_services = &app_ui_manager.generic_background_screen.available_mpris_services;
                    if let Some((group, idx)) = layers.get_mut(active_layer).ok_or(crate::MainError::LayerNotFound(*active_layer))?.hit_test(x, width as i32, Some(active_layer.clone()), available_mpris_services) {
                        if group == "modules" {
                            touches.insert(dn.seat_slot(), (active_layer.clone(), group, idx));
                            
                            // Delegate to Media Player touch handler
                            Self::handle_touch_down(
                                x, y, width, height, current_window_class, app_ui_manager,
                                media_player_touch_active, media_player_drag_position, media_player_helper_stream,
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
                        
                        // Delegate to Media Player touch handler
                        Self::handle_touch_motion(
                            x, y, width, height, current_window_class, app_ui_manager,
                            media_player_touch_active, media_player_drag_position, media_player_helper_stream,
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
                        // Delegate to Media Player touch handler
                        Self::handle_touch_up(
                            current_window_class, app_ui_manager, media_player_touch_active,
                            media_player_drag_position, needs_complete_redraw
                        )?;
                        
                        // Remove touch slot - Media Player handler manages its own slots
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

    /// Handles Media Player touch down events
    pub fn handle_touch_down(
        x: f64,
        y: f64,
        width: u32,
        height: u32,
        current_window_class: &Option<String>,
        app_ui_manager: &mut AppUiManager,
        media_player_touch_active: &mut bool,
        media_player_drag_position: &mut Option<f64>,
        media_player_helper_stream: &mut Option<UnixStream>,
        needs_complete_redraw: &mut bool,
        cfg_enable_pixel_shift: bool,
    ) -> crate::MainResult<()> {
        if media_player_helper_stream.is_some() {
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
                    println!("[media_player_touch] Media Player action detected: {:?}", app_action);
                    Self::handle_media_player_action(
                        app_action, app_ui_manager, media_player_touch_active,
                        media_player_drag_position, media_player_helper_stream, needs_complete_redraw
                    )?;
                }
            }
        Ok(())
    }

    /// Handles Media Player touch motion events
    pub fn handle_touch_motion(
        x: f64,
        y: f64,
        width: u32,
        height: u32,
        current_window_class: &Option<String>,
        app_ui_manager: &mut AppUiManager,
        media_player_touch_active: &bool,
        media_player_drag_position: &mut Option<f64>,
        media_player_helper_stream: &mut Option<UnixStream>,
        needs_complete_redraw: &mut bool,
        cfg_enable_pixel_shift: bool,
    ) -> crate::MainResult<()> {
        if *media_player_touch_active && media_player_helper_stream.is_some() {
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
                    println!("[media_player_touch] Media Player motion action detected: {:?}", app_action);
                    Self::handle_media_player_action_motion(
                        app_action, media_player_drag_position, media_player_helper_stream, needs_complete_redraw
                    )?;
                }
            }
        Ok(())
    }

    /// Handles Media Player touch up events
    pub fn handle_touch_up(
        current_window_class: &Option<String>,
        app_ui_manager: &mut AppUiManager,
        media_player_touch_active: &mut bool,
        _media_player_drag_position: &mut Option<f64>,
        needs_complete_redraw: &mut bool,
    ) -> crate::MainResult<()> {
        if *media_player_touch_active {
            println!("[media_player_touch] Media Player touch interaction ended");
            *media_player_touch_active = false;
            app_ui_manager.media_player_screen.reset_drag_state();
            *needs_complete_redraw = true;
            println!("[media_player_touch] Media Player touch interaction ended, keeping drag position for smooth transition");
        }
        Ok(())
    }

    /// Handles Media Player-specific actions
    fn handle_media_player_action(
        app_action: AppAction,
        _app_ui_manager: &mut AppUiManager,
        media_player_touch_active: &mut bool,
        media_player_drag_position: &mut Option<f64>,
        media_player_helper_stream: &mut Option<UnixStream>,
        needs_complete_redraw: &mut bool,
    ) -> crate::MainResult<()> {
        match app_action {
            AppAction::MediaPlayer(MediaPlayerAction::TogglePlayPause) => {
                *media_player_touch_active = true;
                if let Some(stream) = media_player_helper_stream {
                    Self::send_media_player_command(stream, "play_pause")?;
                }
            }
            AppAction::MediaPlayer(MediaPlayerAction::Seek(position)) => {
                *media_player_touch_active = true;
                if let Some(stream) = media_player_helper_stream {
                    let seek_command = format!("seek:{}", position);
                    Self::send_media_player_command(stream, &seek_command)?;
                }
            }
            AppAction::MediaPlayer(MediaPlayerAction::DragHead(position)) => {
                *media_player_touch_active = true;
                *media_player_drag_position = Some(position);
                *needs_complete_redraw = true;
                
                static mut LAST_SEEK_POSITION: f64 = 0.0;
                unsafe {
                    if (position - LAST_SEEK_POSITION).abs() > 0.01 {
                        LAST_SEEK_POSITION = position;
                        if let Some(stream) = media_player_helper_stream {
                            let seek_command = format!("seek:{}", position);
                            Self::send_media_player_command(stream, &seek_command)?;
                        }
                    }
                }
            }
            AppAction::MediaPlayer(MediaPlayerAction::Next) => {
                println!("[media_player_touch] Executing Media Player Next");
                *media_player_touch_active = true;
                if let Some(stream) = media_player_helper_stream {
                    Self::send_media_player_command(stream, "next")?;
                }
            }
            AppAction::MediaPlayer(MediaPlayerAction::Previous) => {
                println!("[media_player_touch] Executing Media Player Previous");
                *media_player_touch_active = true;
                if let Some(stream) = media_player_helper_stream {
                    Self::send_media_player_command(stream, "previous")?;
                }
            }
            AppAction::MediaPlayer(MediaPlayerAction::Stop) => {
                println!("[media_player_touch] Executing Media Player Stop");
                *media_player_touch_active = true;
                if let Some(stream) = media_player_helper_stream {
                    Self::send_media_player_command(stream, "stop")?;
                }
            }
            AppAction::MediaPlayer(MediaPlayerAction::Raise) => {
                println!("[media_player_touch] Executing Media Player Raise");
                *media_player_touch_active = true;
                if let Some(stream) = media_player_helper_stream {
                    Self::send_media_player_command(stream, "raise")?;
                }
            }
            AppAction::MediaPlayer(MediaPlayerAction::Quit) => {
                println!("[media_player_touch] Executing Media Player Quit");
                *media_player_touch_active = true;
                if let Some(stream) = media_player_helper_stream {
                    Self::send_media_player_command(stream, "quit")?;
                }
            }
            AppAction::Spotify(SpotifyAction::TogglePlayPause) => {
                *media_player_touch_active = true;
                if let Some(stream) = media_player_helper_stream {
                    Self::send_media_player_command(stream, "play_pause")?;
                }
            }
            AppAction::Spotify(SpotifyAction::Seek(position)) => {
                *media_player_touch_active = true;
                if let Some(stream) = media_player_helper_stream {
                    let seek_command = format!("seek:{}", position);
                    Self::send_media_player_command(stream, &seek_command)?;
                }
            }
            AppAction::Spotify(SpotifyAction::DragHead(position)) => {
                *media_player_touch_active = true;
                *media_player_drag_position = Some(position);
                *needs_complete_redraw = true;
                
                static mut LAST_SEEK_POSITION: f64 = 0.0;
                unsafe {
                    if (position - LAST_SEEK_POSITION).abs() > 0.01 {
                        LAST_SEEK_POSITION = position;
                        if let Some(stream) = media_player_helper_stream {
                            let seek_command = format!("seek:{}", position);
                            Self::send_media_player_command(stream, &seek_command)?;
                        }
                    }
                }
            }
            AppAction::Spotify(SpotifyAction::Next) => {
                println!("[media_player_touch] Executing Spotify Next");
                *media_player_touch_active = true;
                if let Some(stream) = media_player_helper_stream {
                    Self::send_media_player_command(stream, "next")?;
                }
            }
            AppAction::Spotify(SpotifyAction::Previous) => {
                println!("[media_player_touch] Executing Spotify Previous");
                *media_player_touch_active = true;
                if let Some(stream) = media_player_helper_stream {
                    Self::send_media_player_command(stream, "previous")?;
                }
            }
            AppAction::Spotify(SpotifyAction::Stop) => {
                println!("[media_player_touch] Executing Spotify Stop");
                *media_player_touch_active = true;
                if let Some(stream) = media_player_helper_stream {
                    Self::send_media_player_command(stream, "stop")?;
                }
            }
            AppAction::Spotify(SpotifyAction::Raise) => {
                println!("[media_player_touch] Executing Spotify Raise");
                *media_player_touch_active = true;
                if let Some(stream) = media_player_helper_stream {
                    Self::send_media_player_command(stream, "raise")?;
                }
            }
            AppAction::Spotify(SpotifyAction::Quit) => {
                println!("[media_player_touch] Executing Spotify Quit");
                *media_player_touch_active = true;
                if let Some(stream) = media_player_helper_stream {
                    Self::send_media_player_command(stream, "quit")?;
                }
            }
            _ => {
                println!("[media_player_touch] Ignoring non-Media Player/Spotify action: {:?}", app_action);
            }
        }
        Ok(())
    }

    /// Handles Media Player action motion events
    fn handle_media_player_action_motion(
        app_action: AppAction,
        media_player_drag_position: &mut Option<f64>,
        media_player_helper_stream: &mut Option<UnixStream>,
        needs_complete_redraw: &mut bool,
    ) -> crate::MainResult<()> {
        match app_action {
            AppAction::MediaPlayer(MediaPlayerAction::Seek(position)) => {
                println!("[media_player_touch] Media Player seek during motion to position: {}", position);
                if let Some(stream) = media_player_helper_stream {
                    let seek_command = format!("seek:{}", position);
                    Self::send_media_player_command(stream, &seek_command)?;
                }
            }
            AppAction::MediaPlayer(MediaPlayerAction::DragHead(position)) => {
                println!("[media_player_touch] Media Player drag head during motion to position: {}", position);
                *media_player_drag_position = Some(position);
                *needs_complete_redraw = true;
                if let Some(stream) = media_player_helper_stream {
                    let seek_command = format!("seek:{}", position);
                    Self::send_media_player_command(stream, &seek_command)?;
                }
            }
            AppAction::Spotify(SpotifyAction::Seek(position)) => {
                println!("[media_player_touch] Spotify seek during motion to position: {}", position);
                if let Some(stream) = media_player_helper_stream {
                    let seek_command = format!("seek:{}", position);
                    Self::send_media_player_command(stream, &seek_command)?;
                }
            }
            AppAction::Spotify(SpotifyAction::DragHead(position)) => {
                println!("[media_player_touch] Spotify drag head during motion to position: {}", position);
                *media_player_drag_position = Some(position);
                *needs_complete_redraw = true;
                if let Some(stream) = media_player_helper_stream {
                    let seek_command = format!("seek:{}", position);
                    Self::send_media_player_command(stream, &seek_command)?;
                }
            }
            _ => {
                println!("[media_player_touch] Ignoring non-seek Media Player/Spotify action during motion: {:?}", app_action);
            }
        }
        Ok(())
    }

    /// Sends Media Player commands via the helper stream
    fn send_media_player_command(stream: &mut UnixStream, command: &str) -> Result<(), std::io::Error> {
        let command_with_newline = format!("{}\n", command);
        stream.write_all(command_with_newline.as_bytes())?;
        Ok(())
    }
} 