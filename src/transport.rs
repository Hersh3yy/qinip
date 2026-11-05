//! CDJ-style transport buttons — brushed-metal look with a colored glow ring.

use crate::theme::Palette;
use eframe::egui::{self, Align2, Color32, FontId, Pos2, RichText, Sense, Stroke, Ui, Vec2};

/// Fraction of the way into the current beat (0 = on the beat), counting
/// beats from 0:00 at the given tempo.
pub fn beat_phase(position_secs: f64, bpm: f32) -> f32 {
    if bpm <= 0.0 || !position_secs.is_finite() {
        return 0.0;
    }
    let beat_secs = 60.0 / bpm as f64;
    (position_secs.max(0.0) / beat_secs).fract() as f32
}

/// Tempo readout: a light that pulses on every beat, plus the BPM.
///
/// It counts beats from 0:00, so it shows the tempo, not where beat 1 of a
/// bar falls; that needs the beat grid (roadmap f7). The earlier 1-2-3-4 bar
/// counter claimed more than it knew and drifted on most tracks.
pub fn tempo_indicator(ui: &mut Ui, position_secs: f64, bpm: Option<f32>, palette: &Palette) {
    let accent = Color32::from_rgb(76, 201, 240);
    // Bright cyan reads on dark panels; light panels need a deeper one.
    let bpm_color = if palette.is_dark {
        accent
    } else {
        Color32::from_rgb(0x1F, 0x6F, 0x86)
    };

    ui.vertical(|ui| {
        ui.label(RichText::new("Tempo").small().weak());
        let (rect, _) = ui.allocate_exact_size(Vec2::new(96.0, BUTTON_SIZE), Sense::hover());
        let painter = ui.painter_at(rect);
        let light = Pos2::new(rect.left() + 12.0, rect.center().y);
        let text_at = Pos2::new(rect.left() + 30.0, rect.center().y);

        let Some(bpm) = bpm.filter(|b| *b > 0.0) else {
            painter.circle_filled(light, 8.0, palette.tick_dim);
            painter.text(
                text_at,
                Align2::LEFT_CENTER,
                "—",
                FontId::proportional(18.0),
                palette.text_weak,
            );
            return;
        };

        // Full brightness on the beat, fading out before the next one.
        let glow = (1.0 - beat_phase(position_secs, bpm)).powi(3);
        painter.circle_filled(light, 8.0, palette.tick_dim);
        painter.circle_filled(light, 8.0, accent.gamma_multiply(0.2 + 0.8 * glow));
        if glow > 0.4 {
            painter.circle_stroke(
                light,
                10.0 + 4.0 * glow,
                Stroke::new(1.5, accent.gamma_multiply(0.6 * glow)),
            );
        }
        painter.text(
            text_at,
            Align2::LEFT_CENTER,
            format!("{bpm:.1}"),
            FontId::monospace(16.0),
            bpm_color,
        );
    });
}

/// Outer size of a transport button, glow ring included.
const BUTTON_SIZE: f32 = 36.0;
/// Room the glow ring needs outside the cap: ring gap (3.5) + lit halo (1.5)
/// + half the halo stroke (0.5), so the lit ring ends exactly at the edge.
const RING_ALLOWANCE: f32 = 5.5;

/// Circular transport button. Returns `true` if clicked.
pub fn transport_button(
    ui: &mut Ui,
    label: &str,
    accent: Color32,
    enabled: bool,
    active: bool,
) -> bool {
    let size = Vec2::splat(BUTTON_SIZE);
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
    // Everything drawn must stay inside `rect` (painter_at clips to it), so
    // the cap radius leaves room for the glow ring around it.
    let radius = rect.width() * 0.5 - RING_ALLOWANCE;

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
        egui::FontId::proportional(11.5),
        color,
    );
}

#[cfg(test)]
mod tests {
    use super::beat_phase;

    #[test]
    fn phase_counts_from_zero_at_the_tempo() {
        assert_eq!(beat_phase(0.0, 120.0), 0.0);
        assert!((beat_phase(0.25, 120.0) - 0.5).abs() < 1e-6);
        assert!(beat_phase(1.0, 120.0).abs() < 1e-5);
        assert_eq!(beat_phase(3.0, 0.0), 0.0);
        assert_eq!(beat_phase(f64::NAN, 120.0), 0.0);
    }
}
