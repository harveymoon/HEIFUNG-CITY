//! The sound card: a cpal output stream on its own thread, running the
//! [`Engine`] in its callback. Commands go in through a channel the
//! callback drains without blocking; the readout comes back through
//! [`Telemetry`]'s atomics. With no device (CI, tests, a headless box, or
//! `ND_AUDIO=0`) the handle is silent: everything sent to it is dropped.

use std::sync::mpsc::{channel, Receiver, Sender};
use std::sync::Arc;
use std::time::{Duration, Instant};

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};

use crate::engine::{Command, Engine, Telemetry};

/// Whether the engine is playing, and on what.
#[derive(Clone, Debug, PartialEq)]
pub enum Status {
    Playing { device: String, sample_rate: u32, channels: u16 },
    /// No sound: why.
    Silent(String),
}

/// The game's end of the engine.
pub struct Handle {
    tx: Option<Sender<Command>>,
    pub telemetry: Arc<Telemetry>,
    pub status: Status,
    /// Dropping this stops the stream's thread.
    _stop: Option<Sender<()>>,
}

impl Handle {
    /// No sound at all.
    pub fn silent(why: impl Into<String>) -> Self {
        Self { tx: None, telemetry: Arc::new(Telemetry::default()), status: Status::Silent(why.into()), _stop: None }
    }

    /// The default output device, or silence if there is none.
    pub fn start() -> Self {
        let device = cpal::default_host().default_output_device();
        Self::start_on(device)
    }

    /// Open `device` (None: no device, so silent).
    pub fn start_on(device: Option<cpal::Device>) -> Self {
        let Some(device) = device else {
            return Self::silent("no output device");
        };
        let telemetry = Arc::new(Telemetry::default());
        let (tx, rx) = channel::<Command>();
        let (stop_tx, stop_rx) = channel::<()>();
        let (ready_tx, ready_rx) = channel::<Status>();
        let t = telemetry.clone();
        let spawned = std::thread::Builder::new().name("nd-audio".into()).spawn(move || {
            match open(device, rx, t) {
                Ok((stream, status)) => {
                    let _ = ready_tx.send(status);
                    // Hold the stream until the handle goes.
                    let _ = stop_rx.recv();
                    drop(stream);
                }
                Err(why) => {
                    let _ = ready_tx.send(Status::Silent(why));
                }
            }
        });
        if spawned.is_err() {
            return Self::silent("could not start the audio thread");
        }
        let status = ready_rx.recv_timeout(Duration::from_secs(3)).unwrap_or_else(|_| Status::Silent("the device did not open in time".into()));
        if matches!(status, Status::Silent(_)) {
            return Self { tx: None, telemetry, status, _stop: None };
        }
        Self { tx: Some(tx), telemetry, status, _stop: Some(stop_tx) }
    }

    pub fn playing(&self) -> bool {
        self.tx.is_some()
    }

    /// Send a command; dropped when silent (never blocks, never fails).
    pub fn send(&self, cmd: Command) {
        if let Some(tx) = &self.tx {
            let _ = tx.send(cmd);
        }
    }
}

/// Build and start the stream on `device`.
fn open(device: cpal::Device, rx: Receiver<Command>, telemetry: Arc<Telemetry>) -> Result<(cpal::Stream, Status), String> {
    let name = device.description().map(|d| d.name().to_string()).unwrap_or_else(|_| "output".into());
    let supported = device.default_output_config().map_err(|e| format!("no output config: {e}"))?;
    let format = supported.sample_format();
    let config: cpal::StreamConfig = supported.into();
    let sr = config.sample_rate;
    let channels = config.channels;
    let status = Status::Playing { device: name, sample_rate: sr, channels };
    let stream = match format {
        cpal::SampleFormat::F32 => build::<f32>(&device, &config, rx, telemetry),
        cpal::SampleFormat::I16 => build::<i16>(&device, &config, rx, telemetry),
        cpal::SampleFormat::U16 => build::<u16>(&device, &config, rx, telemetry),
        cpal::SampleFormat::I32 => build::<i32>(&device, &config, rx, telemetry),
        cpal::SampleFormat::F64 => build::<f64>(&device, &config, rx, telemetry),
        other => return Err(format!("unsupported sample format {other}")),
    }?;
    stream.play().map_err(|e| format!("could not play: {e}"))?;
    Ok((stream, status))
}

fn build<T>(device: &cpal::Device, config: &cpal::StreamConfig, rx: Receiver<Command>, telemetry: Arc<Telemetry>) -> Result<cpal::Stream, String>
where
    T: cpal::SizedSample + cpal::FromSample<f32>,
{
    let channels = config.channels.max(1) as usize;
    let sr = config.sample_rate as f32;
    let mut engine = Engine::new(sr, telemetry.clone());
    let mut stereo: Vec<f32> = Vec::new();
    let lost = telemetry.clone();
    device
        .build_output_stream(
            config,
            move |data: &mut [T], _: &cpal::OutputCallbackInfo| {
                let start = Instant::now();
                // A bounded drain: a flood of commands cannot stall the card.
                for cmd in rx.try_iter().take(512) {
                    engine.apply(cmd);
                }
                let frames = data.len() / channels;
                stereo.resize(frames * 2, 0.0);
                engine.render(&mut stereo);
                for (frame, lr) in data.chunks_exact_mut(channels).zip(stereo.chunks_exact(2)) {
                    match channels {
                        1 => frame[0] = T::from_sample((lr[0] + lr[1]) * 0.5),
                        _ => {
                            frame[0] = T::from_sample(lr[0]);
                            frame[1] = T::from_sample(lr[1]);
                            for s in &mut frame[2..] {
                                *s = T::from_sample(0.0f32);
                            }
                        }
                    }
                }
                let budget = frames as f32 / sr;
                if budget > 0.0 {
                    engine.note_cpu(start.elapsed().as_secs_f32() / budget);
                }
            },
            move |err| {
                if matches!(err, cpal::StreamError::DeviceNotAvailable | cpal::StreamError::StreamInvalidated) {
                    lost.lost.store(true, std::sync::atomic::Ordering::Relaxed);
                }
            },
            None,
        )
        .map_err(|e| format!("could not open the stream: {e}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cues::Cue;
    use crate::mix::MixLevels;

    #[test]
    fn with_no_device_the_handle_is_silent_and_takes_everything() {
        let h = Handle::start_on(None);
        assert!(!h.playing());
        assert!(matches!(h.status, Status::Silent(_)));
        for _ in 0..10_000 {
            h.send(Command::Cue(Cue::Click, 1.0));
            h.send(Command::Mix(MixLevels::default()));
        }
        let h = Handle::silent("ND_AUDIO=0");
        h.send(Command::Cue(Cue::Alert, 1.0));
        assert_eq!(h.status, Status::Silent("ND_AUDIO=0".into()));
    }
}
