//! [plant-groups] Solar and wind lots that touch run as one installation:
//! a "Sunfield" or a "Wind Park". The grouping is derived, never stored:
//! it is worked out from which plant lots touch (share an edge, or face
//! each other across a lot line a couple of metres wide, never across a
//! road), so placing, bulldozing or loading a save always gives the right
//! groups, and old saves need nothing new.
//!
//! A group's output is the sum of its members' (each member's lot area,
//! times whatever site factor applies to that lot, times a small bonus for
//! the shared inverters and grid connection); upkeep is the members' sum.
//! A lot too small for a wind farm on its own may join one next door.

use super::*;
use std::collections::VecDeque;

/// Gap two plant lots may have between them and still count as touching,
/// metres: a lot line, a hedge, a ditch; less than any street or path.
pub const PLANT_GROUP_GAP: f32 = 2.5;
/// How much of their outlines two lots must have that close to touch,
/// metres: more than a corner.
pub const PLANT_GROUP_MIN_CONTACT: f32 = 4.0;
/// Output bonus per lot beyond the first (shared inverters, one grid
/// connection, one maintenance crew), and its cap.
pub const PLANT_GROUP_BONUS_PER_LOT: f32 = 0.04;
pub const PLANT_GROUP_BONUS_MAX: f32 = 0.12;

/// A group of touching solar or wind lots run as one plant.
#[derive(Clone, Debug, PartialEq)]
pub struct PlantGroup {
    pub kind: PowerPlantKind,
    /// "Helios Sunfield", "Kestrel Wind Park": from the founding member.
    pub name: String,
    /// The plants, by lot id (the first is the group's anchor: its row id
    /// in the plant table).
    pub members: Vec<BuildingId>,
    pub lots: Vec<LotId>,
    /// The oldest member, which names the group.
    pub founder: BuildingId,
    /// Total lot area, m².
    pub area: f32,
}

impl PlantGroup {
    pub fn anchor(&self) -> BuildingId {
        self.members[0]
    }
}

/// What placing a solar or wind farm on a lot would join.
#[derive(Clone, Debug, PartialEq)]
pub struct PlantJoin {
    /// The name the joined installation would have (the existing group's
    /// or the existing lone plant's group name); `None` when it touches
    /// nothing of its kind.
    pub joins: Option<String>,
    /// Plants it would join (all touching groups).
    pub members: Vec<BuildingId>,
    /// Lot area of the joined installation, the new lot included, m².
    pub area: f32,
    /// Its nameplate once joined, MW (the bonus included).
    pub capacity_mw: f32,
}

/// Output share for a group of `n` lots.
pub fn group_bonus(n: usize) -> f32 {
    1.0 + (PLANT_GROUP_BONUS_PER_LOT * n.saturating_sub(1) as f32).min(PLANT_GROUP_BONUS_MAX)
}

/// The installation name for a group founded by `founder`.
pub fn group_name(kind: PowerPlantKind, founder: BuildingId) -> String {
    use slotmap::Key;
    const SUN: [&str; 12] = ["Helios", "Amberlight", "Halcyon", "Brightwater", "Goldcrest", "Saffron", "Daybreak", "Marigold", "Lumen", "Solace", "Sunreach", "Copperline"];
    const WIND: [&str; 12] = ["Kestrel", "Westerly", "Highmoor", "Galeway", "Northgate", "Skyline", "Harrier", "Cirrus", "Thornfield", "Blackwater", "Windrush", "Saltmarsh"];
    let mut h = founder.data().as_ffi().wrapping_mul(0x9E37_79B9_7F4A_7C15);
    h ^= h >> 29;
    match kind {
        PowerPlantKind::Wind => format!("{} Wind Park", WIND[(h % WIND.len() as u64) as usize]),
        _ => format!("{} Sunfield", SUN[(h % SUN.len() as u64) as usize]),
    }
}

impl World {
    /// Do two plant lots touch: enough of their outlines within
    /// [`PLANT_GROUP_GAP`] of each other, with no road or path between?
    pub fn plant_lots_touch(&self, a: LotId, b: LotId) -> bool {
        let (Some(la), Some(lb)) = (self.lots.get(a), self.lots.get(b)) else { return false };
        lots_touch(&la.polygon, &lb.polygon, &self.roads)
    }

