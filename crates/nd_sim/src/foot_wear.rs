//! [r10-worn-paths] Where people really walk: a walking-heat field over the
//! city, from the walkers themselves.
//!
//! - Every tick each walker on foot at grade lays its step (from where it
//!   stood to where it stands) into a raster of 2 m cells, one count per
//!   cell it passes (`World::step_foot_wear`); someone waiting at a stop or
//!   sitting on a park bench adds a little where they are. Raised walkways
//!   and decks lay nothing on the ground below.
//! - Every hour the whole field fades a little ([`HALF_LIFE_HOURS`]), so a
//!   cell holds about the passes of the last few days: a value of `S` is
//!   about `S / 4.3` passes a day kept up. A path that falls out of use
//!   fades back to nothing in a week or two.
//! - The field is sparse: chunks of 64 x 64 cells (128 m) only where
//!   someone walked, dropped again once they fade out. Saved quantised on a
//!   log scale, one letter a cell with zero runs as numbers, so an old save
//!   loads clean (`#[serde(default)]`) and wears in.
//!
//! The renderer turns it into worn grass and darkened paving
//! (`nd_app` `render/foot_wear.rs`), the `foot_wear` map layer shows it,
//! and the remote `foot_wear` command reports it. Nothing here draws a
//! random number: the same walks give the same field.

use glam::Vec2;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// Side of a cell, metres.
pub const CELL: f32 = 2.0;
/// Cells along a chunk's side.
pub const CHUNK: usize = 64;
/// Side of a chunk, metres.
pub const CHUNK_M: f32 = CELL * CHUNK as f32;
/// Hours for an unused cell to fade to half.
pub const HALF_LIFE_HOURS: f32 = 72.0;
/// What one hour leaves of a cell.
pub const HOURLY_KEEP: f32 = 0.990_418;
/// The most a cell holds (a cap so nothing runs away).
pub const HEAT_MAX: f32 = 30_000.0;
/// Below this a cell is forgotten.
const EPS: f32 = 0.05;
/// What a minute waiting at a stop (or a kerb) leaves under the feet.
pub const STAND_PER_TICK: f32 = 0.06;
/// What a minute on a park bench leaves in front of it.
pub const SIT_PER_TICK: f32 = 0.03;
/// Heat at which grass starts to show wear (about 10 passes a day kept up).
pub const WEAR_START: f32 = 40.0;
/// Heat at which grass is worn to bare earth (about 140 passes a day).
pub const WEAR_FULL: f32 = 600.0;
/// Log scale of the save's letters: a letter is `round(ln(1 + v) * QSCALE)`.
const QSCALE: f32 = 5.0;
/// The highest letter (`z`).
const QMAX: u32 = 52;

/// The walking-heat field (see the module docs).
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(into = "FootWearSave", from = "FootWearSave")]
pub struct FootWear {
    /// Chunk coordinates (cell coordinates divided by `CHUNK`) to the
    /// chunk's cells, row by row (`y * CHUNK + x`).
    chunks: BTreeMap<(i32, i32), Vec<f32>>,
    /// Bumped when the field fades (hourly) or is reset, for the renderer.
    pub version: u64,
}

/// How a field is saved: per chunk its coordinates and its cells as text.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default)]
struct FootWearSave {
    chunks: Vec<(i32, i32, String)>,
    version: u64,
}

