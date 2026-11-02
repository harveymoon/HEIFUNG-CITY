//! [build-highlight] Picking a section of the Build window that puts down
//! buildings (Services, Safety, Power, Metro, Parking...) shows the ones
//! already on the map: each gets a coloured outline on its lot and a marker
//! above it, in its kind's colour; a picked item ("Hospital") stands out
//! and the rest of its section is shown faintly.
//!
//! With them, where they serve: the sim's own service areas. Police, fire,
//! health and schools cover lots by road distance
//! (`World::service_reach`, one station's share of `refresh_coverage`), so
//! the lots each station reaches are tinted, additively, so overlap reads
//! brighter; trash depots, parking wardens, metro stations and vertiports
//! reach a straight-line radius (a ring, and the lots in it tinted); bus
//! stops have their walk-in catchment. Homes no station of the kind
//! reaches are tinted red. Power has no areas (the grid is one network):
//! the plants carry their output instead.
//!
//! The Build window sets [`BuildHighlight::sel`] each frame
//! (`ui::build::highlight_sel`) and shows the summary; the remote API sets
//! the window with `build_highlight {category}`. The view is worked out
//! again only when what it depends on changes (checked twice a second):
//! the roads, the matching buildings, the homes, the districts.

use std::collections::HashMap;
use std::hash::{Hash, Hasher};
use std::sync::Arc;

use bevy::prelude::*;
use bevy_egui::egui;
use nd_procgen::MeshData;
use nd_sim::services::{COVERAGE_FADE, COVERED};
use nd_sim::{BuildingId, LotId, Placeable, RailFacility, ServiceKind, Vec2 as SVec2, World};

use crate::overlay::shapes::{fill, flat_at, light_wall, outline_band};
use crate::overlay::mesh::{sync, MeshOverlay, MeshOverlayPlugin, OverlayCx, OverlayMat, Pieces, Wanted};
use crate::overlay::OverlaySet;
use bevy::ecs::system::SystemParamItem;
use crate::sim::Sim;
use crate::ui::build::{norm, Cat};

/// What to highlight: a Build window section, optionally one of its
/// sub-sections, and the picked item among them.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct HighlightSel {
    pub cat: Cat,
    pub sub: Option<&'static str>,
    /// The picked item (`ui::build::norm`), shown strongly.
    pub focus: Option<Placeable>,
}

/// The kinds of thing highlighted, each with its colour and marker.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Family {
    Police,
    Fire,
    Health,
    School,
    Dispatch,
    Jail,
    Trash,
    Wardens,
    Power,
    Battery,
    Buses,
    Metro,
    Air,
    Parking,
    Freight,
    Rail,
    Water,
    Attraction,
}

impl Family {
    pub fn of(p: Placeable) -> Option<Family> {
        Some(match p {
            Placeable::Service(k, _) => match k {
                ServiceKind::Police => Family::Police,
                ServiceKind::Fire => Family::Fire,
                ServiceKind::Health => Family::Health,
                ServiceKind::School | ServiceKind::University => Family::School,
                ServiceKind::Dispatch => Family::Dispatch,
                ServiceKind::Jail => Family::Jail,
                ServiceKind::TrashDepot => Family::Trash,
                // [r5-trash] the disposal sites outline with the depots
                k if k.is_waste() => Family::Trash,
                ServiceKind::ParkingEnforcement => Family::Wardens,
                ServiceKind::Vertiport => Family::Air,
                ServiceKind::Monument | ServiceKind::Stadium => Family::Attraction,
                // [attractions] the landmarks
                _ => Family::Attraction,
            },
            Placeable::Plant(_) => Family::Power,
            Placeable::Battery(_) => Family::Battery,
            Placeable::BusDepot => Family::Buses,
            Placeable::MetroStation => Family::Metro,
            Placeable::ParkingLot(_) => Family::Parking,
            Placeable::CargoHub => Family::Freight,
            Placeable::Rail(_) => Family::Rail,
            Placeable::Dock(_) => Family::Water,
            // [airport]
            Placeable::Airport(_) => Family::Air,
            Placeable::Toll(_) => return None,
        })
    }

    /// Its colour, sRGB.
    pub fn rgb(self) -> [u8; 3] {
        match self {
            Family::Police => [70, 135, 255],
            Family::Fire => [255, 100, 40],
            Family::Health => [70, 235, 150],
            Family::School => [255, 205, 60],
            Family::Dispatch => [150, 165, 255],
            Family::Jail => [175, 150, 215],
            Family::Trash => [175, 205, 70],
            Family::Wardens => [240, 140, 220],
            Family::Power => [255, 215, 60],
            Family::Battery => [110, 250, 205],
            Family::Buses => [80, 210, 255],
            Family::Metro => [200, 125, 255],
            Family::Air => [140, 230, 255],
            Family::Parking => [95, 160, 255],
            Family::Freight => [235, 160, 90],
            Family::Rail => [225, 205, 170],
            Family::Water => [60, 200, 235],
            Family::Attraction => [255, 120, 200],
        }
    }

    pub fn color32(self) -> egui::Color32 {
        let [r, g, b] = self.rgb();
        egui::Color32::from_rgb(r, g, b)
    }

    /// Its colour in linear light, for the meshes and gizmos.
    pub fn linear(self) -> [f32; 3] {
        let [r, g, b] = self.rgb();
        let c = Color::srgb_u8(r, g, b).to_linear();
        [c.red, c.green, c.blue]
    }

    /// (one, many): "hospital", "hospitals".
    pub fn noun(self) -> (&'static str, &'static str) {
        match self {
            Family::Police => ("police station", "police stations"),
            Family::Fire => ("fire station", "fire stations"),
            Family::Health => ("hospital", "hospitals"),
            Family::School => ("school", "schools"),
            Family::Dispatch => ("dispatch centre", "dispatch centres"),
            Family::Jail => ("jail", "jails"),
            Family::Trash => ("trash depot", "trash depots"),
            Family::Wardens => ("parking office", "parking offices"),
            Family::Power => ("power plant", "power plants"),
            Family::Battery => ("battery", "batteries"),
            Family::Buses => ("bus depot", "bus depots"),
            Family::Metro => ("metro station", "metro stations"),
            Family::Air => ("vertiport", "vertiports"),
            Family::Parking => ("city car park", "city car parks"),
            Family::Freight => ("cargo hub", "cargo hubs"),
            Family::Rail => ("rail station", "rail stations"),
            Family::Water => ("dock", "docks"),
            Family::Attraction => ("attraction", "attractions"),
        }
    }

    fn count(self, n: usize) -> String {
        let (one, many) = self.noun();
        format!("{n} {}", if n == 1 { one } else { many })
    }

    /// Whether its service area is about where people live: homes it does
    /// not reach are marked.
    fn serves_homes(self) -> bool {
        matches!(self, Family::Police | Family::Fire | Family::Health | Family::School | Family::Trash | Family::Metro | Family::Buses)
    }

    /// The word for what its area is.
    fn area_word(self) -> &'static str {
        match self {
            Family::Police | Family::Fire | Family::Health | Family::School => "covered by road",
            Family::Metro | Family::Buses | Family::Air => "within a walk",
            _ => "reached",
        }
    }
}

