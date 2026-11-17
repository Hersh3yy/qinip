//! The screen: top bar, playlist table, waveform screen and transport bar, rebuilt from the
//! `App` state whenever something changes. Nothing here mutates state directly: handlers
//! are closures Rune runs with `&mut App` between frames, then the view runs again and Rune
//! diffs the result (so colours and layout spring to their new values).

use std::borrow::Cow;

use qinip::playlist::{AnalysisState, TrackEntry};
use qinip::theme::ThemeChoice;
use qinip::transport::beat_phase;
use rune::{
    Align, Color, Cursor, Cx, Drag, El, FileDialogOptions, LineCap, Phase, PopupMenu, Shape, TextStyle, Tx,
    canvas, col, el, row, spacer, text,
};
use rune_kit::{Button, ButtonVariant, Cell, Table, TableColumn, TableState};

use crate::colors::{self, Ink};
use crate::state::{App, clean};

/// The waveform is drawn in an 800 x 110 view box (one unit per peak) and stretched to the
/// element, so the shape does not depend on the window's width.
const WAVE_W: f32 = 800.0;
const WAVE_H: f32 = 110.0;
/// Tabular digits, so the clock does not wobble as it counts.
const MONO: TextStyle = TextStyle::new(12.0, 500.0).features("'tnum' 1");

pub fn view(s: &App, _cx: &mut Cx<App>) -> El {
    let ink = s.ink;
    col()
        .key("root")
        .w_full()
        .h_full()
        .fill(ink.panel)
        .child(top_bar(s))
        .child(playlist(s))
        .child(waveform(s))
        .child(bottom_bar(s))
        // Files or folders dropped anywhere on the window.
        .on_drop_files(|s: &mut App, files, _tx| s.load_dropped(files))
        .on_drop_hover(|s: &mut App, over, _tx| s.drop_over = over)
}

// ---- Top bar ---------------------------------------------------------------------------------

fn top_bar(s: &App) -> El {
    let ink = s.ink;
    let k = &s.kit;
    let load = Button::new("Load Folder")
        .on_press_tx(|_: &mut App, tx: &mut Tx| {
            let options = FileDialogOptions { title: "Load Folder".into(), ..Default::default() };
            // The native folder dialog (rfd on macOS); the answer arrives on a later frame.
            tx.open_folder(options, |s: &mut App, result, _tx| {
                if let Ok(Some(folder)) = result {
                    s.load_folder(folder);
                }
            });
        })
        .build(k)
        .key("load-folder");
    let clear = Button::new("Clear Playlist")
        .variant(ButtonVariant::Ghost)
        .on_press(|s: &mut App| s.clear_playlist())
        .build(k)
        .key("clear-playlist");
    let theme = Button::new(format!("Theme: {}", s.theme.label()))
        .variant(ButtonVariant::Ghost)
        .on_press_tx(|s: &mut App, tx: &mut Tx| {
            // A native pop-up menu over the button, like a SwiftUI picker.
            let menu = PopupMenu::picker(ThemeChoice::ALL.iter().map(|t| (t.label(), t.label())), s.theme.label());
            tx.popup_menu(menu, "theme", |s: &mut App, picked, _tx| {
                if let Some(id) = picked {
                    let id: String = id.into();
                    s.set_theme_by_label(&id);
                }
            });
        })
        .build(k)
        .key("theme");
    let status = text(s.status_message.clone())
        .key("status")
        .font(TextStyle::new(12.0, 400.0))
        .color(ink.text_weak)
        .truncate()
        .shrink();
    row()
        .key("top-bar")
        .w_full()
        .h(48.0)
        .px(12.0)
        .gap(8.0)
        .items_center()
        .fill(ink.recessed)
        .child(text("Qinip").font(TextStyle::new(16.0, 700.0)).color(ink.accent_text))
        .child(text("on Rune").font(TextStyle::new(11.0, 500.0)).color(ink.text_weak))
        .child(load)
        .child(clear)
        .child(theme)
        .child(spacer())
        .child(status)
}

// ---- Playlist --------------------------------------------------------------------------------

