//! Tempo detection without a beat tracker: an onset-strength envelope, its
//! autocorrelation for the period, and a comb filter for the phase.
//!
//! aubio's beat tracker was tried first and ran 1.5–2.4 % fast on every
//! tagged test track (its internal period drifts and re-syncs). The period
//! of the autocorrelation peak does not have that bias, and the phase search
//! gives where the beats fall, which the tempo light and the loop buttons use.

use rustfft::{num_complex::Complex, FftPlanner};

use crate::decode::DecodedAudio;

pub struct BpmResult {
    /// 0 when the track is too short or has no beat to speak of.
    pub bpm: f32,
    /// 0..1: how clearly the beat period stood out (autocorrelation at the
    /// period relative to lag 0).
    pub confidence: f32,
    /// Time of the first beat, so beat `k` is at `beat_offset_secs + k * 60 / bpm`.
    pub beat_offset_secs: f32,
}

const FRAME: usize = 1024;
const HOP: usize = 256;
/// Tempos a DJ track can have; the autocorrelation is searched in this range
/// so half- and double-time peaks never win.
pub const BPM_MIN: f32 = 70.0;
pub const BPM_MAX: f32 = 180.0;

pub fn detect_bpm(audio: &DecodedAudio) -> BpmResult {
    let mono = audio.downmix_to_mono();
    let sample_rate = audio.sample_rate.max(1) as f32;
    let fps = sample_rate / HOP as f32;
    let envelope = onset_envelope(&mono, fps);
    estimate_tempo(&envelope, fps)
        .map(|(bpm, confidence, offset_hops)| BpmResult {
            bpm,
            confidence,
            // An onset in frame t sits around the frame's centre.
            beat_offset_secs: offset_hops / fps + (FRAME as f32 / 2.0) / sample_rate,
        })
        .unwrap_or(BpmResult {
            bpm: 0.0,
            confidence: 0.0,
            beat_offset_secs: 0.0,
        })
}

/// Spectral flux per hop (how much louder each frequency got since the last
/// frame), with slow level changes removed so only transients remain.
pub fn onset_envelope(mono: &[f32], fps: f32) -> Vec<f32> {
    let fft = FftPlanner::<f32>::new().plan_fft_forward(FRAME);
    let window: Vec<f32> = (0..FRAME)
        .map(|i| 0.5 - 0.5 * (std::f32::consts::TAU * i as f32 / (FRAME - 1) as f32).cos())
        .collect();
    let bins = FRAME / 2 + 1;
    let mut prev = vec![0.0f32; bins];
    let mut buf = vec![Complex::new(0.0f32, 0.0); FRAME];

    let mut flux: Vec<f32> = Vec::with_capacity(mono.len() / HOP + 1);
    let mut pos = 0;
    while pos + FRAME <= mono.len() {
        for (c, (&x, &w)) in buf.iter_mut().zip(mono[pos..pos + FRAME].iter().zip(&window)) {
            *c = Complex::new(x * w, 0.0);
        }
        fft.process(&mut buf);
        let mut sum = 0.0f32;
        for (k, p) in prev.iter_mut().enumerate() {
            // Log compression: a quiet hi-hat counts as much as a loud kick.
            let mag = (1.0 + 100.0 * buf[k].norm()).ln();
            let d = mag - *p;
            if d > 0.0 {
                sum += d;
            }
            *p = mag;
        }
        flux.push(sum);
        pos += HOP;
    }

    // Subtract a 1 s moving average and keep what sticks out above it.
    let half = ((0.5 * fps) as usize).max(1);
    let n = flux.len();
    let mut prefix = vec![0.0f64; n + 1];
    for (i, v) in flux.iter().enumerate() {
        prefix[i + 1] = prefix[i] + *v as f64;
    }
    (0..n)
        .map(|i| {
            let lo = i.saturating_sub(half);
            let hi = (i + half + 1).min(n);
            let mean = (prefix[hi] - prefix[lo]) / (hi - lo) as f64;
            (flux[i] as f64 - mean).max(0.0) as f32
        })
        .collect()
}

/// `(bpm, confidence, phase in hops)` from an onset envelope sampled at
/// `fps` hops per second, or `None` when there is too little to go on.
pub fn estimate_tempo(envelope: &[f32], fps: f32) -> Option<(f32, f32, f32)> {
    let n = envelope.len();
    let min_lag = (60.0 * fps / BPM_MAX).floor() as usize;
    let max_lag = (60.0 * fps / BPM_MIN).ceil() as usize;
    if min_lag < 2 || n < max_lag * 4 {
        return None;
    }

    // Normalised autocorrelation for every lag up to twice the slowest beat,
    // so a candidate can be backed by its double period.
    let autocorr = |lag: usize| -> f32 {
        let m = n - lag;
        let mut s = 0.0f64;
        for t in 0..m {
            s += envelope[t] as f64 * envelope[t + lag] as f64;
        }
        (s / m as f64) as f32
    };
    let rs: Vec<f32> = (0..=2 * max_lag + 1).map(autocorr).collect();
    let r0 = rs[0];
    if r0.is_nan() || r0 <= 0.0 {
        return None;
    }

    // A candidate is backed by its double period, and a mild preference for
    // DJ tempos breaks the ties an even click train would otherwise leave
    // (it never overrides a clear peak: 0.78 at 90 BPM, 0.55 at 70 and 180).
    let prior = |lag: usize| {
        let bpm = 60.0 * fps / lag as f32;
        (-((bpm - 125.0) / 50.0).powi(2) / 2.0).exp()
    };
    let score = |lag: usize| (rs[lag] + 0.5 * rs[2 * lag]) * prior(lag);
    let best = (min_lag..=max_lag).max_by(|a, b| score(*a).total_cmp(&score(*b)))?;

    // Parabolic interpolation around the peak gives a first fractional period;
    // the comb filter then refines it to a hundredth of a hop (~0.06 ms).
    let (y0, y1, y2) = (rs[best - 1], rs[best], rs[best + 1]);
    let denom = y0 - 2.0 * y1 + y2;
    let delta = if denom.abs() > 1e-12 {
        (0.5 * (y0 - y2) / denom).clamp(-0.5, 0.5)
    } else {
        0.0
    };
    let (period, phase) = refine_period(envelope, best as f32 + delta);
    let bpm = 60.0 * fps / period;
    let confidence = (rs[best] / r0).clamp(0.0, 1.0);
    Some((bpm, confidence, phase))
}

