use crate::decode::DecodedAudio;
use aubio_rs::{OnsetMode, Tempo};

pub struct BpmResult {
    pub bpm: f32,
    pub confidence: f32,
}

pub fn detect_bpm(audio: &DecodedAudio) -> BpmResult {
    let mono = audio.downmix_to_mono();

    let buf_size = 1024;
    let hop_size = 512;

    let mut tempo = Tempo::new(OnsetMode::Complex, buf_size, hop_size, audio.sample_rate)
        .expect("failed to create tempo detector");

    let mut readings: Vec<(f32, f32)> = Vec::new();

    let mut pos = 0;
    while pos + hop_size <= mono.len() {
        let chunk = &mono[pos..pos + hop_size];
        tempo
            .do_result(chunk)
            .expect("tempo detection failed on chunk");

        let bpm = tempo.get_bpm();
        let confidence = tempo.get_confidence();
        if bpm > 0.0 {
            readings.push((bpm, confidence));
        }
        pos += hop_size;
    }

    let skip = readings.len() / 5;
    let stable_readings = &readings[skip.min(readings.len())..];

    let best = stable_readings
        .iter()
        .cloned()
        .fold((0.0f32, 0.0f32), |best, current| {
            if current.1 > best.1 { current } else { best }
        });

    let avg_confidence =
        stable_readings.iter().map(|(_, c)| c).sum::<f32>() / stable_readings.len().max(1) as f32;

    BpmResult {
        bpm: best.0,
        confidence: avg_confidence,
    }
}
