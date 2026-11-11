# Changelog

Every release of Qinip, newest first. Format: [Keep a Changelog](https://keepachangelog.com/en/1.1.0/); version numbers: [Semantic Versioning](https://semver.org/).

Work in progress goes under **Unreleased**. `scripts/release.sh` turns that section into the next version when a release is cut, and the release page on GitHub shows exactly that section.

## [Unreleased]

### Added
- Send songs to rekordbox: right-click → Add to Rekordbox playlist. Qinip writes a rekordbox XML file that rekordbox 6/7 can import; a Rekordbox window walks you through the one-time setup.
- Shuffle and Repeat (off, all, one) buttons next to the transport controls.
- Themes: Match system, Dark, Light, Solarized Dark, Solarized Light.
- Select many songs: Shift+click, Cmd/Ctrl+click, Cmd/Ctrl+A, Esc. Backspace/Delete removes them all from the list.
- Folder headers in the playlist when songs come from more than one folder.
- Volume fader with mute, next to the transport buttons.
- Keyboard and headphone play/pause keys and macOS Now Playing (Control Center).
- About window with license information.
- Clear Playlist button (files stay on disk).
- Qinip reads the BPM and key already stored in your files (`TBPM`/`TKEY`, whether written as `Am`, `8A` or `A Minor`), so a tagged library shows them instantly with no analysis. Hover a value to see where it came from; right-click → Re-analyze BPM/Key lets Qinip listen anyway.
- A tempo light next to the transport pulses on the beats Qinip heard; the 2/4/8/16 loop buttons snap to that beat grid.
- Two more themes: Red and Forest Green.
- Windows installer (`qinip-setup-x64.exe`): Start menu entry, uninstaller in Settings → Apps, no admin needed; the exe has an icon now. The zip stays as the portable version.

### Changed
- Writing BPM/Key into your MP3 files is now opt-in (off by default). Qinip no longer changes your files unless you ask.
- Volume, autoplay, theme, shuffle, repeat and the tag-writing choice are remembered between launches.
- Dropping several folders at once loads all of them.
- Qinip is now licensed GPL-3.0-or-later.
- New tempo detection, Qinip's own (aubio is no longer used): within 0.1 BPM of the tagged value on every test track, where the old one read 1.5–2.4 % high. Building from source needs no C toolchain anymore.
- Keys are written to files in the short form other DJ tools use (`Am`, `F#`) instead of `A Minor`, and the file's own ID3 version is kept.
- The 1-2-3-4 bar counter is gone; it counted from 0:00 and drifted on most tracks. The tempo light replaces it until a real bar counter lands.
- Decoded audio is kept only for the playing and upcoming tracks, so a big folder no longer eats gigabytes of memory; waveform building moved off the UI thread.
- Shuffle starts a fresh "every track once" round after each pass.

### Fixed
- Qinip no longer crashes at launch when no audio output is available; it shows a Retry audio button.
- Moving a song to a folder on another drive (USB stick, external disk) now works. A moved song leaves the playlist, and if the destination already has a file with that name you are asked whether to replace it.
- Transport buttons and the Bar readout are no longer cut off.
- Windows: Qinip crashed at launch (the media-keys integration needed a window handle), and a console window opened behind the app.
- MP3s with a low sample rate (8–12 kHz voice memos, old sample packs) crashed the app.
- Keyboard shortcuts no longer fire while you type a playlist name (Backspace used to remove the selected tracks).
- Writing tags can no longer wipe a file's existing tags when they fail to parse.
- Double-clicking a second track while the first was still loading no longer makes the first one start later.
- Loading a folder while the list is sorted keeps it sorted. Clear Playlist also resets the player (no ghost 0:00 / 4:15).
- Short tracks (under 25 s) can count as played, so autoplay keeps chaining; listening time follows the playhead, so a hidden window still counts.
- Rekordbox playlists are stored apart from settings, and the XML file is backed up (`.xml.bak`) before each rewrite.

## [1.0.0] - 2026-07-21

First public release (then called DJ Player): playlist with sortable columns, coloured 4-band waveform, EQ knobs, background BPM/key/Camelot analysis, gapless autoplay, beat loops, and right-click file actions.

[Unreleased]: https://github.com/Hersh3yy/qinip/compare/v1.0.0...HEAD
[1.0.0]: https://github.com/Hersh3yy/qinip/releases/tag/v1.0.0