/// Search periods around `coarse` for the one whose comb of beats collects
/// the most onset energy; returns `(period, phase)` in hops.
fn refine_period(envelope: &[f32], coarse: f32) -> (f32, f32) {
    let mut best = (coarse, 0.0f32, f32::MIN);
    scan_periods(envelope, coarse - 0.6, coarse + 0.6, 0.02, &mut best);
    let centre = best.0;
    scan_periods(envelope, centre - 0.02, centre + 0.02, 0.002, &mut best);
    (best.0, best.1)
}

/// Try every period from `lo` to `hi` in `step`s; keep the best
/// `(period, phase, energy)` seen so far in `best`.
fn scan_periods(envelope: &[f32], lo: f32, hi: f32, step: f32, best: &mut (f32, f32, f32)) {
    let mut p = lo;
    while p <= hi + 1e-6 {
        let (phi, energy) = best_phase(envelope, p);
        if energy > best.2 {
            *best = (p, phi, energy);
        }
        p += step;
    }
}

/// The offset (in hops, within one period) whose comb of beats collects the
/// most onset energy, and that mean energy: that is where the beats fall.
fn best_phase(envelope: &[f32], period: f32) -> (f32, f32) {
    let n = envelope.len();
    if n < 2 || period < 1.0 {
        return (0.0, 0.0);
    }
    let step = 0.25f32;
    let steps = (period / step).ceil() as usize;
    let mut best = (0.0f32, f32::MIN);
    for s in 0..steps {
        let phi = s as f32 * step;
        let mut sum = 0.0f32;
        let mut count = 0u32;
        let mut x = phi;
        while x < (n - 1) as f32 {
            let i = x as usize;
            let f = x - i as f32;
            sum += envelope[i] * (1.0 - f) + envelope[i + 1] * f;
            count += 1;
            x += period;
        }
        let mean = if count > 0 { sum / count as f32 } else { 0.0 };
        if mean > best.1 {
            best = (phi, mean);
        }
    }
    best
}

#[cfg(test)]
mod tests {
    use super::*;

    const FPS: f32 = 44_100.0 / HOP as f32;

    /// A click track: an onset every `period` hops starting at `phase`, over
    /// `secs` seconds. Each onset is a 3-hop bump like a real transient, and
    /// `weights` cycles over successive onsets (so eighths can alternate
    /// strong/weak like a kick and a hi-hat).
    fn clicks_weighted(bpm: f32, phase: f32, secs: f32, weights: &[f32]) -> Vec<f32> {
        let period = 60.0 * FPS / bpm;
        let n = (secs * FPS) as usize;
        let mut env = vec![0.0f32; n + 4];
        let mut t = phase;
        let mut k = 0usize;
        while (t as usize) + 2 < n {
            let w = weights[k % weights.len()];
            let i = t as usize;
            let f = t - i as f32;
            env[i] += w * 0.5 * (1.0 - f);
            env[i + 1] += w * (1.0 - 0.5 * f);
            env[i + 2] += w * 0.5 * (1.0 + f) * 0.5;
            t += period;
            k += 1;
        }
        env.truncate(n);
        env
    }

    fn clicks(bpm: f32, phase: f32, secs: f32) -> Vec<f32> {
        clicks_weighted(bpm, phase, secs, &[1.0])
    }

    #[test]
    fn finds_the_tempo_to_a_tenth_of_a_bpm() {
        // 174 (drum & bass) is left out on purpose: an even click train at
        // that speed is indistinguishable from 87, and the DJ-tempo prior
        // picks the half-time reading, as most DJ tools do.
        for want in [118.0f32, 120.0, 123.0, 128.0, 140.0, 160.0] {
            let (bpm, confidence, _) = estimate_tempo(&clicks(want, 7.0, 60.0), FPS).unwrap();
            assert!((bpm - want).abs() < 0.1, "{want}: got {bpm}");
            assert!(confidence > 0.3, "{want}: confidence {confidence}");
        }
    }

    #[test]
    fn finds_where_the_beats_fall() {
        let (_, _, phase) = estimate_tempo(&clicks(120.0, 20.0, 60.0), FPS).unwrap();
        // The bump peaks one hop after its start.
        assert!((phase - 21.0).abs() < 0.6, "phase {phase}");
    }

    #[test]
    fn eighth_notes_do_not_double_the_tempo() {
        // Kick on the beat, hi-hat in between: onsets at 250 per minute, beat at 125.
        let env = clicks_weighted(250.0, 3.0, 60.0, &[1.0, 0.4]);
        let (bpm, _, _) = estimate_tempo(&env, FPS).unwrap();
        assert!((bpm - 125.0).abs() < 0.1, "{bpm}");
    }

    #[test]
    fn too_short_or_silent_is_no_answer() {
        assert!(estimate_tempo(&clicks(120.0, 0.0, 1.0), FPS).is_none());
        assert!(estimate_tempo(&vec![0.0; 20_000], FPS).is_none());
    }
}
