//! [r11-lot-memory] Lots remember what stood there: a short history of
//! the buildings that came down and the shops that changed hands or trade,
//! so the city shows its past (the ghost of a house on its neighbour's
//! wall, an old slab under the weeds, a sign painted over).
//!
//! - A [`Trace`] is recorded when a building is gone (`World::
//!   finish_demolition`, or at once when nothing is left standing to clear:
//!   `demolish_with`) with its outline, height, trade, name and the day it
//!   went; and when a standing building's trade changes (`World::
//!   set_business`) or a shopfront's tenant does (`World::sync_tenants`),
//!   with what it was and what it became.
//! - Traces are found by place, not by lot key: a lot that is recut keeps
//!   the history of the ground it covers (a trace is "on" the lot whose
//!   outline holds its centre). At most [`PER_LOT`] on one lot and
//!   [`MAX_TRACES`] in the city; the oldest go first.
//! - Saved compactly (a tuple a trace, outlines in decimetres), and
//!   `#[serde(default)]` on `World`, so an old save loads with no history.
//!
//! The renderer reads it (`nd_procgen::lot_memory`): ghosts on the walls a
//! building stood against and its slab on the vacant lot, fading over weeks
//! ([`stage`]), and the old sign painted over on a shop that changed. The
//! inspector and the remote `inspect` show "What stood here". Recording is
//! a pure copy of what the sim already knows: no random numbers.

use crate::buildings::{BuildingId, DemolitionReason};
use crate::clock::MINUTES_PER_DAY;
use crate::market::BusinessKind;
use crate::zoning::{LandUse, Lot};
use glam::Vec2;
use serde::{Deserialize, Serialize};
use slotmap::{Key, KeyData};
use std::collections::HashMap;

/// Traces kept on one lot: enough for "a bakery, then a laundromat, then
/// pulled down".
pub const PER_LOT: usize = 4;
/// Traces kept in the city.
pub const MAX_TRACES: usize = 3000;
/// Most corners kept of an outline.
pub const MAX_OUTLINE: usize = 16;
/// Side of the lookup grid's cells, m.
const GRID: f32 = 64.0;
/// Days a fade stage lasts (a week).
pub const STAGE_DAYS: u64 = 7;
/// The last stage of a wall ghost: faint, and it stays.
pub const WALL_STAGES: u8 = 6;
/// Stages of a slab before the weeds have it all (ten weeks).
pub const SLAB_STAGES: u8 = 10;
/// Stages of a painted-over sign's lettering before it settles faint.
pub const SIGN_STAGES: u8 = 4;

/// What happened.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum TraceKind {
    /// The building came down; its outline is the ground it stood on.
    Demolished,
    /// The building stands, but its trade (or a shopfront's) changed.
    Changed,
}

/// One remembered building or shop (see the module docs).
#[derive(Clone, Debug, PartialEq)]
pub struct Trace {
    /// Unique in the city, in the order recorded.
    pub id: u32,
    pub kind: TraceKind,
    /// When it went (or changed).
    pub tick: u64,
    /// When it was built (Demolished), so "stood 40 days".
    pub built: u64,
    /// The centre of its lot: the trace is on the lot holding it.
    pub at: Vec2,
    /// Demolished: the outline it stood on (its lot, or its merged plots),
    /// counter-clockwise. Empty for a change.
    pub outline: Vec<Vec2>,
    pub floors: u8,
    pub land_use: LandUse,
    /// Its trade (Demolished, Changed: what it was), None for homes.
    pub business: Option<BusinessKind>,
    pub name: String,
    /// The old building's seed: the look of its ghost.
    pub seed: u64,
    /// Demolished: why it came down.
    pub reason: DemolitionReason,
    /// Changed: the shopfront (unit index) whose tenant changed; None for
    /// the building's own trade.
    pub unit: Option<u8>,
    /// Changed: what it became (None: shut, to let).
    pub now: Option<BusinessKind>,
    /// Changed: the building it happened in.
    pub building: BuildingId,
}

impl Trace {
    /// Whole weeks since it went, as of `tick`.
    pub fn weeks(&self, tick: u64) -> u64 {
        tick.saturating_sub(self.tick) / (MINUTES_PER_DAY * STAGE_DAYS)
    }

