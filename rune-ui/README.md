# Qinip on Rune (spike)

Qinip's main screen rebuilt on [Rune](https://github.com/anomalyco/rune), a GPU-rendered Rust UI
framework that is still pre-release. It reuses the `qinip` library for everything that is not
UI (decoding, waveform peaks, BPM/key analysis, playback, palettes), so this folder only holds
the screen: `src/main.rs` (entry, scenes, hooks), `src/state.rs` (playlist, worker, analysis,
playback, the skin choice), `src/view.rs` (the screen), `src/colors.rs` (palette to Rune
colours), `src/gfx.rs` (the compositor that paints the two skins).

This is **not** the shipped app. The egui app at the repo root is what 1.1.0 is. This spike is
the candidate for a 2.0, built on 2026-11-17 against Rune's `dev` commit `3d037b49`. On
2026-11-20 it grew two optional looks ("skins", below) painted with Rune's effect crate
`rune-fx` through a small compositor in `src/gfx.rs`.

## Run it

From this folder (it is its own workspace, with its own `Cargo.lock`; the first build takes
about three minutes, later ones about twenty seconds):

```bash
cd rune-ui && cargo run --release
cd rune-ui && cargo run --release -- "/path/to/a/folder/of/mp3s"     # open with that folder loaded
```

Headless, no window (Rune's agent tooling): a PNG of a scene, or the element tree as JSON.

```bash
cd rune-ui && cargo run --release -- --snap out.png --scene playing "/path/to/folder"
cd rune-ui && cargo run --release -- --tree tree.json --scene loaded "/path/to/folder"
```

Scenes: `empty`, `loaded`, `playing`, `metal` (the playing scene in the Liquid Metal skin),
`disco` (the same in the Disco Ball skin). Add `--frames 60 --fps 60` to any of them and the
summary on stderr ends with the compositor's line: the GPU time per frame, averaged over the run.

## What works

Load Folder (native dialog), file drop, Clear Playlist, theme picker (all seven palettes), a
virtualised playlist table (click selects, double-click or Return plays, right-click menu:
Play, Remove, Open enclosing folder, Write BPM/Key tags, Re-analyze; click a header to sort,
click again to flip), the EQ as a mixer channel strip beside the waveform (HI at the top, LOW
at the bottom like a DJM channel; a pot per band with a 0 dB detent mark, a bipolar arc in the
band's colour and a dB readout; drag up/down turns it, double-click or right-click resets,
RESET clears all four; they shape the sound and the waveform), the four-band waveform with
click/drag seek, Previous / Play-Pause / Next / Stop, tempo light,
volume slider, seek bar, space bar for play/pause.

Two skins on top of the palettes, picked from the same Theme menu as two entries after a
separator; picking a palette keeps the skin and picking a skin keeps the palette, and picking
the skin that is on turns it off again (back to the plain look, "Flat"):

- **Liquid Metal**: the top and bottom bars become dark gunmetal slabs, the four EQ pots and
  the transport buttons chrome, the waveform screen gets a chrome tube around it and the
  playing row's title a gold one. Labels that would sit straight on metal go in dark pills.
- **Disco Ball**: the whole window background is a wall of small chrome mirror tiles (28 pt,
  2 pt apart, each tilted on its own, their glints wandering), with the list, the waveform panel
  and the bars on translucent scrims so the text stays readable, stars twinkling over both
  bars, and the chrome pots and buttons of the metal skin. Bigger windows get bigger tiles so
  the wall never exceeds about 1,500 tiles; at 1440 x 900 on a Retina display the frame costs
  about 8 ms of GPU time on an M1 Pro (3.5 ms of which is the plain screen).

How it works, for the curious: the view places "effect slots" (boxes that draw nothing) under
or over the ordinary elements, and the compositor in `src/gfx.rs` paints a rune-fx layer in
each one, so the liquid metal animates on its own without the view being rebuilt. The Flat skin
places no slots and draws exactly as before.

Not ported: beat loops, multi-select, folder headers, rekordbox playlists, Move to folder,
Delete from disk, settings persistence, media keys, About. All of that still lives only in the egui app.

## Verdict (2026-11-17)

*Update 2026-11-20: Hiren chose to continue 2.0 on Rune regardless; since then: the two skins, sortable headers, the mixer strip. The caveats below still hold.*

Stay on egui for shipping. Rune compiled first try and the headless tooling is excellent (file
dialogs, drops and menus can be scripted; egui_kittest cannot), the table and the rendering are
better than the hand-rolled egui playlist, and the result looks more native. Against it: 296
crates and a three-minute clean build, a 12 MB bigger binary, no slider or knob widgets in the
kit, a moving `dev` branch with breaking changes every week and no crates.io release, and a
`[patch]` table of five forks that has to be re-pinned with every update. Re-run this spike
against a newer revision in a few months, or when Rune publishes to crates.io; move then if the
churn has settled.

## Updating the Rune pin

Change the `rev` in all seven places in `Cargo.toml` (the `rune`, `rune-kit` and `rune-fx`
dependencies and the `[patch.crates-io]` table) to the same commit, then `cargo build --release`
and fix what the changelog broke.
