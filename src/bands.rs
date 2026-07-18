use crate::decode::DecodedAudio;
use biquad::*;

pub struct BandSignals {
    pub low: Vec<f32>,
    pub low_mid: Vec<f32>,
    pub hi_mid: Vec<f32>,
    pub hi: Vec<f32>,
}

pub struct BandLevels {
    pub low: f32,
    pub low_mid: f32,
    pub hi_mid: f32,
    pub hi: f32,
}

pub fn filter_bands(audio: &DecodedAudio) -> BandSignals {
    let mono = audio.downmix_to_mono();
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

pub fn band_summary(bands: &BandSignals) -> BandLevels {
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

pub fn print_band_summary(bands: &BandLevels) {
    let total = bands.low + bands.low_mid + bands.hi_mid + bands.hi;
    println!("Spectral balance (share of total energy):");
    println!("Low     {:>5.1}%", bands.low / total * 100.0);
    println!("LowMid  {:>5.1}%", bands.low_mid / total * 100.0);
    println!("HiMid   {:>5.1}%", bands.hi_mid / total * 100.0);
    println!("Hi      {:>5.1}%", bands.hi / total * 100.0);
}

pub fn print_band_heatmap(bands: &BandSignals, num_segments: usize) {
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

/// Private helper (no `pub`) — only used inside this file, so it stays
/// invisible to the rest of the crate. Rust items are private by default;
/// you opt IN to visibility with `pub`, opposite of some languages.
fn segment_rms(signal: &[f32], num_segments: usize) -> Vec<f32> {
    let chunk_size = (signal.len() / num_segments).max(1);
    signal
        .chunks(chunk_size)
        .map(|chunk| {
            (chunk.iter().map(|s| (s * s) as f64).sum::<f64>() / chunk.len() as f64).sqrt() as f32
        })
        .collect()
}
