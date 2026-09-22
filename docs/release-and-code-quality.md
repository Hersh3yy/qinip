# Qinip: trustworthy macOS release + code quality

Written 2026-09-22 from a read-only audit of `src/` and the packaging scripts, plus web checks of
current Apple/Microsoft tooling. The short version lives in `PROJECT.md` (roadmap items r1..r10);
this is the depth.

## 1. Trustworthy macOS release checklist

Goal: a stranger downloads `Qinip.app`, double-clicks, and sees no "cannot be opened" or
"unidentified developer" dialog. macOS checks three things at first launch: a Developer ID
signature, the hardened runtime, and a notarization ticket. `scripts/package-macos.sh` currently
does none of them (it ad-hoc signs with `-s -` at line 88 and then tells users to run `xattr -cr`).

### Enrollment and certificate
1. Join the Apple Developer Program (99 USD/year, individual is fine).
2. Xcode > Settings > Accounts > Manage Certificates > create **Developer ID Application**
   (not "Apple Development", not "Mac App Distribution").
3. Confirm: `security find-identity -v -p codesigning` shows
   `Developer ID Application: Hiren Budhrani (TEAMID)`.
4. Create an App Store Connect API key (Users and Access > Integrations > Keys, role Developer)
   for notarytool. An app-specific password also works; the API key is what Apple recommends for CI.

### Signing
The binary is a single Mach-O (aubio is compiled in via the `builtin` feature), so there are no
dylibs to sign separately and `--deep` is unnecessary (Apple deprecates it anyway).

```bash
codesign --force --timestamp --options runtime \
  --entitlements scripts/qinip.entitlements \
  --sign "Developer ID Application: Hiren Budhrani (TEAMID)" \
  "$APP_DIR"
codesign --verify --deep --strict --verbose=2 "$APP_DIR"
```

`--options runtime` = hardened runtime, `--timestamp` = secure timestamp. Both are hard
notarization requirements.

**Entitlements: keep them empty.** Rust is ahead-of-time compiled; eframe's Metal backend and
cpal's CoreAudio backend generate no code at runtime, so `allow-jit` and
`allow-unsigned-executable-memory` are not needed. `device.audio-input` is for microphone capture;
Qinip only plays. `disable-library-validation` is not needed because nothing is dynamically
loaded. A minimal `scripts/qinip.entitlements`:

```xml
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0"><dict/></plist>
```

### Info.plist
What the script writes is already close: `CFBundleIdentifier`, `LSMinimumSystemVersion` 11.0,
`NSHighResolutionCapable`, `LSApplicationCategoryType` are present. No `NS*UsageDescription` is
required: Now Playing and media keys go through `MPRemoteCommandCenter`, which needs no privacy
string, and Bluetooth headphone buttons arrive through the same route (not CoreBluetooth). Bump
`CFBundleVersion` per build, and set `MACOSX_DEPLOYMENT_TARGET=11.0` in the build env so the
binary's min version matches the plist.

### Media keys need the bundle
`SystemMedia` (`src/media.rs`) wraps souvlaki, which registers with `MPRemoteCommandCenter`.
macOS only routes hardware media keys and Control Center's Now Playing to a real `.app` with a
`CFBundleIdentifier`. `cargo run` never receives them. Test media keys only against the packaged
app, ideally on a clean user account.

### Notarize and staple

```bash
xcrun notarytool store-credentials qinip-notary \
  --key AuthKey_XXXX.p8 --key-id XXXX --issuer <issuer-uuid>
ditto -c -k --keepParent "$APP_DIR" qinip-macos-arm64.zip
xcrun notarytool submit qinip-macos-arm64.zip --keychain-profile qinip-notary --wait
xcrun notarytool log <submission-id> --keychain-profile qinip-notary   # only on failure
xcrun stapler staple "$APP_DIR"
spctl --assess --type execute --verbose=4 "$APP_DIR"   # must say: accepted ... Notarized Developer ID
```

`altool --notarize-app` was switched off in November 2023; `notarytool` is the only path. After
stapling, re-zip the stapled `.app` (the ticket lives inside the bundle), or build a dmg:

```bash
hdiutil create -volname Qinip -srcfolder "$APP_DIR" -ov -format UDZO Qinip.dmg
codesign --sign "Developer ID Application: ..." Qinip.dmg
xcrun notarytool submit Qinip.dmg --keychain-profile qinip-notary --wait
xcrun stapler staple Qinip.dmg
```

