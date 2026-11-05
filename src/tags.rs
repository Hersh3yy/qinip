//! Read and write analysis results (BPM / key) in MP3 ID3 tags.

use std::path::Path;

use id3::{Tag, TagLike, Version};

use crate::key::{normalize_key_tag, short_key};

/// BPM and key as another tool (or Qinip) already wrote them into the file.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct TaggedAnalysis {
    pub bpm: Option<f32>,
    /// Qinip's key name, e.g. "A Minor".
    pub key: Option<String>,
}

fn text_of(tag: &Tag, ids: &[&str]) -> Option<String> {
    ids.iter().find_map(|id| {
        tag.get(id)
            .and_then(|frame| frame.content().text())
            .map(str::trim)
            .filter(|text| !text.is_empty())
            .map(str::to_string)
    })
}

fn existing_text_is_set(tag: &Tag, ids: &[&str]) -> bool {
    text_of(tag, ids).is_some()
}

/// Read TBPM / TKEY (ID3v2.3 and v2.4). A missing or unreadable tag is simply
/// "nothing known"; it never fails the load.
pub fn read_analysis_tags(path: &Path) -> TaggedAnalysis {
    let Ok(tag) = Tag::read_from_path(path) else {
        return TaggedAnalysis::default();
    };
    TaggedAnalysis {
        bpm: text_of(&tag, &["TBPM", "TBP"]).and_then(|t| parse_bpm(&t)),
        key: text_of(&tag, &["TKEY", "TKE"]).and_then(|t| normalize_key_tag(&t)),
    }
}

/// "120", "120.00", "121,9" and "120 BPM" all read as a tempo; nonsense and
/// out-of-range values (a 0 or a 9999) do not.
pub fn parse_bpm(text: &str) -> Option<f32> {
    let digits: String = text
        .trim()
        .chars()
        .map(|c| if c == ',' { '.' } else { c })
        .take_while(|c| c.is_ascii_digit() || *c == '.')
        .collect();
    let bpm: f32 = digits.parse().ok()?;
    (20.0..=400.0).contains(&bpm).then_some(bpm)
}

/// Write BPM and musical key into the file's ID3v2 tags.
///
/// Fills only missing frames — never overwrites an existing `TBPM`/`TBP`
/// or `TKEY`/`TKE`. Other tags (title, artist, …) are left intact. The key
/// is written in the short form other DJ tools expect (`Am`, `F#`).
///
/// Returns `Ok(true)` if the file was updated, `Ok(false)` if both frames
/// were already present so the file was not touched.
pub fn write_analysis_tags(path: &Path, bpm: f32, key: &str) -> Result<bool, String> {
    let (mut tag, version) = match Tag::read_from_path(path) {
        Ok(tag) => {
            let version = tag.version();
            (tag, version)
        }
        // No tag at all is normal; a tag we cannot parse is not: writing a
        // fresh one over it would wipe whatever title/artist it held.
        Err(err) if matches!(err.kind, id3::ErrorKind::NoTag) => (Tag::new(), Version::Id3v24),
        Err(err) => return Err(format!("existing tag unreadable, left alone: {err}")),
    };
    let mut changed = false;

    if !existing_text_is_set(&tag, &["TBPM", "TBP"]) {
        tag.set_text("TBPM", format!("{:.0}", bpm.round()));
        changed = true;
    }
    if !existing_text_is_set(&tag, &["TKEY", "TKE"]) {
        tag.set_text("TKEY", short_key(key).unwrap_or_else(|| key.to_string()));
        changed = true;
    }

    if !changed {
        return Ok(false);
    }

    // Keep the file's own tag version (a v2.3 library stays v2.3).
    tag.write_to_path(path, version)
        .map_err(|err| format!("failed to write tags: {err}"))?;
    Ok(true)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch_mp3(name: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("qinip-tags-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join(name);
        // id3 only needs a file to prepend its header to; a few bytes stand
        // in for the audio stream.
        std::fs::write(&path, b"\xff\xfb\x90\x00not really mpeg audio").unwrap();
        path
    }

    #[test]
    fn bpm_text_variants_parse() {
        assert_eq!(parse_bpm("120"), Some(120.0));
        assert_eq!(parse_bpm("121.90"), Some(121.9));
        assert_eq!(parse_bpm("121,9"), Some(121.9));
        assert_eq!(parse_bpm(" 128 BPM"), Some(128.0));
        assert_eq!(parse_bpm("0"), None);
        assert_eq!(parse_bpm("fast"), None);
    }

    #[test]
    fn writes_short_key_and_reads_it_back() {
        let path = scratch_mp3("write.mp3");
        assert_eq!(write_analysis_tags(&path, 121.9, "A Minor"), Ok(true));
        let tag = Tag::read_from_path(&path).unwrap();
        assert_eq!(tag.get("TKEY").and_then(|f| f.content().text()), Some("Am"));
        assert_eq!(tag.get("TBPM").and_then(|f| f.content().text()), Some("122"));

        let read = read_analysis_tags(&path);
        assert_eq!(read.bpm, Some(122.0));
        assert_eq!(read.key.as_deref(), Some("A Minor"));

        // Second write finds both frames present and leaves the file alone.
        assert_eq!(write_analysis_tags(&path, 99.0, "C Major"), Ok(false));
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn reads_camelot_and_flat_keys_other_tools_wrote() {
        let path = scratch_mp3("camelot.mp3");
        let mut tag = Tag::new();
        tag.set_text("TKEY", "8A");
        tag.set_text("TBPM", "120");
        tag.write_to_path(&path, Version::Id3v23).unwrap();
        let read = read_analysis_tags(&path);
        assert_eq!(read.key.as_deref(), Some("A Minor"));
        assert_eq!(read.bpm, Some(120.0));

        let mut tag = Tag::new();
        tag.set_text("TKEY", "Bbm");
        tag.write_to_path(&path, Version::Id3v24).unwrap();
        assert_eq!(read_analysis_tags(&path).key.as_deref(), Some("A# Minor"));
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn missing_file_reads_as_nothing_known() {
        assert_eq!(read_analysis_tags(Path::new("/nope/none.mp3")), TaggedAnalysis::default());
    }
}
