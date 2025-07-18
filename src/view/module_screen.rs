use cairo::Context;
use drm::control::ClipRect;
use crate::services::sessionmanager::SessionState;
use crate::config::Config;
use crate::Key;

const BUTTON_COLOR_ACTIVE: f64 = 0.600;
const BUTTON_COLOR_INACTIVE: f64 = 0.350;

/// Draws the modules section of the app.
pub fn draw_module_section(
    c: &Context,
    split_modules: &mut [crate::Button],
    modules_button_widths: &[f64],
    modules_width: f64,
    modules_count: usize,
    left_edge: f64,
    bot: f64,
    top: f64,
    radius: f64,
    height: i32,
    config: &Config,
    complete_redraw: bool,
    modified_regions: &mut Vec<ClipRect>,
    session_state: Option<&SessionState>,
) {
    let mut local_left_edge = left_edge;
    for (i, button) in split_modules.iter_mut().enumerate() {
        let this_button_width = modules_button_widths[i];
        if button.changed || complete_redraw {
            let color = if button.active {
                BUTTON_COLOR_ACTIVE
            } else if config.show_button_outlines {
                BUTTON_COLOR_INACTIVE
            } else {
                0.0
            };
            if !complete_redraw {
                c.set_source_rgb(0.0, 0.0, 0.0);
                c.rectangle(local_left_edge, bot - radius, this_button_width, top - bot + radius * 2.0);
                c.fill().unwrap();
            }
            if (button.action != Key::Unknown && button.action != Key::Time && button.action != Key::Macro1 && button.action != Key::Macro2 && button.action != Key::Macro3 && button.action != Key::Macro4) && (button.background || button.active) {
                c.set_source_rgb(color, color, color);
                c.new_sub_path();
                let left = local_left_edge + radius;
                let right = (local_left_edge + this_button_width.ceil()) - radius;
                c.arc(right, bot, radius, (-90.0f64).to_radians(), (0.0f64).to_radians());
                c.arc(right, top, radius, (0.0f64).to_radians(), (90.0f64).to_radians());
                c.arc(left, top, radius, (90.0f64).to_radians(), (180.0f64).to_radians());
                c.arc(left, bot, radius, (180.0f64).to_radians(), (270.0f64).to_radians());
                c.close_path();
                c.fill().unwrap();
            }
            c.set_source_rgb(1.0, 1.0, 1.0);
            button.render(c, height, local_left_edge, this_button_width.ceil() as u64, 0.0);
            // Show current user info and status on first module if available

            button.changed = false;
            if !complete_redraw {
                modified_regions.push(ClipRect::new(
                    height as u16 - top as u16 - radius as u16,
                    local_left_edge as u16,
                    height as u16 - bot as u16 + radius as u16,
                    local_left_edge as u16 + this_button_width as u16
                ));
            }
        }
        local_left_edge += this_button_width;
        if i != modules_count - 1 {
            local_left_edge += crate::BUTTON_SPACING_PX as f64;
        }
    }
} 