# Qinip — User Manual (v1.0.0)

A desktop MP3 player for auditioning packs: playlist, waveform, EQ, BPM/key analysis, and file housekeeping.

Named after the **qinip** fruit from Sint Maarten — crack open a folder, taste the tracks, keep the sweet ones.

---

## Install

### From GitHub Releases (recommended)

Download the zip for your OS from the project's **Releases** page:

| File | What to do |
|------|------------|
| `qinip-macos-arm64.zip` | Unzip → drag `Qinip.app` to Applications |
| `qinip-windows-x64.zip` | Unzip → run `Qinip.exe` |

**macOS Gatekeeper:** if macOS blocks the app on first launch:

```bash
xattr -cr ~/Applications/Qinip.app
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
# → target/release/bundle/osx/Qinip.app

# Windows (on a Windows machine)
bash scripts/package-windows.sh
# → target/release/qinip-windows-x64.zip
```

---

## Quick start

1. Click **Load Folder** (or drag a folder / MP3s onto the window).
2. Wait for the **Title** bar to fill (decode → analyze). BPM / Key / Camelot appear when done.
3. **Double-click** a track (or select it and press **Enter**) to play.
4. Use the **waveform** to seek; use **EQ knobs** and **Transport** buttons beside them.
5. With **Autoplay** on, the next track starts with minimal gap (if you listened long enough — see below).
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
| **Column header click** | Sort asc → desc → off (click again to cycle) |
| **Drag column header edge** | Resize column |

### Right-click menu

| Action | Result |
|--------|--------|
| **Play** | Start the track |
| **Remove from playlist** | Drop from the list only (file stays on disk) |
| **Open enclosing folder** | Opens the folder containing the file in Finder / Explorer |
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

- **Transport buttons** — beside the EQ knobs: Previous · Play/Pause · Next · Stop · ±10s seek.
- **Seek bar** — bottom; drag to jump in time.
- **Vol** — master volume.
- **Autoplay** — when on, plays the next playlist row after the current track finishes (gapless when the next file is already decoded), but only if that track was "played" (≥ 20s listened).

---

## Waveform & EQ

- Colored layers = frequency bands:
  - **Low** (orange) · **LowMid** (gold) · **HiMid** (green) · **Hi** (cyan)
- **Click the waveform** to seek.
- **EQ knobs** (0%–200%, default 100%):
  - Change what you hear **and** how visible that color is on the waveform.
  - All knobs at **0%** → silence.
  - Double-click or **right-click** a knob → reset that band to 100%.
  - **Reset EQ** → all bands to 100%.

---

## Write BPM/Key tags — where to verify

Qinip writes `TBPM` and `TKEY` into the MP3 file. **macOS Finder Get Info does not show these fields.**

To verify a write worked:
- Use **Mp3tag**, **Rekordbox**, or **Serato**
- Or terminal: `id3info yourtrack.mp3 | grep -E 'TBPM|TKEY'`

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

---

## Support

Qinip is free. If it's useful, consider leaving a tip — see the project README for links.