    /// Bounding box of the outline (or the centre).
    pub fn bounds(&self) -> (Vec2, Vec2) {
        if self.outline.is_empty() {
            return (self.at, self.at);
        }
        self.outline.iter().fold((Vec2::splat(f32::MAX), Vec2::splat(f32::MIN)), |(lo, hi), p| (lo.min(*p), hi.max(*p)))
    }

    /// "a 4-storey bakery", "a 2-storey block of homes", "a car park".
    pub fn what(&self) -> String {
        let thing = match self.business {
            Some(k) => k.label().to_string(),
            None => match self.land_use {
                LandUse::Residential => "block of homes".into(),
                LandUse::Park => "park".into(),
                lu => format!("{} building", lu.label().to_lowercase()),
            },
        };
        if self.kind == TraceKind::Changed || self.floors == 0 {
            return crate::names::a_or_bare(thing);
        }
        crate::names::a_or_bare(format!("{}-storey {thing}", self.floors))
    }

    /// One line for the inspector: "Day 12: Lucky Bowl, a 3-storey bakery,
    /// pulled down (new development planned); stood 30 days", "Day 40: a
    /// bakery shopfront became a grocery".
    pub fn line(&self) -> String {
        let day = self.tick / MINUTES_PER_DAY + 1;
        let place = if self.unit.is_some() { " shopfront" } else { "" };
        let who = if self.name.is_empty() { format!("{}{place}", self.what()) } else { format!("{}, {}{place},", self.name, self.what()) };
        match self.kind {
            TraceKind::Demolished => {
                let stood = self.tick.saturating_sub(self.built) / MINUTES_PER_DAY;
                let why = match self.reason {
                    DemolitionReason::Unknown => String::new(),
                    r => format!(" ({})", r.label()),
                };
                format!("Day {day}: {who} pulled down{why}; stood {stood} days")
            }
            TraceKind::Changed => {
                let became = match self.now {
                    Some(k) => format!("became {}", crate::names::a_or_bare(k.label())),
                    None => "shut".to_string(),
                };
                format!("Day {day}: {who} {became}")
            }
        }
    }
}

/// Fade stage of a trace, `0..=max`, by whole weeks since it went.
pub fn stage(t: &Trace, tick: u64, max: u8) -> u8 {
    t.weeks(tick).min(max as u64) as u8
}

/// The city's lot history (see the module docs).
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(into = "LotMemorySave", from = "LotMemorySave")]
pub struct LotMemory {
    /// Oldest first.
    traces: Vec<Trace>,
    next: u32,
    /// Grid cell to the traces (indices into `traces`) whose bounds touch
    /// it. Rebuilt on every change and on load.
    grid: HashMap<(i32, i32), Vec<u32>>,
    /// Bumped on every change, for the renderer.
    pub version: u64,
}

/// One trace as saved: (id, kind, tick, built, centre, outline, floors,
/// land use, trade, name, seed, reason, unit, now, building), lengths in
/// decimetres.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[allow(clippy::type_complexity)]
struct TraceSave(u32, TraceKind, u64, u64, (i32, i32), Vec<(i32, i32)>, u8, LandUse, Option<BusinessKind>, String, u64, DemolitionReason, Option<u8>, Option<BusinessKind>, u64);

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default)]
struct LotMemorySave {
    traces: Vec<TraceSave>,
    next: u32,
}

fn dm(p: Vec2) -> (i32, i32) {
    ((p.x * 10.0).round() as i32, (p.y * 10.0).round() as i32)
}

fn from_dm(p: (i32, i32)) -> Vec2 {
    Vec2::new(p.0 as f32 * 0.1, p.1 as f32 * 0.1)
}

impl From<LotMemory> for LotMemorySave {
    fn from(m: LotMemory) -> Self {
        let traces = m
            .traces
            .into_iter()
            .map(|t| TraceSave(t.id, t.kind, t.tick, t.built, dm(t.at), t.outline.iter().map(|p| dm(*p)).collect(), t.floors, t.land_use, t.business, t.name, t.seed, t.reason, t.unit, t.now, t.building.data().as_ffi()))
            .collect();
        LotMemorySave { traces, next: m.next }
    }
}

