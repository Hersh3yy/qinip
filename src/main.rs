use dj_player::app::DjApp;

fn main() -> eframe::Result<()> {
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
