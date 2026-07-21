//! CDJ-style transport buttons — brushed-metal look with a colored glow ring.

use eframe::egui::{self, Color32, Pos2, Sense, Stroke, Ui, Vec2};

/// Circular transport button. Returns `true` if clicked.
pub fn transport_button(
    ui: &mut Ui,
    label: &str,
    accent: Color32,
    enabled: bool,
    active: bool,
) -> bool {
    let size = Vec2::splat(40.0);
    let sense = if enabled {
        Sense::click()
    } else {
        Sense::hover()
    };
    let (rect, response) = ui.allocate_exact_size(size, sense);
    let clicked = response.clicked();

    if enabled {
        let _ = response.on_hover_cursor(egui::CursorIcon::PointingHand);
    }

    let painter = ui.painter_at(rect);
    let center = rect.center();
    let radius = rect.width() * 0.44;

    draw_glow_ring(&painter, center, radius, accent, active && enabled);
    draw_metal_cap(&painter, center, radius, enabled);
    draw_button_label(&painter, center, label, enabled);

    enabled && clicked
}

fn draw_glow_ring(painter: &egui::Painter, center: Pos2, radius: f32, accent: Color32, lit: bool) {
    let ring_r = radius + 3.5;
    let alpha = if lit { 220 } else { 90 };
    let color = accent.gamma_multiply(alpha as f32 / 255.0);
    painter.circle_stroke(center, ring_r, Stroke::new(2.5, color));
    if lit {
        painter.circle_stroke(center, ring_r + 1.5, Stroke::new(1.0, color.gamma_multiply(0.35)));
    }
}

/// Brushed-metal dome: radial highlight bands to suggest a convex surface.
fn draw_metal_cap(painter: &egui::Painter, center: Pos2, radius: f32, enabled: bool) {
    let base = if enabled {
        Color32::from_rgb(58, 60, 63)
    } else {
        Color32::from_rgb(38, 39, 41)
    };
    painter.circle_filled(center, radius, base);
    painter.circle_stroke(center, radius, Stroke::new(1.0, Color32::from_gray(18)));

    // Radial "brushed metal" bands — lighter toward top-left, darker bottom-right.
    let bands = 14;
    for i in 0..bands {
        let t = i as f32 / bands as f32;
        let band_r = radius * (0.25 + t * 0.72);
        let shade = (28.0 + (1.0 - t) * 55.0) as u8;
        let band_color = if enabled {
            Color32::from_gray(shade)
        } else {
            Color32::from_gray(shade / 2)
        };
        painter.circle_stroke(center, band_r, Stroke::new(1.2, band_color.gamma_multiply(0.55)));
    }

    // Specular highlight — the "3D shine" spot on the upper-left of the cap.
    let highlight = center + Vec2::new(-radius * 0.28, -radius * 0.32);
    let hi_alpha = if enabled { 70 } else { 30 };
    painter.circle_filled(
        highlight,
        radius * 0.28,
        Color32::from_rgba_unmultiplied(255, 255, 255, hi_alpha),
    );
    painter.circle_filled(
        highlight + Vec2::new(radius * 0.08, radius * 0.1),
        radius * 0.12,
        Color32::from_rgba_unmultiplied(255, 255, 255, hi_alpha / 2),
    );
}

fn draw_button_label(painter: &egui::Painter, center: Pos2, label: &str, enabled: bool) {
    let color = if enabled {
        Color32::WHITE
    } else {
        Color32::from_gray(100)
    };
    painter.text(
        center,
        egui::Align2::CENTER_CENTER,
        label,
        egui::FontId::proportional(13.0),
        color,
    );
}