impl From<LotMemorySave> for LotMemory {
    fn from(s: LotMemorySave) -> Self {
        let traces = s
            .traces
            .into_iter()
            .map(|TraceSave(id, kind, tick, built, at, outline, floors, land_use, business, name, seed, reason, unit, now, building)| Trace {
                id,
                kind,
                tick,
                built,
                at: from_dm(at),
                outline: outline.into_iter().map(from_dm).collect(),
                floors,
                land_use,
                business,
                name,
                seed,
                reason,
                unit,
                now,
                building: if building == 0 { BuildingId::default() } else { KeyData::from_ffi(building).into() },
            })
            .collect();
        let mut m = LotMemory { traces, next: s.next, grid: HashMap::new(), version: 1 };
        m.reindex();
        m
    }
}

fn cell(p: Vec2) -> (i32, i32) {
    ((p.x / GRID).floor() as i32, (p.y / GRID).floor() as i32)
}

impl LotMemory {
    pub fn len(&self) -> usize {
        self.traces.len()
    }

    pub fn is_empty(&self) -> bool {
        self.traces.is_empty()
    }

    /// Every trace, oldest first.
    pub fn traces(&self) -> &[Trace] {
        &self.traces
    }

    /// Dev and testing: everything recorded `ticks` earlier (to see the
    /// ghosts fade without waiting weeks).
    pub fn age(&mut self, ticks: u64) {
        for t in self.traces.iter_mut() {
            t.tick = t.tick.saturating_sub(ticks);
            t.built = t.built.min(t.tick);
        }
        self.version += 1;
    }

    pub fn clear(&mut self) {
        self.traces.clear();
        self.grid.clear();
        self.version += 1;
    }

    fn reindex(&mut self) {
        self.grid.clear();
        for (i, t) in self.traces.iter().enumerate() {
            let (lo, hi) = t.bounds();
            let (a, b) = (cell(lo), cell(hi));
            for x in a.0..=b.0 {
                for y in a.1..=b.1 {
                    self.grid.entry((x, y)).or_default().push(i as u32);
                }
            }
        }
    }

    /// Traces whose bounds may reach the box `lo..hi` (each once, oldest
    /// first).
    pub fn near(&self, lo: Vec2, hi: Vec2) -> Vec<&Trace> {
        let (a, b) = (cell(lo), cell(hi));
        let mut ix: Vec<u32> = Vec::new();
        for x in a.0..=b.0 {
            for y in a.1..=b.1 {
                if let Some(v) = self.grid.get(&(x, y)) {
                    ix.extend_from_slice(v);
                }
            }
        }
        ix.sort_unstable();
        ix.dedup();
        ix.into_iter().filter_map(|i| self.traces.get(i as usize)).filter(|t| {
            let (tl, th) = t.bounds();
            tl.x <= hi.x && tl.y <= hi.y && th.x >= lo.x && th.y >= lo.y
        }).collect()
    }

    /// The history of `lot`: the traces on it, newest first.
    pub fn on_lot(&self, lot: &Lot) -> Vec<&Trace> {
        let (lo, hi) = lot.polygon.iter().fold((lot.center, lot.center), |(lo, hi), p| (lo.min(*p), hi.max(*p)));
        let mut v: Vec<&Trace> = self.near(lo, hi).into_iter().filter(|t| lot.contains(t.at)).collect();
        v.sort_by_key(|t| std::cmp::Reverse(t.id));
        v
    }

