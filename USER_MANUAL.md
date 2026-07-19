# DJ Player — User Manual (v1)

A desktop MP3 player for auditioning packs: playlist, waveform, EQ, BPM/key analysis.

---

## Install & run

Requires a Mac (or other machine with Rust) and speakers/headphones.

```bash
# From the project folder
cargo run --release
```

Optional debug logging in the terminal:

```bash
DJ_PLAYER_DEV=1 cargo run --release
```

---

## Quick start

1. Click **Load Folder** (or drag a folder / MP3s onto the window).
2. Wait for the **Title** bar to fill (decode → analyze). BPM / Key / Camelot appear when done.
3. **Double-click** a track (or select it and press **Enter**) to play.
4. Use the **waveform** to seek; use **EQ knobs** to shape the sound.
5. With **Autoplay** on, the next track starts after the current one ends (if you listened long enough — see below).

---

## Playlist

| Action | Result |
|--------|--------|
| **Click** | Select the track (does not start playback) |
| **Double-click** or **Enter** | Play the selected track |
| **Backspace** (Mac) / **Delete** | Remove from playlist only — **never deletes the file** |
| **Load Folder** / drop files | **Adds** to the existing list (skips duplicates) |

### Columns

| Column | Meaning |
|--------|---------|
| **#** | Track number from tags (if present) |
| **Artist** | From tags, or “Unknown Artist” |
| **Title** | From tags, or filename; bar fills while loading/analyzing |
| **Duration** | Length |
| **BPM** | Estimated tempo |
| **Key** | Estimated musical key |
| **Camelot** | Same key as Camelot wheel code (e.g. `8A`) |
| **Played** | ✓ after you’ve listened ≥ ~20 seconds (pauses don’t count) |

Hover the title to see the original filename.

---

## Transport

- **Play/Pause** — top bar (also resumes the current deck track).
- **Seek bar** — bottom; drag to jump in time.
- **Vol** — master volume.
- **Autoplay** — when on, plays the next playlist row after the current track finishes, but only if that track was “played” (≥ 20s listened).

---

## Waveform & EQ

- Colored layers = frequency bands:
  - **Low** (orange) · **LowMid** (gold) · **HiMid** (green) · **Hi** (cyan)
- **Click the waveform** to seek.
- **EQ knobs** (0%–200%, default 100%):
  - Change what you hear **and** how visible that color is on the waveform.
  - All knobs at **0%** → silence.
  - Double-click a knob → reset that band to 100%.
  - **Reset EQ** → all bands to 100%.

---

## Tips

- Prefer **`cargo run --release`** — analysis is much faster than debug builds.
- Large folders analyze in the background; you can play a track as soon as its title bar starts filling / it’s decoded.
- Removing a track only affects the session playlist.

---

## Not in v1

- Tempo (BPM) pitch control  
- Multiple decks / mixing  
- Writing analysis back into file tags  
- Installer / App Store packaging  

These are planned for later versions.