/// `0..1`: how worn ground under heat `v` looks: none below
/// [`WEAR_START`], bare at [`WEAR_FULL`], eased on a log scale between.
pub fn wear(v: f32) -> f32 {
    if v <= WEAR_START {
        return 0.0;
    }
    let t = ((v / WEAR_START).ln() / (WEAR_FULL / WEAR_START).ln()).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

/// The cell a point lies in.
pub fn cell_of(p: Vec2) -> (i32, i32) {
    ((p.x / CELL).floor() as i32, (p.y / CELL).floor() as i32)
}

/// The middle of a cell.
pub fn cell_center(x: i32, y: i32) -> Vec2 {
    Vec2::new((x as f32 + 0.5) * CELL, (y as f32 + 0.5) * CELL)
}

fn split(x: i32, y: i32) -> ((i32, i32), usize) {
    let n = CHUNK as i32;
    ((x.div_euclid(n), y.div_euclid(n)), (y.rem_euclid(n) * n + x.rem_euclid(n)) as usize)
}

fn quantise(v: f32) -> u32 {
    if v <= 0.0 {
        return 0;
    }
    ((1.0 + v).ln() * QSCALE).round().clamp(0.0, QMAX as f32) as u32
}

fn unquantise(q: u32) -> f32 {
    if q == 0 { 0.0 } else { (q as f32 / QSCALE).exp() - 1.0 }
}

/// One chunk's cells as text: a letter per cell (`A` = 1 .. `z` = 52 on
/// the log scale), a run of empty cells as its length in digits.
fn encode(cells: &[f32]) -> String {
    let mut out = String::new();
    let mut zeros = 0usize;
    for &v in cells {
        let q = quantise(v);
        if q == 0 {
            zeros += 1;
            continue;
        }
        if zeros > 0 {
            out.push_str(&zeros.to_string());
            zeros = 0;
        }
        out.push(if q <= 26 { (b'A' + (q - 1) as u8) as char } else { (b'a' + (q - 27) as u8) as char });
    }
    // Trailing empty cells need no mention.
    out
}

fn decode(s: &str) -> Vec<f32> {
    let mut cells = vec![0.0; CHUNK * CHUNK];
    let mut i = 0usize;
    let mut run = 0usize;
    for ch in s.chars() {
        if let Some(d) = ch.to_digit(10) {
            run = run * 10 + d as usize;
            continue;
        }
        i += run;
        run = 0;
        let q = match ch {
            'A'..='Z' => ch as u32 - 'A' as u32 + 1,
            'a'..='z' => ch as u32 - 'a' as u32 + 27,
            _ => continue,
        };
        if i >= cells.len() {
            break;
        }
        cells[i] = unquantise(q);
        i += 1;
    }
    cells
}

impl From<FootWear> for FootWearSave {
    fn from(f: FootWear) -> Self {
        Self { chunks: f.chunks.iter().map(|(&(x, y), c)| (x, y, encode(c))).filter(|(_, _, s)| !s.is_empty()).collect(), version: f.version }
    }
}

impl From<FootWearSave> for FootWear {
    fn from(s: FootWearSave) -> Self {
        let mut chunks = BTreeMap::new();
        for (x, y, text) in s.chunks {
            let cells = decode(&text);
            if cells.iter().any(|&v| v > 0.0) {
                chunks.insert((x, y), cells);
            }
        }
        Self { chunks, version: s.version.wrapping_add(1) }
    }
}

impl FootWear {
    pub fn is_empty(&self) -> bool {
        self.chunks.is_empty()
    }

    /// Add `w` to one cell.
    pub fn add_cell(&mut self, x: i32, y: i32, w: f32) {
        let (key, i) = split(x, y);
        let cells = self.chunks.entry(key).or_insert_with(|| vec![0.0; CHUNK * CHUNK]);
        cells[i] = (cells[i] + w).min(HEAT_MAX);
    }

    /// Add `w` to the cell under `p`.
    pub fn add(&mut self, p: Vec2, w: f32) {
        let (x, y) = cell_of(p);
        self.add_cell(x, y, w);
    }

    /// A step from `a` to `b`: `w` to every cell it passes, once each
    /// (sampled every half cell, so a diagonal step misses no corner cell
    /// it really crosses by much).
    pub fn walk(&mut self, a: Vec2, b: Vec2, w: f32) {
        let d = a.distance(b);
        let n = ((d / (CELL * 0.5)).ceil() as usize).max(1);
        let mut last = None;
        // A step mostly stays in one chunk: look it up once.
        let mut chunk: Option<((i32, i32), &mut Vec<f32>)> = None;
        for k in 0..n {
            let p = a.lerp(b, (k as f32 + 0.5) / n as f32);
            let c = cell_of(p);
            if last == Some(c) {
                continue;
            }
            last = Some(c);
            let (key, i) = split(c.0, c.1);
            if chunk.as_ref().is_none_or(|(k, _)| *k != key) {
                chunk = Some((key, self.chunks.entry(key).or_insert_with(|| vec![0.0; CHUNK * CHUNK])));
            }
            if let Some((_, cells)) = chunk.as_mut() {
                cells[i] = (cells[i] + w).min(HEAT_MAX);
            }
        }
    }

    /// The heat of one cell.
    pub fn cell(&self, x: i32, y: i32) -> f32 {
        let (key, i) = split(x, y);
        self.chunks.get(&key).map_or(0.0, |c| c[i])
    }

    /// The heat of the cell under `p`.
    pub fn at(&self, p: Vec2) -> f32 {
        let (x, y) = cell_of(p);
        self.cell(x, y)
    }

    /// The heat at `p` blended between cell middles.
    pub fn smooth_at(&self, p: Vec2) -> f32 {
        let f = p / CELL - Vec2::splat(0.5);
        let (x0, y0) = (f.x.floor() as i32, f.y.floor() as i32);
        let (tx, ty) = (f.x - x0 as f32, f.y - y0 as f32);
        let a = self.cell(x0, y0);
        let b = self.cell(x0 + 1, y0);
        let c = self.cell(x0, y0 + 1);
        let d = self.cell(x0 + 1, y0 + 1);
        let top = a + (b - a) * tx;
        let bot = c + (d - c) * tx;
        top + (bot - top) * ty
    }

    /// Everything fades by `keep` (what is left of each cell); cells too
    /// faint to matter are forgotten and empty chunks dropped.
    pub fn fade(&mut self, keep: f32) {
        self.chunks.retain(|_, cells| {
            let mut any = false;
            for v in cells.iter_mut() {
                *v *= keep;
                if *v < EPS {
                    *v = 0.0;
                } else {
                    any = true;
                }
            }
            any
        });
        self.version = self.version.wrapping_add(1);
    }

    /// The hour's fade.
    pub fn hourly(&mut self) {
        self.fade(HOURLY_KEEP);
    }

    /// Forget everything (the remote `foot_wear {reset}`).
    pub fn clear(&mut self) {
        self.chunks.clear();
        self.version = self.version.wrapping_add(1);
    }

    /// Every chunk: its coordinates and its cells (`y * CHUNK + x`).
    pub fn chunks(&self) -> impl Iterator<Item = ((i32, i32), &[f32])> + '_ {
        self.chunks.iter().map(|(&k, c)| (k, c.as_slice()))
    }

    /// The cells of one chunk, if anyone walked there.
    pub fn chunk(&self, key: (i32, i32)) -> Option<&[f32]> {
        self.chunks.get(&key).map(|c| c.as_slice())
    }

    /// The south-west corner of a chunk, metres.
    pub fn chunk_origin(key: (i32, i32)) -> Vec2 {
        Vec2::new(key.0 as f32, key.1 as f32) * CHUNK_M
    }

    /// Totals for the API and the probes: chunks held, cells with any heat,
    /// cells worn (past [`WEAR_START`]), cells worn bare (past
    /// [`WEAR_FULL`]), the hottest cell.
    pub fn stats(&self) -> FootWearStats {
        let mut s = FootWearStats { chunks: self.chunks.len(), ..Default::default() };
        for cells in self.chunks.values() {
            for &v in cells {
                if v > 0.0 {
                    s.cells += 1;
                    s.total += v as f64;
                }
                if v >= WEAR_START {
                    s.worn += 1;
                }
                if v >= WEAR_FULL {
                    s.bare += 1;
                }
                s.max = s.max.max(v);
            }
        }
        s
    }

    /// The hottest cells (heat, middle), hottest first, at most `n`.
    pub fn hottest(&self, n: usize) -> Vec<(f32, Vec2)> {
        let mut all: Vec<(f32, Vec2)> = Vec::new();
        for (&(cx, cy), cells) in &self.chunks {
            for (i, &v) in cells.iter().enumerate() {
                if v >= WEAR_START {
                    let (x, y) = (cx * CHUNK as i32 + (i % CHUNK) as i32, cy * CHUNK as i32 + (i / CHUNK) as i32);
                    all.push((v, cell_center(x, y)));
                }
            }
        }
        all.sort_by(|a, b| b.0.total_cmp(&a.0).then(a.1.x.total_cmp(&b.1.x)).then(a.1.y.total_cmp(&b.1.y)));
        all.truncate(n);
        all
    }
}