A dmg is the friendlier download; a signed, stapled zip is acceptable. Avoid `.pkg` (a 2026
Tahoe report had notarized pkgs rejected while `.app` bundles passed).

### What `scripts/package-macos.sh` is missing, concretely
- Line 88: ad-hoc `-s -`, no `--options runtime`, no `--timestamp`, no entitlements, uses `--deep`.
- No notarytool submit, no stapler, no spctl assessment.
- Lines 99 to 101: the `xattr -cr` and right-click-Open advice. Delete once notarized; that is the
  exact workaround a trustworthy build must not need.
- Needs `SIGN_IDENTITY` and `NOTARY_PROFILE` variables (or `APPLE_API_KEY` secrets in CI).
- Option: replace the hand-rolled plist with `cargo bundle` since `[package.metadata.bundle]` is
  already filled in. Either path is fine.

### Windows
No notarization exists. Sign `Qinip.exe` with Authenticode
(`signtool sign /fd SHA256 /tr <timestamp-url> /td SHA256`). Since 2024, EV certs no longer skip
SmartScreen; every new publisher builds reputation by download volume, so expect "Windows protected
your PC" early on regardless. Azure Artifact Signing (~10 USD/month) is the cheap route but
individual enrollment is US/Canada only, EU eligibility is organizations only, so an OV cert from a
CA like Certum is the practical option for a Dutch individual. Both `package-windows.sh` and CI
currently produce an unsigned zip.

### Sources
Apple TN3147 (notarytool migration), Apple "Resolving common notarization issues", Scripting OS X
notarytool guide, Eclectic Light on hardened runtime, rsms macOS distribution gist, electron/notarize
issue 189 (altool decommissioned), Forasoft 2026 notarization guide (Tahoe note), Microsoft
SmartScreen reputation docs, Microsoft code signing options, Certum on Authenticode and SmartScreen.

## 2. Rust / egui audit (file:line)

**Startup panics.** `src/app.rs` `PlaybackEngine::new().expect(...)` and
`SystemMedia::new(...).expect(...)` in `QinipApp::new` abort before a window exists. A Mac with no
output device (headless mini, Bluetooth mid-switch) gets a crash report. Hold
`playback: Option<PlaybackEngine>` and render a "No audio output device" banner with retry. Related:
`PlaybackEngine::new` builds one stream against `default_output_device()` (`src/playback.rs:89`)
and never rebuilds, so plugging in headphones after launch leaves audio on the old device.

**Blocking on the UI thread.** `rfd::FileDialog::pick_folder()` (move_to_folder, Load Folder
button) blocks the frame; acceptable because the dialog is modal and audio has its own thread.
More serious: `std::fs::rename` in `move_to_folder` fails with "cross-device link" when the
destination is another volume (USB stick, external SSD), a normal DJ workflow. Fall back to
copy-then-remove. `PlaybackCommand::Load` resamples the whole track on the UI thread
(`src/playback.rs:294`) when device rate differs (44.1 kHz MP3 on a 48 kHz interface is the norm):
a tens-of-ms stall per track. Move `resolve_track_samples` to the worker or cache per rate.

**Index identity.** `playing_track: Option<usize>` and `confirm_delete: Option<usize>` are
positions into `playlist.tracks`. `apply_playlist_sort` round-trips through `path`; `remove_from_playlist`
hand-shifts indices. (The 2026-09-22 session moved `selected` into a `Selection` model with
`remap`/`after_remove`, which contains the problem for selection; `playing_track` and
`confirm_delete` still do the dance.) Replacing them with a `TrackId(u32)` taken from the existing
`load_order` field, plus `Playlist::get(id)` / `position(id)`, deletes the shifting code entirely.

**God object.** `QinipApp` owns ~24 fields and ~1800 lines of state, polling, transport logic
and drawing. Split using the code's own names: `playlist_view.rs` (columns, headers, cells, title
cell, the grid body, returning `Vec<PlaylistAction>`), `transport_view.rs` (transport, EQ knobs,
waveform, seek/volume panel, `format_clock`), `theme.rs` (`hardware_theme_visuals`), `analysis.rs`
(`AnalysisResult`, `spawn_analysis`, `run_bpm_key_analysis`, `apply_analysis_result`). `app.rs` keeps
`QinipApp`, `logic`, and the action dispatcher.

