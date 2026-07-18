use crate::decode::DecodedAudio;
use rustfft::{FftPlanner, num_complex::Complex};
use std::f32::consts::PI;

pub struct KeyResult {
    pub key_name: String,
    pub correlation: f32,
}

const NOTE_NAMES: [&str; 12] = [
    "C", "C#", "D", "D#", "E", "F", "F#", "G", "G#", "A", "A#", "B",
];
const MAJOR_PROFILE: [f32; 12] = [
    6.35, 2.23, 3.48, 2.33, 4.38, 4.09, 2.52, 5.19, 2.39, 3.66, 2.29, 2.88,
];
const MINOR_PROFILE: [f32; 12] = [
    6.33, 2.68, 3.52, 5.38, 2.60, 3.53, 2.54, 4.75, 3.98, 2.69, 3.34, 3.17,
];

pub fn detect_key(audio: &DecodedAudio) -> KeyResult {
    let mono = audio.downmix_to_mono();
    let chroma = compute_chroma_vector(&mono, audio.sample_rate);

    let mut best_correlation = f32::NEG_INFINITY;
    let mut best_key = String::new();

    for tonic in 0..12 {
        let major_rotated = rotate(&MAJOR_PROFILE, tonic);
        let minor_rotated = rotate(&MINOR_PROFILE, tonic);

        let major_corr = pearson_correlation(&chroma, &major_rotated);
        let minor_corr = pearson_correlation(&chroma, &minor_rotated);

        if major_corr > best_correlation {
            best_correlation = major_corr;
            best_key = format!("{} Major", NOTE_NAMES[tonic]);
        }
        if minor_corr > best_correlation {
            best_correlation = minor_corr;
            best_key = format!("{} Minor", NOTE_NAMES[tonic]);
        }
    }

    KeyResult {
        key_name: best_key,
        correlation: best_correlation,
    }
}

/// Convert a key name like `"A Minor"` / `"C Major"` to Camelot notation (`"8A"` / `"8B"`).
pub fn to_camelot(key_name: &str) -> Option<String> {
    let key_name = key_name.trim();
    let (note, mode) = if let Some(note) = key_name.strip_suffix(" Major") {
        (note, 'B')
    } else if let Some(note) = key_name.strip_suffix(" Minor") {
        (note, 'A')
    } else {
        return None;
    };

    let number = match (note, mode) {
        // Open Key / Camelot major (B) and relative minor (A)
        ("B", 'B') | ("G#", 'A') | ("Ab", 'A') => 1,
        ("F#", 'B') | ("Gb", 'B') | ("D#", 'A') | ("Eb", 'A') => 2,
        ("C#", 'B') | ("Db", 'B') | ("A#", 'A') | ("Bb", 'A') => 3,
        ("G#", 'B') | ("Ab", 'B') | ("F", 'A') => 4,
        ("D#", 'B') | ("Eb", 'B') | ("C", 'A') => 5,
        ("A#", 'B') | ("Bb", 'B') | ("G", 'A') => 6,
        ("F", 'B') | ("D", 'A') => 7,
        ("C", 'B') | ("A", 'A') => 8,
        ("G", 'B') | ("E", 'A') => 9,
        ("D", 'B') | ("B", 'A') => 10,
        ("A", 'B') | ("F#", 'A') | ("Gb", 'A') => 11,
        ("E", 'B') | ("C#", 'A') | ("Db", 'A') => 12,
        _ => return None,
    };

    Some(format!("{number}{mode}"))
}

#[cfg(test)]
mod tests {
    use super::to_camelot;

    #[test]
    fn camelot_common_keys() {
        assert_eq!(to_camelot("C Major").as_deref(), Some("8B"));
        assert_eq!(to_camelot("A Minor").as_deref(), Some("8A"));
        assert_eq!(to_camelot("G Major").as_deref(), Some("9B"));
        assert_eq!(to_camelot("E Minor").as_deref(), Some("9A"));
        assert_eq!(to_camelot("F Major").as_deref(), Some("7B"));
        assert_eq!(to_camelot("D Minor").as_deref(), Some("7A"));
    }
}

fn compute_chroma_vector(mono: &[f32], sample_rate: u32) -> [f32; 12] {
    let window_size = 4096;
    let hop_size = 2048;

    let mut planner = FftPlanner::<f32>::new();
    let fft = planner.plan_fft_forward(window_size);

    let mut chroma = [0.0f32; 12];
    let min_freq = 80.0;
    let max_freq = 5000.0;

    let mut pos = 0;
    while pos + window_size <= mono.len() {
        let mut buffer: Vec<Complex<f32>> = mono[pos..pos + window_size]
            .iter()
            .enumerate()
            .map(|(i, &sample)| {
                let hann = 0.5 * (1.0 - (2.0 * PI * i as f32 / (window_size - 1) as f32).cos());
                Complex {
                    re: sample * hann,
                    im: 0.0,
                }
            })
            .collect();

        fft.process(&mut buffer);

        for (bin, value) in buffer.iter().take(window_size / 2).enumerate() {
            let freq = bin as f32 * sample_rate as f32 / window_size as f32;
            if freq < min_freq || freq > max_freq {
                continue;
            }

            let magnitude = (value.re * value.re + value.im * value.im).sqrt();
            let midi_note = 69.0 + 12.0 * (freq / 440.0).log2();
            let pitch_class = ((midi_note.round() as i32).rem_euclid(12)) as usize;

            chroma[pitch_class] += magnitude;
        }

        pos += hop_size;
    }

    chroma
}

fn rotate(profile: &[f32; 12], by: usize) -> [f32; 12] {
    let mut rotated = [0.0f32; 12];
    for i in 0..12 {
        rotated[i] = profile[(i + 12 - by) % 12];
    }
    rotated
}

fn pearson_correlation(a: &[f32; 12], b: &[f32; 12]) -> f32 {
    let mean_a = a.iter().sum::<f32>() / 12.0;
    let mean_b = b.iter().sum::<f32>() / 12.0;

    let mut numerator = 0.0;
    let mut denom_a = 0.0;
    let mut denom_b = 0.0;

    for i in 0..12 {
        let da = a[i] - mean_a;
        let db = b[i] - mean_b;
        numerator += da * db;
        denom_a += da * da;
        denom_b += db * db;
    }

    numerator / (denom_a.sqrt() * denom_b.sqrt()).max(0.0001)
}
