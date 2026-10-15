//! OS file-manager helpers — open the folder that contains a track, and
//! move a track on disk (including across volumes).

use std::io;
use std::path::Path;
use std::process::Command;

/// Open the folder that contains this file.
pub fn open_enclosing_folder(path: &Path) -> Result<(), String> {
    let folder = path
        .parent()
        .filter(|p| p.exists())
        .ok_or_else(|| format!("no enclosing folder for {}", path.display()))?;

    #[cfg(target_os = "macos")]
    {
        Command::new("open")
            .arg(folder)
            .spawn()
            .map_err(|err| format!("failed to open folder in Finder: {err}"))?;
    }

    #[cfg(target_os = "windows")]
    {
        Command::new("explorer")
            .arg(folder)
            .spawn()
            .map_err(|err| format!("failed to open folder in Explorer: {err}"))?;
    }

    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    {
        Command::new("xdg-open")
            .arg(folder)
            .spawn()
            .map_err(|err| format!("failed to open folder: {err}"))?;
    }

    Ok(())
}

/// Launch rekordbox (macOS, by bundle id so the "rekordbox 7" folder name
/// doesn't matter).
pub fn open_rekordbox() -> Result<(), String> {
    #[cfg(target_os = "macos")]
    {
        Command::new("open")
            .args(["-b", "com.pioneerdj.rekordboxdj"])
            .spawn()
            .map_err(|err| format!("couldn't open rekordbox: {err}"))?;
        Ok(())
    }
    #[cfg(not(target_os = "macos"))]
    {
        Err("Open rekordbox from the Start menu.".to_string())
    }
}

/// Move a file, falling back to copy-then-delete when `rename` can't cross
/// filesystems (USB stick, external SSD, network share). `rename` is atomic
/// on one volume; across volumes the OS refuses it with `CrossesDevices`, so
/// we copy, check the size matches, and only then remove the original. If
/// removing the original fails, the copy is deleted so the track never ends
/// up in two places.
pub fn move_file(from: &Path, to: &Path) -> io::Result<()> {
    match std::fs::rename(from, to) {
        Ok(()) => Ok(()),
        Err(err) if err.kind() == io::ErrorKind::CrossesDevices => copy_then_remove(from, to),
        Err(err) => Err(err),
    }
}

fn copy_then_remove(from: &Path, to: &Path) -> io::Result<()> {
    let copied = std::fs::copy(from, to)?;
    let original = std::fs::metadata(from)?.len();
    if copied != original {
        let _ = std::fs::remove_file(to);
        return Err(io::Error::other(format!(
            "copy was incomplete ({copied} of {original} bytes)"
        )));
    }
    if let Err(err) = std::fs::remove_file(from) {
        let _ = std::fs::remove_file(to);
        return Err(err);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_dir(name: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("qinip-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn copy_then_remove_moves_contents_and_deletes_source() {
        let dir = temp_dir("copy-move");
        let from = dir.join("a.mp3");
        let to = dir.join("b.mp3");
        std::fs::write(&from, b"not really audio").unwrap();

        copy_then_remove(&from, &to).unwrap();

        assert!(!from.exists());
        assert_eq!(std::fs::read(&to).unwrap(), b"not really audio");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn move_file_same_volume_renames() {
        let dir = temp_dir("rename");
        let from = dir.join("a.mp3");
        let to = dir.join("sub").join("a.mp3");
        std::fs::create_dir_all(to.parent().unwrap()).unwrap();
        std::fs::write(&from, b"x").unwrap();

        move_file(&from, &to).unwrap();

        assert!(!from.exists() && to.exists());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