/// How a kind reaches the city around it.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Reach {
    /// Along the streets (`World::service_reach`).
    Road,
    /// A straight-line radius; `tint` false for a reach too wide to tint
    /// (a cargo hub's catchment is most of the city): only the ring.
    Radius { r: f32, tint: bool },
}

fn reach_of(p: Placeable) -> Option<Reach> {
    match p {
        Placeable::Service(k, _) if k.coverage_index().is_some() => Some(Reach::Road),
        Placeable::Service(k @ (ServiceKind::TrashDepot | ServiceKind::ParkingEnforcement), t) => Some(Reach::Radius { r: k.radius(t), tint: true }),
        Placeable::Service(ServiceKind::Vertiport, _) => Some(Reach::Radius { r: nd_sim::air::REACH, tint: true }),
        Placeable::MetroStation => Some(Reach::Radius { r: nd_sim::subway::REACH, tint: true }),
        Placeable::Rail(RailFacility::Central) => Some(Reach::Radius { r: nd_sim::subway::CENTRAL_REACH, tint: true }),
        Placeable::CargoHub => Some(Reach::Radius { r: nd_sim::cargo_hub::HUB_REACH, tint: false }),
        // Bus depots: the stops of the lines are the catchment.
        Placeable::BusDepot => Some(Reach::Radius { r: nd_sim::transit::STOP_REACH, tint: true }),
        _ => None,
    }
}

/// How strongly a building is shown.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Level {
    /// The picked item's kind.
    Focus,
    /// A section or sub-section picked, no item.
    Normal,
    /// The rest of the picked item's section.
    Faint,
}

/// One highlighted building.
#[derive(Clone, Debug)]
pub struct Marked {
    pub id: BuildingId,
    pub lot: LotId,
    pub at: SVec2,
    /// Height of its roof above the ground, metres.
    pub roof: f32,
    pub family: Family,
    pub level: Level,
    pub name: String,
    /// A figure under the marker: a plant's output, a car park's spaces.
    pub label: Option<String>,
}

/// One lot inside a service area.
#[derive(Clone, Copy, Debug)]
pub struct Tint {
    pub lot: LotId,
    pub family: Family,
    /// How many buildings of the family reach it.
    pub count: u16,
    /// The best coverage among them, `0..1`.
    pub cov: f32,
}

/// An uncovered place, for the summary's buttons.
#[derive(Clone, Debug)]
pub struct Spot {
    pub name: String,
    pub at: SVec2,
    pub homes: u32,
}

#[derive(Clone, Debug, Default)]
pub struct Summary {
    /// "12 hospitals · 84% of homes covered · 3 districts uncovered".
    pub headline: String,
    pub detail: Vec<String>,
    /// The colour key: (colour, what it means).
    pub legend: Vec<(egui::Color32, String)>,
    /// Uncovered places, the most homes first.
    pub spots: Vec<Spot>,
}

/// Everything drawn for one selection.
#[derive(Clone, Debug, Default)]
pub struct HighlightView {
    pub marked: Vec<Marked>,
    pub tints: Vec<Tint>,
    /// Home lots no building of the (one) family shown reaches.
    pub uncovered: Vec<LotId>,
    /// Residential lots with no homes yet that it would not reach either:
    /// where new homes would go unserved (drawn fainter).
    pub uncovered_zoned: Vec<LotId>,
    /// Straight-line reaches: centre, radius, family.
    pub rings: Vec<(SVec2, f32, Family)>,
    pub summary: Summary,
    /// Share of homes covered (when one home-serving family is shown).
    pub homes_covered: Option<f32>,
    /// [fix/buildings-ui] Areas drawn as their outline instead of tinted
    /// lots (the Crime layer's police: the heat map stays readable).
    pub outline_only: bool,
    /// The outline of the areas: segments on the ground, and their family.
    pub outline: Vec<(SVec2, SVec2, Family)>,
}

/// The selection and its worked-out view.
#[derive(Resource, Default)]
pub struct BuildHighlight {
    /// Set every frame by the Build window (None when it is closed or its
    /// section puts down no buildings).
    pub sel: Option<HighlightSel>,
    pub view: Option<Arc<HighlightView>>,
    /// What the view was worked out from, and when that was last checked.
    key: Option<u64>,
    checked: f64,
    checked_sel: Option<HighlightSel>,
    /// Bumped with every new view, for the meshes.
    pub version: u64,
    /// [feat/pt-ux] The selected building the view is about, when the Build
    /// window shows nothing: it gets the marker, figure and area its kind
    /// shows in build mode (a car park's spaces, a plant's output, a
    /// station's reach).
    pub inspected: Option<BuildingId>,
    /// [fix/buildings-ui] The view is the Crime layer's police stations.
    pub police: bool,
}

/// [fix/buildings-ui] The Crime layer's police: every station marked with
/// its shield and its lot outlined, and the ground they reach by road (the
/// lots they cover, `COVERED` or better) drawn as one outline.
pub fn police_view(w: &World) -> HighlightView {
    let police = norm(Placeable::Service(ServiceKind::Police, nd_sim::Tier::Mid));
    let sel = HighlightSel { cat: Cat::Services, sub: None, focus: Some(police) };
    let found = matching(w, &[police]);
    let mut v = compute_from(w, &sel, &[police], &[police], &found);
    let covered: Vec<LotId> = v.tints.iter().filter(|t| t.family == Family::Police && t.cov >= COVERED).map(|t| t.lot).collect();
    v.outline = reach_outline(w, &covered, REACH_GROW).into_iter().map(|(a, b)| (a, b, Family::Police)).collect();
    v.tints.clear();
    v.uncovered.clear();
    v.uncovered_zoned.clear();
    v.outline_only = true;
    v
}

/// How far the covered lots are grown when outlining them: across a
/// street, so the lots either side join into one area.
const REACH_GROW: f32 = 9.0;

