//! [r17-logos] Billboard and LED-screen ads for the companies in town.
//!
//! A screen shows one or two ads, each a frame with the company's logo and
//! name (and its line under the name), and on a wide screen a second frame
//! with its slogan. Every frame is drawn into the building's Glow mesh with
//! a vertex mark ([`ad_alpha`]) saying which frame of how many it is and
//! the screen's phase; the window-lights shader (`nd_app render/lights.rs`)
//! shows one frame at a time, cycling every few seconds, so a street of
//! screens changes at different moments and no mesh is rebuilt. Where the
//! glow is not tagged (previews, the building viewer) only the first frame
//! is drawn.
//!
//! The advertisers are real: [`ads_for`] picks mega-corps, chains and
//! local businesses of the world by the building's seed (a business's own
//! screen often shows its own ad); a city with none (a preview) gets
//! fictional brands from the corp name generator.

use crate::logo::{self, Ink, Mark, Paint, Place, Prim, Tess};
use crate::mesh::MeshData;
use glam::{Vec2, Vec3};
use nd_sim::hash::{mix, unit};
use nd_sim::{BuildingId, BusinessKind, World};

/// One company's ad.
#[derive(Clone, Debug, PartialEq)]
pub struct Ad {
    pub mark: Mark,
    pub name: String,
    pub slogan: String,
}

/// Where a building's screens get their ads: called with a salt per
/// screen, returns up to two ads (the closure reads the world).
#[derive(Clone, Copy)]
pub struct AdSource<'a>(pub &'a (dyn Fn(u64) -> Vec<Ad> + Sync + 'a));

impl std::fmt::Debug for AdSource<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("AdSource")
    }
}

/// Seconds an ad frame stays up (the shader's period; kept here for the
/// docs and tests).
pub const FRAME_SECONDS: f32 = 5.0;

/// The vertex alpha marking glow as frame `frame` of `frames` (1..=4) of
/// a screen whose cycle starts at `phase` (0..16): below -1, apart from
/// every other glow mark (window ranks are 2..3, neon 1.5, night 1.75).
pub fn ad_alpha(frame: u8, frames: u8, phase: u8) -> f32 {
    let code = (frame as u32 & 3) | ((frames.clamp(1, 4) as u32 - 1) << 2) | ((phase as u32 & 15) << 4);
    -(1.0 + (code as f32 + 0.5) / 256.0)
}

/// The code an [`ad_alpha`] carries (frame in bits 0-1, frames - 1 in
/// 2-3, phase in 4-7), or `None` for any other alpha.
pub fn ad_code(alpha: f32) -> Option<u32> {
    (alpha < -0.999 && alpha > -2.001).then(|| (((-alpha - 1.0) * 256.0).floor() as i64).clamp(0, 255) as u32)
}

/// Which frame of a screen shows at `seconds`, as the shader picks it.
pub fn frame_at(code: u32, seconds: f32) -> u32 {
    let frames = ((code >> 2) & 3) + 1;
    let phase = (code >> 4) as f32;
    ((seconds / FRAME_SECONDS + phase * 0.37).floor() as i64).rem_euclid(frames as i64) as u32
}

// ── who advertises ────────────────────────────────────────────────────────────

