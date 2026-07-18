use crate::decode::DecodedAudio;

pub fn waveform_peaks(audio: &DecodedAudio, num_peaks: usize) -> Vec<(f32, f32)> {
    let mono = audio.downmix_to_mono();
    let chunk_size = (mono.len() / num_peaks).max(1);

    mono.chunks(chunk_size)
        .map(|chunk| {
            let min = chunk.iter().cloned().fold(f32::INFINITY, f32::min);
            let max = chunk.iter().cloned().fold(f32::NEG_INFINITY, f32::max);
            (min, max)
        })
        .collect()
}

pub fn print_ascii_waveform(peaks: &[(f32, f32)]) {
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