/// The outline of the lots `lots` grown by `grow` metres, as segments
/// (marching squares on a 6 m grid; a corner is inside when it lies in a
/// lot or within `grow` of one).
pub fn reach_outline(w: &World, lots: &[LotId], grow: f32) -> Vec<(SVec2, SVec2)> {
    const CELL: f32 = 6.0;
    let polys: Vec<&[SVec2]> = lots.iter().filter_map(|l| w.lots.get(*l)).map(|l| l.polygon.as_slice()).filter(|p| p.len() >= 3).collect();
    if polys.is_empty() {
        return Vec::new();
    }
    let pad = SVec2::splat(grow + CELL * 2.0);
    let (mut lo, mut hi) = (SVec2::splat(f32::MAX), SVec2::splat(f32::MIN));
    for p in &polys {
        for q in p.iter() {
            lo = lo.min(*q);
            hi = hi.max(*q);
        }
    }
    let (lo, hi) = (lo - pad, hi + pad);
    let nx = ((hi.x - lo.x) / CELL).ceil() as usize + 1;
    let ny = ((hi.y - lo.y) / CELL).ceil() as usize + 1;
    if nx * ny > 6_000_000 {
        return Vec::new();
    }
    let at = |i: usize, j: usize| lo + SVec2::new(i as f32 * CELL, j as f32 * CELL);
    let mut inside = vec![false; nx * ny];
    for poly in &polys {
        let (mut a, mut b) = (SVec2::splat(f32::MAX), SVec2::splat(f32::MIN));
        for q in poly.iter() {
            a = a.min(*q);
            b = b.max(*q);
        }
        let (i0, j0) = (((a.x - grow - lo.x) / CELL).floor().max(0.0) as usize, ((a.y - grow - lo.y) / CELL).floor().max(0.0) as usize);
        let (i1, j1) = ((((b.x + grow - lo.x) / CELL).ceil() as usize).min(nx - 1), (((b.y + grow - lo.y) / CELL).ceil() as usize).min(ny - 1));
        for j in j0..=j1 {
            for i in i0..=i1 {
                let k = j * nx + i;
                if inside[k] {
                    continue;
                }
                let p = at(i, j);
                inside[k] = nd_sim::zoning::point_in_polygon(p, poly) || (0..poly.len()).any(|e| nd_sim::roads::project_on_segment(p, poly[e], poly[(e + 1) % poly.len()]).1.distance(p) < grow);
            }
        }
    }
    let mut out = Vec::new();
    for j in 0..ny - 1 {
        for i in 0..nx - 1 {
            // Corners anticlockwise from the bottom left, and the edge
            // midpoints between them (bottom, right, top, left).
            let c = [inside[j * nx + i], inside[j * nx + i + 1], inside[(j + 1) * nx + i + 1], inside[(j + 1) * nx + i]];
            let case = c.iter().enumerate().fold(0u8, |m, (k, &v)| m | ((v as u8) << k));
            if case == 0 || case == 15 {
                continue;
            }
            let o = at(i, j);
            let h = CELL * 0.5;
            let mid = [o + SVec2::new(h, 0.0), o + SVec2::new(CELL, h), o + SVec2::new(h, CELL), o + SVec2::new(0.0, h)];
            // Edges whose corners differ, in order round the cell; a saddle
            // (two of them across) joins them pairwise.
            let crossing: Vec<usize> = (0..4).filter(|&e| c[e] != c[(e + 1) % 4]).collect();
            match crossing.as_slice() {
                [a, b] => out.push((mid[*a], mid[*b])),
                [a, b, cc, d] => {
                    out.push((mid[*a], mid[*b]));
                    out.push((mid[*cc], mid[*d]));
                }
                _ => {}
            }
        }
    }
    out
}

/// [feat/pt-ux] What build mode would show for building `id` alone: its
/// marker with its figure, and its service area. A private car park (a
/// business) gets the car park's marker and spaces too. None for buildings
/// build mode has nothing for (homes, shops, offices).
pub fn inspected_view(w: &World, id: BuildingId) -> Option<HighlightView> {
    let b = w.buildings.get(id)?;
    match w.placed_as(id).filter(|p| Family::of(*p).is_some()) {
        Some(p) => {
            let sel = HighlightSel { cat: Cat::Services, sub: None, focus: Some(norm(p)) };
            let reach = if reach_of(p).is_some() { vec![norm(p)] } else { Vec::new() };
            Some(compute_from(w, &sel, &[norm(p)], &reach, &[(id, p)]))
        }
        None if b.parking_capacity > 0 && b.business.as_ref().is_some_and(|z| z.kind == nd_sim::BusinessKind::Parking) => {
            let lot = w.lots.get(b.lot)?;
            let mut v = HighlightView::default();
            v.marked.push(Marked { id, lot: b.lot, at: lot.center, roof: b.floors as f32 * 3.6 + 4.0, family: Family::Parking, level: Level::Focus, name: b.name.clone(), label: Some(format!("{} / {} spaces", b.parking_used, b.parking_capacity)) });
            Some(v)
        }
        None => None,
    }
}

/// [feat/pt-ux] What the inspected building's view depends on.
fn inspected_key(w: &World, id: BuildingId) -> u64 {
    let mut h = std::collections::hash_map::DefaultHasher::new();
    if let Some(b) = w.buildings.get(id) {
        (id, b.lot, b.parking_used, b.parking_capacity, b.is_derelict(), b.construction.is_some(), w.roads.version, w.lots.len()).hash(&mut h);
        if b.plant.is_some() {
            (w.plant_output_mw(b).round() as i64).hash(&mut h);
        }
        b.battery.as_ref().map(|x| x.charge_mwh.round() as i64).hash(&mut h);
    }
    h.finish()
}

/// The kinds `sel` highlights (normalised), and the ones whose areas show:
/// the picked item's, else all of them.
fn kinds(sel: &HighlightSel) -> (Vec<Placeable>, Vec<Placeable>) {
    let all = crate::ui::build::highlight_kinds(sel.cat, sel.sub);
    let areas = match sel.focus {
        Some(f) => vec![f],
        None => all.clone(),
    };
    (all, areas.into_iter().filter(|p| reach_of(*p).is_some()).collect())
}

/// Matching buildings, with their kind (as placed, its tier kept).
fn matching(w: &World, all: &[Placeable]) -> Vec<(BuildingId, Placeable)> {
    w.buildings.keys().filter_map(|id| w.placed_as(id).filter(|p| all.contains(&norm(*p))).map(|p| (id, p))).collect()
}

/// What the view depends on, hashed: when it is unchanged the view is kept.
fn signature(w: &World, sel: &HighlightSel, found: &[(BuildingId, Placeable)]) -> u64 {
    let mut h = std::collections::hash_map::DefaultHasher::new();
    format!("{sel:?}").hash(&mut h);
    (w.roads.version, w.lots.len(), w.districts.version, w.transit.routes.len()).hash(&mut h);
    for (id, p) in found {
        let b = &w.buildings[*id];
        (id, format!("{p:?}"), b.lot, b.is_derelict(), b.construction.is_some(), b.parking_used / 5).hash(&mut h);
        if b.plant.is_some() {
            (w.plant_output_mw(b).round() as i64).hash(&mut h);
        }
    }
    let (n, homes) = w.buildings.values().filter(|b| b.homes > 0 && b.construction.is_none()).fold((0usize, 0u64), |(n, s), b| (n + 1, s + b.homes as u64));
    (n, homes).hash(&mut h);
    for r in w.transit.routes.values() {
        r.stops.len().hash(&mut h);
        for s in &r.stops {
            (s.pos.x.to_bits(), s.pos.y.to_bits()).hash(&mut h);
        }
    }
    h.finish()
}

/// Lots within `r` of `c`, with a coverage that fades out over the last
/// fifth of the radius (at most `COVERAGE_FADE`).
fn lots_within(w: &World, c: SVec2, r: f32) -> Vec<(LotId, f32)> {
    let fade = (r * 0.2).min(COVERAGE_FADE).max(1.0);
    w.lots
        .iter()
        .filter(|(_, l)| l.usable())
        .filter_map(|(id, l)| {
            let cov = ((r - l.center.distance(c)) / fade).clamp(0.0, 1.0);
            (cov > 0.0).then_some((id, cov))
        })
        .collect()
}

