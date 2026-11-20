//! The screen: top bar, playlist table, waveform screen and transport bar, rebuilt from the
//! `App` state whenever something changes. Nothing here mutates state directly: handlers
//! are closures Rune runs with `&mut App` between frames, then the view runs again and Rune
//! diffs the result (so colours and layout spring to their new values).
//!
//! Skins. With `Skin::Flat` this is the plain screen. The other two skins add effect slots
//! (`rune::effect(MARK)`): boxes that draw nothing themselves but tell the compositor in
//! `gfx.rs` to paint a material there, under or over the ordinary elements depending on where
//! the slot sits among its siblings. Liquid Metal puts gunmetal slabs under both bars, chrome
//! on the knob pots and transport buttons, a chrome border on the waveform screen and a gold
//! border on the playing row; labels that would sit on metal go in dark pills. Disco Ball
//! covers the window with a wall of small chrome tiles, puts the bars, the list and the
//! waveform panel on translucent scrims, and sprinkles stars over the bars.

use std::borrow::Cow;
use std::f32::consts::{PI, TAU};

use qinip::eq::BAND_LABELS;
use qinip::playlist::{AnalysisState, TrackEntry};
use qinip::theme::ThemeChoice;
use qinip::transport::beat_phase;
use rune::{
    Align, Color, Cursor, Cx, Drag, El, FileDialogOptions, LineCap, Path, Phase, PointerPress, PopupItem, PopupMenu,
    Shape, TextStyle, Tx, canvas, col, el, row, spacer, text,
};
use rune_kit::{Button, ButtonVariant, Cell, SortDir, Table, TableColumn, TableState};

use crate::colors::{self, Ink};
use crate::gfx::{CHROME_BORDER, CHROME_FILL, DISCO_TILE, GOLD_BORDER, GUNMETAL_FILL, SPARKLES};
use crate::state::{App, Skin, clean};

/// The waveform is drawn in an 800 x 110 view box (one unit per peak) and stretched to the
/// element, so the shape does not depend on the window's width.
const WAVE_W: f32 = 800.0;
const WAVE_H: f32 = 110.0;
/// Tabular digits, so the clock does not wobble as it counts.
const MONO: TextStyle = TextStyle::new(12.0, 500.0).features("'tnum' 1");

pub fn view(s: &App, cx: &mut Cx<App>) -> El {
    let ink = s.ink;
    // On macOS the window has a transparent title bar with our content under it, so the
    // top bar must start to the right of the traffic lights (0 on Windows/Linux and in
    // full screen).
    let leading = cx.chrome_insets().leading;
    let disco = s.skin == Skin::DiscoBall;
    let mut root = col()
        .key("root")
        .w_full()
        .h_full()
        .fill(ink.panel)
        // The root's size, so the Disco Ball wall knows how many tiles cover the window.
        .on_layout(|s: &mut App, r: [f32; 4]| s.note_window(r[2], r[3]));
    if disco {
        // First child, so everything else is painted over the tiles.
        root = root.child(disco_wall(s));
    }
    root.child(top_bar(s, leading))
        .child(playlist(s))
        .child(waveform(s))
        .child(bottom_bar(s))
        // Files or folders dropped anywhere on the window.
        .on_drop_files(|s: &mut App, files, _tx| s.load_dropped(files))
        .on_drop_hover(|s: &mut App, over, _tx| s.drop_over = over)
}

// ---- Skin helpers ----------------------------------------------------------------------------

/// A slot pinned over its parent's whole box; the compositor fills it with `mark`.
fn slab(mark: u32, data: [f32; 4], radius: f32) -> El {
    rune::effect(mark).effect_data(data).pin(Some(0.0), Some(0.0), Some(0.0), Some(0.0)).radius(radius)
}

/// A bar's own fill: the palette's recessed colour, or in Disco Ball a half-see-through
/// version of it so the tiles glint through under the controls.
fn bar_fill(s: &App) -> Color {
    match s.skin {
        Skin::DiscoBall => s.ink.recessed.alpha(0.78),
        _ => s.ink.recessed,
    }
}

