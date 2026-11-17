# Qinip on Rune (spike)

Qinip's main screen rebuilt on [Rune](https://github.com/anomalyco/rune), a GPU-rendered Rust UI
framework that is still pre-release. It reuses the `qinip` library for everything that is not
UI (decoding, waveform peaks, BPM/key analysis, playback, palettes), so this folder only holds
the screen: `src/main.rs` (entry, scenes, hooks), `src/state.rs` (playlist, worker, analysis,
playback), `src/view.rs` (the screen), `src/colors.rs` (palette to Rune colours).

This is **not** the shipped app. The egui app at the repo root is what 1.1.0 is. This spike is
the candidate for a 2.0, built on 2026-11-17 against Rune's `dev` commit `3d037b49`.

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

Scenes: `empty`, `loaded`, `playing`.

## What works

Load Folder (native dialog), file drop, Clear Playlist, theme picker (all seven palettes), a
virtualised playlist table (click selects, double-click or Return plays), the four-band
waveform with click/drag seek, Previous / Play-Pause / Next / Stop, tempo light, volume slider,
seek bar, space bar for play/pause.

Not ported: EQ knobs, beat loops, multi-select, folder headers, rekordbox, tag writing, settings,
media keys, About. All of that still lives only in the egui app.

## Verdict (2026-11-17)

Stay on egui for shipping. Rune compiled first try and the headless tooling is excellent (file
dialogs, drops and menus can be scripted; egui_kittest cannot), the table and the rendering are
better than the hand-rolled egui playlist, and the result looks more native. Against it: 296
crates and a three-minute clean build, a 12 MB bigger binary, no slider or knob widgets in the
kit, a moving `dev` branch with breaking changes every week and no crates.io release, and a
`[patch]` table of five forks that has to be re-pinned with every update. Re-run this spike
against a newer revision in a few months, or when Rune publishes to crates.io; move then if the
churn has settled.

## Updating the Rune pin

Change the `rev` in all six places in `Cargo.toml` (the `rune` and `rune-kit` dependencies and
the `[patch.crates-io]` table) to the same commit, then `cargo build --release` and fix what the
changelog broke.
