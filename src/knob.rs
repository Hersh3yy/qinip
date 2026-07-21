//! Circular EQ knob widget — matte black pot with knurled grip, specular
//! highlight, and a single white position line (Xone / CDJ hardware style).

use eframe::egui::{self, Color32, Pos2, Sense, Stroke, Vec2};

const SWEEP_START: f32 = std::f32::consts::FRAC_PI_2 + 0.75;
const SWEEP_RANGE: f32 = std::f32::consts::TAU * 0.75;

/// Circular drag knob for EQ gains in `0.0..=2.0` (1.0 = unity).
pub fn eq_knob(ui: &mut egui::Ui, value: &mut f32, accent: Color32) -> egui::Response {
    let size = Vec2::splat(48.0);
    let (rect, mut response) = ui.allocate_exact_size(size, Sense::click_and_drag());

    if response.secondary_clicked() {
        *value = 1.0;
        response.mark_changed();
    } else if response.dragged() || response.double_clicked() {
        apply_knob_input(&response, value);
        response.mark_changed();
    }

    response = response.on_hover_cursor(egui::CursorIcon::Grab);

    let center = rect.center();
    let radius = rect.width() * 0.40;
    let t = travel_fraction(*value);
    let pointer_angle = SWEEP_START + SWEEP_RANGE * t;

    let painter = ui.painter_at(rect);
    draw_base_accent(&painter, center, radius, accent, t);
    draw_knob_shadow(&painter, center, radius);
    draw_knob_body(&painter, center, radius);
    draw_specular_shine(&painter, center, radius);
    draw_grip_ribs(&painter, center, radius);
    draw_pointer(&painter, center, radius, pointer_angle);

    response.on_hover_text(
        "Drag ↑/↓ to adjust · Double-click or right-click = reset to 100%",
    )
}

fn travel_fraction(value: f32) -> f32 {
    value.clamp(0.0, 2.0) / 2.0
}

fn apply_knob_input(response: &egui::Response, value: &mut f32) {
    if response.double_clicked() {
        *value = 1.0;
        return;
    }
    if response.dragged() {
        let delta = response.drag_delta().y;
        *value = (*value - delta * 0.015).clamp(0.0, 2.0);
    }
}

fn draw_base_accent(painter: &egui::Painter, center: Pos2, radius: f32, accent: Color32, t: f32) {
    let base_radius = radius + 3.0;
    let steps = 32;
    for i in 0..steps {
        let a0 = SWEEP_START + SWEEP_RANGE * (i as f32 / steps as f32);
        let a1 = SWEEP_START + SWEEP_RANGE * ((i + 1) as f32 / steps as f32);
        let lit = (i as f32 / steps as f32) <= t;
        let color = if lit {
            accent
        } else {
            accent.gamma_multiply(0.22)
        };
        painter.line_segment(
            [polar(center, base_radius, a0), polar(center, base_radius, a1)],
            Stroke::new(2.5, color),
        );
    }
}

fn draw_knob_shadow(painter: &egui::Painter, center: Pos2, radius: f32) {
    let shadow_center = center + Vec2::new(1.5, 2.5);
    painter.circle_filled(shadow_center, radius + 1.0, Color32::from_rgba_unmultiplied(0, 0, 0, 80));
}

fn draw_knob_body(painter: &egui::Painter, center: Pos2, radius: f32) {
    // Layered circles simulate a convex cylinder: dark edge → mid → lighter cap.
    painter.circle_filled(center, radius, Color32::from_rgb(14, 14, 16));
    painter.circle_stroke(center, radius, Stroke::new(1.5, Color32::from_rgb(55, 55, 58)));

    let mid_r = radius * 0.88;
    painter.circle_filled(center, mid_r, Color32::from_rgb(24, 24, 26));

    let cap_r = radius * 0.72;
    painter.circle_filled(center, cap_r, Color32::from_rgb(32, 32, 35));

    // Subtle vertical gradient bands (top lighter, bottom darker).
    let bands = 10;
    for i in 0..bands {
        let band_t = i as f32 / bands as f32;
        let band_r = cap_r * (0.5 + band_t * 0.5);
        let shade = (22.0 + (1.0 - band_t) * 28.0) as u8;
        painter.circle_stroke(
            center,
            band_r,
            Stroke::new(1.0, Color32::from_gray(shade).gamma_multiply(0.4)),
        );
    }
}

/// Specular highlight on the upper-left — the "3D shine" that sells the analog look.
fn draw_specular_shine(painter: &egui::Painter, center: Pos2, radius: f32) {
    let hi = center + Vec2::new(-radius * 0.22, -radius * 0.30);
    painter.circle_filled(
        hi,
        radius * 0.32,
        Color32::from_rgba_unmultiplied(255, 255, 255, 38),
    );
    painter.circle_filled(
        hi + Vec2::new(radius * 0.06, radius * 0.08),
        radius * 0.10,
        Color32::from_rgba_unmultiplied(255, 255, 255, 55),
    );
}

fn draw_grip_ribs(painter: &egui::Painter, center: Pos2, radius: f32) {
    let ribs = 28;
    for i in 0..ribs {
        let angle = std::f32::consts::TAU * (i as f32 / ribs as f32);
        let inner = polar(center, radius * 0.78, angle);
        let outer = polar(center, radius * 0.97, angle);
        let shade = if i % 2 == 0 { 52 } else { 10 };
        painter.line_segment([inner, outer], Stroke::new(1.2, Color32::from_gray(shade)));
    }
}

fn draw_pointer(painter: &egui::Painter, center: Pos2, radius: f32, angle: f32) {
    let tip = polar(center, radius - 5.0, angle);
    painter.line_segment([center, tip], Stroke::new(2.8, Color32::WHITE));
    painter.circle_filled(tip, 2.2, Color32::WHITE);
    painter.circle_filled(center, 3.0, Color32::from_gray(200));
}

fn polar(center: Pos2, radius: f32, angle: f32) -> Pos2 {
    Pos2::new(
        center.x + radius * angle.cos(),
        center.y + radius * angle.sin(),
    )
}