/// The scrim the list and the waveform panel sit on in Disco Ball: the panel colour, mostly
/// opaque, so the text keeps its contrast whatever the palette. Rune blends in linear light,
/// where a dark veil leaves chrome highlights brighter than it would in sRGB (0.9 here is
/// about what 0.75 would be in sRGB), so the tiles still show through as a faint texture.
fn scrim(ink: Ink) -> Color {
    ink.panel.alpha(0.9)
}

/// In Liquid Metal, labels that would sit straight on gunmetal go in a dark translucent pill;
/// other skins return the element untouched.
fn pill(s: &App, e: El) -> El {
    if s.skin == Skin::LiquidMetal {
        el().px(8.0).py(3.0).radius(6.0).fill(s.ink.panel.alpha(0.72)).shrink().min_w(0.0).items_center().child(e)
    } else {
        e
    }
}

/// Ghost buttons vanish on metal; the skins use the filled neutral variant instead.
fn quiet(s: &App) -> ButtonVariant {
    if s.skin.metal_parts() { ButtonVariant::Neutral } else { ButtonVariant::Ghost }
}

/// The ink for icons and the knob pointer: dark on chrome, the palette's text colour otherwise.
fn mark_ink(s: &App) -> Color {
    if s.skin.metal_parts() { colors::rgb(18, 18, 22) } else { s.ink.text }
}

// ---- Disco wall ------------------------------------------------------------------------------

/// Tile size and gap in points, and the most tiles a window gets: past that the tiles grow,
/// so a 1440 x 900 window is about 1,440 slots.
const TILE: f32 = 28.0;
const TILE_GAP: f32 = 2.0;
const MAX_TILES: usize = 1500;

/// The wall of mirror tiles under everything. Each tile is one effect slot whose studio angle
/// and ripple seed come from a hash of its row and column, so the wall glints unevenly.
fn disco_wall(s: &App) -> El {
    let (w, h) = s.window;
    let count = |pitch: f32| ((w / pitch).ceil() as usize) * ((h / pitch).ceil() as usize);
    let mut pitch = TILE + TILE_GAP;
    if count(pitch) > MAX_TILES {
        pitch = (w * h / MAX_TILES as f32).sqrt().ceil();
    }
    let tile = pitch - TILE_GAP;
    let (cols, rows) = ((w / pitch).ceil() as usize, (h / pitch).ceil() as usize);
    let mut wall = el().key("disco-wall").pin(Some(0.0), Some(0.0), Some(0.0), Some(0.0));
    for row in 0..rows {
        for col in 0..cols {
            let hash = tile_hash(row as u32, col as u32);
            let angle = (hash & 0xffff) as f32 / 65535.0 * TAU - PI;
            let seed = (hash >> 16) as f32;
            wall = wall.child(
                rune::effect(DISCO_TILE)
                    .effect_data([angle, seed, 0.0, 0.0])
                    .absolute(col as f32 * pitch, row as f32 * pitch)
                    .size(tile, tile)
                    .radius(2.0),
            );
        }
    }
    wall
}

/// A small integer hash of a tile's row and column (fixed, so the wall is stable frame to
/// frame and the same in a headless render).
fn tile_hash(row: u32, col: u32) -> u32 {
    let mut x = row.wrapping_mul(0x9E37_79B9) ^ col.wrapping_mul(0x85EB_CA6B);
    x ^= x >> 16;
    x = x.wrapping_mul(0x7FEB_352D);
    x ^= x >> 15;
    x = x.wrapping_mul(0x846C_A68B);
    x ^ (x >> 16)
}

// ---- Top bar ---------------------------------------------------------------------------------

