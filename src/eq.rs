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

/// Band cut-offs in Hz: low < 250, low-mid around 1 k, hi-mid around 4 k, hi > 6 k.
const LOW_HZ: f32 = 250.0;
const LOW_MID_HZ: f32 = 1000.0;
const HI_MID_HZ: f32 = 4000.0;
const HI_HZ: f32 = 6000.0;

/// Filter coefficients that can never fail: a cut-off is pulled below the
/// Nyquist limit for low sample rates (an 11 kHz voice memo or an 8 kHz
/// Bluetooth headset would otherwise make the maths refuse and the app die).
fn coefficients(kind: Type<f32>, sample_rate: u32, cutoff_hz: f32, q: f32) -> Coefficients<f32> {
    let fs_hz = (sample_rate.max(1000) as f32).max(1000.0);
    let f0 = cutoff_hz.min(fs_hz * 0.45);
    Coefficients::<f32>::from_params(kind, fs_hz.hz(), f0.hz(), q)
        .expect("cut-off is below Nyquist after clamping")
}

impl LiveEq {
    pub fn new(sample_rate: u32) -> Self {
        let low = coefficients(Type::LowPass, sample_rate, LOW_HZ, Q_BUTTERWORTH_F32);
        let low_mid = coefficients(Type::BandPass, sample_rate, LOW_MID_HZ, 0.7);
        let hi_mid = coefficients(Type::BandPass, sample_rate, HI_MID_HZ, 0.7);
        let hi = coefficients(Type::HighPass, sample_rate, HI_HZ, Q_BUTTERWORTH_F32);

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
    let low_c = coefficients(Type::LowPass, sample_rate, LOW_HZ, Q_BUTTERWORTH_F32);
    let lowmid_c = coefficients(Type::BandPass, sample_rate, LOW_MID_HZ, 0.7);
    let himid_c = coefficients(Type::BandPass, sample_rate, HI_MID_HZ, 0.7);
    let hi_c = coefficients(Type::HighPass, sample_rate, HI_HZ, Q_BUTTERWORTH_F32);

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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn low_sample_rates_do_not_panic() {
        for rate in [8_000, 11_025, 12_000, 16_000, 22_050, 44_100, 96_000] {
            let _ = LiveEq::new(rate);
            let mono = vec![0.1f32; rate as usize];
            let peaks = band_waveform_peaks(&mono, rate, 50);
            assert!(!peaks.low.is_empty(), "{rate}");
        }
    }
}
