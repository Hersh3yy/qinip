//! OS media keys + Now Playing integration (Control Center, keyboard media keys).

use std::sync::mpsc::{self, Receiver};
use std::time::Duration;

use souvlaki::{
    MediaControlEvent, MediaControls, MediaMetadata, MediaPlayback, MediaPosition, PlatformConfig,
};

pub struct SystemMedia {
    controls: MediaControls,
    events_rx: Receiver<MediaControlEvent>,
    last_title: String,
    last_artist: String,
    last_duration: f64,
    last_playing: Option<bool>,
    last_position: f64,
}

impl SystemMedia {
    pub fn new(ctx: egui::Context) -> anyhow::Result<Self> {
        let config = PlatformConfig {
            dbus_name: "qinip",
            display_name: "Qinip",
            hwnd: None,
        };

        let mut controls = MediaControls::new(config)?;
        let (tx, events_rx) = mpsc::channel();
        controls.attach(move |event| {
            let _ = tx.send(event);
            ctx.request_repaint();
        })?;

        Ok(Self {
            controls,
            events_rx,
            last_title: String::new(),
            last_artist: String::new(),
            last_duration: f64::NAN,
            last_playing: None,
            last_position: f64::NAN,
        })
    }

    pub fn poll(&mut self) -> Vec<MediaControlEvent> {
        self.events_rx.try_iter().collect()
    }

    pub fn set_metadata(&mut self, title: &str, artist: &str, duration_secs: f64) {
        let same_text = self.last_title == title && self.last_artist == artist;
        let same_duration = (self.last_duration - duration_secs).abs() < 0.05;
        if same_text && same_duration {
            return;
        }

        self.last_title = title.to_string();
        self.last_artist = artist.to_string();
        self.last_duration = duration_secs;
        // souvlaki replaces the whole Now Playing dictionary — force a playback
        // resync so elapsed time is written back after metadata.
        self.last_playing = None;

        let duration = (duration_secs > 0.0).then(|| Duration::from_secs_f64(duration_secs));
        let _ = self.controls.set_metadata(MediaMetadata {
            title: Some(title),
            artist: Some(artist),
            duration,
            ..Default::default()
        });
    }

    pub fn set_playback(&mut self, playing: bool, position_secs: f64, duration_secs: f64) {
        let playing_changed = self.last_playing != Some(playing);
        let jumped = !self.last_position.is_finite()
            || (self.last_position - position_secs).abs() > 1.5;
        let tick = !self.last_position.is_finite()
            || (self.last_position - position_secs).abs() >= 1.0;
        if !playing_changed && !jumped && !tick {
            return;
        }

        self.last_playing = Some(playing);
        self.last_position = position_secs;

        let progress = (duration_secs > 0.0)
            .then(|| MediaPosition(Duration::from_secs_f64(position_secs.max(0.0))));

        let playback = if playing {
            MediaPlayback::Playing { progress }
        } else if duration_secs > 0.0 {
            MediaPlayback::Paused { progress }
        } else {
            MediaPlayback::Stopped
        };

        let _ = self.controls.set_playback(playback);
    }
}
