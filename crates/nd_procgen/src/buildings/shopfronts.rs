//! [tenants] Shopfronts on a building let in parts (`nd_sim::buildings::Unit`): one
//! sign over each shop along the street face of the ground floor, lit in
//! a neon of its own when the shop is let, a pale unlit board when it is
//! to let. Placed off the geometry already drawn (the ground floor's
//! extent), like the brands, so it works on any style without redrawing it.

use super::parts::*;
use super::{Ctx, Face};
use crate::surface::{Geometry, Surface::*};
use glam::{Vec2, Vec3};

const BOARD: [f32; 3] = [0.03, 0.03, 0.035];
const TO_LET: [f32; 3] = [0.62, 0.60, 0.55];
/// Height of the signs' middle, m: over a shop door.
const SIGN_Y: f32 = 3.3;

/// Signs over the shopfronts in `units` (the others are upper floors).
pub fn draw(g: &mut Geometry, c: &Ctx, units: &[nd_sim::buildings::Unit], seed: u64) {
    let shops: Vec<&nd_sim::buildings::Unit> = units.iter().filter(|u| u.shopfront).collect();
    if shops.is_empty() {
        return;
    }
    // The ground floor's mass, lot-local; the street is on its low-y side.
    let Some((lo, hi)) = super::brand::extent(g, c, 1.8, SIGN_Y + 0.6) else { return };
    let front = Face::from_edge(Vec2::new(lo.x, lo.y), Vec2::new(hi.x, lo.y));
    if front.len < 4.0 {
        return;
    }
    let f = &c.frame;
    let n = shops.len() as f32;
    let seg = front.len / n;
    let w = (seg * 0.78).min(8.0);
    let signs = &c.palette.sign_colors;
    for (i, u) in shops.iter().enumerate() {
        let t = -front.len * 0.5 + seg * (i as f32 + 0.5);
        // On the facade; narrower by a corner that leaves the lot (a
        // slanted side line), none if even that does not fit.
        let fits = |w: f32| c.all_inside(&[front.at(t - w * 0.5, 0.0), front.at(t + w * 0.5, 0.0)]);
        let w = if fits(w) { w } else if fits(w * 0.5) { w * 0.5 } else { continue };
        if u.vacant() {
            front.bx(g, f, Matte, t, SIGN_Y, -0.02, Vec3::new(w * 0.6, 0.6, 0.12), TO_LET);
            continue;
        }
        let k = (nd_sim::hash::mix2(seed, i as u64 + 1) % signs.len().max(1) as u64) as usize;
        let col = signs.get(k).copied().unwrap_or([1.0, 0.3, 0.8]);
        front.bx(g, f, Matte, t, SIGN_Y, 0.0, Vec3::new(w, 0.8, 0.16), BOARD);
        front.decal(g, f, Glow, t, SIGN_Y, 0.1, w * 0.9, 0.42, mul(col, 2.2));
    }
}

#[cfg(test)]
mod tests {
    use super::super::*;
    use crate::surface::Surface;
    use nd_sim::buildings::{Occupant, Unit};
    use nd_sim::world::{World, WorldConfig};

    /// [tenants] A block of flats with three shopfronts, two let and one
    /// to let, wears up to two lit signs (and boards) over its street front,
    /// low down, on almost every style; without units it is drawn as before.
    #[test]
    fn a_sign_over_each_shop_on_the_street_front() {
        let mut w = World::new(WorldConfig::default());
        w.add_road(&[glam::Vec2::new(-200.0, 0.0), glam::Vec2::new(200.0, 0.0)], 1, 1);
        w.add_zone(vec![glam::Vec2::new(-150.0, 8.0), glam::Vec2::new(150.0, 8.0), glam::Vec2::new(150.0, 80.0), glam::Vec2::new(-150.0, 80.0)], LandUse::Residential);
        let mut lots: Vec<Lot> = w.lots.values().filter(|l| l.usable()).cloned().collect();
        lots.sort_by(|a, b| b.area().total_cmp(&a.area()));
        let lot = &lots[0];
        let (rules, pal) = (BuildingRules::default(), Palette::default());
        let unit = |occupant| Unit { lo: 0, hi: 0, share: 1.0 / 3.0, area: 80.0, shopfront: true, jobs: 3, occupant, vacant_since: 0, role: Default::default() };
        let units = [unit(Occupant::Main), unit(Occupant::Vacant), unit(Occupant::Main)];
        let mut drawn = 0;
        for seed in 0..12u64 {
            let plain = generate_building(lot, LandUse::Residential, 6, Tier::Mid, 1.0, seed, &rules, &pal, &Extras::default());
            let signed = generate_building(lot, LandUse::Residential, 6, Tier::Mid, 1.0, seed, &rules, &pal, &Extras { units: &units, ..Extras::default() });
            let glow = |g: &crate::surface::Geometry| g.parts.get(Surface::Glow.index()).map_or(0, |m| m.positions.len());
            let matte = |g: &crate::surface::Geometry| g.parts.get(Surface::Matte.index()).map_or(0, |m| m.positions.len());
            if glow(&signed) > glow(&plain) {
                drawn += 1;
                // Two lit signs (a quad each), boards for all three.
                assert!(glow(&signed) - glow(&plain) <= 2 * 4, "seed {seed}");
                assert!(matte(&signed) > matte(&plain));
                let new: Vec<_> = signed.parts[Surface::Glow.index()].positions[glow(&plain)..].to_vec();
                assert!(new.iter().all(|p| p[1] > 2.5 && p[1] < 4.0), "over the shop doors: {new:?}");
            }
        }
        assert!(drawn >= 10, "signs on most styles: {drawn} of 12");
    }
}
