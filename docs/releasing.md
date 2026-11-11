# Releasing Qinip, from zero

For Hiren, before every release. Ten minutes to read once; two minutes to skim later.
The rules themselves are in `PROJECT.md` ("Branches and releases"); this is the why and the how
in plain words.

## 1. What a release is

A release is a frozen, numbered copy of the app (v1.1.0) that people download. Once it is out
you never change it. Found a bug? Fix it and publish the next number (v1.1.1). People, links and
bug reports all refer to numbers, so a number must mean one exact build, forever.

Between releases you work normally on branches and merge to `master`. `master` is "what the next
release will be", so it must always build and pass its tests; CI checks that on every push.

## 2. The number

`MAJOR.MINOR.PATCH`, for example `1.1.0`:

- Only bug fixes since the last release → raise PATCH: `1.1.1`.
- Something new (a feature, a theme) → raise MINOR, PATCH back to 0: `1.2.0`.
- Something that breaks what people saved or how they used it → raise MAJOR: `2.0.0`.

Never reuse a number, never skip the rule to make a number "look" bigger.

## 3. Where the files live

**GitHub Releases is the home of the downloads.** It is free, keeps every version forever,
counts downloads, and people trust `github.com/<you>/<app>/releases` because the source, the
date and the checksums sit right next to the file. Everything else (your website, itch.io, a
Reddit post) only links there.

Our file names never change (`qinip-macos-arm64.dmg`, `qinip-windows-x64.zip`), so this link
always points at the newest version and can go in a bio or on a website once:

    https://github.com/Hersh3yy/qinip/releases/latest/download/qinip-macos-arm64.dmg

(`.../latest` without the file name is the page itself: both downloads, the changelog, the
checksums, the "Open Anyway" steps.)

## 4. The protocol, every time

1. **Be on `master`, clean, in sync, CI green.** Merge your branch first
   (`git checkout master && git merge --ff-only <branch> && git push`). Look at the Actions tab:
   the CI run on that push must be green.
2. **Write the changelog in human sentences.** Everything since the last release goes under
   `## [Unreleased]` in `CHANGELOG.md` as Added / Changed / Fixed. This text becomes the release
   page, so write it for a DJ, not for yourself ("Shuffle no longer repeats a track before every
   track played once", not "fix history reset").
3. **Dry run:** on GitHub, Actions → **Cut release** → Run workflow → version `1.2.0`, tick
   "Only check". (Terminal equivalent: `./scripts/release.sh 1.2.0 --dry-run`.) It checks the
   branch, the clean tree, the version, the changelog, the tests and clippy, and changes nothing.
4. **Real run:** the same button without the tick (terminal: `./scripts/release.sh 1.2.0`). It
   moves Unreleased under `## [1.2.0] - <date>`, bumps `Cargo.toml`, commits, tags `v1.2.0`,
   pushes, and starts the build.
5. **Wait for the tag build** (Actions tab, ~15 minutes): it refuses a tag that does not match
   `Cargo.toml` or the changelog, builds Mac and Windows, writes `SHA256SUMS.txt`, and publishes
   the release page with your changelog section plus the download instructions.
6. **Download it like a stranger.** Open the release page in Safari, download the dmg, drag the
   app to Applications, open it. You get the macOS block; do the Open Anyway steps once. If what
   happens does not match the text on the page, fix the text (`.github/workflows/release.yml`,
   "Release notes from CHANGELOG.md") and ship a PATCH. Same for Windows if a tester is around.
7. **Announce** (section 6). Then watch GitHub issues for 48 hours and answer; collect bugs under
   Unreleased. Only a crash or data loss earns an immediate PATCH; the rest waits for the next
   normal release.

Things you never do: change a published release's files, delete a release, force-push `master`,
tag by hand.

## 5. "Free and pure": what to promise and where

Free and open is a set of promises; write them where people decide to download:

- **Licence**: Qinip is GPL-3.0-or-later (`LICENSE`): anyone may use, share and change it, and
  changed versions must stay open under the same licence. aubio, the reason GPL was chosen, is
  gone since 1.1.0, so MIT or Apache-2.0 (anyone may do anything, including closed versions) is
  now possible. Changing a licence is a one-commit decision by the author, in a release of its own.
- **No account, no network, no telemetry, no ads.** All true today; say it on the page and in
  the About window (it is). If a future update-checker makes a network call, say that too.
- **Your files are yours.** Qinip only reads them unless you tick the box. Also true; also say it.
- **Donations are not selling.** GitHub Sponsors or Ko-fi on the page keeps the app free; a
  sponsor could cover the 99 USD/year Apple fee that removes the macOS block. Selling it later
  is a different project (licence, the ZHU file in history, a company); not today.

## 6. The OS warnings, honestly

- **macOS** blocks un-notarized apps once per app: System Settings → Privacy & Security → Open
  Anyway. Notarization needs the paid Apple Developer account (99 USD/year); then
  `scripts/package-macos.sh` with `SIGN_IDENTITY` and `NOTARY_PROFILE` does the rest, and CI does
  it from secrets. Until then, every download page says the three steps.
- **Windows** gets a real installer (`qinip-setup-x64.exe`, Inno Setup, built by CI) plus the portable zip. SmartScreen still shows "unknown publisher" on an unsigned installer: More info → Run anyway.
  Free ways out, in order of effort: the Microsoft Store (individual account is free since 2025;
  an MSIX package is signed by Microsoft and never warns), and SignPath Foundation's free
  open-source signing for the GitHub `.exe` once the project has some history.

## 7. Where to put it, and in what order

1. GitHub Releases (automatic).
2. Your own website: one page, the latest links, screenshots, the three promises, the OS steps.
3. itch.io: tools category, pay-what-you-want with 0 minimum, your cut at 0 % or 10 %; another
   place people search, and the donation box.
4. Reddit: r/DJs, r/Beatmatch, r/rekordbox (read each sub's self-promotion rule first; one post,
   answer every comment), r/rust and r/opensource for the "how it's built" angle.
5. AlternativeTo (list it as an alternative to Clementine, Strawberry, rekordbox's preview).
6. Instagram: `docs/launch/instagram-post.md`.

Product Hunt is a poor fit for a DJ niche; skip it.

## 8. The rhythm after launch

Weeks of small fixes on branches, merged to `master` when tested, each with a changelog line.
A release once in a while, when Unreleased reads like something worth downloading again. Keep
`PROJECT.md`'s diary honest about what is verified and what is not (Windows, media keys).

## 9. Open decisions (yours)

- Licence: stay GPL, or MIT/Apache now that aubio is gone.
- Apple account: pay, find a sponsor, or live with Open Anyway.
- The ZHU test MP3 is still in git history (untracked since 1.1.0, but any clone has it in
  `.git`). Scrubbing it is one `git filter-repo` run plus a force push, painless now, messy after
  others have forked. Leaving it is low risk for a free project; it matters the day Qinip is sold.
- A Windows tester before calling the Windows build more than "beta".
