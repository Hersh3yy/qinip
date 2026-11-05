# Qinip launch post (Instagram, weekend of 2026-11-08)

Instagram does not make links in captions clickable, so the link lives in the bio
(and in a Story link sticker). Set it before posting:

    https://github.com/Hersh3yy/qinip/releases/latest

That page always shows the newest version with both downloads, the checksums and
the macOS "Open Anyway" steps. It only works once v1.1.0 is published
(`./scripts/release.sh 1.1.0` from `master`).

## Pre-flight (10 minutes)

1. Release v1.1.0 is live on GitHub and the page shows `qinip-macos-arm64.dmg`,
   `qinip-windows-x64.zip` and `SHA256SUMS.txt`.
2. Download the dmg **in Safari on your own Mac** (not from the build folder) and
   follow the Open Anyway steps once, so the instructions are proven on a real
   download with the quarantine flag.
3. Bio link set. Post the Story first, then the feed post.

## Caption (feed)

I built a music player 🍈

Qinip: a free app for DJs who get sent packs of new tracks. Drop a folder on it and
every track shows up with its waveform in colour, BPM, key and Camelot code. Loop
4, 8 or 16 beats, shape the sound with four EQ knobs, and send the keepers straight
to a rekordbox playlist.

Named after the guinep fruit: crack one open, see what's inside.

Free and open source, for Mac and Windows. Built in Rust over a few months of
evenings. Link in bio.

#dj #djlife #djtools #rekordbox #housemusic #techno #opensource #rustlang
#musicproduction #djsetup #newmusic #indiedev

## Story (one slide + link sticker)

"Made a free app for auditioning DJ packs. Waveform, BPM, key, loops, send to
rekordbox. Mac + Windows. ⬇ link"

## Visuals, in posting order

1. **Hero image**: the app with a track playing, Dark theme, a folder of real
   tracks loaded. `assets/qinip-screenshot.png` (1280×800) is a rendered frame of
   the real UI; a window screenshot of your own Mac (Cmd+Shift+4, space, click the
   window) looks better because it includes the title bar.
2. **Screen recording, 12–15 s, no sound needed**: drag a folder onto the empty
   window → rows appear and title bars fill → double-click a track → turn the Low
   knob down and up (the orange band dims and returns) → press the 8-beat loop
   button → right-click → Add to Rekordbox playlist. Record with QuickTime
   (File → New Screen Recording) or Cmd+Shift+5; crop to 4:5 for the feed.
3. **Carousel slide 2**: four big words on the dark background: Waveform ·
   BPM + Key · Loops · Rekordbox.
4. **Carousel slide 3**: the fruit (`assets/qinip-fruit.png`) and one line:
   "Qinip = guinep. Crack it open, see what's inside."
5. **Last slide**: "Free. Open source. Mac + Windows. Link in bio."

Themes make good extra slides: Red, Forest Green and Solarized Light are in
`docs/launch/` if you want a "pick your colour" slide.

## Expect these replies, and the answers

- "Mac says it can't open it" → System Settings → Privacy & Security → Open
  Anyway, once. (It is not notarized: that is a 99 USD/year Apple fee.)
- "Windows says unknown publisher" → More info → Run anyway. Windows build is
  beta: it has not been run by a human yet.
- "Does it do FLAC/WAV?" → Not yet, MP3 only. On the list.
- "Is it a DJ software?" → No decks, no mixing. It is for listening through
  packs fast and sorting the keepers.
- "Why free?" → Clementine died on Apple Silicon and the alternative wanted
  €10 a month. Tips welcome in information, knowledge and/or compliments.

## r/DJs version (if you post there too; read their self-promo rule first)

Title: I made a free, open-source app for auditioning promo packs (Mac/Win)

Body: Drop a folder, every track gets a coloured 4-band waveform, BPM (within 0.1
of Mixed In Key on my tests), key + Camelot, 4/8/16-beat loops, EQ knobs, and a
right-click "Add to rekordbox playlist" that goes through rekordbox XML. Reads
the BPM/key your files already have. MP3 only for now. GPL, Rust, no account,
no network. Download: github.com/Hersh3yy/qinip/releases/latest