**Tests.** Before this session only `src/key.rs` had a test (one Camelot case). `src/bin/analyze.rs`
is a manual harness with a hardcoded path, not a test. Cheap pure targets: `compare_tracks`,
`append_paths` dedupe, `remove_at`, `beat_in_bar` (`src/transport.rs`), `resample_interleaved`
(`src/playback.rs`, check length + endpoint), `format_clock`, `write_analysis_tags` against a temp
copy from `test_audio_files/`. `remove_from_playlist` index remapping most needs a test before any
refactor.

**Worker loop.** `src/worker.rs:75` sleeps 20 ms and polls; block on `job_rx.recv()` for the first
job, then drain with `try_recv()`. The `priority` slot silently replaces an earlier autoplay job
(intended, deserves a comment).

**Error handling.** `PlaybackEngine` methods return `anyhow::Result` and every caller discards
with `let _ =`. The only failure is a closed channel, which cannot happen while `cmd_rx` lives in
the same struct: make them infallible or log once. `status()` clones an `Arc` and is called 5 to 6
times per frame; return a reference. `expect("playback track lock")` would panic only on a poisoned
mutex; consider `lock().unwrap_or_else(|e| e.into_inner())`.

**Clippy (7 warnings, all minor):** `src/playback.rs:142`, `:157` clone a `Copy` `StreamConfig`;
`src/playlist.rs` wants `impl Default for Playlist`; collapsible `if` in `src/decode.rs:241` and two
spots in `app.rs`; `src/key.rs:27` index loop over `NOTE_NAMES` should be `iter().enumerate()`.
`cargo clippy --fix --lib` handles six.

## 3. Design patterns that genuinely fit

- **Command** (refactoring.guru). Already present twice: `PlaybackCommand` queued over mpsc, and
  the `pending_select/play/remove/...` locals in the playlist grid, executed after the closure. Keep
  the deferral but name it: `enum PlaylistAction { Select(TrackId), Play(TrackId), ... }` collected
  by the view and drained by the app. This is also the Elm/MVU shape egui fits naturally.
- **Observer as producer/consumer.** The `WorkerUpdate`, `AnalysisResult`, `MediaControlEvent`
  channels polled each frame are the right shape for immediate mode; do not swap for callbacks.
  Improvement: give the worker a `Context` so a finished decode repaints immediately instead of
  waiting on the 100 ms timer.
- **State.** `AnalysisState` with `Pending -> Loading -> Analyzing -> Done | Failed` is a clean
  state enum. Gap: transitions are set from many places in `app.rs`; a `TrackEntry::transition(to)`
  would centralize the invariant that `Done` implies `bpm` and `key` are `Some`.
- **Facade.** `PlaybackEngine` over cpal and `SystemMedia` over souvlaki already are facades.
  `Playlist` should become one over `Vec<TrackEntry>` so `tracks` stops being `pub`.

Nothing else is warranted at this size.

## 4. Top 10 before release, by risk reduced per hour

1. Developer ID signing + hardened runtime + notarytool + stapler; delete the xattr advice: `scripts/package-macos.sh`.
2. Signing secrets + notarize/staple in CI so a tag produces a notarized zip/dmg: `.github/workflows/release.yml`.
3. Stop panicking on a missing audio device; banner + retry: `src/app.rs` (`QinipApp::new`), `src/playback.rs:87`.
4. Copy+delete fallback when `rename` crosses volumes: `src/app.rs` (`move_to_folder`).
5. Test media keys + Now Playing only in the bundled app on a clean account: `src/media.rs`.
6. `TrackId` identity for `playing_track` / `confirm_delete`; delete index shifting: `src/app.rs`.
7. Unit tests for `remove_from_playlist`, `compare_tracks`, `beat_in_bar`, `resample_interleaved`.
8. Resampling off the UI thread or cached per device rate: `src/playback.rs:294`, `:413`.
9. Block on `recv()` in the worker instead of a 20 ms sleep: `src/worker.rs:75`.
10. Extract `playlist_view.rs`, `transport_view.rs`, and a `PlaylistAction` enum from `app.rs`.