const SLOGAN_FOOD: &[&str] = &["HOT & FRESH 24/7", "TASTE THE NIGHT", "OPEN LATE", "EAT. SLEEP. REPEAT.", "NOW WITH EXTRA CHILLI", "TWO FOR $9"];
const SLOGAN_NIGHT: &[&str] = &["LIVE TONIGHT", "HAPPY HOUR 6-9", "THE NIGHT NEVER ENDS", "DOORS AT 9", "BE LOUD"];
const SLOGAN_TECH: &[&str] = &["UPGRADE YOUR LIFE", "NEW MODEL OUT NOW", "CHROME INSIDE", "FASTER. BRIGHTER.", "ZERO LAG"];
const SLOGAN_MOVE: &[&str] = &["WE MOVE THE CITY", "ON TIME. EVERY TIME.", "BUILT TO LAST", "HEAVY DUTY", "DOOR TO DOOR"];
const SLOGAN_SHOP: &[&str] = &["EVERYTHING. TONIGHT.", "PRICES SLASHED", "FRESH EVERY DAY", "YOUR CORNER SHOP", "MEMBERS SAVE 20%"];
const SLOGAN_CORP: &[&str] = &["THE FUTURE IS HERE", "TOMORROW, DELIVERED", "ALWAYS ON", "TRUST THE NETWORK", "MORE. NOW.", "BUILDING YOUR CITY"];

/// A slogan for a company of `kind` (a mega-corp when `corp`).
pub fn slogan(kind: Option<BusinessKind>, corp: bool, h: u64) -> String {
    use BusinessKind as B;
    let set: &[&str] = if corp {
        SLOGAN_CORP
    } else {
        match kind {
            Some(B::Diner | B::NoodleBar | B::PizzaPlace | B::FastFood | B::FoodHall | B::StreetFood | B::FineDining | B::BubbleTea | B::Bakery | B::Cafe | B::SeafoodRestaurant | B::PierRestaurant) => SLOGAN_FOOD,
            Some(B::Bar | B::KaraokeBox | B::Venue | B::BeachBar | B::Cinema | B::Theatre | B::ConcertHall | B::BowlingAlley | B::Arcade | B::ComedyClub) => SLOGAN_NIGHT,
            Some(B::GadgetShop | B::Fab | B::Assembly | B::Office | B::CarDealer | B::BikeShop) => SLOGAN_TECH,
            Some(B::FleetLot | B::CargoHub | B::TaxiCompany | B::ContainerYard | B::TowYard | B::Contractor | B::BuildersMerchant | B::TrashCompany | B::Mechanic) => SLOGAN_MOVE,
            Some(_) => SLOGAN_SHOP,
            None => SLOGAN_CORP,
        }
    };
    set[(h % set.len() as u64) as usize].to_string()
}

/// A business building's ad (its trading name and mark: its corp's when a
/// mega-corp owns it).
pub fn ad_of(world: &World, id: BuildingId, h: u64) -> Option<Ad> {
    let b = world.buildings.get(id)?;
    if let Some(c) = world.corp_of(id) {
        return Some(Ad { mark: Mark::of_brand(&c.brand), name: c.brand.name.clone(), slogan: slogan(None, true, h) });
    }
    let kind = b.business.as_ref()?.kind;
    if b.name.trim().is_empty() {
        return None;
    }
    let name = logo::trading_name(&b.name).to_string();
    Some(Ad { mark: Mark::local(&name), name, slogan: slogan(Some(kind), false, h) })
}

/// [r17-logos] By unit of building `id`: the mark and trading name of the
/// business in it (its own business, or the tenant's), for its shop sign.
pub fn shop_marks(world: &World, id: BuildingId) -> Vec<Option<(Mark, String)>> {
    let Some(b) = world.buildings.get(id) else { return Vec::new() };
    b.units
        .iter()
        .map(|u| {
            let who = match u.occupant {
                nd_sim::buildings::Occupant::Main => id,
                nd_sim::buildings::Occupant::Tenant(t) => t,
                nd_sim::buildings::Occupant::Vacant => return None,
            };
            if !u.shopfront {
                return None;
            }
            ad_of(world, who, 0).map(|a| (a.mark, a.name))
        })
        .collect()
}

