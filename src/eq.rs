use biquad::{Biquad, Coefficients, DirectForm1, ToHertz, Type, Q_BUTTERWORTH_F32};

/// Four DJ-style band gains. `1.0` = unity, `0.0` = kill that band.
#[derive(Clone, Copy, Debug)]
pub struct EqGains {
    pub low: f32,
    pub low_mid: f32,
    pub hi_mid: f32,
    pub hi: f32,
}

impl Default for EqGains {
    fn default() -> Self {
        Self {
            low: 1.0,
            low_mid: 1.0,
            hi_mid: 1.0,
            hi: 1.0,
        }
    }
}

impl EqGains {
    pub fn clamped(self) -> Self {
        Self {
            low: self.low.clamp(0.0, 2.0),
            low_mid: self.low_mid.clamp(0.0, 2.0),
            hi_mid: self.hi_mid.clamp(0.0, 2.0),
            hi: self.hi.clamp(0.0, 2.0),
        }
    }

    pub fn is_unity(&self) -> bool {
        (self.low - 1.0).abs()
            + (self.low_mid - 1.0).abs()
            + (self.hi_mid - 1.0).abs()
            + (self.hi - 1.0).abs()
            < 0.02
    }

    pub fn is_killed(&self) -> bool {
        self.low <= 0.001
            && self.low_mid <= 0.001
            && self.hi_mid <= 0.001
            && self.hi <= 0.001
    }
}

/// Per-channel live EQ.
///
/// Band-sum mix so knobs at 0% truly kill. All knobs at 0 → silence.
/// All knobs near 100% → dry passthrough (bit-identical, no filter color).
pub struct LiveEq {
    low: DirectForm1<f32>,
    low_mid: DirectForm1<f32>,
    hi_mid: DirectForm1<f32>,
    hi: DirectForm1<f32>,
}

impl LiveEq {
    pub fn new(sample_rate: u32) -> Self {
        let fs = (sample_rate as f32).hz();
        let low = Coefficients::<f32>::from_params(Type::LowPass, fs, 250.0.hz(), Q_BUTTERWORTH_F32)
            .expect("low EQ coeffs");
        let low_mid = Coefficients::<f32>::from_params(Type::BandPass, fs, 1000.0.hz(), 0.7)
            .expect("low-mid EQ coeffs");
        let hi_mid = Coefficients::<f32>::from_params(Type::BandPass, fs, 4000.0.hz(), 0.7)
            .expect("hi-mid EQ coeffs");
        let hi = Coefficients::<f32>::from_params(Type::HighPass, fs, 6000.0.hz(), Q_BUTTERWORTH_F32)
            .expect("hi EQ coeffs");

        Self {
            low: DirectForm1::<f32>::new(low),
            low_mid: DirectForm1::<f32>::new(low_mid),
            hi_mid: DirectForm1::<f32>::new(hi_mid),
            hi: DirectForm1::<f32>::new(hi),
        }
    }

    pub fn reset(&mut self, sample_rate: u32) {
        *self = Self::new(sample_rate);
    }

    /// Process one sample. Stateful — call in order, never parallel-map.
    pub fn process(&mut self, sample: f32, gains: EqGains) -> f32 {
        if gains.is_killed() {
            // Still run filters so state stays consistent when knobs move again.
            let _ = self.low.run(sample);
            let _ = self.low_mid.run(sample);
            let _ = self.hi_mid.run(sample);
            let _ = self.hi.run(sample);
            return 0.0;
        }

        let low = self.low.run(sample);
        let low_mid = self.low_mid.run(sample);
        let hi_mid = self.hi_mid.run(sample);
        let hi = self.hi.run(sample);

        if gains.is_unity() {
            return sample;
        }

        // Band-sum: each knob scales its band. All 0 → silence (handled above).
        gains.low * low
            + gains.low_mid * low_mid
            + gains.hi_mid * hi_mid
            + gains.hi * hi
    }
}

/// Waveform envelopes per band — min/max peaks for drawing.
#[derive(Clone, Debug, Default)]
pub struct BandWaveform {
    pub low: Vec<(f32, f32)>,
    pub low_mid: Vec<(f32, f32)>,
    pub hi_mid: Vec<(f32, f32)>,
    pub hi: Vec<(f32, f32)>,
}

pub fn band_waveform_peaks(mono: &[f32], sample_rate: u32, num_peaks: usize) -> BandWaveform {
    let fs = (sample_rate as f32).hz();
    let low_c =
        Coefficients::<f32>::from_params(Type::LowPass, fs, 250.0.hz(), Q_BUTTERWORTH_F32).unwrap();
    let lowmid_c =
        Coefficients::<f32>::from_params(Type::BandPass, fs, 1000.0.hz(), 0.7).unwrap();
    let himid_c =
        Coefficients::<f32>::from_params(Type::BandPass, fs, 4000.0.hz(), 0.7).unwrap();
    let hi_c =
        Coefficients::<f32>::from_params(Type::HighPass, fs, 6000.0.hz(), Q_BUTTERWORTH_F32)
            .unwrap();

    let mut low_f = DirectForm1::<f32>::new(low_c);
    let mut lowmid_f = DirectForm1::<f32>::new(lowmid_c);
    let mut himid_f = DirectForm1::<f32>::new(himid_c);
    let mut hi_f = DirectForm1::<f32>::new(hi_c);

    let mut low = Vec::with_capacity(mono.len());
    let mut low_mid = Vec::with_capacity(mono.len());
    let mut hi_mid = Vec::with_capacity(mono.len());
    let mut hi = Vec::with_capacity(mono.len());

    for &sample in mono {
        low.push(low_f.run(sample));
        low_mid.push(lowmid_f.run(sample));
        hi_mid.push(himid_f.run(sample));
        hi.push(hi_f.run(sample));
    }

    BandWaveform {
        low: peaks_of(&low, num_peaks),
        low_mid: peaks_of(&low_mid, num_peaks),
        hi_mid: peaks_of(&hi_mid, num_peaks),
        hi: peaks_of(&hi, num_peaks),
    }
}

fn peaks_of(signal: &[f32], num_peaks: usize) -> Vec<(f32, f32)> {
    let chunk_size = (signal.len() / num_peaks.max(1)).max(1);
    signal
        .chunks(chunk_size)
        .map(|chunk| {
            let min = chunk.iter().cloned().fold(f32::INFINITY, f32::min);
            let max = chunk.iter().cloned().fold(f32::NEG_INFINITY, f32::max);
            (min, max)
        })
        .collect()
}

pub const BAND_COLORS: [(u8, u8, u8); 4] = [
    (232, 93, 4),   // Low — orange
    (255, 186, 8),  // Low-mid — gold
    (45, 198, 83),  // Hi-mid — green
    (76, 201, 240), // Hi — cyan
];

pub const BAND_LABELS: [&str; 4] = ["Low", "LowMid", "HiMid", "Hi"];
