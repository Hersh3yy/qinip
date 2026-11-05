//! Rekordbox bridge: Qinip keeps its own "crates" (playlists of tracks the
//! user picked while auditioning) and exports them as a rekordbox XML file,
//! the import format Pioneer documents and rekordbox 6/7 read.
//!
//! The noob-friendly part is *where* the file goes. rekordbox already has a
//! path it reads its "rekordbox xml" list from (`bridgeImportedLibraryFile`
//! in its settings). When that file is missing, or was written by Qinip, we
//! write there, so the user never has to point rekordbox at anything. If
//! another tool owns that file, we never touch it and fall back to our own
//! file plus a one-time "point rekordbox here" step.
//!
//! We only READ rekordbox's settings; we never write them and never touch its
//! library database (`master.db` is encrypted and undocumented, and a bad
//! write could damage a DJ's library).

use std::path::{Path, PathBuf};

/// A track as exported. A snapshot of what Qinip knew when it was added, so a
/// crate keeps working after the Qinip playlist is cleared.
#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct CrateTrack {
    pub path: PathBuf,
    pub title: String,
    pub artist: String,
    pub duration_secs: Option<f64>,
    pub bpm: Option<f32>,
    /// Qinip's key name, e.g. "A Minor".
    pub key: Option<String>,
}

#[derive(Clone, Debug, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Crate {
    pub name: String,
    pub tracks: Vec<CrateTrack>,
}

#[derive(Clone, Debug, Default, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(default)]
pub struct Crates {
    pub crates: Vec<Crate>,
}

impl Crates {
    pub fn names(&self) -> Vec<String> {
        self.crates.iter().map(|c| c.name.clone()).collect()
    }

    /// Add tracks to the crate called `name` (created if missing). Tracks
    /// already in it are skipped. Returns how many were added.
    pub fn add(&mut self, name: &str, tracks: impl IntoIterator<Item = CrateTrack>) -> usize {
        let index = match self.crates.iter().position(|c| c.name == name) {
            Some(i) => i,
            None => {
                self.crates.push(Crate {
                    name: name.to_string(),
                    tracks: Vec::new(),
                });
                self.crates.len() - 1
            }
        };
        let target = &mut self.crates[index];
        let mut added = 0;
        for track in tracks {
            if !target.tracks.iter().any(|t| t.path == track.path) {
                target.tracks.push(track);
                added += 1;
            }
        }
        added
    }

    pub fn remove_crate(&mut self, name: &str) {
        self.crates.retain(|c| c.name != name);
    }

    pub fn remove_track(&mut self, name: &str, path: &Path) {
        if let Some(c) = self.crates.iter_mut().find(|c| c.name == name) {
            c.tracks.retain(|t| t.path != path);
        }
    }

