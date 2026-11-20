//! Qinip on Rune (spike): the entry point. It builds the Rune `AppSpec`, wires the frame
//! hooks (`setup` hands over the waker for background threads, `poll` runs before every
//! frame, `on_key` takes the space bar), names the headless scenes the PNG renders use, and
//! attaches the compositor (`gfx`) that paints the Liquid Metal and Disco Ball skins.
//!
//! Run the window:   cargo run -p qinip-rune [-- /folder/with/mp3s]
//! Headless render:  cargo run -p qinip-rune -- --snap out.png --scene loaded /folder
//! Element tree:     cargo run -p qinip-rune -- --tree tree.json --scene loaded /folder
//! Skins headless:   cargo run -p qinip-rune -- --snap out.png --scene metal /folder
//!                   cargo run -p qinip-rune -- --snap out.png --scene disco /folder
//!
//! The modules: `state` (playlist, worker, analysis, playback, the skin choice), `view` (the
//! screen), `colors` (Qinip's palette as Rune colours) and `gfx` (the compositor that turns the
//! view's effect slots into rune-fx liquid metal and sparkles).

mod colors;
mod gfx;
mod state;
mod view;

use std::time::Duration;

use rune::{AppSpec, Key, KeyEvent, Tx, Waker};

use state::{App, Skin, WINDOW};

fn setup(s: &mut App, waker: Waker) {
    s.attach_waker(waker);
}

/// Before every frame: take what the threads sent and mirror the playback status. While a
/// track plays, ask for another frame in 33 ms so the clock, playhead and tempo light move.
fn poll(s: &mut App, _secs: f64, tx: &mut Tx) -> bool {
    let changed = s.tick();
    if s.is_playing {
        tx.wake_after(Duration::from_millis(33));
    }
    // Headless runs (`--script`, `--snap`) step a virtual clock as fast as the GPU allows,
    // while the decode worker runs in real time. While work is outstanding, hold each frame
    // for a frame's worth of real time so a scripted run sees the results land.
    if s.has_background_work() && rune::tooling::headless_run().is_some() {
        std::thread::sleep(Duration::from_millis(16));
    }
    changed
}

fn on_key(s: &mut App, event: &KeyEvent, _tx: &mut Tx) -> bool {
    match &event.key {
        Key::Text(t) if t == " " => {
            s.toggle_play();
            true
        }
        _ => false,
    }
}

fn main() {
    let spec = AppSpec::<App> {
        // Named states for `--snap --scene <name>`; each modifies the fresh state.
        scenes: &[
            ("empty", |_| {}),
            ("loaded", |s| s.scene_load_sync()),
            ("playing", |s| {
                s.scene_load_sync();
                s.scene_play_preview();
            }),
            // The two skins, on the playing scene.
            ("metal", |s| {
                s.scene_load_sync();
                s.scene_play_preview();
                s.set_skin(Skin::LiquidMetal);
            }),
            ("disco", |s| {
                s.scene_load_sync();
                s.scene_play_preview();
                s.set_skin(Skin::DiscoBall);
            }),
        ],
        setup: Some(setup),
        poll: Some(poll),
        on_key: Some(on_key),
        min_size: Some((820.0, 520.0)),
        ..AppSpec::new("Qinip on Rune", WINDOW, App::new, view::view)
    };
    // `run_with` attaches the compositor: it draws every frame, in the window and headless
    // alike, and paints the skins' effect slots. With the Flat skin there are none to paint.
    rune::run_with(spec, gfx::Gfx::default());
}
