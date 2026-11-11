//! Build script: embeds the app icon and product name into the Windows exe.
//!
//! Only runs when building on Windows for Windows: the resource compiler
//! (rc.exe) lives there, and a cross-check from a Mac must not fail on it.

fn main() {
    println!("cargo:rerun-if-changed=assets/qinip.ico");
    println!("cargo:rerun-if-changed=build.rs");
    embed_windows_icon();
}

#[cfg(windows)]
fn embed_windows_icon() {
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("windows") {
        return;
    }
    let mut res = winresource::WindowsResource::new();
    res.set_icon("assets/qinip.ico");
    res.set("ProductName", "Qinip");
    res.set("FileDescription", "Qinip, MP3 pack audition player");
    res.set("LegalCopyright", "Copyright 2026 Hiren Budhrani, GPL-3.0-or-later");
    if let Err(err) = res.compile() {
        println!("cargo:warning=Windows icon not embedded: {err}");
    }
}

#[cfg(not(windows))]
fn embed_windows_icon() {}