fn playlist(s: &App) -> El {
    let ink = s.ink;
    let area = col()
        .key("playlist")
        .w_full()
        .grow(1.0)
        .min_h(0.0)
        .fill(if s.drop_over { ink.accent.alpha(0.12) } else { ink.panel });
    if s.playlist.tracks.is_empty() {
        return area
            .center()
            .gap(6.0)
            .child(text("No tracks loaded").font(TextStyle::new(15.0, 600.0)).color(ink.text))
            .child(
                text("Load a folder of MP3s, or drop files and folders onto the window.")
                    .key("empty-state")
                    .color(ink.text_weak),
            );
    }
    // rune-kit's virtualised table: it builds only the rows on screen, keeps the selection
    // and cursor in `App::table`, and handles the arrow keys, Return and double-click.
    let table = Table::new(table_state, table_state_mut, track_count, row_id)
        .column(TableColumn::new("num", "#").width(44.0).align(Align::End))
        .column(TableColumn::new("artist", "Artist").flex(1.0).min(100.0))
        .column(TableColumn::new("title", "Title").flex(2.0).min(160.0))
        .column(TableColumn::new("duration", "Duration").width(80.0).align(Align::End))
        .column(TableColumn::new("bpm", "BPM").width(70.0).align(Align::End))
        .column(TableColumn::new("key", "Key").width(80.0))
        .column(TableColumn::new("camelot", "Camelot").width(80.0))
        .cell(cell)
        .on_open(|s: &mut App, id: &str, _tx: &mut Tx| {
            if let Some(index) = s.index_of_id(id) {
                s.request_play(index);
            }
        })
        .name("Playlist")
        .build(s, &s.kit)
        // The table fills the height its parent gives it only when told to grow; without
        // `min_h(0.0)` the flex item keeps its content height and the viewport collapses.
        .grow(1.0)
        .min_h(0.0);
    area.child(table)
}

fn table_state(s: &App) -> &TableState {
    &s.table
}

fn table_state_mut(s: &mut App) -> &mut TableState {
    &mut s.table
}

fn track_count(s: &App) -> usize {
    s.playlist.tracks.len()
}

/// Rows are named by their path, so the selection follows a track through a re-sort.
fn row_id(s: &App, i: usize) -> Cow<'_, str> {
    s.playlist.tracks[i].path.to_string_lossy()
}

fn cell(s: &App, i: usize, column: &str) -> Cell {
    let ink = s.ink;
    let Some(t) = s.playlist.tracks.get(i) else { return Cell::text("") };
    let value = match t.analysis {
        AnalysisState::Pending => ink.value_pending,
        AnalysisState::Loading | AnalysisState::Analyzing => ink.value_busy,
        AnalysisState::Done => ink.value_done,
        AnalysisState::Failed(_) => ink.value_failed,
    };
    match column {
        "num" => Cell::text(t.track_number_label()).mono().muted(),
        "artist" => Cell::text(clean(&t.artist)),
        "title" => Cell::el(title_cell(ink, t, s.playing_track == Some(i))),
        "duration" => Cell::text(t.duration_label()).mono(),
        "bpm" => Cell::text(t.bpm_label()).mono().color(value),
        "key" => Cell::text(t.key_label()).color(value),
        "camelot" => Cell::text(t.camelot_label()).mono().color(value),
        _ => Cell::text(""),
    }
}

/// The title over a progress track, as the egui title cell: the fill grows with the
/// analysis state (pending, loading, analysing, done) and turns red when it failed.
fn title_cell(ink: Ink, t: &TrackEntry, playing: bool) -> El {
    let fill = match t.analysis {
        AnalysisState::Pending => ink.fill_pending,
        AnalysisState::Loading | AnalysisState::Analyzing => ink.fill_busy,
        AnalysisState::Done => ink.fill_done,
        AnalysisState::Failed(_) => ink.fill_failed,
    };
    let (color, weight) = if playing { (ink.accent_text, 700.0) } else { (ink.row_text, 500.0) };
    el().w_full()
        .h(22.0)
        .radius(4.0)
        .fill(ink.row_bg)
        .items_center()
        .child(el().w_frac(t.analysis_progress()).h_full().radius(4.0).fill(fill))
        .child(
            text(clean(&t.title))
                .absolute(8.0, 3.0)
                .font(TextStyle::new(12.5, weight))
                .color(color)
                .truncate(),
        )
}

// ---- Waveform --------------------------------------------------------------------------------

