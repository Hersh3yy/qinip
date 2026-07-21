# DJ Player — User Manual (v1.0.0)

A desktop MP3 player for auditioning packs: playlist, waveform, EQ, BPM/key analysis, and file housekeeping.

---

## Install

### From GitHub Releases (recommended)

Download the zip for your OS from the project's **Releases** page:

| File | What to do |
|------|------------|
| `dj-player-macos-arm64.zip` | Unzip → drag `DJ Player.app` to Applications |
| `dj-player-windows-x64.zip` | Unzip → run `DJ Player.exe` |

**macOS Gatekeeper:** if macOS blocks the app on first launch:

```bash
xattr -cr ~/Applications/DJ\ Player.app
```

Then right-click → Open → Open.

**Windows SmartScreen:** choose More info → Run anyway (unsigned build).

### From source (requires Rust)

```bash
cargo run --release
```

### Build installers locally

```bash
# macOS
bash scripts/package-macos.sh
# → target/release/bundle/osx/DJ Player.app

# Windows (on a Windows machine)
bash scripts/package-windows.sh
# → target/release/dj-player-windows-x64.zip
```

---

## Quick start

1. Click **Load Folder** (or drag a folder / MP3s onto the window).
2. Wait for the **Title** bar to fill (decode → analyze). BPM / Key / Camelot appear when done.
3. **Double-click** a track (or select it and press **Enter**) to play.
4. Use the **waveform** to seek; use **EQ knobs** to shape the sound.
5. With **Autoplay** on, the next track starts after the current one ends (if you listened long enough — see below).
6. **Right-click** a track title for file actions (move, write tags, delete).

---

## Playlist

| Action | Result |
|--------|--------|
| **Click** | Select the track (does not start playback) |
| **Double-click** or **Enter** | Play the selected track |
| **Backspace** (Mac) / **Delete** | Remove from playlist only — **never deletes the file** |
| **Load Folder** / drop files | **Adds** to the existing list (skips duplicates) |
| **Right-click** title | Context menu (see below) |

### Right-click menu

| Action | Result |
|--------|--------|
| **Play** | Start the track |
| **Remove from playlist** | Drop from the list only (file stays on disk) |
| **Move to folder…** | Pick a destination folder; moves the MP3 on disk and updates the playlist path |
| **Write BPM/Key tags** | Writes analysis into the file's ID3 tags (`TBPM`, `TKEY`) — requires analysis to be finished |
| **Delete from disk…** | Permanently deletes the MP3 after a confirm dialog — **cannot be undone** |

### Columns

| Column | Meaning |
|--------|---------|
| **#** | Track number from tags (if present) |
| **Artist** | From tags, or "Unknown Artist" |
| **Title** | From tags, or filename; bar fills while loading/analyzing |
| **Duration** | Length |
| **BPM** | Estimated tempo |
| **Key** | Estimated musical key |
| **Camelot** | Same key as Camelot wheel code (e.g. `8A`) |
| **Played** | ✓ after you've listened ≥ ~20 seconds (pauses don't count) |

Hover the title to see the original filename.

---

## Transport

- **Play/Pause** — top bar (also resumes the current deck track).
- **Seek bar** — bottom; drag to jump in time.
- **Vol** — master volume.
- **Autoplay** — when on, plays the next playlist row after the current track finishes, but only if that track was "played" (≥ 20s listened).

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

- Prefer `cargo run --release` or the release builds — analysis is much faster than debug.
- Large folders analyze in the background; you can play a track as soon as its title bar starts filling.
- Backspace only removes from the session playlist; use right-click → Delete from disk for permanent removal.
- Malformed MP3 packets are skipped gracefully — the player will still load the rest of the file.

---

## Not in v1

- Tempo / pitch control or multi-deck mixing
- Apple notarization / Windows Authenticode (Gatekeeper / SmartScreen may warn)
- Writing Camelot or custom tags (BPM + Key only)
- Rekordbox export

These are deferred to later versions.
