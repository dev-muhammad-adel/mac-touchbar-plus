//! UI logic for the module selection and interaction screen.
use cairo::Context;



pub fn draw_module_screen(
    c: &Context,
    x: f64,
    y: f64,
    width: f64,
    height: f64,
    radius: f64,
    _area_height: i32,
    _complete_redraw: bool,
    window_class: &str,
    anim_progress: f64, // 0.0 = transparent, 1.0 = opaque
    show_pill_background: bool, // Whether to show the pill background or just text
) {
    // --- Modern pill background (rounded rectangle) ---
    let pill_x = x;
    let pill_y = y - radius;
    let pill_w = width;
    let pill_h = height + radius * 2.0;
    
    // Always clear the area first to prevent text overlap
    c.save().unwrap();
    c.set_source_rgb(0.0, 0.0, 0.0);
    c.rectangle(pill_x, pill_y, pill_w, pill_h);
    c.fill().unwrap();
    c.restore().unwrap();
    
    // Draw pill background if requested
    if show_pill_background {
        c.save().unwrap();
        c.set_source_rgba(0.0, 0.0, 0.0, anim_progress); // Inactive button color, faded in
        // Draw rounded rectangle (pill)
        c.new_sub_path();
        c.arc(pill_x + pill_w - radius, pill_y + radius, radius, (-90.0f64).to_radians(), (0.0f64).to_radians());
        c.arc(pill_x + pill_w - radius, pill_y + pill_h - radius, radius, (0.0f64).to_radians(), (90.0f64).to_radians());
        c.arc(pill_x + radius, pill_y + pill_h - radius, radius, (90.0f64).to_radians(), (180.0f64).to_radians());
        c.arc(pill_x + radius, pill_y + radius, radius, (180.0f64).to_radians(), (270.0f64).to_radians());
        c.close_path();
        c.fill().unwrap();
        c.restore().unwrap();
    }

    // --- Centered window class text ---
    c.save().unwrap();
    // Font size logic similar to login screen
    let text_size = (pill_h * 0.38).min(22.0).max(14.0);
    c.set_font_size(text_size);
    c.select_font_face("Sans", cairo::FontSlant::Normal, cairo::FontWeight::Bold);
    
    // Handle empty window class (logout state)
    let display_text = if window_class.is_empty() { "No Active Window" } else { window_class };
    
    let ext = c.text_extents(display_text).unwrap();
    let group_h = ext.height();
    let group_w = ext.width();
    let group_y = pill_y + (pill_h - group_h) / 2.0;
    let group_x = pill_x + (pill_w - group_w) / 2.0;
    c.set_source_rgba(1.0, 1.0, 1.0, anim_progress); // White text, faded in
    c.move_to(group_x, group_y + group_h - 2.0);
    c.show_text(display_text).unwrap();
    c.restore().unwrap();
}

 