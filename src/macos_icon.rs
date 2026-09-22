//! macOS Dock icon — `cargo run` doesn't use the .app bundle's AppIcon.icns.

#[cfg(target_os = "macos")]
pub fn set_dock_icon() {
    use objc2::AnyThread;
    use objc2_app_kit::{NSApplication, NSImage};
    use objc2_foundation::{MainThreadMarker, NSData};

    let Some(mtm) = MainThreadMarker::new() else {
        return;
    };

    let png = include_bytes!("../assets/icon.png");
    let data = NSData::from_vec(png.to_vec());

    let Some(image) = NSImage::initWithData(NSImage::alloc(), &data) else {
        eprintln!("qinip: failed to decode dock icon");
        return;
    };

    let app = NSApplication::sharedApplication(mtm);
    unsafe {
        app.setApplicationIconImage(Some(&image));
    }
}

#[cfg(not(target_os = "macos"))]
pub fn set_dock_icon() {}