/// Work out what `sel` shows (the tests; `refresh` reuses its search).
#[cfg(test)]
pub fn compute(w: &World, sel: &HighlightSel) -> HighlightView {
    let (all, area_kinds) = kinds(sel);
    let found = matching(w, &all);
    compute_from(w, sel, &all, &area_kinds, &found)
}

fn compute_from(w: &World, sel: &HighlightSel, all: &[Placeable], area_kinds: &[Placeable], found: &[(BuildingId, Placeable)]) -> HighlightView {
    let mut view = HighlightView::default();
    let mut acc: HashMap<(LotId, Family), (u16, f32)> = HashMap::new();
    let add = |acc: &mut HashMap<(LotId, Family), (u16, f32)>, fam: Family, reach: Vec<(LotId, f32)>| {
        for (lot, cov) in reach {
            let e = acc.entry((lot, fam)).or_insert((0, 0.0));
            e.0 += 1;
            e.1 = e.1.max(cov);
        }
    };
    let mut bus_depots = false;
    for &(id, p) in found {
        let b = &w.buildings[id];
        let Some(lot) = w.lots.get(b.lot) else { continue };
        let Some(family) = Family::of(p) else { continue };
        let level = match sel.focus {
            Some(f) if f == norm(p) => Level::Focus,
            Some(_) => Level::Faint,
            None => Level::Normal,
        };
        let label = match p {
            Placeable::Plant(_) => Some(format!("{:.0} / {:.0} MW", w.plant_output_mw(b), w.plant_capacity_mw(b))),
            Placeable::Battery(_) => b.battery.as_ref().map(|bat| format!("{:.0} MWh", bat.charge_mwh)),
            Placeable::ParkingLot(_) => Some(format!("{} / {} spaces", b.parking_used, b.parking_capacity)),
            _ => None,
        };
        view.marked.push(Marked { id, lot: b.lot, at: lot.center, roof: b.floors as f32 * 3.6 + 4.0, family, level, name: b.name.clone(), label });
        if !area_kinds.contains(&norm(p)) {
            continue;
        }
        match reach_of(p) {
            Some(Reach::Road) => add(&mut acc, family, w.service_reach(id)),
            Some(Reach::Radius { .. }) if p == Placeable::BusDepot => bus_depots = true,
            Some(Reach::Radius { r, tint }) => {
                if !b.is_derelict() && b.construction.is_none() {
                    view.rings.push((lot.center, r, family));
                    if tint {
                        add(&mut acc, family, lots_within(w, lot.center, r));
                    }
                }
            }
            None => {}
        }
    }
    // Buses: the catchment is every stop of every line (a depot alone
    // carries nobody).
    if bus_depots || area_kinds.contains(&Placeable::BusDepot) {
        for r in w.transit.routes.values() {
            for s in &r.stops {
                view.rings.push((s.pos, nd_sim::transit::STOP_REACH, Family::Buses));
                add(&mut acc, Family::Buses, lots_within(w, s.pos, nd_sim::transit::STOP_REACH));
            }
        }
    }
    view.tints = acc.into_iter().map(|((lot, family), (count, cov))| Tint { lot, family, count, cov }).collect();
    view.tints.sort_by_key(|t| (t.lot, t.family as u8));

    // Families with areas, and the one whose gaps are shown.
    let mut area_fams: Vec<Family> = area_kinds.iter().filter_map(|p| Family::of(*p)).collect();
    area_fams.dedup();
    let gaps = match area_fams.as_slice() {
        [f] if f.serves_homes() => Some(*f),
        _ => None,
    };
    let mut spots: Vec<Spot> = Vec::new();
    if let Some(fam) = gaps {
        let cov: HashMap<LotId, f32> = view.tints.iter().filter(|t| t.family == fam).map(|t| (t.lot, t.cov)).collect();
        let (mut homes, mut covered) = (0u64, 0u64);
        // Uncovered homes, grouped by district (or a 400 m square).
        let mut groups: HashMap<(u16, i32, i32), (u32, SVec2, String, u16)> = HashMap::new();
        for b in w.buildings.values().filter(|b| b.homes > 0 && b.construction.is_none()) {
            homes += b.homes as u64;
            if cov.get(&b.lot).copied().unwrap_or(0.0) >= COVERED {
                covered += b.homes as u64;
                continue;
            }
            view.uncovered.push(b.lot);
            let Some(lot) = w.lots.get(b.lot) else { continue };
            let key = match w.districts.at(lot.center) {
                Some(d) => (d.id, 0, 0),
                None => (u16::MAX, (lot.center.x / 400.0).floor() as i32, (lot.center.y / 400.0).floor() as i32),
            };
            let e = groups.entry(key).or_insert_with(|| (0, SVec2::ZERO, String::new(), 0));
            e.0 += b.homes as u32;
            e.1 += lot.center * b.homes as f32;
            if b.homes > e.3 {
                e.3 = b.homes;
                e.2 = b.name.clone();
            }
        }
        view.uncovered.sort();
        view.uncovered.dedup();
        view.uncovered_zoned = w
            .lots
            .iter()
            .filter(|(id, l)| l.land_use == nd_sim::LandUse::Residential && l.usable() && l.building.is_none_or(|b| w.buildings.get(b).is_none_or(|b| b.homes == 0)) && cov.get(id).copied().unwrap_or(0.0) < COVERED)
            .map(|(id, _)| id)
            .collect();
        let any_district = groups.keys().any(|k| k.0 != u16::MAX);
        for ((d, _, _), (n, sum, biggest, _)) in groups {
            if n < 5 {
                continue;
            }
            let name = match w.districts.get(d) {
                Some(dist) if d != u16::MAX => dist.name.clone(),
                _ => format!("near {biggest}"),
            };
            spots.push(Spot { name, at: sum / n as f32, homes: n });
        }
        spots.sort_by(|a, b| b.homes.cmp(&a.homes).then(a.name.cmp(&b.name)));
        let share = if homes == 0 { 1.0 } else { covered as f32 / homes as f32 };
        view.homes_covered = Some(share);
        let n = found.iter().filter(|(_, p)| Family::of(*p) == Some(fam)).count();
        let mut parts = vec![fam.count(n), format!("{:.0}% of homes {}", share * 100.0, fam.area_word())];
        if !spots.is_empty() {
            let word = if any_district && spots.iter().all(|s| !s.name.starts_with("near ")) { "districts" } else { "areas" };
            parts.push(format!("{} {} uncovered", spots.len(), if spots.len() == 1 { &word[..word.len() - 1] } else { word }));
        } else if homes > 0 {
            parts.push("no gaps".into());
        }
        view.summary.headline = parts.join(" · ");
        view.summary.legend.push((fam.color32().gamma_multiply(0.7), format!("{} area", fam.noun().0)));
        view.summary.legend.push((fam.color32(), "overlap".into()));
        view.summary.legend.push((egui::Color32::from_rgb(230, 60, 50), "homes left out".into()));
        view.summary.legend.push((egui::Color32::from_rgb(120, 40, 36), "home zones left out".into()));
    } else {
        // A count of each kind shown, in menu order.
        let mut fams: Vec<Family> = all.iter().filter_map(|p| Family::of(*p)).collect();
        fams.dedup();
        let fams: Vec<Family> = fams.into_iter().fold(Vec::new(), |mut v, f| {
            if !v.contains(&f) {
                v.push(f);
            }
            v
        });
        let parts: Vec<String> = fams.iter().map(|f| (f, found.iter().filter(|(_, p)| Family::of(*p) == Some(*f)).count())).filter(|(_, n)| *n > 0).map(|(f, n)| f.count(n)).collect();
        view.summary.headline = if parts.is_empty() { "None on the map yet".into() } else { parts.join(" · ") };
        // The key: the kinds on the map (all of them while none is).
        let present: Vec<Family> = fams.iter().copied().filter(|f| found.iter().any(|(_, p)| Family::of(*p) == Some(*f))).collect();
        for f in if present.is_empty() { &fams } else { &present } {
            view.summary.legend.push((f.color32(), f.noun().1.to_string()));
        }
        if area_fams.len() > 1 {
            view.summary.detail.push("Service areas tinted in each kind's colour: where two overlap the colours mix. Pick one to see its gaps.".into());
        }
    }
    view.summary.spots = spots;
    // Figures for the kinds without areas.
    if all.iter().any(|p| matches!(p, Placeable::Plant(_))) {
        let p = &w.economy.power;
        let cap: f32 = found.iter().filter(|(_, k)| matches!(k, Placeable::Plant(_))).map(|(id, _)| w.plant_capacity_mw(&w.buildings[*id])).sum();
        view.summary.detail.push(format!("{:.0} MW built · making {:.0} MW for {:.0} MW demand ({:.0}% met)", cap, p.supply_mw, p.demand_mw, p.served * 100.0));
        view.summary.detail.push("The grid is one network: every plant feeds the whole city.".into());
    }
    if all.iter().any(|p| matches!(p, Placeable::ParkingLot(_))) {
        let (used, cap) = found.iter().filter(|(_, k)| matches!(k, Placeable::ParkingLot(_))).fold((0u32, 0u32), |(u, c), (id, _)| (u + w.buildings[*id].parking_used as u32, c + w.buildings[*id].parking_capacity as u32));
        if cap > 0 {
            view.summary.detail.push(format!("{used} of {cap} city parking spaces taken"));
        }
    }
    if gaps == Some(Family::Buses) {
        let stops: usize = w.transit.routes.values().map(|r| r.stops.len()).sum();
        view.summary.detail.push(format!("{stops} stops on {} lines: the rings are a {:.0} m walk", w.transit.routes.len(), nd_sim::transit::STOP_REACH));
    }
    view
}

