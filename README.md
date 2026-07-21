# DJ Player — Developer / LLM README

Rust desktop MP3 audition tool (egui + cpal + symphonia + aubio).  
**Current release: v1.0.0** (single-deck player + analysis + file ops + installables).  
End-user docs: [`USER_MANUAL.md`](./USER_MANUAL.md).

---

## Commands

```bash
cargo run --release                 # GUI (default binary: dj_player)
DJ_PLAYER_DEV=1 cargo run --release # stderr [dev] logs
cargo run --release --bin analyze   # terminal analysis demo (single hardcoded path)
cargo build --release
cargo test
```

**Always prefer `--release` for analysis/playback demos** — debug is ~10–20× slower on DSP.

Apple Silicon aubio: use crate features `bindgen` + `builtin` (bundled C). Do not rely on Homebrew `aubio.h` layout with `pkg-config` alone.

---

## Releases (GitHub)

Push a version tag to trigger CI builds and publish a GitHub Release with assets:

```bash
git tag v1.0.0
git push origin v1.0.0
```

| Asset | Platform |
|-------|----------|
| `dj-player-macos-arm64.zip` | macOS Apple Silicon `.app` |
| `dj-player-windows-x64.zip` | Windows x64 portable `.exe` |

Workflow: [`.github/workflows/release.yml`](.github/workflows/release.yml).

Builds are **unsigned / ad-hoc** (not Apple-notarized, not Windows Authenticode). Fine for sharing with people who trust you; Gatekeeper / SmartScreen may warn.

---

## Local packaging

```bash
# macOS → target/release/bundle/osx/DJ Player.app
bash scripts/package-macos.sh

# Windows (run on Windows / CI) → target/release/dj-player-windows-x64.zip
bash scripts/package-windows.sh
```

macOS first-launch unblock:

```bash
xattr -cr ~/Applications/DJ\ Player.app
```

---

## Architecture (v1)

```
UI thread (eframe App::logic / App::ui)
  ├── playlist state, EQ gains, selection, autoplay
  ├── right-click: remove / move / write tags / delete from disk
  ├── polls worker + analysis channels
  └── PlaybackEngine::poll() applies Load/Play/Seek/EQ commands

Worker thread
  └── decode MP3 → TrackReady → UI caches audio + band waveform
      └── spawn BPM/key analysis → AnalysisComplete

cpal audio callback
  └── reads decoded buffer + LiveEq (biquad) → device
```

### Core types

| Type | Role |
|------|------|
| `DecodedAudio` | Interleaved `f32` samples + rate + channels |
| `TrackEntry` | Playlist row: path, tags, BPM/key/camelot, analysis state, listened_secs/played |
| `EqGains` / `LiveEq` | 4-band gains; band-sum EQ (0% all = silence; ~100% all = dry passthrough) |
| `BandWaveform` | Per-band min/max peaks for colored waveform |
| `PlaybackEngine` | Command channel + shared atomics; playhead advances **only** in cpal callback |

### Established patterns (do not reinvent)

1. **`DecodedAudio`** is the unit of analysis input — never pass naked sample slices without rate/channels.
2. **`downmix_to_mono()`** for mono-only analysis; share helpers instead of duplicating.
3. **Biquads are stateful** — `.run()` sample-by-sample in order; never `rayon` over filter state.
4. **Chunk-and-summarize** for waveforms / band heatmaps.
5. **Identity by `PathBuf`** in caches/workers (not playlist index) so append/remove stay coherent.
6. **`played` / autoplay chain**: accumulate `listened_secs` while cpal is playing; threshold **20s** (`TrackEntry::PLAYED_THRESHOLD_SECS`).
7. **Playlist load appends**; never wipe on second folder load.
8. **Click = select**, **double-click / Enter = play**, **Backspace/Delete = remove from list only**.
9. **Destructive file ops** (delete/move) go through confirm / folder picker — never via Backspace alone.

---

## Module map

| File | Responsibility |
|------|----------------|
| `src/main.rs` | `eframe::run_native` |
| `src/app.rs` | UI, playlist interaction, EQ knobs, waveform, autoplay, context menu |
| `src/playlist.rs` | Scan/append MP3s, `TrackEntry`, Camelot via key helper |
| `src/decode.rs` | Symphonia decode + ID3 probe (title/artist/track#) |
| `src/tags.rs` | Write BPM (`TBPM`) + key (`TKEY`) ID3v2 frames |
| `src/playback.rs` | cpal output, seek, volume, live EQ |
| `src/eq.rs` | `EqGains`, `LiveEq`, `band_waveform_peaks`, band colors |
| `src/knob.rs` | Circular EQ knob widget |
| `src/worker.rs` | Background decode jobs (path-keyed) |
| `src/bpm.rs` | aubio tempo |
| `src/key.rs` | Chroma / Krumhansl-style key + `to_camelot` |
| `src/bands.rs` | Offline band filter helpers (ASCII / analysis) |
| `src/waveform.rs` | Mono peak helpers |
| `src/dev.rs` | `DJ_PLAYER_DEV` logging |
| `src/bin/analyze.rs` | Headless analysis smoke test |

---

## Tech stack

- **GUI:** egui / eframe **0.35** — `App::logic` + `App::ui` (not old `update`-only). Panels: `egui::Panel::{top,bottom}`, `CentralPanel::show(ui, …)`.
- **Audio I/O:** cpal 0.18  
- **Decode:** symphonia (`mp3`); returns `Result` — malformed packets are skipped, not panicked  
- **Tags:** `id3` crate for write-back of BPM/key  
- **DSP:** biquad; BPM via `aubio-rs`  
- **Key:** rustfft chroma (libKeyFinder still planned, not integrated)

---

## Conventions for agents

- Address code changes minimally; match existing style.
- Explain Rust ownership/`Result` when non-obvious (owner is backend-experienced, new to Rust).
- Verify crate APIs against installed versions before inventing methods.
- Prefer FFI to proven DSP libs for BPM/key over greenfield algorithms (product decision).
- Do not commit unless asked.

---

## Known v1 limitations

- Single deck / one playing buffer.
- No tempo/pitch control.
- Tag write-back is BPM + Key only (not Camelot / custom fields).
- Band EQ uses overlapping filters; unity uses dry passthrough to avoid coloration.
- Short tracks (<20s) never get `played` ✓ and won't chain autoplay under current rule.
- Releases are unsigned (notarization / Authenticode deferred).

---

## Version roadmap (high level)

| Version | Focus |
|---------|--------|
| **v1** *(current)* | Playlist, playback, waveform+EQ, BPM/key/Camelot, autoplay, right-click file ops, tag write-back, Mac+Windows GitHub Releases |
| **v2** *(parked on `wip/v2-parked`)* | Multi-deck, tempo fader, WSOLA pitch-lock, sync — seek desync under investigation |
| **v3+** | Notarized DMG, Authenticode, Rekordbox export, richer per-deck UI |
