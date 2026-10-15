//! Colour themes. A `ThemeChoice` (persisted) resolves to a `Palette`, which
//! every hand-painted widget reads instead of hardcoding colours, and to the
//! egui `Visuals` for stock widgets.
//!
//! Two things deliberately stay the same in every theme:
//! - the waveform is a dark "screen" (like a CDJ display), so the four band
//!   colours keep their contrast on light themes too;
//! - the transport buttons and EQ knobs are drawn as physical hardware
//!   (metal caps, black pots), which reads fine on a light panel.

use eframe::egui::{self, Color32};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum ThemeChoice {
    /// Follow the OS light/dark setting.
    #[default]
    System,
    Dark,
    Light,
    SolarizedDark,
    SolarizedLight,
}

impl ThemeChoice {
    pub const ALL: [ThemeChoice; 5] = [
        ThemeChoice::System,
        ThemeChoice::Dark,
        ThemeChoice::Light,
        ThemeChoice::SolarizedDark,
        ThemeChoice::SolarizedLight,
    ];

    pub fn label(self) -> &'static str {
        match self {
            ThemeChoice::System => "Match system",
            ThemeChoice::Dark => "Dark",
            ThemeChoice::Light => "Light",
            ThemeChoice::SolarizedDark => "Solarized Dark",
            ThemeChoice::SolarizedLight => "Solarized Light",
        }
    }

    /// `system_dark` is the OS preference; `None` (unknown) counts as dark,
    /// Qinip's original look.
    pub fn palette(self, system_dark: Option<bool>) -> Palette {
        match self {
            ThemeChoice::System => {
                if system_dark.unwrap_or(true) {
                    Palette::dark()
                } else {
                    Palette::light()
                }
            }
            ThemeChoice::Dark => Palette::dark(),
            ThemeChoice::Light => Palette::light(),
            ThemeChoice::SolarizedDark => Palette::solarized_dark(),
            ThemeChoice::SolarizedLight => Palette::solarized_light(),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Palette {
    pub is_dark: bool,
    pub panel: Color32,
    pub recessed: Color32,
    pub control: Color32,
    pub control_hover: Color32,
    /// Fills: selection wash, active widgets, hover rings.
    pub accent: Color32,
    /// Accent used as text on `panel` (darker on light themes for contrast).
    pub accent_text: Color32,
    /// Text drawn on top of an `accent` fill.
    pub on_accent: Color32,
    pub text: Color32,
    pub text_weak: Color32,
    /// Playlist title-cell track (behind the analysis progress fill).
    pub row_bg: Color32,
    pub row_text: Color32,
    /// Outline of the focused (anchor) row.
    pub focus_outline: Color32,
    /// Title-cell progress fill per analysis state.
    pub fill_failed: Color32,
    pub fill_done: Color32,
    pub fill_busy: Color32,
    pub fill_pending: Color32,
    /// BPM / key / Camelot text colour per analysis state.
    pub value_failed: Color32,
    pub value_done: Color32,
    pub value_busy: Color32,
    pub value_pending: Color32,
    /// The waveform "screen" and its playhead.
    pub screen_bg: Color32,
    pub playhead: Color32,
    /// Bar-indicator tick when not lit.
    pub tick_dim: Color32,
}

impl Palette {
    pub fn dark() -> Self {
        Self {
            is_dark: true,
            panel: rgb(0x2A2C2E),
            recessed: rgb(0x1C1D1E),
            control: rgb(0x3A3C3E),
            control_hover: rgb(0x48494C),
            accent: rgb(0xE8A93C),
            accent_text: rgb(0xE8A93C),
            on_accent: rgb(0x1C1D1E),
            text: rgb(0xE4E4E2),
            text_weak: rgb(0x9A9C9E),
            row_bg: rgb(0x1C1D1E),
            row_text: rgb(0xF4F4F2),
            focus_outline: rgb(0xF4F4F2),
            fill_failed: rgb(0x5A1E1E),
            fill_done: rgb(0x23462D),
            fill_busy: rgb(0x504614),
            fill_pending: rgb(0x2D2D2D),
            value_failed: rgb(0xFF6B6B),
            value_done: rgb(0x90EE90),
            value_busy: rgb(0xF2D24B),
            value_pending: rgb(0x8C8C8C),
            screen_bg: rgb(0x1C1D1E),
            playhead: rgb(0xFFFFFF),
            tick_dim: rgb(0x3A3C3E),
        }
    }

    pub fn light() -> Self {
        Self {
            is_dark: false,
            panel: rgb(0xF2F1EE),
            recessed: rgb(0xE4E3DF),
            control: rgb(0xE0DFDA),
            control_hover: rgb(0xD2D0CA),
            accent: rgb(0xE8A93C),
            accent_text: rgb(0x8F5A00),
            on_accent: rgb(0x1C1D1E),
            text: rgb(0x22252A),
            text_weak: rgb(0x6B6F75),
            row_bg: rgb(0xE6E5E1),
            row_text: rgb(0x22252A),
            focus_outline: rgb(0x22252A),
            fill_failed: rgb(0xF2C4C0),
            fill_done: rgb(0xC9E6CF),
            fill_busy: rgb(0xF3E2A6),
            fill_pending: rgb(0xDCDBD7),
            value_failed: rgb(0xB3261E),
            value_done: rgb(0x2E7D32),
            value_busy: rgb(0x8F5A00),
            value_pending: rgb(0x7A7D82),
            screen_bg: rgb(0x24272A),
            playhead: rgb(0xFFFFFF),
            tick_dim: rgb(0xCFCDC8),
        }
    }

    /// Ethan Schoonover's Solarized, dark variant (base03 background).
    pub fn solarized_dark() -> Self {
        Self {
            is_dark: true,
            panel: rgb(0x002B36),
            recessed: rgb(0x00212B),
            control: rgb(0x073642),
            control_hover: rgb(0x0E4B59),
            accent: rgb(0xB58900),
            accent_text: rgb(0xB58900),
            on_accent: rgb(0x002B36),
            text: rgb(0x93A1A1),
            text_weak: rgb(0x657B83),
            row_bg: rgb(0x00212B),
            row_text: rgb(0xEEE8D5),
            focus_outline: rgb(0xEEE8D5),
            fill_failed: rgb(0x5C2320),
            fill_done: rgb(0x2F3D10),
            fill_busy: rgb(0x4A3C00),
            fill_pending: rgb(0x073642),
            value_failed: rgb(0xDC322F),
            value_done: rgb(0x859900),
            value_busy: rgb(0xB58900),
            value_pending: rgb(0x586E75),
            screen_bg: rgb(0x00212B),
            playhead: rgb(0xFDF6E3),
            tick_dim: rgb(0x073642),
        }
    }

    /// Solarized, light variant (base3 background).
    pub fn solarized_light() -> Self {
        Self {
            is_dark: false,
            panel: rgb(0xFDF6E3),
            recessed: rgb(0xEEE8D5),
            control: rgb(0xE6DFC8),
            control_hover: rgb(0xDAD2B8),
            accent: rgb(0xB58900),
            accent_text: rgb(0xCB4B16),
            on_accent: rgb(0xFDF6E3),
            text: rgb(0x586E75),
            text_weak: rgb(0x93A1A1),
            row_bg: rgb(0xEEE8D5),
            row_text: rgb(0x073642),
            focus_outline: rgb(0x073642),
            fill_failed: rgb(0xF4CFC4),
            fill_done: rgb(0xDCE3B8),
            fill_busy: rgb(0xF1DFA6),
            fill_pending: rgb(0xE6DFC8),
            value_failed: rgb(0xDC322F),
            value_done: rgb(0x6C7C00),
            value_busy: rgb(0x9A7400),
            value_pending: rgb(0x93A1A1),
            screen_bg: rgb(0x073642),
            playhead: rgb(0xFDF6E3),
            tick_dim: rgb(0xE6DFC8),
        }
    }

    /// egui `Visuals` for stock widgets (buttons, sliders, checkboxes, menus).
    pub fn visuals(&self) -> egui::Visuals {
        let corner = egui::CornerRadius::same(4);
        let mut visuals = if self.is_dark {
            egui::Visuals::dark()
        } else {
            egui::Visuals::light()
        };

        visuals.panel_fill = self.panel;
        visuals.window_fill = self.panel;
        visuals.extreme_bg_color = self.recessed;
        visuals.faint_bg_color = self.control;
        visuals.override_text_color = Some(self.text);
        visuals.hyperlink_color = self.accent_text;
        visuals.selection.bg_fill = self.accent.gamma_multiply(0.45);
        visuals.selection.stroke.color = self.accent_text;
        visuals.warn_fg_color = self.accent_text;

        for widget in [
            &mut visuals.widgets.noninteractive,
            &mut visuals.widgets.inactive,
            &mut visuals.widgets.hovered,
            &mut visuals.widgets.active,
            &mut visuals.widgets.open,
        ] {
            widget.corner_radius = corner;
        }

        visuals.widgets.noninteractive.bg_fill = self.panel;
        visuals.widgets.noninteractive.weak_bg_fill = self.panel;

        visuals.widgets.inactive.bg_fill = self.control;
        visuals.widgets.inactive.weak_bg_fill = self.control;

        visuals.widgets.hovered.bg_fill = self.control_hover;
        visuals.widgets.hovered.weak_bg_fill = self.control_hover;
        visuals.widgets.hovered.bg_stroke.color = self.accent;

        visuals.widgets.active.bg_fill = self.accent;
        visuals.widgets.active.weak_bg_fill = self.accent;
        visuals.widgets.active.fg_stroke.color = self.on_accent;

        visuals.widgets.open.bg_fill = self.control_hover;
        visuals.widgets.open.weak_bg_fill = self.control_hover;

        visuals
    }
}

const fn rgb(hex: u32) -> Color32 {
    Color32::from_rgb((hex >> 16) as u8, (hex >> 8) as u8, hex as u8)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// WCAG relative luminance contrast ratio.
    fn contrast(a: Color32, b: Color32) -> f32 {
        fn lum(c: Color32) -> f32 {
            let ch = |v: u8| {
                let s = v as f32 / 255.0;
                if s <= 0.03928 {
                    s / 12.92
                } else {
                    ((s + 0.055) / 1.055).powf(2.4)
                }
            };
            0.2126 * ch(c.r()) + 0.7152 * ch(c.g()) + 0.0722 * ch(c.b())
        }
        let (x, y) = (lum(a), lum(b));
        (x.max(y) + 0.05) / (x.min(y) + 0.05)
    }

    #[test]
    fn body_text_is_readable_in_every_theme() {
        for choice in ThemeChoice::ALL {
            let p = choice.palette(Some(true));
            assert!(contrast(p.text, p.panel) >= 4.5, "{choice:?} text on panel");
            assert!(contrast(p.row_text, p.row_bg) >= 4.5, "{choice:?} row text");
            for fill in [p.fill_failed, p.fill_done, p.fill_busy, p.fill_pending] {
                assert!(contrast(p.row_text, fill) >= 4.5, "{choice:?} row text on fill");
            }
            assert!(contrast(p.accent_text, p.panel) >= 3.0, "{choice:?} accent text");
            assert!(contrast(p.playhead, p.screen_bg) >= 4.5, "{choice:?} playhead");
        }
        let light = ThemeChoice::System.palette(Some(false));
        assert!(!light.is_dark);
    }
}
