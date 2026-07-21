use dj_player::bands::{band_summary, filter_bands, print_band_heatmap, print_band_summary};
use dj_player::bpm::detect_bpm;
use dj_player::decode::decode_to_samples;
use dj_player::key::detect_key;
use dj_player::waveform::{print_ascii_waveform, waveform_peaks};

fn main() {
    let path = "test_audio_files/ZHU - Cocaine Model (Original Mix).mp3";
    let audio = decode_to_samples(path).expect("decode failed");

    println!("Total samples: {}", audio.samples.len());
    println!(
        "Sample rate: {} Hz | Channels: {}",
        audio.sample_rate, audio.channels
    );
    println!("Duration: {:.1}s", audio.duration_secs());
    println!("Peak: {:.4} | RMS: {:.4}", audio.peak(), audio.rms());
    println!();

    let peaks = waveform_peaks(&audio, 120);
    print_ascii_waveform(&peaks);

    let bands = filter_bands(&audio);
    let summary = band_summary(&bands);
    print_band_summary(&summary);
    println!();
    print_band_heatmap(&bands, 100);

    println!();
    let bpm_result = detect_bpm(&audio);
    println!(
        "Estimated BPM: {:.1} (confidence: {:.2})",
        bpm_result.bpm, bpm_result.confidence
    );

    println!();
    let key_result = detect_key(&audio);
    println!(
        "Estimated Key: {} (correlation: {:.3})",
        key_result.key_name, key_result.correlation
    );
}