    /// Record `t` (its id is set here), on `lot` when there is one: the
    /// oldest on the lot go past [`PER_LOT`], the oldest in the city past
    /// [`MAX_TRACES`]. A change noted twice in the same hour (a business
    /// swapped and its shopfront's sign following it) is kept once.
    pub fn record(&mut self, mut t: Trace, lot: Option<&Lot>) {
        if t.kind == TraceKind::Changed {
            let dup = self.traces.iter().rev().take(16).any(|o| o.kind == TraceKind::Changed && o.building == t.building && o.business == t.business && t.tick.saturating_sub(o.tick) < 60);
            if dup {
                return;
            }
        }
        // A demolition swallows the changes noted while it was being
        // started (its business let go a moment before).
        if t.kind == TraceKind::Demolished {
            let b = t.building;
            let tick = t.tick;
            self.traces.retain(|o| !(o.kind == TraceKind::Changed && o.building == b && !b.is_null() && tick.saturating_sub(o.tick) < 60 && o.now.is_none()));
        }
        t.id = self.next;
        self.next = self.next.wrapping_add(1);
        t.outline.truncate(MAX_OUTLINE);
        if let Some(lot) = lot {
            let mine: Vec<u32> = self.traces.iter().filter(|o| lot.contains(o.at)).map(|o| o.id).collect();
            if mine.len() >= PER_LOT {
                let drop: Vec<u32> = mine[..mine.len() + 1 - PER_LOT].to_vec();
                self.traces.retain(|o| !drop.contains(&o.id));
            }
        }
        self.traces.push(t);
        if self.traces.len() > MAX_TRACES {
            let n = self.traces.len() - MAX_TRACES;
            self.traces.drain(..n);
        }
        self.reindex();
        self.version += 1;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn trace(at: Vec2, tick: u64) -> Trace {
        Trace {
            id: 0,
            kind: TraceKind::Demolished,
            tick,
            built: 0,
            at,
            outline: vec![at + Vec2::new(-5.0, -5.0), at + Vec2::new(5.0, -5.0), at + Vec2::new(5.0, 5.0), at + Vec2::new(-5.0, 5.0)],
            floors: 3,
            land_use: LandUse::Residential,
            business: None,
            name: "Gutter Court".into(),
            seed: 7,
            reason: DemolitionReason::Bulldozed,
            unit: None,
            now: None,
            building: BuildingId::default(),
        }
    }

    /// A trace survives the save's compact form (to the decimetre), and
    /// the lookup grid is rebuilt on load.
    #[test]
    fn saved_and_loaded() {
        let mut m = LotMemory::default();
        m.record(trace(Vec2::new(100.04, -40.0), 5000), None);
        let mut c = trace(Vec2::new(300.0, 300.0), 6000);
        c.kind = TraceKind::Changed;
        c.outline.clear();
        c.business = Some(BusinessKind::Bakery);
        c.now = Some(BusinessKind::Grocery);
        c.unit = Some(2);
        m.record(c, None);
        let text = ron::to_string(&m).unwrap();
        let back: LotMemory = ron::from_str(&text).unwrap();
        assert_eq!(back.len(), 2);
        let t = &back.traces()[0];
        assert!((t.at.x - 100.0).abs() < 0.051, "{:?}", t.at);
        assert_eq!(t.outline.len(), 4);
        assert_eq!(back.traces()[1].unit, Some(2));
        assert_eq!(back.traces()[1].now, Some(BusinessKind::Grocery));
        assert_eq!(back.near(Vec2::new(90.0, -50.0), Vec2::new(110.0, -30.0)).len(), 1);
        assert!(back.near(Vec2::new(500.0, 500.0), Vec2::new(510.0, 510.0)).is_empty());
        // An old save (no field) is an empty memory.
        let empty: LotMemory = ron::from_str("()").unwrap();
        assert!(empty.is_empty());
        assert!(text.len() < 400, "compact: {} bytes: {text}", text.len());
    }

    #[test]
    fn bounded_and_oldest_go_first() {
        let mut m = LotMemory::default();
        for i in 0..(MAX_TRACES + 10) {
            m.record(trace(Vec2::new(i as f32 * 20.0, 0.0), i as u64), None);
        }
        assert_eq!(m.len(), MAX_TRACES);
        assert_eq!(m.traces()[0].tick, 10);
    }

    #[test]
    fn fades_by_the_week() {
        let t = trace(Vec2::ZERO, 0);
        let day = MINUTES_PER_DAY;
        assert_eq!(stage(&t, 6 * day, WALL_STAGES), 0);
        assert_eq!(stage(&t, 7 * day, WALL_STAGES), 1);
        assert_eq!(stage(&t, 400 * day, WALL_STAGES), WALL_STAGES);
    }
}
