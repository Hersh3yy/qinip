use std::fs::File;

use symphonia::core::audio::SampleBuffer;
use symphonia::core::codecs::{DecoderOptions, CODEC_TYPE_NULL};
use symphonia::core::errors::Error;
use symphonia::core::formats::FormatOptions;
use symphonia::core::io::MediaSourceStream;
use symphonia::core::meta::MetadataOptions;
use symphonia::core::probe::Hint;

pub struct DecodedAudio {
    pub samples: Vec<f32>,
    pub sample_rate: u32,
    pub channels: usize,
}

impl DecodedAudio {
    pub fn duration_secs(&self) -> f64 {
        let frame_count = self.samples.len() / self.channels.max(1);
        frame_count as f64 / self.sample_rate as f64
    }

    pub fn peak(&self) -> f32 {
        self.samples
            .iter()
            .map(|s| s.abs())
            .fold(0.0f32, f32::max)
    }

    pub fn rms(&self) -> f32 {
        if self.samples.is_empty() {
            return 0.0;
        }
        let sum_sq: f64 = self.samples.iter().map(|s| (*s as f64) * (*s as f64)).sum();
        (sum_sq / self.samples.len() as f64).sqrt() as f32
    }

    /// Average interleaved channels down to a single mono signal.
    pub fn downmix_to_mono(&self) -> Vec<f32> {
        if self.channels <= 1 {
            return self.samples.clone();
        }

        let frame_count = self.samples.len() / self.channels;
        let channels = self.channels;

        (0..frame_count)
            .map(|frame| {
                let start = frame * channels;
                let end = start + channels;
                self.samples[start..end].iter().sum::<f32>() / channels as f32
            })
            .collect()
    }
}

pub fn decode_to_samples(path: &str) -> DecodedAudio {
    let file = File::open(path).unwrap_or_else(|err| panic!("failed to open {path}: {err}"));
    let mss = MediaSourceStream::new(Box::new(file), Default::default());

    let mut hint = Hint::new();
    if let Some(ext) = std::path::Path::new(path).extension().and_then(|e| e.to_str()) {
        hint.with_extension(ext);
    }

    let probed = symphonia::default::get_probe()
        .format(
            &hint,
            mss,
            &FormatOptions::default(),
            &MetadataOptions::default(),
        )
        .unwrap_or_else(|err| panic!("failed to probe {path}: {err}"));

    let mut format = probed.format;
    let track = format
        .tracks()
        .iter()
        .find(|t| t.codec_params.codec != CODEC_TYPE_NULL)
        .unwrap_or_else(|| panic!("no audio track found in {path}"));

    let track_id = track.id;
    let mut decoder = symphonia::default::get_codecs()
        .make(&track.codec_params, &DecoderOptions::default())
        .unwrap_or_else(|err| panic!("failed to create decoder for {path}: {err}"));

    let mut sample_rate = track
        .codec_params
        .sample_rate
        .unwrap_or(44_100);
    let mut channels = track
        .codec_params
        .channels
        .map(|c| c.count())
        .unwrap_or(2);

    let mut samples: Vec<f32> = Vec::new();
    let mut sample_buf: Option<SampleBuffer<f32>> = None;

    loop {
        let packet = match format.next_packet() {
            Ok(packet) => packet,
            Err(Error::ResetRequired) => continue,
            Err(Error::IoError(_)) => break,
            Err(err) => panic!("decode error in {path}: {err}"),
        };

        if packet.track_id() != track_id {
            continue;
        }

        let decoded = decoder
            .decode(&packet)
            .unwrap_or_else(|err| panic!("packet decode failed for {path}: {err}"));

        let spec = *decoded.spec();
        sample_rate = spec.rate;
        channels = spec.channels.count();

        let frame_count = decoded.capacity() as u64;
        let buf = sample_buf.get_or_insert_with(|| SampleBuffer::<f32>::new(frame_count, spec));
        if buf.capacity() < decoded.capacity() {
            *buf = SampleBuffer::<f32>::new(frame_count, spec);
        }

        buf.copy_interleaved_ref(decoded);
        samples.extend_from_slice(buf.samples());
    }

    DecodedAudio {
        samples,
        sample_rate,
        channels,
    }
}