    /// Render the rekordbox XML. Every distinct track goes into COLLECTION
    /// once with a numeric TrackID; each crate becomes a playlist (Type 1,
    /// KeyType 0 = entries reference TrackIDs) inside a "Qinip" folder.
    pub fn to_xml(&self, qinip_version: &str) -> String {
        let mut ids: Vec<&CrateTrack> = Vec::new();
        for c in &self.crates {
            for t in &c.tracks {
                if !ids.iter().any(|seen| seen.path == t.path) {
                    ids.push(t);
                }
            }
        }
        let id_of = |path: &Path| ids.iter().position(|t| t.path == path).map(|i| i + 1);

        let mut out = String::new();
        out.push_str("<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n");
        out.push_str("<DJ_PLAYLISTS Version=\"1.0.0\">\n");
        out.push_str(&format!(
            "  <PRODUCT Name=\"{PRODUCT_NAME}\" Version=\"{}\" Company=\"Hiren Budhrani\"/>\n",
            xml_escape(qinip_version)
        ));
        out.push_str(&format!("  <COLLECTION Entries=\"{}\">\n", ids.len()));
        for (i, t) in ids.iter().enumerate() {
            out.push_str(&format!(
                "    <TRACK TrackID=\"{}\" Name=\"{}\" Artist=\"{}\" Kind=\"MP3 File\"",
                i + 1,
                xml_escape(&t.title),
                xml_escape(&t.artist)
            ));
            if let Some(secs) = t.duration_secs {
                out.push_str(&format!(" TotalTime=\"{}\"", secs.round() as u64));
            }
            if let Some(bpm) = t.bpm.filter(|b| *b > 0.0) {
                out.push_str(&format!(" AverageBpm=\"{bpm:.2}\""));
            }
            if let Some(tonality) = t.key.as_deref().and_then(tonality) {
                out.push_str(&format!(" Tonality=\"{}\"", xml_escape(&tonality)));
            }
            out.push_str(&format!(
                " Location=\"{}\"/>\n",
                xml_escape(&location_url(&t.path))
            ));
        }
        out.push_str("  </COLLECTION>\n");
        out.push_str("  <PLAYLISTS>\n");
        out.push_str("    <NODE Type=\"0\" Name=\"ROOT\" Count=\"1\">\n");
        out.push_str(&format!(
            "      <NODE Type=\"0\" Name=\"Qinip\" Count=\"{}\">\n",
            self.crates.len()
        ));
        for c in &self.crates {
            out.push_str(&format!(
                "        <NODE Name=\"{}\" Type=\"1\" KeyType=\"0\" Entries=\"{}\">\n",
                xml_escape(&c.name),
                c.tracks.len()
            ));
            for t in &c.tracks {
                if let Some(id) = id_of(&t.path) {
                    out.push_str(&format!("          <TRACK Key=\"{id}\"/>\n"));
                }
            }
            out.push_str("        </NODE>\n");
        }
        out.push_str("      </NODE>\n");
        out.push_str("    </NODE>\n");
        out.push_str("  </PLAYLISTS>\n");
        out.push_str("</DJ_PLAYLISTS>\n");
        out
    }
}

const PRODUCT_NAME: &str = "Qinip";

/// Qinip key name ("A Minor", "C# Major") to rekordbox Tonality ("Am", "C#").
pub fn tonality(key_name: &str) -> Option<String> {
    if let Some(note) = key_name.strip_suffix(" Minor") {
        Some(format!("{note}m"))
    } else {
        key_name.strip_suffix(" Major").map(str::to_string)
    }
}

/// `file://localhost/` + the absolute path, percent-encoded except for the
/// characters that are safe in a URL path. Windows paths become
/// `file://localhost/C:/Users/...` with forward slashes.
pub fn location_url(path: &Path) -> String {
    let raw = path.to_string_lossy().replace('\\', "/");
    let raw = raw.strip_prefix('/').unwrap_or(&raw);
    let mut out = String::from("file://localhost/");
    for byte in raw.bytes() {
        let keep = byte.is_ascii_alphanumeric() || b"-._~/:".contains(&byte);
        if keep {
            out.push(byte as char);
        } else {
            out.push_str(&format!("%{byte:02X}"));
        }
    }
    out
}

fn xml_escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for ch in s.chars() {
        match ch {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&apos;"),
            // XML 1.0 forbids most control characters outright.
            c if (c as u32) < 0x20 && !matches!(c, '\t' | '\n' | '\r') => {}
            c => out.push(c),
        }
    }
    out
}

/// What we know about the user's rekordbox install, read from its settings.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Setup {
    /// rekordbox's settings file, if found (means rekordbox 6/7 is installed).
    pub settings_file: Option<PathBuf>,
    /// The XML file rekordbox reads for its "rekordbox xml" list.
    pub bridge_file: Option<PathBuf>,
    /// Whether the "rekordbox xml" list is switched on in rekordbox's tree.
    pub xml_list_visible: Option<bool>,
}

impl Setup {
    pub fn detect() -> Self {
        let Some(settings_file) = settings_candidates().into_iter().find(|p| p.is_file()) else {
            return Self::default();
        };
        let text = std::fs::read_to_string(&settings_file).unwrap_or_default();
        Self {
            bridge_file: setting_value(&text, "bridgeImportedLibraryFile")
                .filter(|v| !v.is_empty())
                .map(PathBuf::from),
            xml_list_visible: setting_value(&text, "showRbXml").map(|v| v == "1"),
            settings_file: Some(settings_file),
        }
    }