    /// Finished plants of `kind` (not being cleared), with their lots.
    fn plants_of(&self, kind: PowerPlantKind) -> Vec<(BuildingId, LotId)> {
        let mut v: Vec<(BuildingId, LotId)> = self.buildings.iter().filter(|(_, b)| b.plant == Some(kind) && b.demolition.is_none() && self.lots.contains_key(b.lot)).map(|(id, b)| (id, b.lot)).collect();
        v.sort_by_key(|x| x.1);
        v
    }

    /// The plants reachable from `start` lots through touching lots, by lot.
    fn flood(&self, all: &[(BuildingId, LotId)], start: &[LotId]) -> Vec<(BuildingId, LotId)> {
        let mut field: Vec<LotId> = start.to_vec();
        let mut out: Vec<(BuildingId, LotId)> = all.iter().filter(|(_, l)| start.contains(l)).copied().collect();
        let mut next = 0;
        while next < field.len() {
            let here = field[next];
            next += 1;
            for &(id, l) in all {
                if !field.contains(&l) && self.plant_lots_touch(here, l) {
                    field.push(l);
                    out.push((id, l));
                }
            }
        }
        out.sort_by_key(|x| x.1);
        out
    }

    fn make_group(&self, kind: PowerPlantKind, members: &[(BuildingId, LotId)]) -> PlantGroup {
        let founder = members.iter().map(|&(id, _)| id).min_by_key(|&id| (self.buildings[id].built_tick, self.buildings[id].lot)).unwrap();
        PlantGroup {
            kind,
            name: group_name(kind, founder),
            members: members.iter().map(|x| x.0).collect(),
            lots: members.iter().map(|x| x.1).collect(),
            founder,
            area: members.iter().map(|x| self.lots[x.1].area()).sum(),
        }
    }

    /// The group `id` belongs to: `None` for a plant that does not group
    /// (anything but solar and wind), is being cleared, or has no neighbour
    /// of its kind.
    pub fn plant_group_of(&self, id: BuildingId) -> Option<PlantGroup> {
        let b = self.buildings.get(id)?;
        let kind = b.plant.filter(|k| k.merges_with_neighbours())?;
        if b.demolition.is_some() {
            return None;
        }
        let members = self.flood(&self.plants_of(kind), &[b.lot]);
        (members.len() >= 2).then(|| self.make_group(kind, &members))
    }

    /// Every group of two or more plants, solar first, each by anchor lot.
    pub fn plant_groups(&self) -> Vec<PlantGroup> {
        let mut out = Vec::new();
        for kind in [PowerPlantKind::Solar, PowerPlantKind::Wind] {
            let all = self.plants_of(kind);
            let mut seen: Vec<LotId> = Vec::new();
            for &(_, l) in &all {
                if seen.contains(&l) {
                    continue;
                }
                let members = self.flood(&all, &[l]);
                seen.extend(members.iter().map(|x| x.1));
                if members.len() >= 2 {
                    out.push(self.make_group(kind, &members));
                }
            }
        }
        out
    }

    /// The output share a plant gets from its group (1 alone).
    pub fn plant_group_bonus(&self, b: &Building) -> f32 {
        match b.plant {
            Some(k) if k.merges_with_neighbours() && b.demolition.is_none() => {
                let members = self.flood(&self.plants_of(k), &[b.lot]);
                group_bonus(members.len())
            }
            _ => 1.0,
        }
    }

