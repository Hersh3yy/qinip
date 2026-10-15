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

### Changed
- Writing BPM/Key into your MP3 files is now opt-in (off by default). Qinip no longer changes your files unless you ask.
- Volume, autoplay, theme, shuffle, repeat and the tag-writing choice are remembered between launches.
- Dropping several folders at once loads all of them.
- Qinip is now licensed GPL-3.0-or-later (it includes aubio, which is GPL).

### Fixed
- Qinip no longer crashes at launch when no audio output is available; it shows a Retry audio button.
- Moving a song to a folder on another drive (USB stick, external disk) now works.
- Transport buttons and the Bar readout are no longer cut off.

## [1.0.0] - 2026-07-21

First public release (then called DJ Player): playlist with sortable columns, coloured 4-band waveform, EQ knobs, background BPM/key/Camelot analysis, gapless autoplay, beat loops, and right-click file actions.

[Unreleased]: https://github.com/Hersh3yy/qinip/compare/v1.0.0...HEAD
[1.0.0]: https://github.com/Hersh3yy/qinip/releases/tag/v1.0.0