    pub fn rekordbox_found(&self) -> bool {
        self.settings_file.is_some()
    }
}

/// Where Qinip writes its XML and whether rekordbox already reads it there.
#[derive(Clone, Debug, PartialEq)]
pub struct ExportTarget {
    pub path: PathBuf,
    /// `true` = rekordbox's own configured file: no path setup needed.
    pub rekordbox_reads_it: bool,
}

/// Pick the export file: rekordbox's configured bridge file when it is free
/// or already Qinip's, else Qinip's own file in ~/Music/Qinip.
pub fn export_target(setup: &Setup) -> ExportTarget {
    if let Some(bridge) = &setup.bridge_file
        && (!bridge.exists() || written_by_qinip(bridge))
    {
        return ExportTarget {
            path: bridge.clone(),
            rekordbox_reads_it: true,
        };
    }
    ExportTarget {
        path: own_export_file(),
        rekordbox_reads_it: false,
    }
}

/// Write the XML via a temp file + rename, so rekordbox never reads a
/// half-written file.
pub fn write_xml(path: &Path, xml: &str) -> std::io::Result<()> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    // Keep the previous export: a settings hiccup must not cost playlists.
    if path.is_file() {
        let _ = std::fs::copy(path, path.with_extension("xml.bak"));
    }
    let tmp = path.with_extension("xml.qinip-tmp");
    std::fs::write(&tmp, xml)?;
    std::fs::rename(&tmp, path)
}

fn written_by_qinip(path: &Path) -> bool {
    use std::io::Read;
    // The PRODUCT element is in the first lines; a 100 MB export from
    // another tool must not be read whole on every click.
    let Ok(file) = std::fs::File::open(path) else {
        return false;
    };
    let mut head = String::new();
    let _ = file.take(4096).read_to_string(&mut head);
    head.contains(&format!("<PRODUCT Name=\"{PRODUCT_NAME}\""))
}

fn home_dir() -> PathBuf {
    std::env::var_os("HOME")
        .or_else(|| std::env::var_os("USERPROFILE"))
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("."))
}

fn own_export_file() -> PathBuf {
    home_dir().join("Music").join("Qinip").join("qinip-rekordbox.xml")
}

/// rekordbox 6 and 7 both keep settings in a "rekordbox6" folder (checked
/// on 7.1.3); any other `Pioneer/rekordbox*` folder is a fallback.
fn settings_candidates() -> Vec<PathBuf> {
    let mut bases = Vec::new();
    if cfg!(target_os = "macos") {
        bases.push(home_dir().join("Library/Application Support/Pioneer"));
    }
    if let Some(appdata) = std::env::var_os("APPDATA") {
        bases.push(PathBuf::from(appdata).join("Pioneer"));
    }

    let mut out = Vec::new();
    for base in bases {
        out.push(base.join("rekordbox6").join("rekordbox3.settings"));
        let mut others: Vec<PathBuf> = std::fs::read_dir(&base)
            .into_iter()
            .flatten()
            .filter_map(Result::ok)
            .map(|entry| entry.path())
            .filter(|p| {
                p.file_name()
                    .and_then(|n| n.to_str())
                    .is_some_and(|n| n.starts_with("rekordbox") && n != "rekordbox6")
            })
            .map(|p| p.join("rekordbox3.settings"))
            .collect();
        others.sort();
        out.extend(others);
    }
    out
}

/// Value of `<VALUE name="key" val="..."/>` in rekordbox's settings file.
fn setting_value(text: &str, key: &str) -> Option<String> {
    let marker = format!("name=\"{key}\" val=\"");
    let start = text.find(&marker)? + marker.len();
    let end = text[start..].find('"')? + start;
    Some(xml_unescape(&text[start..end]))
}