/// Up to two ads for a screen of building `id` (salt: the screen): its own
/// business sometimes, then mega-corps, chains and other businesses in
/// town by the seed.
pub fn ads_for(world: &World, id: BuildingId, salt: u64) -> Vec<Ad> {
    let seed = world.buildings.get(id).map_or(0, |b| b.seed);
    let h = mix(seed ^ 0xAD5, salt);
    let mut out: Vec<Ad> = Vec::new();
    let push = |a: Option<Ad>, out: &mut Vec<Ad>| {
        if let Some(a) = a {
            if !out.iter().any(|o| o.name == a.name) {
                out.push(a);
            }
        }
    };
    if unit(mix(h, 1)) < 0.5 {
        push(ad_of(world, id, mix(h, 2)), &mut out);
    }
    let corps = &world.corps.corps;
    for k in 0..6u64 {
        if out.len() >= 2 {
            break;
        }
        let r = unit(mix(h, 10 + k));
        let pick = mix(h, 20 + k);
        if r < 0.45 && !corps.is_empty() {
            let c = &corps[(pick % corps.len() as u64) as usize];
            push(Some(Ad { mark: Mark::of_brand(&c.brand), name: c.brand.name.clone(), slogan: slogan(None, true, mix(pick, 3)) }), &mut out);
        } else if r < 0.75 && !world.estate.owners.is_empty() {
            // A firm with branches: a chain.
            let n = world.estate.owners.len();
            let start = (pick % n as u64) as usize;
            let chain = world.estate.owners.values().cycle().skip(start).take(n.min(64)).find_map(|o| match o.kind {
                nd_sim::estate::OwnerKind::Firm { kind } if o.properties.len() >= 2 && !o.name.trim().is_empty() => Some((o.name.clone(), kind)),
                _ => None,
            });
            if let Some((name, kind)) = chain {
                let name = logo::trading_name(&name).to_string();
                push(Some(Ad { mark: Mark::local(&name), name, slogan: slogan(Some(kind), false, mix(pick, 3)) }), &mut out);
            }
        } else if !world.buildings.is_empty() {
            let n = world.buildings.len();
            let start = (pick % n as u64) as usize;
            let other = world.buildings.iter().skip(start).take(48).find(|(_, b)| b.business.is_some() && !b.name.is_empty()).map(|(bid, _)| bid);
            if let Some(bid) = other {
                push(ad_of(world, bid, mix(pick, 3)), &mut out);
            }
        }
    }
    out
}

/// Ads for a screen with no town to draw on (a preview): fictional brands.
pub fn fallback_ads(seed: u64) -> Vec<Ad> {
    (0..2u64)
        .map(|k| {
            let h = mix(seed, 0xFA11 + k);
            let name = nd_sim::corp::corp_name(h);
            let short = name.split(' ').next().unwrap_or(&name).to_string();
            let colour = (mix(h, 1) % nd_sim::corp::BRAND_COLOURS.len() as u64) as u8;
            let shape = nd_sim::corp::LogoShape::CORP[(mix(h, 2) % 7) as usize];
            Ad { mark: Mark::corp(colour, shape, short.chars().next().unwrap_or('X')), name: short, slogan: slogan(None, true, mix(h, 3)) }
        })
        .collect()
}

// ── drawing ───────────────────────────────────────────────────────────────────

/// The frames of a `w` x `h` (m) screen showing `ads`, each its own
/// tessellation in screen metres (x right, y up, centred), and the paint
/// of each. A wide screen gives each ad a slogan frame too; at most four.
pub fn frames(ads: &[Ad], w: f32, h: f32) -> Vec<(Tess, Paint)> {
    let mut out = Vec::new();
    for ad in ads.iter().take(2) {
        out.push(brand_frame(ad, w, h));
        if w >= 6.0 && w >= h * 1.6 && out.len() < 4 {
            out.push(slogan_frame(ad, w, h));
        }
    }
    out.truncate(4);
    out
}

fn back(w: f32, h: f32, ink: Ink) -> Prim {
    Prim::Fill { pts: vec![Vec2::new(-w * 0.5, -h * 0.5), Vec2::new(w * 0.5, -h * 0.5), Vec2::new(w * 0.5, h * 0.5), Vec2::new(-w * 0.5, h * 0.5)], ink }
}