// ── keeping it current ──────────────────────────────────────────────────────

/// Seconds between checks for a change.
const CHECK_EVERY: f64 = 0.5;

fn refresh(sim: Res<Sim>, mut hl: ResMut<BuildHighlight>, time: Res<Time>, selection: Res<crate::selection::Selection>, tools: Res<crate::tools::ToolState>, overlays: Res<crate::render::Overlays>) {
    let crime = overlays.crime && !overlays.map;
    if hl.police && (hl.sel.is_some() || !crime || matches!(*selection, crate::selection::Selection::Building(_))) {
        hl.police = false;
        hl.key = None;
        hl.view = None;
        hl.version += 1;
    }
    let Some(sel) = hl.sel else {
        // [feat/pt-ux] No Build window section: a selected building shows
        // what its kind shows in build mode (not underground).
        let picked = match *selection {
            crate::selection::Selection::Building(id) if tools.depth != crate::tools::Depth::Underground => Some(id),
            _ => None,
        };
        if let Some(id) = picked {
            let now = time.elapsed_secs_f64();
            if hl.inspected == Some(id) && now - hl.checked < CHECK_EVERY && !sim.full_rebuild {
                return;
            }
            hl.checked = now;
            hl.checked_sel = None;
            let key = inspected_key(&sim.world, id);
            if hl.inspected == Some(id) && hl.key == Some(key) && !sim.full_rebuild {
                return;
            }
            hl.inspected = Some(id);
            hl.key = Some(key);
            hl.view = inspected_view(&sim.world, id).map(Arc::new);
            hl.version += 1;
            return;
        }
        // [fix/buildings-ui] The Crime layer: the police stations and the
        // ground they cover.
        if crime {
            let now = time.elapsed_secs_f64();
            if hl.police && now - hl.checked < CHECK_EVERY && !sim.full_rebuild {
                return;
            }
            hl.checked = now;
            hl.checked_sel = None;
            hl.inspected = None;
            let w = &sim.world;
            let police = norm(Placeable::Service(ServiceKind::Police, nd_sim::Tier::Mid));
            let sel = HighlightSel { cat: Cat::Services, sub: None, focus: Some(police) };
            let key = signature(w, &sel, &matching(w, &[police]));
            if hl.police && hl.key == Some(key) && !sim.full_rebuild {
                return;
            }
            hl.police = true;
            hl.key = Some(key);
            hl.view = Some(Arc::new(police_view(w)));
            hl.version += 1;
            return;
        }
        if hl.view.is_some() || hl.inspected.is_some() {
            hl.view = None;
            hl.key = None;
            hl.checked_sel = None;
            hl.inspected = None;
            hl.version += 1;
        }
        return;
    };
    if hl.inspected.take().is_some() {
        hl.key = None;
        hl.checked_sel = None;
    }
    let now = time.elapsed_secs_f64();
    if hl.checked_sel == Some(sel) && now - hl.checked < CHECK_EVERY && !sim.full_rebuild {
        return;
    }
    hl.checked = now;
    hl.checked_sel = Some(sel);
    let w = &sim.world;
    let (all, areas) = kinds(&sel);
    let found = matching(w, &all);
    let key = signature(w, &sel, &found);
    if hl.key == Some(key) && !sim.full_rebuild {
        return;
    }
    hl.key = Some(key);
    hl.view = Some(Arc::new(compute_from(w, &sel, &all, &areas, &found)));
    hl.version += 1;
}

// ── drawing ─────────────────────────────────────────────────────────────────

#[derive(Component, Default)]
struct HighlightTag;

/// Wall height for a camera this far out, in steps (a zoom rebuilds the
/// mesh only now and then).
fn wall_height(distance: f32) -> f32 {
    ((distance * 0.025).clamp(6.0, 30.0) / 3.0).round() * 3.0
}

/// The service areas: lots tinted by how many buildings reach them (the
/// material adds, so overlaps of different kinds mix too), homes left out
/// in red.
fn area_mesh(w: &World, v: &HighlightView) -> MeshData {
    let mut m = MeshData::default();
    if v.outline_only {
        return m;
    }
    for t in &v.tints {
        let Some(lot) = w.lots.get(t.lot) else { continue };
        let c = t.family.linear();
        let a = (0.5 + 0.3 * (t.count.min(4) as f32 - 1.0)) * (0.35 + 0.65 * t.cov);
        fill(&mut m, w, &lot.polygon, 0.25, [c[0], c[1], c[2], a]);
    }
    for id in &v.uncovered_zoned {
        let Some(lot) = w.lots.get(*id) else { continue };
        fill(&mut m, w, &lot.polygon, 0.27, [1.0, 0.08, 0.05, 0.16]);
    }
    for id in &v.uncovered {
        let Some(lot) = w.lots.get(*id) else { continue };
        fill(&mut m, w, &lot.polygon, 0.27, [1.0, 0.06, 0.04, 0.38]);
        outline_band(&mut m, w, &lot.polygon, 1.0, 0.3, [1.6, 0.1, 0.06, 0.8]);
    }
    m
}