    /// What a solar or wind farm on `lot` would join, and the joined
    /// installation's area and nameplate. `None` for other kinds.
    pub fn plant_join(&self, kind: PowerPlantKind, lot: LotId) -> Option<PlantJoin> {
        if !kind.merges_with_neighbours() {
            return None;
        }
        let l = self.lots.get(lot)?;
        let all: Vec<(BuildingId, LotId)> = self.plants_of(kind).into_iter().filter(|x| x.1 != lot).collect();
        let touching: Vec<LotId> = all.iter().filter(|x| self.plant_lots_touch(lot, x.1)).map(|x| x.1).collect();
        let members = if touching.is_empty() { Vec::new() } else { self.flood(&all, &touching) };
        let n = members.len() + 1;
        let base: f32 = members.iter().map(|&(id, _)| kind.capacity_mw() * self.plant_scale(&self.buildings[id])).sum::<f32>() + kind.capacity_mw() * kind.size_scale(l.area());
        let joins = (!members.is_empty()).then(|| {
            // The name the joined park keeps: the oldest member's.
            let founder = members.iter().map(|&(id, _)| id).min_by_key(|&id| (self.buildings[id].built_tick, self.buildings[id].lot)).unwrap();
            group_name(kind, founder)
        });
        Some(PlantJoin {
            joins,
            area: l.area() + members.iter().map(|x| self.lots[x.1].area()).sum::<f32>(),
            capacity_mw: base * group_bonus(n),
            members: members.into_iter().map(|x| x.0).collect(),
        })
    }

    /// [plant-groups] The small-lot exception: a solar or wind farm on a lot
    /// below its kind's minimum may still go in when it joins plants of its
    /// kind next door and the joined installation reaches the minimum.
    /// `Some(name)` of the group it joins when the exception applies.
    pub fn small_plant_lot_joins(&self, kind: PowerPlantKind, lot: LotId) -> Option<String> {
        let j = self.plant_join(kind, lot)?;
        (j.area >= kind.min_area()).then_some(j.joins).flatten()
    }

    /// A row of the plant table per group instead of per member lot: each
    /// group's member rows are folded into one under the group's name and
    /// its anchor's id. Called by `plant_rows`.
    pub(super) fn fold_group_rows(&self, rows: Vec<PlantRow>) -> Vec<PlantRow> {
        let groups = self.plant_groups();
        if groups.is_empty() {
            return rows;
        }
        let mut out: Vec<PlantRow> = Vec::with_capacity(rows.len());
        let mut folded: Vec<Option<PlantRow>> = vec![None; groups.len()];
        let mut order: Vec<usize> = Vec::new();
        for r in rows {
            let Some(gi) = groups.iter().position(|g| g.members.contains(&r.id)) else {
                out.push(r);
                continue;
            };
            match &mut folded[gi] {
                None => {
                    let g = &groups[gi];
                    let mut row = r;
                    row.id = g.anchor();
                    row.name = g.name.clone();
                    row.members = g.members.clone();
                    row.lots = g.lots.clone();
                    row.area_m2 = g.area;
                    // Recomputed once every member is in.
                    row.air *= row.capacity_mw;
                    folded[gi] = Some(row);
                    order.push(gi);
                }
                Some(acc) => {
                    acc.now_mw += r.now_mw;
                    acc.capacity_mw += r.capacity_mw;
                    acc.mean_mw += r.mean_mw;
                    acc.upkeep_per_day += r.upkeep_per_day;
                    acc.co2_t_per_day += r.co2_t_per_day;
                    acc.air += r.air * r.capacity_mw;
                    acc.noise_db = match (acc.noise_db, r.noise_db) {
                        (Some(a), Some(b)) => Some(a.max(b)),
                        (a, b) => a.or(b),
                    };
                    acc.derelict &= r.derelict;
                    acc.pending |= r.pending;
                }
            }
        }
        for gi in order {
            let mut row = folded[gi].take().unwrap();
            row.air /= row.capacity_mw.max(0.01);
            row.cost_per_mwh = (row.mean_mw > 0.05).then(|| row.upkeep_per_day / (row.mean_mw * 24.0));
            out.push(row);
        }
        out
    }

    /// The hourly output record of a plant table row: a lone plant's own,
    /// or its members' summed hour by hour (aligned at the latest hour).
    pub fn plant_row_hours(&self, row: &PlantRow) -> Option<VecDeque<f32>> {
        let logs: Vec<&crate::history::PlantLog> = row.members.iter().filter_map(|&id| self.economy.history.plant(id)).collect();
        let n = logs.iter().map(|l| l.hours.len()).max()?;
        let mut out: VecDeque<f32> = std::iter::repeat_n(0.0, n).collect();
        for l in logs {
            let off = n - l.hours.len();
            for (i, x) in l.hours.iter().enumerate() {
                out[off + i] += x;
            }
        }
        Some(out)
    }
}

