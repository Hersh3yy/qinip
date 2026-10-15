//! CDJ-style transport buttons — brushed-metal look with a colored glow ring.

use crate::theme::Palette;
use eframe::egui::{self, Align2, Color32, FontId, Pos2, Rect, RichText, Sense, Stroke, Ui, Vec2};

/// Which beat (1–4) within the current 4/4 bar, from playback position + BPM.
pub fn beat_in_bar(position_secs: f64, bpm: f32) -> u8 {
    if bpm <= 0.0 {
        return 0;
    }
    let beat_secs = 60.0 / bpm as f64;
    let beat_index = (position_secs / beat_secs).floor() as u64;
    ((beat_index % 4) + 1) as u8
}

/// CDJ-style bar position: four stacked ticks + beat number + BPM.
pub fn beat_bar_indicator(ui: &mut Ui, position_secs: f64, bpm: Option<f32>, palette: &Palette) {
    let accent = Color32::from_rgb(76, 201, 240);
    let dim = palette.tick_dim;
    let text = palette.text;
    let weak = palette.text_weak;
    let bar_w = 22.0;
    let bar_h = 5.0;
    let gap = 3.0;
    let tick_count = 4.0;
    let total_h: f32 = tick_count * bar_h + (tick_count - 1.0) * gap;

    ui.vertical(|ui| {
        ui.label(RichText::new("Bar").small().weak());
        ui.horizontal(|ui| {
            // Tall enough for the beat number above the BPM readout; ticks are
            // inset by 1px so the lit tick's outside stroke isn't clipped.
            let (rect, _) = ui.allocate_exact_size(
                Vec2::new(bar_w + 52.0, total_h.max(BUTTON_SIZE) + 2.0),
                Sense::hover(),
            );
            let tick_top = rect.top() + 1.0 + (rect.height() - 2.0 - total_h) * 0.5;
            let painter = ui.painter_at(rect);

            let Some(bpm) = bpm.filter(|b| *b > 0.0) else {
                for i in 0..4 {
                    let y = tick_top + i as f32 * (bar_h + gap);
                    let tick = Rect::from_min_size(
                        Pos2::new(rect.left() + 1.0, y),
                        Vec2::new(bar_w, bar_h),
                    );
                    painter.rect_filled(tick, 1.0, dim);
                }
                painter.text(
                    rect.left_center() + Vec2::new(bar_w + 10.0, 0.0),
                    Align2::LEFT_CENTER,
                    "—",
                    FontId::proportional(18.0),
                    weak,
                );
                return;
            };

            let current = beat_in_bar(position_secs, bpm);

            for beat in 1..=4u8 {
                let i = (beat - 1) as f32;
                let y = tick_top + i * (bar_h + gap);
                let tick = Rect::from_min_size(
                    Pos2::new(rect.left() + 1.0, y),
                    Vec2::new(bar_w, bar_h),
                );
                let lit = beat == current;
                let fill = if lit { accent } else { dim };
                painter.rect_filled(tick, 1.0, fill);
                if lit {
                    painter.rect_stroke(
                        tick.expand(1.0),
                        1.0,
                        Stroke::new(1.0, accent.gamma_multiply(0.5)),
                        egui::StrokeKind::Outside,
                    );
                }
            }

            let text_x = rect.left() + bar_w + 10.0;
            painter.text(
                Pos2::new(text_x, rect.center().y - 7.0),
                Align2::LEFT_CENTER,
                current.to_string(),
                FontId::proportional(18.0),
                text,
            );
            painter.text(
                Pos2::new(text_x, rect.center().y + 10.0),
                Align2::LEFT_CENTER,
                format!("{bpm:.1}"),
                FontId::monospace(13.0),
                // Bright cyan reads on dark panels; light panels need a deeper one.
                if palette.is_dark { accent } else { Color32::from_rgb(0x1F, 0x6F, 0x86) },
            );
        });
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
