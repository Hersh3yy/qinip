//! Headless UI tests: the real `QinipApp` driven by egui_kittest, with a
//! silent audio engine and no OS integration.
//!
//! Run with `QINIP_UI_RENDER_DIR=<dir> cargo test ui_tests` to also save a
//! PNG of each screen (needs a GPU; uses wgpu).

use std::path::PathBuf;
use std::time::{Duration, Instant};

use egui_kittest::Harness;
use egui_kittest::kittest::Queryable;

use crate::app::QinipApp;
use crate::theme::ThemeChoice;

fn render_dir() -> Option<PathBuf> {
    std::env::var_os("QINIP_UI_RENDER_DIR").map(PathBuf::from)
}

fn harness<'a>() -> Harness<'a, QinipApp> {
    let mut builder = Harness::builder().with_size(egui::vec2(1280.0, 800.0));
    if render_dir().is_some() {
        builder = builder.wgpu();
    }
    builder.build_eframe(|cc| QinipApp::new_headless(cc))
}

fn save_render(harness: &mut Harness<'_, QinipApp>, name: &str) {
    let Some(dir) = render_dir() else {
        return;
    };
    harness.run_steps(2);
    let image = harness.render().expect("render needs the wgpu renderer");
    std::fs::create_dir_all(&dir).unwrap();
    image.save(dir.join(format!("{name}.png"))).unwrap();
}

/// Local-only audio (gitignored): tests that need it skip when absent.
/// `QINIP_UI_FOLDER` points the test at another folder (for renders).
fn test_folder() -> Option<PathBuf> {
    let dir = std::env::var_os("QINIP_UI_FOLDER")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("test_audio_files"));
    let has_mp3 = std::fs::read_dir(&dir)
        .ok()?
        .filter_map(Result::ok)
        .any(|e| e.path().extension().is_some_and(|x| x.eq_ignore_ascii_case("mp3")));
    has_mp3.then_some(dir)
}

/// Step frames until `done` holds or `timeout` passes (analysis runs on
/// background threads, so real time has to pass).
fn step_until(harness: &mut Harness<'_, QinipApp>, timeout: Duration, done: impl Fn(&QinipApp) -> bool) -> bool {
    let start = Instant::now();
    loop {
        harness.step();
        if done(harness.state()) {
            return true;
        }
        if start.elapsed() > timeout {
            return false;
        }
        std::thread::sleep(Duration::from_millis(30));
    }
}

#[test]
fn empty_state_opens_without_panicking() {
    let mut harness = harness();
    harness.run_steps(5);
    assert!(
        harness.state().status().contains("Load a folder"),
        "status was: {}",
        harness.state().status()
    );
    assert_eq!(harness.state().track_count(), 0);
    save_render(&mut harness, "01-empty");
}

#[test]
fn loads_a_folder_analyzes_and_clears() {
    let Some(folder) = test_folder() else {
        eprintln!("no local test audio, skipping");
        return;
    };
    let mut harness = harness();
    harness.run_steps(2);
    harness.state_mut().load_folder(folder);
    harness.run_steps(2);
    assert!(harness.state().track_count() >= 1);
    assert!(harness.state().status().starts_with("Added"), "{}", harness.state().status());

    let analyzed = step_until(&mut harness, Duration::from_secs(90), |app| app.all_analyzed());
    assert!(analyzed, "analysis did not finish");
    let summary = harness.state().track_summary();
    assert!(summary.iter().all(|(_, bpm, cam, _)| bpm.is_some() && cam.is_some()), "{summary:?}");
    save_render(&mut harness, "02-loaded");

    // Select + play through the real path (silent engine), then the waveform shows.
    harness.state_mut().request_play_index(0);
    step_until(&mut harness, Duration::from_secs(5), |_| false);
    save_render(&mut harness, "03-playing");

    for (i, theme) in ThemeChoice::ALL.iter().enumerate() {
        harness.state_mut().set_theme_for_test(*theme);
        harness.run_steps(3);
        let slug = theme.label().to_ascii_lowercase().replace(' ', "-");
        save_render(&mut harness, &format!("theme-{i}-{slug}"));
    }
    harness.state_mut().set_theme_for_test(ThemeChoice::Dark);
    harness.run_steps(3);

    harness.get_by_label("About").click();
    harness.run_steps(3);
    save_render(&mut harness, "07-about");

    harness.get_by_label("Clear Playlist").click();
    harness.run_steps(3);
    assert_eq!(harness.state().track_count(), 0, "{}", harness.state().status());
    assert!(harness.state().status().starts_with("Cleared"));
}