/// Lot outlines `a` and `b` touch: at least [`PLANT_GROUP_MIN_CONTACT`] of
/// `a`'s outline lies on `b`'s or faces it across at most
/// [`PLANT_GROUP_GAP`] (corners meeting do not count), and no road or path
/// runs through the contact.
pub fn lots_touch(a: &[Vec2], b: &[Vec2], roads: &RoadNetwork) -> bool {
    use crate::geom::{bounds, boxes_overlap, project_on_segment};
    // The nearest point of `b`'s outline to `x`.
    let nearest = |x: Vec2| {
        let mut best = (f32::INFINITY, x);
        for j in 0..b.len() {
            let q = project_on_segment(x, b[j], b[(j + 1) % b.len()]).1;
            let d = q.distance(x);
            if d < best.0 {
                best = (d, q);
            }
        }
        best
    };
    if a.len() < 3 || b.len() < 3 || !boxes_overlap(bounds(a), bounds(b), PLANT_GROUP_GAP) {
        return false;
    }
    let step = 0.5;
    let (mut contact, mut sum, mut n) = (0.0f32, Vec2::ZERO, 0.0f32);
    for i in 0..a.len() {
        let (p, q) = (a[i], a[(i + 1) % a.len()]);
        let len = p.distance(q);
        let k = (len / step).ceil().max(1.0) as usize;
        let along = (q - p).normalize_or_zero();
        for s in 0..k {
            let x = p.lerp(q, (s as f32 + 0.5) / k as f32);
            // Close, and facing across the gap (not just near a corner
            // further along the same line).
            let (d, at) = nearest(x);
            if d <= 0.05 || (d <= PLANT_GROUP_GAP && ((at - x) / d).dot(along).abs() < 0.5) {
                contact += len / k as f32;
                sum += x;
                n += 1.0;
            }
        }
    }
    if contact < PLANT_GROUP_MIN_CONTACT {
        return false;
    }
    // Not across a road: nothing on the network runs through the contact.
    let mid = sum / n;
    let r = PLANT_GROUP_GAP + 8.0;
    !roads.segments_in_box(mid - Vec2::splat(r), mid + Vec2::splat(r)).any(|sid| {
        let seg = roads.segment(sid);
        let (p, q) = roads.segment_points(sid);
        crate::geom::project_on_segment(mid, p, q).1.distance(mid) < seg.half_width() + 0.25
    })
}

/// [plant-groups] What a plant of some kind would be on a given lot, for
/// the Buildings tool's card and the remote API: its own nameplate, cost
/// and upkeep there, what it would join, and why it cannot go (if not).
#[derive(Clone, Debug, PartialEq)]
pub struct PlantPreview {
    pub kind: PowerPlantKind,
    pub area: f32,
    /// Its own nameplate on this lot, MW, the group bonus included.
    pub capacity_mw: f32,
    /// Share of nameplate the site supports (hydro's current, geothermal's
    /// heat; 1 elsewhere), 0 when the site does not suit.
    pub site: f32,
    pub cost: f32,
    pub upkeep_per_day: f32,
    pub join: Option<PlantJoin>,
    /// Allowed only through the small-lot exception.
    pub small_lot_joins: bool,
    pub problem: Option<String>,
}

impl World {
    pub fn plant_preview(&self, kind: PowerPlantKind, lot: LotId) -> Option<PlantPreview> {
        let l = self.lots.get(lot)?;
        let area = l.area();
        let scale = kind.size_scale(area);
        let join = self.plant_join(kind, lot);
        let n = join.as_ref().map_or(1, |j| j.members.len() + 1);
        Some(PlantPreview {
            kind,
            area,
            capacity_mw: kind.capacity_mw() * scale * group_bonus(n),
            site: self.power_site(lot, kind).unwrap_or(0.0),
            cost: self.plant_cost_on(kind, lot),
            upkeep_per_day: kind.upkeep_per_day() * scale,
            small_lot_joins: area < kind.min_area() && join.as_ref().is_some_and(|j| j.joins.is_some() && j.area >= kind.min_area()),
            join: join.filter(|j| j.joins.is_some()),
            problem: self.placement_problem(crate::services::Placeable::Plant(kind), lot),
        })
    }
}