fn border(w: f32, h: f32) -> Prim {
    let (x, y) = (w * 0.5 - h * 0.03, h * 0.5 - h * 0.03);
    Prim::Stroke { pts: vec![Vec2::new(-x, -y), Vec2::new(x, -y), Vec2::new(x, y), Vec2::new(-x, y)], closed: true, w: (h * 0.025).max(0.03), ink: Ink::Brand }
}

fn ad_paint(m: &Mark) -> Paint {
    let mut p = Paint::lit(m, 2.2);
    // The screen's ground: the brand colour, dim.
    p.dark = [m.colour[0] * 0.08 + 0.01, m.colour[1] * 0.08 + 0.01, m.colour[2] * 0.08 + 0.012];
    p
}

/// Logo `side` metres square centred at `at`, as primitives.
fn logo_at(m: &Mark, at: Vec2, side: f32) -> Tess {
    let mut t = logo::tessellate(m);
    t.place(side * 0.5, at);
    t
}

/// The logo and the name (and the slogan under it when there is room).
fn brand_frame(ad: &Ad, w: f32, h: f32) -> (Tess, Paint) {
    let mut t = Tess::default();
    t.add(&[back(w, h, Ink::Dark), border(w, h)]);
    let ty = logo::type_of(&ad.mark);
    let name = ad.name.to_uppercase();
    if w >= h * 1.3 {
        // Landscape: the logo on the left, the name beside it on one line
        // or two; a long name on a short screen goes under the logo.
        let side = h * 0.74;
        let lx = -w * 0.5 + h * 0.5;
        let x0 = -w * 0.5 + h * 1.0;
        let room = w * 0.5 - h * 0.12 - x0;
        let one = (room / logo::text_w(&name, 1.0, ty)).min(h * 0.3);
        let lines = split_name(&name);
        let two = lines.as_ref().map_or(0.0, |(a, b)| (room / logo::text_w(a, 1.0, ty).max(logo::text_w(b, 1.0, ty))).min(h * 0.27));
        if one >= h * 0.14 && one >= two * 0.85 {
            add(&mut t, logo_at(&ad.mark, Vec2::new(lx, 0.0), side));
            let nh = one;
            let sh = (room / logo::text_w(&ad.slogan, 1.0, ty)).min(nh * 0.45);
            let with_line = sh >= h * 0.06;
            let ny = if with_line { h * 0.02 } else { -nh * 0.5 };
            let (p, _) = logo::text_prims(&name, x0, ny, nh, ty, Ink::Brand);
            let mut parts = p;
            if ty.underline {
                parts.push(Prim::Stroke { pts: vec![Vec2::new(x0, ny - nh * 0.22), Vec2::new(x0 + logo::text_w(&name, nh, ty), ny - nh * 0.22)], closed: false, w: nh * 0.08, ink: Ink::Accent });
            }
            if with_line {
                let (p, _) = logo::text_prims(&ad.slogan, x0, ny - nh * 0.45 - sh, sh, logo::Type { slant: 0.0, weight: 0.14, underline: false }, Ink::Light);
                parts.extend(p);
            }
            t.add(&parts);
        } else if two >= h * 0.14 {
            add(&mut t, logo_at(&ad.mark, Vec2::new(lx, 0.0), side));
            let (a, b) = lines.expect("two lines");
            let nh = two;
            let (p, _) = logo::text_prims(&a, x0, nh * 0.25, nh, ty, Ink::Brand);
            let (q, _) = logo::text_prims(&b, x0, -nh * 1.25, nh, ty, Ink::Brand);
            t.add(&p.into_iter().chain(q).collect::<Vec<_>>());
        } else {
            let nh = ((w * 0.9) / logo::text_w(&name, 1.0, ty)).min(h * 0.2);
            if nh >= h * 0.07 {
                let side = h * 0.56;
                add(&mut t, logo_at(&ad.mark, Vec2::new(0.0, h * 0.5 - h * 0.08 - side * 0.5), side));
                let nw = logo::text_w(&name, nh, ty);
                let (p, _) = logo::text_prims(&name, -nw * 0.5, -h * 0.5 + h * 0.1, nh, ty, Ink::Brand);
                t.add(&p);
            } else {
                add(&mut t, logo_at(&ad.mark, Vec2::ZERO, h * 0.8));
            }
        }
    } else {
        // Portrait or square: the logo on top, the name under it.
        let side = (w * 0.8).min(h * 0.62);
        add(&mut t, logo_at(&ad.mark, Vec2::new(0.0, h * 0.5 - h * 0.06 - side * 0.5), side));
        let nh = ((w * 0.88) / logo::text_w(&name, 1.0, ty)).min(h * 0.16);
        if nh >= h * 0.05 {
            let nw = logo::text_w(&name, nh, ty);
            let (p, _) = logo::text_prims(&name, -nw * 0.5, -h * 0.5 + h * 0.08, nh, ty, Ink::Brand);
            t.add(&p);
        }
    }
    (t, ad_paint(&ad.mark))
}