fn waveform(s: &App) -> El {
    let ink = s.ink;
    let mut shape = Shape::new();
    if let Some(bands) = s.current_waveform() {
        let mid = WAVE_H / 2.0;
        let half = WAVE_H * 0.45;
        // One vertical line per peak and band, in the band's colour, as the egui painter does.
        for (b, peaks) in [&bands.low, &bands.low_mid, &bands.hi_mid, &bands.hi].into_iter().enumerate() {
            let color = colors::band(b, 0.85);
            let n = peaks.len().max(1) as f32;
            for (i, &(min, max)) in peaks.iter().enumerate() {
                let x = (i as f32 + 0.5) / n * WAVE_W;
                let top = mid - max.clamp(0.0, 1.0) * half;
                let bottom = mid - min.clamp(-1.0, 0.0) * half;
                if bottom - top < 0.5 {
                    continue;
                }
                shape = shape.line([x, top], [x, bottom], 1.0, LineCap::Butt, color);
            }
        }
    }
    if s.duration_secs > 0.0 {
        let x = (s.position_secs / s.duration_secs).clamp(0.0, 1.0) as f32 * WAVE_W;
        shape = shape.line([x, 0.0], [x, WAVE_H], 2.0, LineCap::Butt, ink.playhead);
    }
    let label = match s.current_track() {
        Some(t) if s.current_waveform().is_some() => clean(&t.display_label()),
        Some(t) => format!("{} (decoding…)", clean(&t.display_label())),
        None => "Waveform appears here once a track is decoded.".to_string(),
    };
    col()
        .key("waveform-panel")
        .w_full()
        .px(12.0)
        .pb(8.0)
        .gap(4.0)
        .child(text(label).key("now-playing").font(TextStyle::new(13.0, 600.0)).color(ink.text).truncate())
        .child(
            canvas(shape.view_box(WAVE_W, WAVE_H))
                .key("waveform")
                .w_full()
                .h(WAVE_H)
                .radius(4.0)
                .fill(ink.screen_bg)
                .cursor(Cursor::Pointer)
                // Click or drag to seek: `d.x` is in the element's own points, `d.w` its width.
                .on_drag(|s: &mut App, d: &mut Drag, tx: &mut Tx| {
                    if matches!(d.phase, Phase::Down | Phase::Move) {
                        s.seek_fraction(d.x / d.w.max(1.0));
                        tx.instant();
                    }
                }),
        )
}

// ---- Bottom bar: transport, tempo light, clock, seek bar, volume -----------------------------

fn bottom_bar(s: &App) -> El {
    let ink = s.ink;
    let play_icon = if s.is_playing { pause_icon(ink) } else { play_icon(ink) };
    row()
        .key("bottom-bar")
        .w_full()
        .h(64.0)
        .px(12.0)
        .gap(10.0)
        .items_center()
        .fill(ink.recessed)
        .child(transport_button(ink, "prev", "Previous", prev_icon(ink), |s: &mut App| s.previous()))
        .child(transport_button(ink, "play", if s.is_playing { "Pause" } else { "Play" }, play_icon, |s: &mut App| {
            s.toggle_play()
        }))
        .child(transport_button(ink, "next", "Next", next_icon(ink), |s: &mut App| s.next()))
        .child(transport_button(ink, "stop", "Stop", stop_icon(ink), |s: &mut App| s.stop()))
        .child(tempo_light(s))
        .child(text(clock(s.position_secs)).key("time").font(MONO).color(ink.text))
        .child(seek_bar(s))
        .child(text(clock(s.duration_secs)).key("duration").font(MONO).color(ink.text_weak))
        .child(volume_slider(s))
}

/// A round button with a vector icon, in the palette's control colours.
fn transport_button(ink: Ink, key: &'static str, label: &'static str, icon: Shape, on_press: fn(&mut App)) -> El {
    el().key(key)
        .role("button")
        .label(label)
        .focusable(true)
        .size(36.0, 36.0)
        .radius(18.0)
        .fill(ink.control)
        .hover(ink.control_hover)
        .press(ink.accent.alpha(0.35))
        .border(1.0, ink.tick_dim)
        .center()
        .cursor(Cursor::Pointer)
        .on_press(on_press)
        .child(canvas(icon.view_box(16.0, 16.0)).size(16.0, 16.0))
}

fn rect(x0: f32, y0: f32, x1: f32, y1: f32) -> Vec<[f32; 2]> {
    vec![[x0, y0], [x1, y0], [x1, y1], [x0, y1]]
}

fn play_icon(ink: Ink) -> Shape {
    Shape::new().fill(vec![[4.5, 2.0], [14.0, 8.0], [4.5, 14.0]], ink.text)
}

fn pause_icon(ink: Ink) -> Shape {
    Shape::new().fill(rect(3.0, 2.0, 6.5, 14.0), ink.text).fill(rect(9.5, 2.0, 13.0, 14.0), ink.text)
}

