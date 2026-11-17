//! Colour glue between Qinip and Rune.
//!
//! Qinip's `Palette` (src/theme.rs) stores colours as egui `Color32` bytes. Rune wants its
//! own `Color` (sRGB floats, 0..1). `Ink` is the palette converted once per theme switch, and
//! `kit_for` derives rune-kit's token set (buttons, table) from the same seed colours, so the
//! stock components match the hand-drawn parts.

use qinip::eq::BAND_COLORS;
use qinip::theme::Palette;
use rune::Color;
use rune::color::srgb_decode;
use rune_kit::{Kit, PaletteSeed};

/// A Rune colour from the `[r, g, b, a]` bytes egui's `Color32::to_array` gives. Rune's
/// `Color` is linear light, so the sRGB bytes are decoded (as `rune::color::hex` does).
pub fn rgba(bytes: [u8; 4]) -> Color {
    let f = |v: u8| srgb_decode(v as f32 / 255.0);
    Color { r: f(bytes[0]), g: f(bytes[1]), b: f(bytes[2]), a: bytes[3] as f32 / 255.0 }
}

pub fn rgb(r: u8, g: u8, b: u8) -> Color {
    rgba([r, g, b, 255])
}

/// The waveform colour of band `i` (0 Low, 1 LowMid, 2 HiMid, 3 Hi) at the given opacity.
pub fn band(i: usize, alpha: f32) -> Color {
    let (r, g, b) = BAND_COLORS[i.min(3)];
    rgb(r, g, b).alpha(alpha)
}

/// Qinip's palette as Rune colours. The field names follow `qinip::theme::Palette`
/// (`is_dark` and `on_accent` are left out until something here reads them).
#[derive(Clone, Copy, Debug)]
pub struct Ink {
    pub panel: Color,
    pub recessed: Color,
    pub control: Color,
    pub control_hover: Color,
    pub accent: Color,
    pub accent_text: Color,
    pub text: Color,
    pub text_weak: Color,
    pub row_bg: Color,
    pub row_text: Color,
    pub fill_failed: Color,
    pub fill_done: Color,
    pub fill_busy: Color,
    pub fill_pending: Color,
    pub value_failed: Color,
    pub value_done: Color,
    pub value_busy: Color,
    pub value_pending: Color,
    pub screen_bg: Color,
    pub playhead: Color,
    pub tick_dim: Color,
}

impl Ink {
    pub fn from_palette(p: &Palette) -> Ink {
        Ink {
            panel: rgba(p.panel.to_array()),
            recessed: rgba(p.recessed.to_array()),
            control: rgba(p.control.to_array()),
            control_hover: rgba(p.control_hover.to_array()),
            accent: rgba(p.accent.to_array()),
            accent_text: rgba(p.accent_text.to_array()),
            text: rgba(p.text.to_array()),
            text_weak: rgba(p.text_weak.to_array()),
            row_bg: rgba(p.row_bg.to_array()),
            row_text: rgba(p.row_text.to_array()),
            fill_failed: rgba(p.fill_failed.to_array()),
            fill_done: rgba(p.fill_done.to_array()),
            fill_busy: rgba(p.fill_busy.to_array()),
            fill_pending: rgba(p.fill_pending.to_array()),
            value_failed: rgba(p.value_failed.to_array()),
            value_done: rgba(p.value_done.to_array()),
            value_busy: rgba(p.value_busy.to_array()),
            value_pending: rgba(p.value_pending.to_array()),
            screen_bg: rgba(p.screen_bg.to_array()),
            playhead: rgba(p.playhead.to_array()),
            tick_dim: rgba(p.tick_dim.to_array()),
        }
    }
}

/// rune-kit's semantic tokens, seeded from the palette so buttons and the table fit in.
pub fn kit_for(ink: &Ink) -> Kit {
    let mut k = Kit::from_palette(PaletteSeed { bg: ink.panel, fg: ink.text, accent: ink.accent });
    k.radius.sm = 4.0;
    k
}
