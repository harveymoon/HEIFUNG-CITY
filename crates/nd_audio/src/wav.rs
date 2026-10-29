//! Offline renders: the engine into a buffer, a WAV writer, and the
//! numbers that stand in for listening (peak, RMS, DC, clipping, octave
//! band levels).

use std::io::Write;
use std::path::Path;
use std::sync::Arc;

use crate::cues::Cue;
use crate::dsp::{gain_to_db, Svf};
use crate::engine::{Command, Engine, Telemetry};
use crate::mix::MixLevels;
use crate::scene::BedParams;

pub const SR: u32 = 48_000;

/// `secs` of the beds alone at `params`, settled first (no fade in), with
/// the ambience bus at full and the master at `mix.master`.
pub fn render_beds(params: BedParams, mix: MixLevels, secs: f32) -> Vec<f32> {
    let mut e = Engine::new(SR as f32, Arc::new(Telemetry::default()));
    e.apply(Command::Mix(mix));
    e.apply(Command::Beds(params));
    e.settle();
    // Let the voices (pass-bys, birds) get going before the capture.
    let mut warm = vec![0.0; SR as usize];
    e.render(&mut warm);
    let mut out = vec![0.0; (SR as f32 * secs) as usize * 2];
    for chunk in out.chunks_mut(1024) {
        e.render(chunk);
    }
    out
}

/// One cue on silence: `pre` seconds of nothing, the cue, then up to `secs`.
pub fn render_cue(cue: Cue, mix: MixLevels, secs: f32) -> Vec<f32> {
    let mut e = Engine::new(SR as f32, Arc::new(Telemetry::default()));
    e.apply(Command::Mix(MixLevels { ambience: 0.0, ..mix }));
    e.apply(Command::Beds(BedParams::default()));
    e.settle();
    let mut out = vec![0.0; (SR as f32 * secs) as usize * 2];
    let pre = (SR as f32 * 0.05) as usize * 2;
    e.render(&mut out[..pre]);
    e.apply(Command::Cue(cue, 1.0));
    for chunk in out[pre..].chunks_mut(512) {
        e.render(chunk);
    }
    out
}

/// Write interleaved stereo `f32` as a 16-bit PCM WAV.
pub fn write_wav(path: &Path, stereo: &[f32], sr: u32) -> std::io::Result<()> {
    let data_len = (stereo.len() * 2) as u32;
    let mut f = std::io::BufWriter::new(std::fs::File::create(path)?);
    f.write_all(b"RIFF")?;
    f.write_all(&(36 + data_len).to_le_bytes())?;
    f.write_all(b"WAVEfmt ")?;
    f.write_all(&16u32.to_le_bytes())?;
    f.write_all(&1u16.to_le_bytes())?;
    f.write_all(&2u16.to_le_bytes())?;
    f.write_all(&sr.to_le_bytes())?;
    f.write_all(&(sr * 4).to_le_bytes())?;
    f.write_all(&4u16.to_le_bytes())?;
    f.write_all(&16u16.to_le_bytes())?;
    f.write_all(b"data")?;
    f.write_all(&data_len.to_le_bytes())?;
    for s in stereo {
        let v = (s.clamp(-1.0, 1.0) * 32_767.0).round() as i16;
        f.write_all(&v.to_le_bytes())?;
    }
    f.flush()
}

/// The octave bands' centres for [`Analysis::bands`].
pub const BANDS: [f32; 9] = [63.0, 125.0, 250.0, 500.0, 1_000.0, 2_000.0, 4_000.0, 8_000.0, 16_000.0];

/// What a render measures.
#[derive(Clone, Debug)]
pub struct Analysis {
    pub peak_db: f32,
    pub rms_db: f32,
    pub dc: f32,
    /// Samples at or over full scale.
    pub clipped: usize,
    /// Level in each octave band (dB, of the mono sum).
    pub bands: [f32; 9],
    /// Seconds from the start to the last sample over -60 dB.
    pub sounding: f32,
    /// Left/right correlation (1 mono, 0 wide).
    pub correlation: f32,
}

pub fn analyse(stereo: &[f32], sr: u32) -> Analysis {
    let n = (stereo.len() / 2).max(1);
    let mut peak = 0.0f32;
    let mut sum2 = 0.0f64;
    let mut sum = 0.0f64;
    let mut clipped = 0;
    let mut last_loud = 0;
    let (mut lr, mut ll, mut rr) = (0.0f64, 0.0f64, 0.0f64);
    for (i, f) in stereo.chunks_exact(2).enumerate() {
        for &s in f {
            peak = peak.max(s.abs());
            sum2 += (s as f64) * (s as f64);
            sum += s as f64;
            if s.abs() >= 0.9999 {
                clipped += 1;
            }
        }
        if f[0].abs().max(f[1].abs()) > 0.001 {
            last_loud = i;
        }
        lr += f[0] as f64 * f[1] as f64;
        ll += f[0] as f64 * f[0] as f64;
        rr += f[1] as f64 * f[1] as f64;
    }
    let mut bands = [0.0f32; 9];
    for (b, &hz) in bands.iter_mut().zip(BANDS.iter()) {
        let mut f = Svf::new(hz, 1.41, sr as f32);
        let mut acc = 0.0f64;
        for fr in stereo.chunks_exact(2) {
            let y = f.bp((fr[0] + fr[1]) * 0.5);
            acc += (y as f64) * (y as f64);
        }
        *b = gain_to_db(((acc / n as f64).sqrt()) as f32);
    }
    Analysis {
        peak_db: gain_to_db(peak),
        rms_db: gain_to_db((sum2 / (2 * n) as f64).sqrt() as f32),
        dc: (sum / (2 * n) as f64) as f32,
        clipped,
        bands,
        sounding: last_loud as f32 / sr as f32,
        correlation: if ll > 0.0 && rr > 0.0 { (lr / (ll * rr).sqrt()) as f32 } else { 1.0 },
    }
}

impl Analysis {
    /// One line: peak, RMS, DC, clipped, and the bands.
    pub fn line(&self) -> String {
        let bands: Vec<String> = self.bands.iter().map(|b| format!("{b:>5.0}")).collect();
        format!(
            "peak {:>6.1} dB  rms {:>6.1} dB  dc {:>8.5}  clip {}  corr {:>5.2}  len {:.2}s  bands[63..16k] {}",
            self.peak_db,
            self.rms_db,
            self.dc,
            self.clipped,
            self.correlation,
            self.sounding,
            bands.join(" ")
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_wav_has_the_right_header_and_length() {
        let dir = std::env::temp_dir().join(format!("nd_audio_wav_{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("x.wav");
        let s = render_cue(Cue::Click, MixLevels::default(), 0.2);
        write_wav(&path, &s, SR).unwrap();
        let bytes = std::fs::read(&path).unwrap();
        assert_eq!(&bytes[0..4], b"RIFF");
        assert_eq!(&bytes[8..12], b"WAVE");
        assert_eq!(bytes.len(), 44 + s.len() * 2);
        let a = analyse(&s, SR);
        assert!(a.peak_db < -6.0 && a.peak_db > -40.0, "{}", a.line());
        assert_eq!(a.clipped, 0);
        let _ = std::fs::remove_dir_all(dir);
    }
}