/// See [`FootWear::stats`].
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct FootWearStats {
    pub chunks: usize,
    pub cells: usize,
    pub worn: usize,
    pub bare: usize,
    pub max: f32,
    pub total: f64,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_step_counts_each_cell_it_passes_once() {
        let mut f = FootWear::default();
        f.walk(Vec2::new(0.5, 1.0), Vec2::new(10.5, 1.0), 1.0);
        for x in 0..5 {
            assert_eq!(f.cell(x, 0), 1.0, "cell {x}");
        }
        assert_eq!(f.cell(6, 0), 0.0);
        assert_eq!(f.cell(0, 1), 0.0);
        // Over a chunk's edge and into negative coordinates.
        f.walk(Vec2::new(-3.0, -3.0), Vec2::new(3.0, -3.0), 2.0);
        assert_eq!(f.at(Vec2::new(-1.0, -3.0)), 2.0);
        assert_eq!(f.at(Vec2::new(1.0, -3.0)), 2.0);
        assert_eq!(f.stats().chunks, 3);
    }

    #[test]
    fn heat_fades_and_unused_ground_is_forgotten() {
        let mut f = FootWear::default();
        f.add(Vec2::new(5.0, 5.0), 100.0);
        let v0 = f.version;
        for _ in 0..HALF_LIFE_HOURS as usize {
            f.hourly();
        }
        let v = f.at(Vec2::new(5.0, 5.0));
        assert!((v - 50.0).abs() < 0.5, "half gone after the half-life: {v}");
        assert!(f.version > v0);
        for _ in 0..2000 {
            f.hourly();
        }
        assert!(f.is_empty(), "faded out and dropped");
    }

    #[test]
    fn the_save_is_sparse_and_close() {
        let mut f = FootWear::default();
        for x in 0..40 {
            f.add_cell(x, 3, 10.0 + x as f32 * 40.0);
        }
        f.add_cell(-200, 900, 5.0);
        let text = ron::to_string(&f).unwrap();
        assert!(text.len() < 400, "{} bytes: {text}", text.len());
        let back: FootWear = ron::from_str(&text).unwrap();
        for x in 0..40 {
            let (a, b) = (f.cell(x, 3), back.cell(x, 3));
            assert!((b - a).abs() <= a * 0.11 + 0.1, "cell {x}: {a} saved as {b}");
        }
        assert!((back.cell(-200, 900) - 5.0).abs() < 0.6);
        assert_eq!(back.cell(41, 3), 0.0);
        assert_eq!(back.stats().chunks, 2);
    }

    #[test]
    fn wear_starts_at_the_threshold_and_saturates() {
        assert_eq!(wear(0.0), 0.0);
        assert_eq!(wear(WEAR_START), 0.0);
        assert!(wear(WEAR_START * 2.0) > 0.0);
        assert!(wear(150.0) < wear(400.0));
        assert!((wear(WEAR_FULL) - 1.0).abs() < 1e-5);
        assert_eq!(wear(HEAT_MAX), 1.0);
    }
}
