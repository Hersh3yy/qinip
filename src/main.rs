use dj_player::app::DjApp;

fn main() -> eframe::Result<()> {
    if std::env::var("DJ_PLAYER_DEV")
        .map(|v| v == "1" || v.eq_ignore_ascii_case("true"))
        .unwrap_or(false)
    {
        eprintln!("DJ Player — dev mode (stderr logging enabled)");
        eprintln!("Run with: DJ_PLAYER_DEV=1 cargo run --release");
    }

    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default().with_inner_size([1100.0, 700.0]),
        ..Default::default()
    };

    eframe::run_native(
        "DJ Player",
        options,
        Box::new(|cc| Ok(Box::new(DjApp::new(cc)))),
    )
}