fn xml_unescape(s: &str) -> String {
    s.replace("&quot;", "\"")
        .replace("&apos;", "'")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&amp;", "&")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn track(path: &str, title: &str) -> CrateTrack {
        CrateTrack {
            path: PathBuf::from(path),
            title: title.to_string(),
            artist: "ZHU".to_string(),
            duration_secs: Some(255.1),
            bpm: Some(121.9),
            key: Some("A Minor".to_string()),
        }
    }

    #[test]
    fn tonality_uses_rekordbox_notation() {
        assert_eq!(tonality("A Minor").as_deref(), Some("Am"));
        assert_eq!(tonality("C# Major").as_deref(), Some("C#"));
        assert_eq!(tonality("weird"), None);
    }

    #[test]
    fn location_is_percent_encoded_file_url() {
        assert_eq!(
            location_url(Path::new("/Users/me/Music/ZHU - Cocaine Model (Original Mix).mp3")),
            "file://localhost/Users/me/Music/ZHU%20-%20Cocaine%20Model%20%28Original%20Mix%29.mp3"
        );
        assert_eq!(
            location_url(Path::new("C:\\Users\\me\\Música\\a&b.mp3")),
            "file://localhost/C:/Users/me/M%C3%BAsica/a%26b.mp3"
        );
    }

    #[test]
    fn add_skips_duplicates_and_creates_crates() {
        let mut crates = Crates::default();
        assert_eq!(crates.add("Friday", [track("/a.mp3", "A"), track("/b.mp3", "B")]), 2);
        assert_eq!(crates.add("Friday", [track("/a.mp3", "A")]), 0);
        assert_eq!(crates.add("Warmup", [track("/a.mp3", "A")]), 1);
        assert_eq!(crates.names(), vec!["Friday", "Warmup"]);
    }

    #[test]
    fn xml_shares_track_ids_across_playlists_and_escapes() {
        let mut crates = Crates::default();
        crates.add("Friday & Sat", [track("/a.mp3", "A \"live\""), track("/b.mp3", "B")]);
        crates.add("Warmup", [track("/a.mp3", "A \"live\"")]);
        let xml = crates.to_xml("1.0.0");

        assert!(xml.contains("<PRODUCT Name=\"Qinip\""));
        assert!(xml.contains("<COLLECTION Entries=\"2\">"));
        assert!(xml.contains("Name=\"A &quot;live&quot;\""));
        assert!(xml.contains("TotalTime=\"255\" AverageBpm=\"121.90\" Tonality=\"Am\""));
        assert!(xml.contains("<NODE Name=\"Friday &amp; Sat\" Type=\"1\" KeyType=\"0\" Entries=\"2\">"));
        assert!(xml.contains("<NODE Type=\"0\" Name=\"Qinip\" Count=\"2\">"));
        // Track /a.mp3 is TrackID 1 in both playlists.
        assert_eq!(xml.matches("<TRACK Key=\"1\"/>").count(), 2);
    }

    #[test]
    fn reads_rekordbox_settings_values() {
        let text = r#"<VALUE name="bridgeImportedLibraryFile" val="/Users/me/Library/rekordbox/rekordbox/rekordbox.xml"/>
<VALUE name="showRbXml" val="0"/>"#;
        assert_eq!(
            setting_value(text, "bridgeImportedLibraryFile").as_deref(),
            Some("/Users/me/Library/rekordbox/rekordbox/rekordbox.xml")
        );
        assert_eq!(setting_value(text, "showRbXml").as_deref(), Some("0"));
        assert_eq!(setting_value(text, "missing"), None);
    }

    #[test]
    fn export_target_never_overwrites_another_tools_file() {
        let dir = std::env::temp_dir().join(format!("qinip-rb-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let bridge = dir.join("rekordbox.xml");

        let setup = Setup {
            settings_file: Some(dir.join("rekordbox3.settings")),
            bridge_file: Some(bridge.clone()),
            xml_list_visible: Some(true),
        };

        // Missing: ours to use.
        assert!(export_target(&setup).rekordbox_reads_it);

        // Written by Qinip: still ours.
        write_xml(&bridge, &Crates::default().to_xml("1.0.0")).unwrap();
        assert!(export_target(&setup).rekordbox_reads_it);

        // Someone else's: hands off.
        std::fs::write(&bridge, "<DJ_PLAYLISTS><PRODUCT Name=\"Lexicon\"/></DJ_PLAYLISTS>").unwrap();
        let target = export_target(&setup);
        assert!(!target.rekordbox_reads_it);
        assert_ne!(target.path, bridge);

        let _ = std::fs::remove_dir_all(&dir);
    }
}