fn stop_icon(ink: Ink) -> Shape {
    Shape::new().fill(rect(3.0, 3.0, 13.0, 13.0), ink.text)
}

fn prev_icon(ink: Ink) -> Shape {
    Shape::new().fill(rect(2.0, 2.0, 4.0, 14.0), ink.text).fill(vec![[14.0, 2.0], [5.0, 8.0], [14.0, 14.0]], ink.text)
}

fn next_icon(ink: Ink) -> Shape {
    Shape::new().fill(vec![[2.0, 2.0], [11.0, 8.0], [2.0, 14.0]], ink.text).fill(rect(12.0, 2.0, 14.0, 14.0), ink.text)
}

/// The tempo readout: a light that flashes on every beat (full on the beat, fading before
/// the next one) and the BPM, as `qinip::transport::tempo_indicator`.
fn tempo_light(s: &App) -> El {
    let ink = s.ink;
    let cyan = colors::rgb(76, 201, 240);
    let bpm = s.current_bpm().filter(|b| *b > 0.0);
    let glow = bpm.map_or(0.0, |b| (1.0 - beat_phase(s.position_secs, b)).powi(3));
    let light = el()
        .key("tempo-light")
        .size(16.0, 16.0)
        .radius(8.0)
        .fill(ink.tick_dim)
        // Changes land at once: a pulse must not be smoothed by the colour spring.
        .instant_changes()
        .child(
            el().size(16.0, 16.0)
                .radius(8.0)
                .fill(cyan.alpha(0.2 + 0.8 * glow))
                .border(if glow > 0.4 { 2.0 } else { 0.0 }, cyan.alpha(0.6 * glow)),
        );
    let label = bpm.map_or("—".to_string(), |b| format!("{b:.1}"));
    row()
        .key("tempo")
        .w(88.0)
        .gap(6.0)
        .items_center()
        .child(light)
        .child(
            text(label)
                .key("bpm")
                .font(TextStyle::new(14.0, 600.0).features("'tnum' 1"))
                .color(if bpm.is_some() { cyan } else { ink.text_weak }),
        )
}

fn seek_bar(s: &App) -> El {
    let ink = s.ink;
    let fraction = if s.duration_secs > 0.0 { (s.position_secs / s.duration_secs).clamp(0.0, 1.0) as f32 } else { 0.0 };
    slider(ink, "seek", "Position", fraction, ink.accent)
        .grow(1.0)
        .on_drag(|s: &mut App, d: &mut Drag, tx: &mut Tx| {
            if matches!(d.phase, Phase::Down | Phase::Move) {
                s.seek_fraction(d.x / d.w.max(1.0));
                tx.instant();
            }
        })
}

fn volume_slider(s: &App) -> El {
    let ink = s.ink;
    row()
        .key("volume-row")
        .gap(6.0)
        .items_center()
        .child(text("Vol").font(TextStyle::new(11.0, 500.0)).color(ink.text_weak))
        .child(slider(ink, "volume", "Volume", s.volume, ink.text_weak).w(110.0).on_drag(
            |s: &mut App, d: &mut Drag, tx: &mut Tx| {
                if matches!(d.phase, Phase::Down | Phase::Move) {
                    s.set_volume(d.x / d.w.max(1.0));
                    tx.instant();
                }
            },
        ))
}

/// A slider's look: a thin track, a fill up to `value` and a knob at its end. The caller
/// sizes it and adds the drag handler (drags give `x` in the element's own points).
fn slider(ink: Ink, key: &'static str, label: &'static str, value: f32, fill: Color) -> El {
    let (h, bar, knob) = (24.0, 4.0, 12.0);
    el().key(key)
        .role("slider")
        .label(label)
        .range_value(f64::from(value), 0.0, 1.0)
        .h(h)
        .items_center()
        .cursor(Cursor::Pointer)
        .instant_changes()
        .child(
            el().w_full()
                .h(bar)
                .radius(bar / 2.0)
                .fill(ink.tick_dim)
                .items_center()
                .child(el().w_frac(value).h(bar).radius(bar / 2.0).fill(fill))
                .child(el().size(knob, knob).radius(knob / 2.0).fill(ink.text).ml(-knob / 2.0)),
        )
}

fn clock(secs: f64) -> String {
    let total = secs.max(0.0).round() as u64;
    format!("{}:{:02}", total / 60, total % 60)
}