/// A name in two lines, split at the space nearest its middle.
fn split_name(name: &str) -> Option<(String, String)> {
    let mid = name.len() / 2;
    let at = name.char_indices().filter(|(_, c)| *c == ' ').map(|(i, _)| i).min_by_key(|i| (*i as i64 - mid as i64).abs())?;
    Some((name[..at].to_string(), name[at + 1..].to_string()))
}

/// The slogan big, the logo small beside it.
fn slogan_frame(ad: &Ad, w: f32, h: f32) -> (Tess, Paint) {
    let mut t = Tess::default();
    t.add(&[back(w, h, Ink::Dark), border(w, h)]);
    let side = h * 0.42;
    add(&mut t, logo_at(&ad.mark, Vec2::new(w * 0.5 - h * 0.12 - side * 0.5, -h * 0.5 + h * 0.1 + side * 0.5), side));
    let ty = logo::Type { slant: logo::type_of(&ad.mark).slant, weight: 0.15, underline: false };
    let room = w - h * 0.3;
    let sh = (room / logo::text_w(&ad.slogan, 1.0, ty)).min(h * 0.3);
    let sw = logo::text_w(&ad.slogan, sh, ty);
    let (p, _) = logo::text_prims(&ad.slogan, -sw * 0.5, h * 0.06, sh, ty, Ink::Light);
    t.add(&p);
    let name = ad.name.to_uppercase();
    let nh = ((w - side - h * 0.5) / logo::text_w(&name, 1.0, ty)).min(h * 0.16);
    if nh >= h * 0.05 {
        let (p, _) = logo::text_prims(&name, -w * 0.5 + h * 0.15, -h * 0.5 + h * 0.12, nh, ty, Ink::Brand);
        t.add(&p);
    }
    (t, ad_paint(&ad.mark))
}

/// Append `src` to `t` as the next layers.
fn add(t: &mut Tess, src: Tess) {
    let base_layer = t.layers.iter().copied().max().map_or(0, |l| l + 1);
    let base = t.pts.len() as u32;
    t.pts.extend(src.pts);
    t.inks.extend(src.inks);
    t.layers.extend(src.layers.into_iter().map(|l| l + base_layer));
    t.tris.extend(src.tris.into_iter().map(|tri| tri.map(|i| i + base)));
}

