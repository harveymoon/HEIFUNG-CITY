//! [dams] (H1) A hydro plant's dam wall, drawn by the player: the plant is
//! placed first, then the wall from bank to bank across its river, one end
//! by the plant. The wall carries a footpath along its crest and nothing
//! else: no road may run over it, and it may not be drawn over a road.
//!
//! Plants from before the wall tool (and those a generator places) keep
//! the old automatic arch across the river beside them until the player
//! draws a wall for them.
//!
//! Pure data here; the rules, the cost and the plant's output are in
//! `world/dams.rs`.

use crate::buildings::BuildingId;
use crate::plan::GestureId;
use glam::Vec2;
use serde::{Deserialize, Serialize};

/// Cost of the wall per metre of crest.
pub const DAM_COST_PER_M: f32 = 240.0;
/// Longest wall the tool draws, metres.
pub const DAM_MAX_LENGTH: f32 = 260.0;
/// One end must stand within this of the plant's lot, metres.
pub const DAM_NEAR_PLANT: f32 = 50.0;
/// Each end must be on dry ground within this of the water, metres.
pub const DAM_BANK_REACH: f32 = 30.0;
/// Half the width of the crest, for what it may not touch, metres.
pub const DAM_HALF_WIDTH: f32 = 3.0;
/// The wall must cross the river within this angle of square (cosine of
/// the angle between the wall and the flow at most this).
pub const DAM_MAX_SKEW: f32 = 0.6;
/// Walls of two plants keep this far apart, metres.
pub const DAM_SPACING: f32 = 40.0;

/// A wall the player drew for a hydro plant.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct DamWall {
    pub plant: BuildingId,
    /// The abutments, on the two banks.
    pub a: Vec2,
    pub b: Vec2,
    /// Upstream at the crossing (unit): the arch bows this way.
    pub upstream: Vec2,
    /// The river it holds back (`Landscape::rivers` index).
    pub river: usize,
    /// The footpath laid along the crest, if it could be laid.
    #[serde(default)]
    pub path: Option<GestureId>,
}

impl DamWall {
    pub fn length(&self) -> f32 {
        self.a.distance(self.b)
    }

    /// How far the arch bows upstream at its middle, metres.
    pub fn bow(&self) -> f32 {
        (self.length() * 0.5 * 0.32).clamp(3.0, 22.0)
    }

    /// The crest's centre line, `n + 1` points from `a` to `b`, bowed
    /// upstream like a gravity-arch dam. The renderer draws the dam on it
    /// and the footpath runs along it.
    pub fn crest(&self, n: usize) -> Vec<Vec2> {
        crest_between(self.a, self.b, self.upstream, self.bow(), n)
    }
}

/// The arch from `a` to `b` bowed `bow` metres towards `upstream`.
pub fn crest_between(a: Vec2, b: Vec2, upstream: Vec2, bow: f32, n: usize) -> Vec<Vec2> {
    let n = n.max(1);
    (0..=n)
        .map(|i| {
            let f = i as f32 / n as f32;
            let t = f * 2.0 - 1.0;
            a.lerp(b, f) + upstream * (bow * (1.0 - t * t))
        })
        .collect()
}

/// What a wall from `a` to `b` would be: the tool's live read-out.
#[derive(Clone, Debug, PartialEq)]
pub struct DamPlan {
    pub plant: BuildingId,
    pub a: Vec2,
    pub b: Vec2,
    pub upstream: Vec2,
    pub river: usize,
    pub length: f32,
    /// Width of the water it spans, metres.
    pub span: f32,
    pub cost: f32,
    /// What the plant would make with the wall, MW: its nameplate times
    /// what the river's current supports (before the weather's share).
    pub expected_mw: f32,
}

/// The walls the player has drawn, and the plants waiting for theirs.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Dams {
    pub walls: Vec<DamWall>,
    /// Hydro plants placed with the tool whose wall is not drawn yet: they
    /// make nothing until it is.
    pub pending: Vec<BuildingId>,
    /// Bumped whenever a wall comes or goes (the renderer redraws).
    pub version: u64,
}

impl Dams {
    pub fn wall_of(&self, plant: BuildingId) -> Option<&DamWall> {
        self.walls.iter().find(|w| w.plant == plant)
    }

    pub fn is_pending(&self, plant: BuildingId) -> bool {
        self.pending.contains(&plant)
    }
}
