//! Write analysis results (BPM / key) into MP3 ID3 tags.

use std::path::Path;

use id3::{Tag, TagLike, Version};

/// Write BPM and musical key into the file's ID3v2 tags.
///
/// Preserves existing tags when present. Uses frames:
/// - `TBPM` — beats per minute
/// - `TKEY` — initial key (e.g. `Am`, `C#m`)
///
/// Returns `Ok(())` on success.
pub fn write_analysis_tags(path: &Path, bpm: f32, key: &str) -> Result<(), String> {
    let mut tag = Tag::read_from_path(path).unwrap_or_else(|_| Tag::new());
    tag.set_text("TBPM", format!("{:.0}", bpm.round()));
    tag.set_text("TKEY", key);
    tag.write_to_path(path, Version::Id3v24)
        .map_err(|err| format!("failed to write tags: {err}"))
}
