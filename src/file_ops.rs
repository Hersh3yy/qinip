//! OS file-manager helpers — reveal a file in Finder/Explorer, open its folder.

use std::path::Path;
use std::process::Command;

/// Highlight the file in the system file manager (Finder on macOS, Explorer on Windows).
pub fn reveal_in_file_manager(path: &Path) -> Result<(), String> {
    if !path.exists() {
        return Err(format!("path does not exist: {}", path.display()));
    }

    #[cfg(target_os = "macos")]
    {
        Command::new("open")
            .arg("-R")
            .arg(path)
            .spawn()
            .map_err(|err| format!("failed to reveal in Finder: {err}"))?;
    }

    #[cfg(target_os = "windows")]
    {
        Command::new("explorer")
            .arg(format!("/select,{}", path.display()))
            .spawn()
            .map_err(|err| format!("failed to reveal in Explorer: {err}"))?;
    }

    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    {
        let folder = path.parent().unwrap_or(path);
        Command::new("xdg-open")
            .arg(folder)
            .spawn()
            .map_err(|err| format!("failed to open file manager: {err}"))?;
    }

    Ok(())
}

/// Open the folder that contains this file (without selecting the file).
pub fn show_enclosing_folder(path: &Path) -> Result<(), String> {
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

/// Human-readable label for "Reveal in …" menu item.
pub fn reveal_menu_label() -> &'static str {
    #[cfg(target_os = "macos")]
    {
        "Reveal in Finder"
    }
    #[cfg(target_os = "windows")]
    {
        "Reveal in Explorer"
    }
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    {
        "Reveal in file manager"
    }
}
