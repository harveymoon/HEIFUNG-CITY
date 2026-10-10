//! [feat/pt-ux] Campers and vans at the kerb (`world.society.homeless.rvs`)
//! as things to select: the click, the outline, who lives in it or that it
//! was left behind, and its tow.

use bevy::prelude::*;
use nd_sim::homeless::Rv;
use nd_sim::{CitizenId, Vec2 as SVec2, World};

use crate::overlay::shapes::{vehicle_box, GOLD};

/// Drawn length and width (`render::society::rv`): a camper is longer.
pub fn size(r: &Rv) -> (f32, f32) {
    if r.camper { (7.0, 2.4) } else { (5.2, 2.0) }
}

/// The camper or van under `p`, by its seed.
pub fn pick(w: &World, p: SVec2) -> Option<u64> {
    w.society
        .homeless
        .rvs
        .iter()
        .filter_map(|r| {
            let (len, wid) = size(r);
            let fwd = SVec2::new(r.heading.cos(), r.heading.sin());
            let rel = p - r.pos;
            let (a, b) = (rel.dot(fwd).abs(), rel.perp_dot(fwd).abs());
            (a < len * 0.5 + 1.0 && b < wid * 0.5 + 1.0).then_some((rel.length(), r.seed))
        })
        .min_by(|a, b| a.0.total_cmp(&b.0))
        .map(|(_, s)| s)
}

/// "Camper" / "Old van", and whose.
pub fn title(w: &World, seed: u64) -> String {
    let Some(r) = w.society.homeless.rv(seed) else { return "a camper".into() };
    let kind = if r.camper { "Camper" } else { "Old van" };
    match owner_name(w, r) {
        Some(n) if r.lived_in() => format!("{kind}  ·  home of {n}"),
        _ => format!("Abandoned {}", kind.to_lowercase()),
    }
}

/// The household's people (none for an abandoned one).
pub fn residents(w: &World, r: &Rv) -> Vec<CitizenId> {
    if !r.lived_in() {
        return Vec::new();
    }
    w.households.get(r.household).map(|h| h.members.iter().copied().filter(|c| w.citizens.contains_key(*c)).collect()).unwrap_or_default()
}

/// The name of the first adult of its household (the one the van is in
/// the name of), alive or not.
pub fn owner_name(w: &World, r: &Rv) -> Option<String> {
    w.households.get(r.household).and_then(|h| h.members.iter().find_map(|m| w.citizens.get(*m))).map(|c| c.name.clone())
}

pub fn draw(gizmos: &mut Gizmos, w: &World, seed: u64) {
    let Some(r) = w.society.homeless.rv(seed) else { return };
    let (len, wid) = size(r);
    vehicle_box(gizmos, r.pos, r.heading, len + 0.4, wid + 0.4, w.surface_height(r.pos) + 0.35, GOLD);
}