fn top_bar(s: &App, leading_inset: f32) -> El {
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
        .variant(quiet(s))
        .on_press(|s: &mut App| s.clear_playlist())
        .build(k)
        .key("clear-playlist");
    let theme = Button::new(format!("Theme: {}", s.theme.label()))
        .variant(quiet(s))
        .on_press_tx(|s: &mut App, tx: &mut Tx| {
            // A native pop-up menu over the button, like a SwiftUI picker: the seven palettes,
            // then the two skins after a separator (checked when on; picking it again turns it off).
            let menu = PopupMenu::picker(ThemeChoice::ALL.iter().map(|t| (t.label(), t.label())), s.theme.label())
                .separator()
                .items(Skin::PICKS.iter().map(|(id, label, skin)| PopupItem::new(*id, *label).checked(s.skin == *skin)));
            tx.popup_menu(menu, "theme", |s: &mut App, picked, _tx| {
                if let Some(id) = picked {
                    let id: String = id.into();
                    s.pick_look(&id);
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
    let name = row()
        .gap(8.0)
        .items_center()
        .child(text("Qinip").font(TextStyle::new(16.0, 700.0)).color(ink.accent_text))
        .child(text("on Rune").font(TextStyle::new(11.0, 500.0)).color(ink.text_weak));
    let mut bar = row()
        .key("top-bar")
        .w_full()
        .h(40.0)
        .pl(leading_inset + 12.0)
        .pr(12.0)
        .gap(8.0)
        .items_center()
        .fill(bar_fill(s));
    if s.skin == Skin::LiquidMetal {
        // Under the content: a gunmetal slab the width of the bar.
        bar = bar.child(slab(GUNMETAL_FILL, [0.0; 4], 0.0).key("top-metal"));
    }
    bar = bar.child(pill(s, name)).child(load).child(clear).child(theme).child(spacer()).child(pill(s, status));
    if s.skin == Skin::DiscoBall {
        // Over the content: a few stars twinkling across the bar.
        bar = bar.child(slab(SPARKLES, [10.0, 6.0, 0.0, 0.0], 0.0).key("top-sparkles"));
    }
    bar
}

// ---- Playlist --------------------------------------------------------------------------------

fn playlist(s: &App) -> El {
    let ink = s.ink;
    let fill = if s.drop_over {
        ink.accent.alpha(0.12)
    } else if s.skin == Skin::DiscoBall {
        scrim(ink)
    } else {
        ink.panel
    };
    let area = col().key("playlist").w_full().grow(1.0).min_h(0.0).fill(fill);
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
        .column(TableColumn::new("num", "#").width(44.0).align(Align::End).sortable())
        .column(TableColumn::new("artist", "Artist").flex(1.0).min(100.0).sortable())
        .column(TableColumn::new("title", "Title").flex(2.0).min(160.0).sortable())
        .column(TableColumn::new("duration", "Duration").width(80.0).align(Align::End).sortable())
        .column(TableColumn::new("bpm", "BPM").width(70.0).align(Align::End).sortable())
        .column(TableColumn::new("key", "Key").width(80.0).sortable())
        .column(TableColumn::new("camelot", "Camelot").width(80.0).sortable())
        // A header click: the kit records the column and direction (and draws the arrow),
        // the app sorts its rows.
        .on_sort(|s: &mut App, column: &str, dir: SortDir, _tx: &mut Tx| s.sort_by_column(column, dir))
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

/// What a right-click on a row offers. Ids are what `App::track_action` matches on.
fn track_menu() -> PopupMenu {
    PopupMenu::new()
        .item(PopupItem::new("play", "Play").symbol("play.fill"))
        .item(PopupItem::new("remove", "Remove from playlist").symbol("minus.circle"))
        .separator()
        .item(PopupItem::new("folder", "Open enclosing folder").symbol("folder"))
        .item(PopupItem::new("tags", "Write BPM/Key tags").symbol("tag"))
        .item(PopupItem::new("reanalyze", "Re-analyze BPM/Key").symbol("waveform"))
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
        "title" => Cell::el(title_cell(ink, t, s.playing_track == Some(i), s.skin).on_secondary_press(
            move |_: &mut App, press: PointerPress, tx: &mut Tx| {
                // The right-click menu, native (NSMenu on macOS), at the pointer.
                tx.popup_menu(track_menu(), press, move |s: &mut App, picked, _tx| {
                    if let Some(id) = picked {
                        let id: String = id.into();
                        s.track_action(i, &id);
                    }
                });
            },
        )),
        "duration" => Cell::text(t.duration_label()).mono(),
        "bpm" => Cell::text(t.bpm_label()).mono().color(value),
        "key" => Cell::text(t.key_label()).color(value),
        "camelot" => Cell::text(t.camelot_label()).mono().color(value),
        _ => Cell::text(""),
    }
}

/// The title over a progress track, as the egui title cell: the fill grows with the
/// analysis state (pending, loading, analysing, done) and turns red when it failed. With a
/// skin on, the playing track's title gets a gold liquid-metal border.
fn title_cell(ink: Ink, t: &TrackEntry, playing: bool, skin: Skin) -> El {
    let fill = match t.analysis {
        AnalysisState::Pending => ink.fill_pending,
        AnalysisState::Loading | AnalysisState::Analyzing => ink.fill_busy,
        AnalysisState::Done => ink.fill_done,
        AnalysisState::Failed(_) => ink.fill_failed,
    };
    let (color, weight) = if playing { (ink.accent_text, 700.0) } else { (ink.row_text, 500.0) };
    let mut cell = el()
        .w_full()
        .h(22.0)
        .radius(4.0)
        .fill(ink.row_bg)
        .items_center()
        .child(el().w_frac(t.analysis_progress()).h_full().radius(4.0).fill(fill));
    if playing && skin.metal_parts() {
        cell = cell.child(slab(GOLD_BORDER, [2.0, 0.0, 0.0, 0.0], 4.0));
    }
    cell.child(
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
            // The EQ knob shapes what you hear and how visible the band is, as in the egui app.
            let gain = s.eq_band(b);
            if gain <= 0.001 {
                continue;
            }
            let color = colors::band(b, (gain / 2.0 * 0.86).clamp(0.08, 0.86));
            let n = peaks.len().max(1) as f32;
            for (i, &(min, max)) in peaks.iter().enumerate() {
                let x = (i as f32 + 0.5) / n * WAVE_W;
                let top = mid - (max * gain).clamp(0.0, 1.0) * half;
                let bottom = mid - (min * gain).clamp(-1.0, 0.0) * half;
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
    let screen = canvas(shape.view_box(WAVE_W, WAVE_H))
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
        });
    // With a skin on, a chrome tube runs along the screen's edge, painted after (over) it.
    let screen = if s.skin.metal_parts() {
        el().key("screen-frame")
            .w_full()
            .h(WAVE_H)
            .child(screen)
            .child(slab(CHROME_BORDER, [3.0, 0.0, 0.0, 0.0], 6.0).key("screen-border"))
    } else {
        screen
    };
    let mut panel = col().key("waveform-panel").w_full().px(12.0).pb(8.0).gap(4.0);
    if s.skin == Skin::DiscoBall {
        panel = panel.fill(scrim(ink));
    }
    panel
        .child(text(label).key("now-playing").font(TextStyle::new(13.0, 600.0)).color(ink.text).truncate())
        .child(eq_row(s))
        .child(screen)
}

// ---- EQ knobs --------------------------------------------------------------------------------

/// Knob geometry: a 44 pt box; the pot's radius; the pointer sweeps 270° from 7 o'clock to
/// 5 o'clock.
const KNOB: f32 = 44.0;
const POT_R: f32 = KNOB * 0.40;
const SWEEP_START: f32 = std::f32::consts::FRAC_PI_2 + 0.75;
const SWEEP_RANGE: f32 = std::f32::consts::TAU * 0.75;
/// Vertical drag distance for the whole 0..200 % range.
const DRAG_PX_FULL_RANGE: f32 = 140.0;

/// "EQ" + the four knobs + Reset EQ, above the waveform, like the egui app.
fn eq_row(s: &App) -> El {
    let ink = s.ink;
    let mut r = row()
        .key("eq-row")
        .w_full()
        .gap(10.0)
        .items_end()
        .child(text("EQ").font(TextStyle::new(11.0, 600.0)).color(ink.text_weak).pb(14.0));
    for band in 0..4 {
        r = r.child(eq_knob(s, band));
    }
    let reset = Button::new("Reset EQ")
        .variant(quiet(s))
        .on_press(|s: &mut App| s.reset_eq())
        .build(&s.kit)
        .key("reset-eq")
        .mb(12.0);
    r.child(reset)
}

/// One knob: band name above, the dial, the percentage below. Drag up or down to turn it;
/// double-click or right-click resets the band to 100 %. With a skin on, the pot is a chrome
/// disc the compositor paints under the dial's arcs and pointer.
fn eq_knob(s: &App, band: usize) -> El {
    let ink = s.ink;
    let gain = s.eq_band(band);
    let accent = colors::band(band, 1.0);
    let metal = s.skin.metal_parts();
    let dial = canvas(knob_shape(ink, gain, accent, metal, mark_ink(s)).view_box(KNOB, KNOB))
        .key(format!("eq-knob-{band}"))
        .role("slider")
        .label(BAND_LABELS[band])
        .range_value(f64::from(gain) * 100.0, 0.0, 200.0)
        .size(KNOB, KNOB)
        .cursor(Cursor::Grab)
        .instant_changes()
        .on_drag(move |s: &mut App, d: &mut Drag, tx: &mut Tx| match d.phase {
            // `d.data` keeps the value at the press, so the turn follows the finger exactly.
            Phase::Down => d.data = s.eq_band(band),
            Phase::Move => {
                s.set_eq_band(band, d.data - d.dy / DRAG_PX_FULL_RANGE * 2.0);
                tx.instant();
            }
            _ => {}
        })
        .on_double_press(move |s: &mut App| s.set_eq_band(band, 1.0))
        .on_secondary_press(move |s: &mut App, _press: PointerPress, _tx: &mut Tx| s.set_eq_band(band, 1.0));
    let dial = if metal {
        // The chrome pot: a circle slot the size of the pot, placed before (under) the dial.
        let pot = rune::effect(CHROME_FILL)
            .effect_data([5.0, 0.7, 0.0, 0.0])
            .absolute(KNOB / 2.0 - POT_R, KNOB / 2.0 - POT_R)
            .size(2.0 * POT_R, 2.0 * POT_R)
            .radius(POT_R);
        el().key(format!("eq-pot-{band}")).size(KNOB, KNOB).child(pot).child(dial)
    } else {
        dial
    };
    col()
        .key(format!("eq-{band}"))
        .items_center()
        .gap(2.0)
        .child(text(BAND_LABELS[band]).font(TextStyle::new(10.0, 700.0)).color(accent))
        .child(dial)
        .child(text(format!("{:.0}%", gain * 100.0)).font(MONO).color(ink.text))
}

/// The dial: a dim track arc, the lit arc up to the value, the pot (flat skin only; the
/// compositor paints a chrome one otherwise), a pointer line. On chrome the pointer is dark
/// with a light outline, so it reads on both the bright sky and the dark floor of the metal.
fn knob_shape(ink: Ink, gain: f32, accent: Color, metal: bool, pointer_ink: Color) -> Shape {
    let c = [KNOB / 2.0, KNOB / 2.0];
    let r = POT_R;
    let t = (gain / 2.0).clamp(0.0, 1.0);
    let arc = |from: f32, to: f32, radius: f32| -> Path {
        let steps = 40;
        let mut p = Path::new();
        for i in 0..=steps {
            let a = from + (to - from) * i as f32 / steps as f32;
            let pt = [c[0] + radius * a.cos(), c[1] + radius * a.sin()];
            p = if i == 0 { p.move_to(pt) } else { p.line_to(pt) };
        }
        p
    };
    let circle = |radius: f32| -> Vec<[f32; 2]> {
        (0..48)
            .map(|i| {
                let a = std::f32::consts::TAU * i as f32 / 48.0;
                [c[0] + radius * a.cos(), c[1] + radius * a.sin()]
            })
            .collect()
    };
    let end = SWEEP_START + SWEEP_RANGE * t;
    let pointer = [c[0] + (r - 3.0) * end.cos(), c[1] + (r - 3.0) * end.sin()];
    let mut shape = Shape::new()
        .stroke_path(&arc(SWEEP_START, SWEEP_START + SWEEP_RANGE, r + 4.0), 3.0, LineCap::Round, ink.tick_dim);
    if t > 0.005 {
        shape = shape.stroke_path(&arc(SWEEP_START, end, r + 4.0), 3.0, LineCap::Round, accent);
    }
    if metal {
        shape
            .stroke(circle(r), 1.0, colors::rgb(0, 0, 0).alpha(0.45))
            .line(c, pointer, 4.5, LineCap::Round, colors::rgb(255, 255, 255).alpha(0.85))
            .line(c, pointer, 2.0, LineCap::Round, pointer_ink)
    } else {
        shape
            .fill(circle(r), ink.recessed)
            .stroke(circle(r), 1.0, ink.control_hover)
            .line(c, pointer, 2.5, LineCap::Round, pointer_ink)
    }
}

// ---- Bottom bar: transport, tempo light, clock, seek bar, volume -----------------------------

fn bottom_bar(s: &App) -> El {
    let ink = s.ink;
    let icon_ink = mark_ink(s);
    let play_icon = if s.is_playing { pause_icon(icon_ink) } else { play_icon(icon_ink) };
    let mut bar = row().key("bottom-bar").w_full().h(64.0).px(12.0).gap(10.0).items_center().fill(bar_fill(s));
    if s.skin == Skin::LiquidMetal {
        bar = bar.child(slab(GUNMETAL_FILL, [0.0; 4], 0.0).key("bottom-metal"));
    }
    bar = bar
        .child(transport_button(s, "prev", "Previous", prev_icon(icon_ink), |s: &mut App| s.previous()))
        .child(transport_button(s, "play", if s.is_playing { "Pause" } else { "Play" }, play_icon, |s: &mut App| {
            s.toggle_play()
        }))
        .child(transport_button(s, "next", "Next", next_icon(icon_ink), |s: &mut App| s.next()))
        .child(transport_button(s, "stop", "Stop", stop_icon(icon_ink), |s: &mut App| s.stop()))
        .child(pill(s, tempo_light(s)))
        .child(pill(s, text(clock(s.position_secs)).key("time").font(MONO).color(ink.text)))
        .child(seek_bar(s))
        .child(pill(s, text(clock(s.duration_secs)).key("duration").font(MONO).color(ink.text_weak)))
        .child(pill(s, volume_slider(s)));
    if s.skin == Skin::DiscoBall {
        bar = bar.child(slab(SPARKLES, [12.0, 7.0, 0.0, 0.0], 0.0).key("bottom-sparkles"));
    }
    bar
}

/// A round button with a vector icon, in the palette's control colours. With a skin on, the
/// button is a chrome disc (a slot painted under the icon) with a light hover sheen over it.
fn transport_button(s: &App, key: &'static str, label: &'static str, icon: Shape, on_press: fn(&mut App)) -> El {
    let ink = s.ink;
    let metal = s.skin.metal_parts();
    let mut b = el()
        .key(key)
        .role("button")
        .label(label)
        .focusable(true)
        .size(36.0, 36.0)
        .radius(18.0)
        .fill(if metal { Color::TRANSPARENT } else { ink.control })
        .hover(if metal { Color::TRANSPARENT } else { ink.control_hover })
        .press(ink.accent.alpha(0.35))
        .border(1.0, if metal { colors::rgb(0, 0, 0).alpha(0.5) } else { ink.tick_dim })
        .center()
        .cursor(Cursor::Pointer)
        .on_press(on_press);
    if metal {
        b = b
            .child(slab(CHROME_FILL, [8.0, 0.65, 0.0, 0.0], 18.0))
            // Hover feedback has to sit over the chrome, so it is a child painted after it.
            .child(
                el().pin(Some(0.0), Some(0.0), Some(0.0), Some(0.0))
                    .radius(18.0)
                    .fill(Color::TRANSPARENT)
                    .hover(colors::rgb(255, 255, 255).alpha(0.22)),
            );
    }
    b.child(canvas(icon.view_box(16.0, 16.0)).size(16.0, 16.0))
}

fn rect(x0: f32, y0: f32, x1: f32, y1: f32) -> Vec<[f32; 2]> {
    vec![[x0, y0], [x1, y0], [x1, y1], [x0, y1]]
}

fn play_icon(ink: Color) -> Shape {
    Shape::new().fill(vec![[4.5, 2.0], [14.0, 8.0], [4.5, 14.0]], ink)
}

fn pause_icon(ink: Color) -> Shape {
    Shape::new().fill(rect(3.0, 2.0, 6.5, 14.0), ink).fill(rect(9.5, 2.0, 13.0, 14.0), ink)
}

fn stop_icon(ink: Color) -> Shape {
    Shape::new().fill(rect(3.0, 3.0, 13.0, 13.0), ink)
}

fn prev_icon(ink: Color) -> Shape {
    Shape::new().fill(rect(2.0, 2.0, 4.0, 14.0), ink).fill(vec![[14.0, 2.0], [5.0, 8.0], [14.0, 14.0]], ink)
}

fn next_icon(ink: Color) -> Shape {
    Shape::new().fill(vec![[2.0, 2.0], [11.0, 8.0], [2.0, 14.0]], ink).fill(rect(12.0, 2.0, 14.0, 14.0), ink)
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
