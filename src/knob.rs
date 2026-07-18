use eframe::egui::{self, Color32, Pos2, Sense, Stroke, Vec2};

/// Circular drag knob for EQ gains in `0.0..=2.0` (1.0 = unity).
pub fn eq_knob(ui: &mut egui::Ui, value: &mut f32, color: Color32) -> egui::Response {
    let size = Vec2::splat(44.0);
    let (rect, mut response) = ui.allocate_exact_size(size, Sense::click_and_drag());
    let painter = ui.painter_at(rect);

    let center = rect.center();
    let radius = rect.width() * 0.42;

    // Map value 0..2 → sweep angle (about 270° of travel).
    let t = ((*value).clamp(0.0, 2.0) / 2.0) as f32;
    let start = std::f32::consts::FRAC_PI_2 + 0.75; // lower-left
    let sweep = std::f32::consts::TAU * 0.75;
    let angle = start + sweep * t;

    painter.circle_filled(center, radius, Color32::from_gray(40));
    painter.circle_stroke(center, radius, Stroke::new(2.0, color));

    // Arc hint of current value.
    let arc_steps = 24;
    for i in 0..arc_steps {
        let a0 = start + sweep * (i as f32 / arc_steps as f32) * t;
        let a1 = start + sweep * ((i + 1) as f32 / arc_steps as f32) * t;
        let p0 = polar(center, radius - 3.0, a0);
        let p1 = polar(center, radius - 3.0, a1);
        painter.line_segment([p0, p1], Stroke::new(3.0, color.gamma_multiply(0.85)));
    }

    let tip = polar(center, radius - 6.0, angle);
    painter.line_segment([center, tip], Stroke::new(2.5, Color32::WHITE));
    painter.circle_filled(center, 3.5, Color32::WHITE);

    if response.dragged() {
        let delta = response.drag_delta().y;
        // Drag up = increase, drag down = decrease.
        *value = (*value - delta * 0.015).clamp(0.0, 2.0);
        response.mark_changed();
    }

    if response.double_clicked() {
        *value = 1.0;
        response.mark_changed();
    }

    response.on_hover_text(format!("{:.0}%  (drag ↑/↓, double-click = reset)", *value * 100.0))
}

fn polar(center: Pos2, radius: f32, angle: f32) -> Pos2 {
    Pos2::new(
        center.x + radius * angle.cos(),
        center.y + radius * angle.sin(),
    )
}
