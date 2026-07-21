use dj_player::app::QinipApp;

fn main() -> eframe::Result<()> {
    if dj_player::dev::enabled() {
        eprintln!("Qinip — dev mode (stderr logging enabled)");
        eprintln!("Run with: QINIP_DEV=1 cargo run --release");
    }

    let icon = std::sync::Arc::new(dj_player::app::load_app_icon());

    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([1100.0, 700.0])
            .with_icon(icon),
        ..Default::default()
    };

    eframe::run_native(
        "Qinip",
        options,
        Box::new(|cc| Ok(Box::new(QinipApp::new(cc)))),
    )
}
