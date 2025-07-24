//! UI logic for the media control screen.
use cairo::Context;
use drm::control::ClipRect;
use crate::services::sessionmanager::SessionState;
use crate::config::Config;
use input_linux::Key;

const BUTTON_COLOR_INACTIVE: f64 = 0.350;
const BUTTON_COLOR_ACTIVE: f64 = 0.600;

pub fn draw_media_section(
    c: &Context,
    media_buttons: &mut [crate::Button],
    media_button_widths: &[f64],
    _media_width: f64,
    media_count: usize,
    mut left_edge: f64,
    bot: f64,
    top: f64,
    radius: f64,
    height: i32,
    config: &Config,
    complete_redraw: bool,
    modified_regions: &mut Vec<ClipRect>,
    _session_state: Option<&SessionState>,
) {
    let media_spacing_px = 2.0f64;
    for (i, button) in media_buttons.iter_mut().enumerate() {
        if button.changed || complete_redraw {
            let color = if button.active {
                BUTTON_COLOR_ACTIVE
            } else if config.show_button_outlines {
                BUTTON_COLOR_INACTIVE
            } else {
                0.0
            };
            // Ensure macro buttons always have background
            if matches!(button.action, Key::Macro1 | Key::Macro2 | Key::Macro3 | Key::Macro4) {
                button.background = true;
            }
            let this_button_width = media_button_widths[i];
            let is_first = i == 0;
            let is_last = i == media_count - 1;
            let x = left_edge;
            let y = bot - radius;
            let w = this_button_width;
            let h = top - bot + radius * 2.0;
            let r = radius.min(h / 2.0);
            if (button.action != Key::Unknown) && (button.background || button.active) {
                c.set_source_rgb(color, color, color);
                if media_count == 1 {
                    // Single button: all corners rounded
                    c.new_sub_path();
                    c.arc(x + w - r, y + r, r, (270.0f64).to_radians(), (360.0f64).to_radians());
                    c.arc(x + w - r, y + h - r, r, (0.0f64).to_radians(), (90.0f64).to_radians());
                    c.arc(x + r, y + h - r, r, (90.0f64).to_radians(), (180.0f64).to_radians());
                    c.arc(x + r, y + r, r, (180.0f64).to_radians(), (270.0f64).to_radians());
                    c.close_path();
                    c.fill().unwrap();
                } else {
                    if is_first && is_last {
                        // Single button in group: all corners rounded
                        c.new_sub_path();
                        c.arc(x + w - r, y + r, r, (270.0f64).to_radians(), (360.0f64).to_radians());
                        c.arc(x + w - r, y + h - r, r, (0.0f64).to_radians(), (90.0f64).to_radians());
                        c.arc(x + r, y + h - r, r, (90.0f64).to_radians(), (180.0f64).to_radians());
                        c.arc(x + r, y + r, r, (180.0f64).to_radians(), (270.0f64).to_radians());
                        c.close_path();
                        c.fill().unwrap();
                    } else if is_first {
                        // First button: left corners rounded
                        c.new_sub_path();
                        c.move_to(x + r, y);
                        c.line_to(x + w, y);
                        c.line_to(x + w, y + h);
                        c.line_to(x + r, y + h);
                        c.arc(x + r, y + h - r, r, (90.0f64).to_radians(), (180.0f64).to_radians());
                        c.line_to(x, y + r);
                        c.arc(x + r, y + r, r, (180.0f64).to_radians(), (270.0f64).to_radians());
                        c.close_path();
                        c.fill().unwrap();
                    } else if is_last {
                        // Last button: right corners rounded
                        c.new_sub_path();
                        c.move_to(x, y);
                        c.line_to(x + w - r, y);
                        c.arc(x + w - r, y + r, r, (270.0f64).to_radians(), (360.0f64).to_radians());
                        c.line_to(x + w, y + h - r);
                        c.arc(x + w - r, y + h - r, r, (0.0f64).to_radians(), (90.0f64).to_radians());
                        c.line_to(x, y + h);
                        c.close_path();
                        c.fill().unwrap();
                    } else {
                        // Middle buttons: no rounded corners
                        c.rectangle(x, y, w, h);
                        c.fill().unwrap();
                    }
                }
            }
            // For macro buttons, always draw background with proper rounded corners
            if matches!(button.action, Key::Macro1 | Key::Macro2 | Key::Macro3 | Key::Macro4) && (button.background || button.active) {
                c.set_source_rgb(color, color, color);
                if is_first {
                    // First macro button: left corners rounded
                    c.new_sub_path();
                    c.move_to(x + r, y);
                    c.line_to(x + w, y);
                    c.line_to(x + w, y + h);
                    c.line_to(x + r, y + h);
                    c.arc(x + r, y + h - r, r, (90.0f64).to_radians(), (180.0f64).to_radians());
                    c.line_to(x, y + r);
                    c.arc(x + r, y + r, r, (180.0f64).to_radians(), (270.0f64).to_radians());
                    c.close_path();
                    c.fill().unwrap();
                } else if is_last {
                    // Last macro button: right corners rounded
                    c.new_sub_path();
                    c.move_to(x, y);
                    c.line_to(x + w - r, y);
                    c.arc(x + w - r, y + r, r, (270.0f64).to_radians(), (360.0f64).to_radians());
                    c.line_to(x + w, y + h - r);
                    c.arc(x + w - r, y + h - r, r, (0.0f64).to_radians(), (90.0f64).to_radians());
                    c.line_to(x, y + h);
                    c.close_path();
                    c.fill().unwrap();
                } else {
                    // Middle macro buttons: no rounded corners
                    c.rectangle(x, y, w, h);
                    c.fill().unwrap();
                }
            }
            c.set_source_rgb(1.0, 1.0, 1.0);
            button.render(c, height, left_edge, this_button_width.ceil() as u64, 0.0);
            button.changed = false;
            if !complete_redraw {
                modified_regions.push(ClipRect::new(
                    height as u16 - top as u16 - radius as u16,
                    left_edge as u16,
                    height as u16 - bot as u16 + radius as u16,
                    left_edge as u16 + this_button_width as u16
                ));
            }
        }
        // Always update left_edge
        left_edge += media_button_widths[i];
        if i != media_count - 1 {
            left_edge += media_spacing_px;
        }
    }
}

pub fn media_hit_test(
    x: f64,
    left_edge: f64,
    media_button_widths: &[f64],
    media_count: usize,
) -> Option<usize> {
    let mut current_left = left_edge;
    for i in 0..media_count {
        let right = current_left + media_button_widths[i];
        if x >= current_left && x < right {
            return Some(i);
        }
        current_left = right;
        if i != media_count - 1 {
            current_left += 2.0; // media_spacing_px
        }
    }
    None
} 