/// Outlines and light walls on the highlighted buildings' lots.
fn building_mesh(w: &World, v: &HighlightView, height: f32) -> MeshData {
    let mut m = MeshData::default();
    for b in &v.marked {
        let Some(lot) = w.lots.get(b.lot) else { continue };
        let poly = &lot.polygon;
        if poly.len() < 3 {
            continue;
        }
        let c = b.family.linear();
        let hot = |k: f32| [c[0] * k + 0.4, c[1] * k + 0.4, c[2] * k + 0.4, 1.0];
        match b.level {
            Level::Focus => {
                fill(&mut m, w, poly, 0.32, [c[0], c[1], c[2], 0.35]);
                light_wall(&mut m, w, poly, height, [c[0] * 3.0, c[1] * 3.0, c[2] * 3.0], 1.0);
                outline_band(&mut m, w, poly, (height * 0.12).clamp(1.2, 3.0), 0.4, hot(4.0));
            }
            Level::Normal => {
                fill(&mut m, w, poly, 0.32, [c[0], c[1], c[2], 0.25]);
                light_wall(&mut m, w, poly, height * 0.6, [c[0] * 2.5, c[1] * 2.5, c[2] * 2.5], 0.8);
                outline_band(&mut m, w, poly, (height * 0.1).clamp(1.0, 2.5), 0.4, hot(3.0));
            }
            Level::Faint => {
                outline_band(&mut m, w, poly, (height * 0.06).clamp(0.8, 1.5), 0.4, [c[0] * 1.2, c[1] * 1.2, c[2] * 1.2, 0.7]);
            }
        }
    }
    m
}

/// [overlay] The highlight's meshes: the service areas, and outlines and
/// light walls on the marked buildings' lots (the walls' height steps
/// with the camera's distance).
struct HighlightMeshes;

