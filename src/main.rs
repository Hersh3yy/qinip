use aubio_rs::{OnsetMode, Tempo};
use biquad::*;
use rustfft::{FftPlanner, num_complex::Complex};
use std::f32::consts::PI;
use std::fs::File;
use symphonia::core::audio::SampleBuffer;
use symphonia::core::codecs::DecoderOptions;
use symphonia::core::formats::FormatOptions;
use symphonia::core::io::MediaSourceStream;
use symphonia::core::meta::MetadataOptions;
use symphonia::core::probe::Hint;

// ============================================================
// DATA
// ============================================================

struct DecodedAudio {
    samples: Vec<f32>,
    sample_rate: u32,
    channels: usize,
}

struct BandSignals {
    low: Vec<f32>,
    low_mid: Vec<f32>,
    hi_mid: Vec<f32>,
    hi: Vec<f32>,
}

struct BandLevels {
    low: f32,
    low_mid: f32,
    hi_mid: f32,
    hi: f32,
}

struct BpmResult {
    bpm: f32,
    confidence: f32,
}

struct KeyResult {
    key_name: String,
    correlation: f32,
}

// ============================================================
// MAIN
// ============================================================

fn main() {
    let path = "test_audio_files/ZHU - Cocaine Model (Original Mix).mp3";
    let audio = decode_to_samples(path);

    print_basic_stats(&audio);

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

// ============================================================
// DECODING
// ============================================================

fn decode_to_samples(path: &str) -> DecodedAudio {
    let file = File::open(path).expect("couldn't open file");
    let mss = MediaSourceStream::new(Box::new(file), Default::default());
    let hint = Hint::new();

    let probed = symphonia::default::get_probe()
        .format(
            &hint,
            mss,
            &FormatOptions::default(),
            &MetadataOptions::default(),
        )
        .expect("unsupported format or corrupt file");

    let mut format = probed.format;
    let track = format.tracks().first().expect("no audio track found");
    let track_id = track.id;
    let sample_rate = track.codec_params.sample_rate.expect("no sample rate");

    let mut decoder = symphonia::default::get_codecs()
        .make(&track.codec_params, &DecoderOptions::default())
        .expect("unsupported codec");

    let mut all_samples: Vec<f32> = Vec::new();
    let mut channels = 2usize;

    loop {
        let packet = match format.next_packet() {
            Ok(p) => p,
            Err(_) => break,
        };
        if packet.track_id() != track_id {
            continue;
        }
        match decoder.decode(&packet) {
            Ok(decoded) => {
                channels = decoded.spec().channels.count();
                let mut sample_buf =
                    SampleBuffer::<f32>::new(decoded.capacity() as u64, *decoded.spec());
                sample_buf.copy_interleaved_ref(decoded);
                all_samples.extend_from_slice(sample_buf.samples());
            }
            Err(_) => continue,
        }
    }

    DecodedAudio {
        samples: all_samples,
        sample_rate,
        channels,
    }
}

// ============================================================
// BASIC STATS
// ============================================================

fn print_basic_stats(audio: &DecodedAudio) {
    let peak = audio
        .samples
        .iter()
        .fold(0.0f32, |max, &s| max.max(s.abs()));
    let rms =
        (audio.samples.iter().map(|s| s * s).sum::<f32>() / audio.samples.len() as f32).sqrt();
    let frames = audio.samples.len() / audio.channels;
    let duration_secs = frames as f32 / audio.sample_rate as f32;

    println!("Total samples: {}", audio.samples.len());
    println!(
        "Sample rate: {} Hz | Channels: {}",
        audio.sample_rate, audio.channels
    );
    println!("Duration: {:.1}s", duration_secs);
    println!("Peak: {:.4} | RMS: {:.4}", peak, rms);
    println!();
}

// ============================================================
// WAVEFORM
// ============================================================

fn waveform_peaks(audio: &DecodedAudio, num_peaks: usize) -> Vec<(f32, f32)> {
    let mono = downmix_to_mono(audio);
    let chunk_size = (mono.len() / num_peaks).max(1);

    mono.chunks(chunk_size)
        .map(|chunk| {
            let min = chunk.iter().cloned().fold(f32::INFINITY, f32::min);
            let max = chunk.iter().cloned().fold(f32::NEG_INFINITY, f32::max);
            (min, max)
        })
        .collect()
}

fn print_ascii_waveform(peaks: &[(f32, f32)]) {
    let height = 20;
    for row in 0..height {
        let threshold = 1.0 - (row as f32 / height as f32) * 2.0;
        let mut line = String::new();
        for &(min, max) in peaks {
            if max >= threshold && min <= threshold {
                line.push('█');
            } else {
                line.push(' ');
            }
        }
        println!("{}", line);
    }
    println!();
}

// ============================================================
// FREQUENCY BANDS
// ============================================================

fn filter_bands(audio: &DecodedAudio) -> BandSignals {
    let mono = downmix_to_mono(audio);
    let fs = (audio.sample_rate as f32).hz();

    let low_coeffs =
        Coefficients::<f32>::from_params(Type::LowPass, fs, 250.0.hz(), Q_BUTTERWORTH_F32).unwrap();
    let lowmid_coeffs =
        Coefficients::<f32>::from_params(Type::BandPass, fs, 1000.0.hz(), 0.7).unwrap();
    let himid_coeffs =
        Coefficients::<f32>::from_params(Type::BandPass, fs, 4000.0.hz(), 0.7).unwrap();
    let hi_coeffs =
        Coefficients::<f32>::from_params(Type::HighPass, fs, 6000.0.hz(), Q_BUTTERWORTH_F32)
            .unwrap();

    let mut low_filter = DirectForm1::<f32>::new(low_coeffs);
    let mut lowmid_filter = DirectForm1::<f32>::new(lowmid_coeffs);
    let mut himid_filter = DirectForm1::<f32>::new(himid_coeffs);
    let mut hi_filter = DirectForm1::<f32>::new(hi_coeffs);

    let mut low = Vec::with_capacity(mono.len());
    let mut low_mid = Vec::with_capacity(mono.len());
    let mut hi_mid = Vec::with_capacity(mono.len());
    let mut hi = Vec::with_capacity(mono.len());

    for &sample in &mono {
        low.push(low_filter.run(sample));
        low_mid.push(lowmid_filter.run(sample));
        hi_mid.push(himid_filter.run(sample));
        hi.push(hi_filter.run(sample));
    }

    BandSignals {
        low,
        low_mid,
        hi_mid,
        hi,
    }
}

fn band_summary(bands: &BandSignals) -> BandLevels {
    let rms = |signal: &[f32]| -> f32 {
        (signal.iter().map(|s| (s * s) as f64).sum::<f64>() / signal.len() as f64).sqrt() as f32
    };
    BandLevels {
        low: rms(&bands.low),
        low_mid: rms(&bands.low_mid),
        hi_mid: rms(&bands.hi_mid),
        hi: rms(&bands.hi),
    }
}

fn print_band_summary(bands: &BandLevels) {
    let total = bands.low + bands.low_mid + bands.hi_mid + bands.hi;
    println!("Spectral balance (share of total energy):");
    println!("Low     {:>5.1}%", bands.low / total * 100.0);
    println!("LowMid  {:>5.1}%", bands.low_mid / total * 100.0);
    println!("HiMid   {:>5.1}%", bands.hi_mid / total * 100.0);
    println!("Hi      {:>5.1}%", bands.hi / total * 100.0);
}

fn print_band_heatmap(bands: &BandSignals, num_segments: usize) {
    let rows: [(&str, &[f32]); 4] = [
        ("Low   ", &bands.low),
        ("LowMid", &bands.low_mid),
        ("HiMid ", &bands.hi_mid),
        ("Hi    ", &bands.hi),
    ];

    println!("Band activity over time:");
    for (label, signal) in rows {
        let energies = segment_rms(signal, num_segments);
        let row_max = energies.iter().cloned().fold(0.0f32, f32::max).max(0.0001);

        let shades = [' ', '░', '▒', '▓', '█'];
        let line: String = energies
            .iter()
            .map(|&e| {
                let normalized = (e / row_max).clamp(0.0, 1.0);
                let idx = (normalized * (shades.len() - 1) as f32).round() as usize;
                shades[idx]
            })
            .collect();

        println!("{} {}", label, line);
    }
}

fn segment_rms(signal: &[f32], num_segments: usize) -> Vec<f32> {
    let chunk_size = (signal.len() / num_segments).max(1);
    signal
        .chunks(chunk_size)
        .map(|chunk| {
            (chunk.iter().map(|s| (s * s) as f64).sum::<f64>() / chunk.len() as f64).sqrt() as f32
        })
        .collect()
}

// ============================================================
// BPM
// ============================================================

fn detect_bpm(audio: &DecodedAudio) -> BpmResult {
    let mono = downmix_to_mono(audio);

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

// ============================================================
// KEY DETECTION
// ============================================================

const NOTE_NAMES: [&str; 12] = [
    "C", "C#", "D", "D#", "E", "F", "F#", "G", "G#", "A", "A#", "B",
];

// Krumhansl-Schmuckler key profiles — empirically measured perceived
// "belongingness" of each pitch class within a major/minor key, relative
// to the tonic at index 0. Rotating this array simulates a different tonic.
const MAJOR_PROFILE: [f32; 12] = [
    6.35, 2.23, 3.48, 2.33, 4.38, 4.09, 2.52, 5.19, 2.39, 3.66, 2.29, 2.88,
];
const MINOR_PROFILE: [f32; 12] = [
    6.33, 2.68, 3.52, 5.38, 2.60, 3.53, 2.54, 4.75, 3.98, 2.69, 3.34, 3.17,
];

fn detect_key(audio: &DecodedAudio) -> KeyResult {
    let mono = downmix_to_mono(audio);
    let chroma = compute_chroma_vector(&mono, audio.sample_rate);

    let mut best_correlation = f32::NEG_INFINITY;
    let mut best_key = String::new();

    // Try all 24 candidates: 12 rotations of major, 12 rotations of minor.
    // "Rotating" the profile by `tonic` simulates testing every possible
    // root note as the key center.
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

/// Runs FFT over overlapping windows of the track, folds every frequency
/// bin into one of 12 pitch classes regardless of octave, and sums energy
/// into a single 12-element "fingerprint" of the track's tonal content.
fn compute_chroma_vector(mono: &[f32], sample_rate: u32) -> [f32; 12] {
    let window_size = 4096;
    let hop_size = 2048; // 50% overlap between windows

    let mut planner = FftPlanner::<f32>::new();
    let fft = planner.plan_fft_forward(window_size);

    let mut chroma = [0.0f32; 12];

    // Only fold bins within a musically meaningful range — very low bins
    // are mostly sub-bass rumble/DC offset, very high bins are mostly
    // noise/cymbal energy that doesn't carry tonal (key) information.
    let min_freq = 80.0;
    let max_freq = 5000.0;

    let mut pos = 0;
    while pos + window_size <= mono.len() {
        let mut buffer: Vec<Complex<f32>> = mono[pos..pos + window_size]
            .iter()
            .enumerate()
            .map(|(i, &sample)| {
                // Hann window: fades each window's edges to zero to avoid
                // fake frequency content ("spectral leakage") from the cut.
                let hann = 0.5 * (1.0 - (2.0 * PI * i as f32 / (window_size - 1) as f32).cos());
                Complex {
                    re: sample * hann,
                    im: 0.0,
                }
            })
            .collect();

        fft.process(&mut buffer);

        // Only need the first half of the FFT output — the rest is a
        // mirror image for real-valued input, carries no new information.
        for (bin, value) in buffer.iter().take(window_size / 2).enumerate() {
            let freq = bin as f32 * sample_rate as f32 / window_size as f32;
            if freq < min_freq || freq > max_freq {
                continue;
            }

            let magnitude = (value.re * value.re + value.im * value.im).sqrt();

            // Convert frequency to a pitch class (0=C, 1=C#, ... 11=B),
            // using A4=440Hz as the reference pitch (MIDI note 69).
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

// ============================================================
// SHARED HELPERS
// ============================================================

fn downmix_to_mono(audio: &DecodedAudio) -> Vec<f32> {
    audio
        .samples
        .chunks(audio.channels)
        .map(|frame| frame.iter().sum::<f32>() / audio.channels as f32)
        .collect()
}
