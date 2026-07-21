//! OS file-manager helpers — open the folder that contains a track.

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