impl MeshOverlay for HighlightMeshes {
    type Key = (u64, u32);
    type Param = (Res<'static, BuildHighlight>, Query<'static, 'static, &'static crate::camera::OrbitCamera>, Res<'static, Time>);
    type Marker = HighlightTag;

    fn key((hl, cams, time): &mut SystemParamItem<Self::Param>, cx: &mut OverlayCx) -> Wanted<Self::Key> {
        cx.mat(OverlayMat::HighlightArea);
        let wall_mat = cx.mat(OverlayMat::HighlightWall);
        let h = wall_height(cams.iter().next().map_or(400.0, |c| c.distance));
        if hl.view.is_some() {
            if let Some(mut m) = cx.materials.get_mut(&wall_mat) {
                m.base_color = Color::linear_rgba(1.0, 1.0, 1.0, 0.8 + 0.2 * (time.elapsed_secs() * 3.0).sin());
            }
        }
        match hl.view {
            Some(_) => Wanted::Show((hl.version, h.to_bits())),
            None => Wanted::Nothing,
        }
    }

    fn build((hl, _, _): &mut SystemParamItem<Self::Param>, cx: &mut OverlayCx, &(_, h): &Self::Key, out: &mut Pieces) {
        let Some(v) = &hl.view else { return };
        let w = &cx.sim.world;
        let (area, walls) = (area_mesh(w, v), building_mesh(w, v, f32::from_bits(h)));
        out.add(area, cx.mat(OverlayMat::HighlightArea));
        out.add(walls, cx.mat(OverlayMat::HighlightWall));
    }
}

/// The straight-line reaches as rings on the ground.
fn rings(mut gizmos: Gizmos, hl: Res<BuildHighlight>, sim: Res<Sim>) {
    let Some(v) = &hl.view else { return };
    let w = &sim.world;
    // [fix/buildings-ui] An area's outline.
    for (a, b, fam) in &v.outline {
        let [r, g, bb] = fam.linear();
        let col = Color::linear_rgb(r * 2.5, g * 2.5, bb * 2.5);
        gizmos.line(Vec3::new(a.x, w.surface_height(*a) + 1.0, a.y), Vec3::new(b.x, w.surface_height(*b) + 1.0, b.y), col);
    }
    for (c, r, fam) in &v.rings {
        let [a, b, cc] = fam.linear();
        let col = Color::linear_rgb(a * 2.5, b * 2.5, cc * 2.5);
        let at = Vec3::new(c.x, w.surface_height(*c) + 1.0, c.y);
        gizmos.circle(flat_at(at), *r, col);
        if *r > 60.0 {
            gizmos.circle(flat_at(at), (*r - (*r * 0.2).min(COVERAGE_FADE)).max(1.0), col.with_alpha(0.4));
        }
    }
}

/// A marker over each highlighted building: a disc in its kind's colour
/// with a glyph, the same size on screen at any zoom, so the whole city's
/// hospitals read at a glance; the figure (MW, spaces) under it, and the
/// name when the camera is close.
fn markers(mut contexts: bevy_egui::EguiContexts, hl: Res<BuildHighlight>, sim: Res<Sim>, mode: Res<crate::camera::CameraMode>, flags: Res<crate::ui::UiFlags>, cams: Query<(&Camera, &GlobalTransform, &crate::camera::OrbitCamera)>) -> Result {
    let Some(v) = &hl.view else { return Ok(()) };
    if mode.street() {
        return Ok(());
    }
    let Some((camera, gt, orbit)) = cams.iter().next() else { return Ok(()) };
    let ctx = contexts.ctx_mut()?;
    // [city-data] The map is covered: nothing to label.
    if crate::ui::city_data::is_open(ctx) {
        return Ok(());
    }
    let clip = flags.map_rect(ctx.content_rect());
    let painter = ctx.layer_painter(egui::LayerId::new(egui::Order::Background, egui::Id::new("build_highlight_markers"))).with_clip_rect(clip);
    let w = &sim.world;
    let close = orbit.distance < 450.0;
    // Faint ones first, so the picked kind is drawn on top.
    let mut order: Vec<&Marked> = v.marked.iter().collect();
    order.sort_by_key(|m| match m.level {
        Level::Faint => 0,
        Level::Normal => 1,
        Level::Focus => 2,
    });
    for m in order {
        let top = Vec3::new(m.at.x, w.surface_height(m.at) + m.roof + 6.0, m.at.y);
        let Ok(p) = camera.world_to_viewport(gt, top) else { continue };
        let p = crate::ui::theme::to_points(ctx, p);
        if !clip.expand(20.0).contains(p) {
            continue;
        }
        let (r, alpha) = match m.level {
            Level::Focus => (12.0, 1.0),
            Level::Normal => (10.0, 1.0),
            Level::Faint => (8.0, 0.8),
        };
        let col = m.family.color32();
        let centre = p - egui::vec2(0.0, r + 6.0);
        // A short stem down to the roof.
        painter.line_segment([p, centre + egui::vec2(0.0, r)], egui::Stroke::new(1.5, col.gamma_multiply(0.8 * alpha)));
        painter.circle_filled(centre, r + 1.5, egui::Color32::from_rgba_unmultiplied(12, 14, 24, (230.0 * alpha) as u8));
        painter.circle_stroke(centre, r, egui::Stroke::new(if m.level == Level::Focus { 2.5 } else { 1.8 }, col.gamma_multiply(alpha)));
        glyph(&painter, egui::Rect::from_center_size(centre, egui::vec2(r * 1.25, r * 1.25)), m.family, col.gamma_multiply(alpha));
        let mut below = centre.y + r + 4.0;
        if let Some(label) = &m.label {
            if m.level != Level::Faint {
                let g = painter.layout_no_wrap(label.clone(), crate::ui::theme::small(), egui::Color32::WHITE);
                let rect = egui::Rect::from_center_size(egui::pos2(centre.x, below + g.size().y * 0.5), g.size()).expand2(egui::vec2(4.0, 1.0));
                painter.rect_filled(rect, 3.0, egui::Color32::from_rgba_unmultiplied(12, 14, 24, 200));
                painter.galley(rect.min + egui::vec2(4.0, 1.0), g, egui::Color32::WHITE);
                below = rect.bottom() + 2.0;
            }
        }
        if close && m.level == Level::Focus {
            let g = painter.layout_no_wrap(m.name.clone(), crate::ui::theme::small(), col);
            let rect = egui::Rect::from_center_size(egui::pos2(centre.x, below + g.size().y * 0.5), g.size()).expand2(egui::vec2(4.0, 1.0));
            painter.rect_filled(rect, 3.0, egui::Color32::from_rgba_unmultiplied(12, 14, 24, 200));
            painter.galley(rect.min + egui::vec2(4.0, 1.0), g, col);
        }
    }
    Ok(())
}

/// A small glyph per kind, in a unit square scaled into `r`.
pub fn glyph(painter: &egui::Painter, r: egui::Rect, fam: Family, col: egui::Color32) {
    let s = r.width();
    let p = |x: f32, y: f32| egui::pos2(r.left() + x * s, r.top() + y * s);
    let stroke = egui::Stroke::new((s * 0.12).max(1.5), col);
    let letter = |t: &str| {
        painter.text(r.center(), egui::Align2::CENTER_CENTER, t, egui::FontId::proportional(s * 0.85), col);
    };
    match fam {
        Family::Health => {
            painter.rect_filled(egui::Rect::from_min_max(p(0.38, 0.1), p(0.62, 0.9)), 1.0, col);
            painter.rect_filled(egui::Rect::from_min_max(p(0.1, 0.38), p(0.9, 0.62)), 1.0, col);
        }
        Family::Police => {
            // A shield.
            let pts = vec![p(0.5, 0.05), p(0.9, 0.2), p(0.82, 0.62), p(0.5, 0.95), p(0.18, 0.62), p(0.1, 0.2)];
            painter.add(egui::Shape::convex_polygon(pts, egui::Color32::TRANSPARENT, stroke));
            painter.circle_filled(p(0.5, 0.45), s * 0.13, col);
        }
        Family::Fire => {
            // A flame.
            let pts = vec![p(0.5, 0.02), p(0.78, 0.45), p(0.82, 0.7), p(0.66, 0.92), p(0.34, 0.92), p(0.18, 0.7), p(0.24, 0.42), p(0.36, 0.55)];
            painter.add(egui::Shape::closed_line(pts, stroke));
        }
        Family::School => {
            // A mortarboard.
            painter.add(egui::Shape::convex_polygon(vec![p(0.5, 0.15), p(0.95, 0.38), p(0.5, 0.6), p(0.05, 0.38)], col, egui::Stroke::NONE));
            painter.add(egui::Shape::line(vec![p(0.25, 0.5), p(0.25, 0.78), p(0.5, 0.88), p(0.75, 0.78), p(0.75, 0.5)], stroke));
        }
        Family::Power => {
            // A bolt.
            painter.add(egui::Shape::closed_line(vec![p(0.6, 0.02), p(0.22, 0.55), p(0.48, 0.55), p(0.38, 0.98), p(0.8, 0.42), p(0.53, 0.42)], stroke));
        }
        Family::Battery => {
            painter.rect_stroke(egui::Rect::from_min_max(p(0.12, 0.28), p(0.82, 0.78)), 1.5, stroke, egui::StrokeKind::Middle);
            painter.rect_filled(egui::Rect::from_min_max(p(0.84, 0.43), p(0.94, 0.63)), 0.0, col);
            painter.rect_filled(egui::Rect::from_min_max(p(0.22, 0.38), p(0.52, 0.68)), 0.0, col);
        }
        Family::Jail => {
            for x in [0.25, 0.5, 0.75] {
                painter.line_segment([p(x, 0.12), p(x, 0.88)], stroke);
            }
            painter.line_segment([p(0.12, 0.12), p(0.88, 0.12)], stroke);
        }
        Family::Trash => {
            painter.add(egui::Shape::closed_line(vec![p(0.22, 0.3), p(0.78, 0.3), p(0.7, 0.92), p(0.3, 0.92)], stroke));
            painter.line_segment([p(0.12, 0.2), p(0.88, 0.2)], stroke);
        }
        Family::Attraction => {
            let pts: Vec<egui::Pos2> = (0..10).map(|i| {
                let a = std::f32::consts::TAU * i as f32 / 10.0 - std::f32::consts::FRAC_PI_2;
                let rr = if i % 2 == 0 { 0.48 } else { 0.2 };
                p(0.5 + rr * a.cos(), 0.52 + rr * a.sin())
            }).collect();
            painter.add(egui::Shape::closed_line(pts, stroke));
        }
        Family::Water => {
            for y in [0.35, 0.65] {
                painter.add(egui::Shape::line((0..=8).map(|i| p(0.08 + i as f32 * 0.105, y + 0.08 * ((i as f32) * 1.6).sin())).collect(), stroke));
            }
        }
        Family::Rail => {
            painter.line_segment([p(0.3, 0.05), p(0.3, 0.95)], stroke);
            painter.line_segment([p(0.7, 0.05), p(0.7, 0.95)], stroke);
            for y in [0.25, 0.5, 0.75] {
                painter.line_segment([p(0.18, y), p(0.82, y)], egui::Stroke::new(stroke.width * 0.7, col));
            }
        }
        Family::Freight => {
            painter.rect_stroke(egui::Rect::from_min_max(p(0.15, 0.25), p(0.85, 0.8)), 1.0, stroke, egui::StrokeKind::Middle);
            painter.line_segment([p(0.5, 0.25), p(0.5, 0.8)], stroke);
        }
        Family::Dispatch => letter("911"),
        Family::Wardens => letter("W"),
        Family::Buses => letter("B"),
        Family::Metro => letter("M"),
        Family::Air => letter("V"),
        Family::Parking => letter("P"),
    }
}

pub struct BuildHighlightPlugin;

impl Plugin for BuildHighlightPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<BuildHighlight>()
            .add_plugins(MeshOverlayPlugin::<HighlightMeshes>::default())
            .add_systems(Update, refresh.in_set(OverlaySet::Build).before(sync::<HighlightMeshes>))
            .add_systems(Update, rings.in_set(OverlaySet::Draw))
            .add_systems(bevy_egui::EguiPrimaryContextPass, markers);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use nd_sim::{LandUse, Tier};

