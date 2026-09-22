# Qinip

Free MP3 pack audition player for macOS and Windows — playlist, coloured waveform, 4-band EQ, BPM/key analysis, gapless autoplay.

<p align="center">
  <img src="assets/qinip-screenshot-readme.png" alt="Qinip — playlist, waveform, EQ knobs, and transport controls" width="900">
</p>

---

## Why Qinip?

<p align="center">
  <img src="assets/qinip-fruit-readme.png" alt="A bunch of qinip fruit — green shells cracked open to show the orange pulp inside" width="240">
</p>

**Qinip** is how I say *guinep* — the small Caribbean fruit that grows in bunches. You crack one open to see the juicy bit inside.

Same idea here: load a bunch of songs, crack one open, see the waveform inside. I built it because **Clementine** died on Apple Silicon and **Strawberry** wants €10/month. This one's free.

---

## Features

- Load folders of MP3s — playlist with sortable columns (BPM, key, Camelot, duration…)
- Coloured 4-band waveform + EQ knobs — see and shape what's inside each track
- BPM, musical key, and Camelot wheel codes — analyzed in the background
- Transport controls, gapless autoplay, write BPM/key back to file tags
- Right-click tracks — move, tag, open folder, remove from list, delete from disk

---

## Download

Get the latest release from **[GitHub Releases](https://github.com/Hersh3yy/qinip/releases)**:

| Platform | File | What to do |
|----------|------|------------|
| **macOS** (Apple Silicon) | `qinip-macos-arm64.zip` | Unzip → drag `Qinip.app` to Applications |
| **Windows** (x64) | `qinip-windows-x64.zip` | Unzip → run `Qinip.exe` |

**macOS first launch:** if Gatekeeper blocks the app:

```bash
xattr -cr ~/Applications/Qinip.app
```

Then right-click → Open → Open.

**Windows:** SmartScreen may warn — More info → Run anyway (unsigned build).

---

## Quick start

1. Click **Load Folder** (or drag a folder / MP3s onto the window).
2. Wait for the **Title** bar to fill (decode → analyze). BPM / Key / Camelot appear when done.
3. **Double-click** a track (or select it and press **Enter**) to play.
4. Use the **waveform** to seek; use **EQ knobs** and **Transport** buttons beside them.
5. With **Autoplay** on, the next track starts with minimal gap (once you've listened long enough — see Columns → Played).
6. **Right-click** a track title for file actions (move, write tags, delete).

---

## Using it

### Playlist

| Action | Result |
|--------|--------|
| **Click** | Select the track (does not start playback) |
| **Shift+click** / **Cmd+click** (Ctrl on Windows) | Extend the selection as a range / toggle one row |
| **Cmd+A** (Ctrl+A) | Select every track; **Esc** collapses back to one |
| **Double-click** or **Enter** | Play the selected track |
| **Backspace** (Mac) / **Delete** | Remove the selected track(s) from the playlist only — **never deletes files** |
| **Load Folder** / drop folders or files | **Adds** to the existing list (skips duplicates). Each folder becomes its own group with a header in the unsorted view; drop several at once and they all load. |
| **Right-click** title | Context menu (see below) |
| **Column header click** | Sort asc → desc → off (click again to cycle) |
| **Drag column header edge** | Resize column |

### Right-click menu

| Action | Result |
|--------|--------|
| **Play** | Start the track |
| **Remove from playlist** | Drop from the list only (file stays on disk) |
| **Open enclosing folder** | Opens the containing folder in Finder / Explorer |
| **Move to folder…** | Pick a destination; moves the MP3 on disk and updates the playlist path |
| **Write BPM/Key tags** | Writes analysis into the file's ID3 tags (`TBPM`, `TKEY`) — requires analysis finished |
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

### Transport

- **Transport buttons** (beside the EQ knobs): Previous · Play/Pause · Next · Stop · ±10s seek.
- **Seek bar** (bottom): drag to jump in time.
- **Vol**: master volume.
- **Autoplay**: when on, plays the next playlist row after the current track finishes (gapless when the next file is already decoded), but only if that track was "played" (≥ 20s listened).

### Waveform & EQ

- Colored layers = frequency bands: **Low** (orange) · **LowMid** (gold) · **HiMid** (green) · **Hi** (cyan).
- **Click the waveform** to seek.
- **EQ knobs** (0%–200%, default 100%): change what you hear **and** how visible that colour is on the waveform. All knobs at **0%** → silence. Double-click or right-click a knob to reset that band to 100%; **Reset EQ** resets all bands.

### Verifying BPM/Key tag writes

Qinip writes `TBPM` and `TKEY` into the MP3. **macOS Finder Get Info doesn't show these fields.** To verify a write:

- Use **Mp3tag**, **Rekordbox**, or **Serato**, or
- Terminal: `id3info yourtrack.mp3 | grep -E 'TBPM|TKEY'`

### Not yet in Qinip

Tempo/pitch or multi-deck mixing · Apple notarization / Windows Authenticode (Gatekeeper / SmartScreen may warn) · Camelot/custom tag writing (BPM + Key only) · Rekordbox export.

---

## Build from source

Requires [Rust](https://rustup.rs):

```bash
cargo run --release
```

`--release` matters — analysis is much faster than debug. Dev logging: `QINIP_DEV=1 cargo run --release`.

Package installers locally:

```bash
bash scripts/package-macos.sh    # → target/release/bundle/osx/Qinip.app
bash scripts/package-windows.sh  # → target/release/qinip-windows-x64.zip (on Windows)
```

Architecture, roadmap, and dev notes live in [`PROJECT.md`](./PROJECT.md).

---

## Support

Qinip is free — download, use, share. If it saves you time or money, tips are welcome in the form of information, knowledge, and/or compliments *(donation link coming soon)*.