/// Draw a screen's frames into `m` (a Glow mesh), centred at `centre`,
/// read along `right`, facing `normal`. `animate`: every frame, marked
/// for the shader; otherwise only the first, unmarked. Returns the
/// triangles drawn.
pub fn draw_screen(m: &mut MeshData, centre: Vec3, right: Vec3, normal: Vec3, w: f32, h: f32, ads: &[Ad], animate: bool, phase: u8) -> usize {
    let fr = frames(ads, w, h);
    let n = if animate { fr.len() } else { fr.len().min(1) };
    let lift = (h * 0.004).clamp(0.003, 0.02);
    let mut tris = 0;
    for (k, (t, paint)) in fr.iter().take(n).enumerate() {
        let alpha = if n > 1 { ad_alpha(k as u8, n as u8, phase) } else { 1.0 };
        let place = Place { centre, right: right.normalize_or_zero(), up: Vec3::Y, normal, lift };
        logo::emit(m, t, &place, paint, alpha);
        tris += t.triangles();
    }
    tris
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ad_marks_round_trip_and_stay_clear_of_other_marks() {
        for frames in 1..=4u8 {
            for frame in 0..frames {
                for phase in 0..16u8 {
                    let a = ad_alpha(frame, frames, phase);
                    assert!(a < -1.0 && a > -2.0);
                    let c = ad_code(a).expect("an ad mark");
                    assert_eq!(c & 3, frame as u32);
                    assert_eq!(((c >> 2) & 3) + 1, frames as u32);
                    assert_eq!(c >> 4, phase as u32);
                }
            }
        }
        for other in [1.0, 1.5, 1.75, 2.0, 2.5, 0.0] {
            assert_eq!(ad_code(other), None);
        }
    }

    #[test]
    fn a_screen_shows_each_frame_in_turn() {
        let code = ad_code(ad_alpha(0, 3, 5)).unwrap();
        let shown: Vec<u32> = (0..6).map(|k| frame_at(code, k as f32 * FRAME_SECONDS + 0.1)).collect();
        for f in 0..3 {
            assert!(shown.contains(&f), "{shown:?}");
        }
        // Screens with other phases change at other moments.
        let other = ad_code(ad_alpha(0, 3, 6)).unwrap();
        assert!((0..30).any(|k| frame_at(code, k as f32 * 0.5) != frame_at(other, k as f32 * 0.5)));
    }

    #[test]
    fn a_screen_draws_every_frame_marked_or_the_first_alone() {
        let ads = fallback_ads(42);
        assert_eq!(ads.len(), 2);
        assert_ne!(ads[0].name, ads[1].name);
        let mut m = MeshData::default();
        let tris = draw_screen(&mut m, Vec3::ZERO, Vec3::X, Vec3::Z, 12.0, 5.0, &ads, true, 3);
        assert_eq!(m.indices.len(), tris * 3);
        let codes: std::collections::HashSet<u32> = m.colors.iter().filter_map(|c| ad_code(c[3])).map(|c| c & 3).collect();
        assert_eq!(codes.len(), 4, "two ads, a brand and a slogan frame each");
        let mut still = MeshData::default();
        draw_screen(&mut still, Vec3::ZERO, Vec3::X, Vec3::Z, 12.0, 5.0, &ads, false, 3);
        assert!(still.colors.iter().all(|c| c[3] == 1.0));
        assert!(still.indices.len() < m.indices.len());
    }

    #[test]
    fn a_long_name_breaks_at_the_middle_space() {
        assert_eq!(split_name("HALO ARCADE"), Some(("HALO".into(), "ARCADE".into())));
        assert_eq!(split_name("MAMA CHEN'S 24/7"), Some(("MAMA CHEN'S".into(), "24/7".into())));
        assert_eq!(split_name("KWIK-E-FRESH"), None);
    }

    #[test]
    fn screen_triangles_stay_modest() {
        let ads = fallback_ads(7);
        for (w, h) in [(18.0, 8.0), (8.0, 3.0), (3.0, 2.0), (1.6, 2.2), (10.0, 4.0)] {
            let mut m = MeshData::default();
            let tris = draw_screen(&mut m, Vec3::ZERO, Vec3::X, Vec3::Z, w, h, &ads, true, 0);
            assert!(tris < 4000, "{w}x{h}: {tris}");
        }
    }
}