    /// A town of homes along streets, with civic lots for stations.
    fn town() -> World {
        let mut w = World::new(nd_sim::WorldConfig { dead_ends_lead_elsewhere: true, emergencies: false, society: false, landscape: nd_sim::LandscapeParams::flat(), ..nd_sim::WorldConfig::default() });
        w.economy.treasury = 1_000_000.0;
        let v = SVec2::new;
        w.add_road(&[v(-1500.0, 0.0), v(1500.0, 0.0)], 1, 1);
        w.add_road(&[v(-1500.0, 120.0), v(1500.0, 120.0)], 1, 1);
        for x in [-1400.0, -700.0, 0.0, 700.0, 1400.0] {
            w.add_road(&[v(x, 0.0), v(x, 120.0)], 1, 1);
        }
        let rect = |x0: f32, y0: f32, wd: f32, h: f32| vec![v(x0, y0), v(x0 + wd, y0), v(x0 + wd, y0 + h), v(x0, y0 + h)];
        w.add_zone(rect(-1490.0, 8.0, 2980.0, 50.0), LandUse::Civic);
        w.add_zone(rect(-1490.0, -60.0, 2980.0, 52.0), LandUse::Residential);
        let homes: Vec<LotId> = w.lots.iter().filter(|(_, l)| l.land_use == LandUse::Residential && l.usable() && l.building.is_none()).map(|(id, _)| id).collect();
        for lot in homes {
            w.build_on(lot);
        }
        // Finished, with people's homes in them.
        for b in w.buildings.values_mut() {
            b.construction = None;
            b.homes = b.homes.max(12);
        }
        w
    }

    /// The free civic lot nearest `x`.
    fn civic(w: &World, x: f32) -> LotId {
        w.lots
            .iter()
            .filter(|(_, l)| l.land_use == LandUse::Civic && l.usable() && l.building.is_none() && l.car_access() && l.area() >= 420.0 && l.car_point.y.abs() < 12.0)
            .min_by(|a, b| (a.1.center.x - x).abs().total_cmp(&(b.1.center.x - x).abs()))
            .map(|(id, _)| id)
            .expect("a civic lot")
    }

    #[test]
    fn every_building_kind_has_a_family_and_tolls_none() {
        for bc in nd_sim::BuildCategory::ALL {
            for p in bc.items() {
                assert_eq!(Family::of(p).is_none(), matches!(p, Placeable::Toll(_)), "{p:?}");
            }
        }
        // The covering services reach along roads.
        for k in [ServiceKind::Police, ServiceKind::Fire, ServiceKind::Health, ServiceKind::School] {
            assert_eq!(reach_of(Placeable::Service(k, Tier::Mid)), Some(Reach::Road));
        }
        assert!(reach_of(Placeable::Plant(nd_sim::PowerPlantKind::Coal)).is_none(), "power shows plants, not areas");
    }

    #[test]
    fn hospitals_show_their_area_and_the_homes_they_miss() {
        let mut w = town();
        let homes = w.buildings.values().filter(|b| b.homes > 0).count();
        assert!(homes > 10, "the town has homes");
        let sel = |focus| HighlightSel { cat: Cat::Services, sub: Some("Health and education"), focus };
        // No hospital yet: every home is a gap.
        let v = compute(&w, &sel(Some(Placeable::Service(ServiceKind::Health, Tier::Mid))));
        assert!(v.marked.is_empty());
        assert_eq!(v.homes_covered, Some(0.0));
        assert!(!v.uncovered.is_empty());
        assert!(v.summary.headline.starts_with("0 hospitals"), "{}", v.summary.headline);
        assert!(!v.summary.spots.is_empty(), "the gaps are listed");
        // One hospital and one school.
        let lot = civic(&w, -1200.0);
        let hosp = w.build_service(lot, ServiceKind::Health, Tier::High).expect("hospital");
        let lot2 = civic(&w, 1200.0);
        let school = w.build_service(lot2, ServiceKind::School, Tier::Mid).expect("school");
        let v = compute(&w, &sel(Some(Placeable::Service(ServiceKind::Health, Tier::Mid))));
        let m = |id| v.marked.iter().find(|m| m.id == id).map(|m| m.level);
        assert_eq!(m(hosp), Some(Level::Focus));
        assert_eq!(m(school), Some(Level::Faint), "the rest of the sub-section, faintly");
        // Only the hospital's area: its own reach, nothing for the school.
        let reach = w.service_reach(hosp);
        assert!(!reach.is_empty());
        assert!(v.tints.iter().all(|t| t.family == Family::Health && t.count == 1));
        assert_eq!(v.tints.len(), reach.len());
        let share = v.homes_covered.unwrap();
        assert!(share > 0.0, "the hospital covers some homes");
        // Gaps are homes outside the area.
        for lot in &v.uncovered {
            assert!(reach.iter().find(|(l, _)| l == lot).is_none_or(|(_, c)| *c < COVERED));
        }
        // A second hospital next to it: overlap counts two.
        let lot3 = civic(&w, -1100.0);
        w.build_service(lot3, ServiceKind::Health, Tier::Low).expect("clinic");
        let v2 = compute(&w, &sel(Some(Placeable::Service(ServiceKind::Health, Tier::Mid))));
        assert!(v2.tints.iter().any(|t| t.count == 2), "overlap shows");
        assert!(v2.homes_covered.unwrap() >= share);
        assert!(v2.summary.headline.starts_with("2 hospitals"), "{}", v2.summary.headline);
        // The whole sub-section, no item: both kinds, normal strength, no gaps
        // (two kinds of area at once).
        let v3 = compute(&w, &sel(None));
        assert!(v3.marked.iter().all(|m| m.level == Level::Normal));
        assert_eq!(v3.marked.len(), 3);
        assert!(v3.uncovered.is_empty());
        assert!(v3.tints.iter().any(|t| t.family == Family::School));
    }

    #[test]
    fn the_view_is_kept_until_something_it_shows_changes() {
        let mut w = town();
        let sel = HighlightSel { cat: Cat::Services, sub: Some("Safety"), focus: Some(Placeable::Service(ServiceKind::Police, Tier::Mid)) };
        let (all, _) = kinds(&sel);
        let k0 = signature(&w, &sel, &matching(&w, &all));
        assert_eq!(k0, signature(&w, &sel, &matching(&w, &all)));
        let lot = civic(&w, 0.0);
        w.build_service(lot, ServiceKind::Police, Tier::Mid).unwrap();
        assert_ne!(k0, signature(&w, &sel, &matching(&w, &all)), "a new station changes it");
    }
}
