//! Write analysis results (BPM / key) into MP3 ID3 tags.

use std::path::Path;

use id3::{Tag, TagLike, Version};

fn existing_text_is_set(tag: &Tag, ids: &[&str]) -> bool {
    ids.iter().any(|id| {
        tag.get(id)
            .and_then(|frame| frame.content().text())
            .is_some_and(|text| !text.trim().is_empty())
    })
}

/// Write BPM and musical key into the file's ID3v2 tags.
///
/// Fills only missing frames — never overwrites an existing `TBPM`/`TBP`
/// or `TKEY`/`TKE`. Other tags (title, artist, …) are left intact.
///
/// Returns `Ok(true)` if the file was updated, `Ok(false)` if both frames
/// were already present so the file was not touched.
pub fn write_analysis_tags(path: &Path, bpm: f32, key: &str) -> Result<bool, String> {
    let mut tag = Tag::read_from_path(path).unwrap_or_else(|_| Tag::new());
    let mut changed = false;

    if !existing_text_is_set(&tag, &["TBPM", "TBP"]) {
        tag.set_text("TBPM", format!("{:.0}", bpm.round()));
        changed = true;
    }
    if !existing_text_is_set(&tag, &["TKEY", "TKE"]) {
        tag.set_text("TKEY", key);
        changed = true;
    }

    if !changed {
        return Ok(false);
    }

    tag.write_to_path(path, Version::Id3v24)
        .map_err(|err| format!("failed to write tags: {err}"))?;
    Ok(true)
}
