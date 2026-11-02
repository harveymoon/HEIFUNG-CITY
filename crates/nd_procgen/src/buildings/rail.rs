//! Railway stations and freight yards. Both sit on a lot that fronts a
//! street and back onto the track: the station has its hall at the road
//! and a canopied platform along the rails; the yard has a warehouse at
//! the road, an apron and a gantry crane along the rails.

use super::parts::*;
use super::{BuildingRules, Ctx, Frame};
use crate::palette::Palette;
use nd_sim::{rng_for, LandUse, Tier};
use crate::surface::{Geometry, Surface::*};
use glam::{Vec2, Vec3};
use rand::Rng;
use std::f32::consts::FRAC_PI_2;

pub fn station(g: &mut Geometry, c: &Ctx, rng: &mut impl Rng) {
    let f = &c.frame;
    let pal = c.palette;
    if c.rail.is_none() {
        return;
    }
    // Forecourt the whole lot, paved.
    ground(g, f, Paving, &offset_poly(&c.lot_poly, 0.3), 0.04, mul(pal.paving, 1.1));
    // Hall: on the road half of the lot, as wide as the lot allows.
    let poly = &c.poly;
    let (lo, hi) = bounds(poly);
    let hall = fit_inside(c, &rect((lo.x + hi.x) * 0.5, lo.y + (hi.y - lo.y) * 0.32, (hi.x - lo.x) * 0.42, (hi.y - lo.y) * 0.22));
    let h = 8.0;
    let brick = pick(rng, &pal.brick);
    extrude(g, f, Brick, &hall, 0.0, h, brick);
    cornice(g, c, &hall, h - 0.5, 1.0, 0.4, Stone, mul(pal.stone, 1.3));
    let front = c.front_face(&hall);
    // Tall arched glass front with a canopy and a clock.
    front.bx(g, f, Glass, 0.0, h * 0.5, 0.06, Vec3::new((front.len * 0.7).max(3.0), h * 0.7, 0.1), pick(rng, &pal.glass));
    front.bx(g, f, Glow, 0.0, h * 0.5, 0.0, Vec3::new((front.len * 0.66).max(2.6), h * 0.62, 0.08), mul(pal.window_warm, 0.55));
    canopy(g, c, &front, 0.0, 4.0, (front.len * 0.8).max(4.0), 3.0, mul(pal.steel, 0.9));
    front.bx(g, f, Panel, 0.0, h + 1.0, -0.6, Vec3::new(2.0, 2.0, 0.6), mul(pal.plaster(0), 1.1));
    front.decal(g, f, Glow, 0.0, h + 1.0, -0.29, 1.2, 1.2, pal.chalk);
    sign_board(g, c, rng, &front, 0.0, h - 1.4, (front.len * 0.6).min(14.0), 1.0);
    for face in c.faces(&hall) {
        if !face.is_front && face.len > 5.0 {
            band_windows(g, c, rng, &face, h * 0.55, 1, 3.0);
        }
    }
    platform(g, c, rng, poly_center(&hall));
    // Forecourt: taxi rank, bike racks, bins, bollards, a lamp.
    let gate = c.front_face(&offset_poly(&c.lot_poly, 0.3));
    for s in [-1.0f32, 1.0] {
        let p = gate.at(s * gate.len * 0.3, -4.0);
        if vehicle_fits(c, p, 0.0, Vehicle::Car) {
            let col = pick(rng, &pal.car_colors);
            vehicle(g, f, pal, rng, p, 0.0, 0.0, Vehicle::Car, col);
        }
    }
    for k in 0..4 {
        let p = gate.at(-gate.len * 0.42 + k as f32 * 1.0, -2.0);
        if c.inside_local(p) {
            f.bx(g, Matte, p.x, 0.45, p.y, Vec3::new(0.05, 0.9, 0.8), mul(pal.steel, 0.8));
        }
    }
    bollards(g, c, &gate, 6, -1.0);
    lamp_post(g, f, gate.at(gate.len * 0.42, -1.6), 5.0, pal.lamp);
}

/// The platform along the track, with a canopy on posts, benches, lamps, a
/// yellow line and name boards, and a ramped path to it from `from` (a
/// lot-local point: the hall's middle). Shared by the railway station and
/// the Central Station (`subway::central_station`), whose platform is the
/// link's longer one.
pub fn platform(g: &mut Geometry, c: &Ctx, rng: &mut impl Rng, from: Vec2) {
    let f = &c.frame;
    let pal = c.palette;
    let Some(link) = c.rail.as_ref() else { return };
    let [pa, pb] = link.platform;
    let along = link.along();
    let out = Vec2::new(-along.y, along.x) * link.side;
    let length = pa.distance(pb);
    let concrete = mul(pal.concrete(0), 1.2);
    let edge_a = pa - out * 1.3;
    let edge_b = pb - out * 1.3;
    g.part(Concrete).path_box(&[edge_a + out * 2.0, edge_b + out * 2.0], 0.0, 4.0, 0.0, 0.95, rgba(concrete));
    g.part(Glow).path_strip(&[edge_a + out * 0.5, edge_b + out * 0.5], 0.0, 0.15, 0.96, rgba(pal.platform_line));
    let posts = ((length / 7.0).floor() as i32).max(2);
    for k in 0..=posts {
        let p = pa.lerp(pb, k as f32 / posts as f32) + out * 2.4;
        g.part(Matte).boxed(Vec3::new(p.x, 2.7, p.y), Vec3::new(0.18, 3.5, 0.18), 0.0, rgba(mul(pal.steel, 0.7)));
        if k % 2 == 0 && k < posts {
            let b = pa.lerp(pb, (k as f32 + 0.5) / posts as f32) + out * 1.6;
            bench_world(g, b, along.y.atan2(along.x), pal.wood, CHARCOAL);
        }
    }
    g.part(Panel).path_box(&[pa + out * 2.4, pb + out * 2.4], 0.0, 4.4, 4.4, 4.7, rgba(mul(pick(rng, &pal.metal_paint), 0.9)));
    g.part(Glow).path_box(&[pa + out * 2.4, pb + out * 2.4], 0.0, 0.3, 4.3, 4.4, rgba(mul(pal.lamp, 0.7)));
    for t in [0.3f32, 0.7] {
        let p = pa.lerp(pb, t) + out * 1.2;
        g.part(Matte).boxed(Vec3::new(p.x, 2.0, p.y), Vec3::new(0.1, 2.2, 0.1), 0.0, rgba(mul(pal.steel, 0.6)));
        g.part(Panel).boxed(Vec3::new(p.x, 3.2, p.y), Vec3::new(2.4, 0.5, 0.06), along.y.atan2(along.x), rgba([0.1, 0.2, 0.5]));
        g.part(Glow).boxed(Vec3::new(p.x, 3.2, p.y), Vec3::new(2.2, 0.3, 0.07), along.y.atan2(along.x), rgba([1.3, 1.3, 1.2]));
    }
    // Path from the hall to the platform, ramped up. It leaves the lot at
    // the lot's point nearest the platform, not in a straight line that
    // might cut across a neighbour.
    let hc = from;
    let hw = f.to_world(hc.x, hc.y);
    let pc = link.centre() + out * 2.8;
    let exit_l = clamp_into(&c.lot_poly, f.to_local(pc));
    // Step in from the edge so the 3 m path does not spill sideways.
    let exit_l = exit_l + (hc - exit_l).normalize_or_zero() * 1.6;
    let exit = f.to_world(exit_l.x, exit_l.y);
    g.part(Paving).path_strip(&[hw, exit, pc], 0.0, 3.0, 0.05, rgba(mul(pal.paving, 1.2)));
    g.part(Concrete).slab(Vec3::new(exit.x, 0.05, exit.y), Vec3::new(pc.x, 0.95, pc.y), 3.0, 0.15, rgba(concrete));
}

/// [r4] (K33) Height of the yard crane's runway beams.
pub const YARD_RUNWAY_Y: f32 = 8.4;

/// [r4] (K33) A freight yard laid out across its track, as offsets along
/// `out` (away from the track, on the yard's side) from the platform line:
/// the main line's centre, the apron's near edge and width, and the crane
/// runways (the near one on the apron, the far one beyond the main line).
/// The yard is drawn to it and the renderer runs the crane on it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct YardLayout {
    pub main: f32,
    pub sidings: usize,
    pub apron_in: f32,
    pub apron_w: f32,
    pub near_off: f32,
    pub far_off: f32,
}

/// [r4] (K33) The layout of the yard on `link` with lot outline `lot`
/// (world coordinates).
pub fn yard_layout(link: &nd_sim::RailLink, lot: &[Vec2]) -> YardLayout {
    let [pa, pb] = link.platform;
    let along = link.along();
    let out = Vec2::new(-along.y, along.x) * link.side;
    let length = pa.distance(pb);
    // The platform line sits 3.8 m off the track centre; sidings every 4.6 m.
    let main = -(nd_sim::roads::RAIL_HALF_WIDTH + 1.4);
    let sidings = if length > 80.0 { 3 } else { 2 };
    let apron_in = main + 4.6 * sidings as f32 + 3.4;
    // The apron ends inside the lot: its far edge is the lot's extent
    // across the track, measured from the platform line.
    let lot_across = lot.iter().map(|p| (*p - pa).dot(out)).fold(f32::MIN, f32::max);
    let apron_w = (lot_across - 0.6 - apron_in).clamp(3.0, 9.0);
    YardLayout { main, sidings, apron_in, apron_w, near_off: apron_in + apron_w - 0.6, far_off: main - 3.0 }
}

pub fn freight_yard(g: &mut Geometry, c: &Ctx, rng: &mut impl Rng) {
    let f = &c.frame;
    let pal = c.palette;
    let Some(link) = c.rail.as_ref() else { return };
    let yard = yard(g, c, 0.4, Concrete, mul(pal.concrete(0), 0.95));
    // Warehouse by the road, doors to the yard.
    let poly = &c.poly;
    let (lo, hi) = bounds(poly);
    let shed = fit_inside(c, &rect((lo.x + hi.x) * 0.5, lo.y + (hi.y - lo.y) * 0.28, (hi.x - lo.x) * 0.4, (hi.y - lo.y) * 0.2));
    let h = 9.0;
    let panel = mul(pick(rng, &pal.panel), 0.85);
    extrude(g, f, Panel, &shed, 0.0, h, panel);
    cornice(g, c, &shed, h - 0.4, 0.8, 0.3, Panel, mul(panel, 0.8));
    for face in c.faces(&shed) {
        if face.len < 6.0 {
            continue;
        }
        let doors = ((face.len / 6.0).floor() as i32).clamp(1, 4);
        for k in 0..doors {
            let t = -face.len * 0.5 + (k as f32 + 0.5) * face.len / doors as f32;
            roller_door(g, c, &face, t, 4.0, 4.5, mul(pal.metal_paint(0), 0.9));
        }
        grime(g, c, rng, &face, 0.0, h, 2.5);
    }
    sign_board(g, c, rng, &c.front_face(&shed), 0.0, h - 1.3, 8.0, 0.9);
    roof_gear(g, c, rng, &shed, h, 0.2);
    // [r8] Lorry bays on the forecourt before the shed, else beside it.
    if c.real_boxes {
        let (slo, _) = bounds(&shed);
        let n = lorry_bays(g, c, Vec2::new(lo.x + 2.0, lo.y + 1.5), Vec2::new(hi.x - 2.0, slo.y - 1.0), 5, 0.03);
        if n < 5 {
            let (_, shi) = bounds(&shed);
            lorry_bays(g, c, Vec2::new(shi.x + 1.5, lo.y + 1.5), Vec2::new(hi.x - 2.0, lo.y + (hi.y - lo.y) * 0.45), 5 - n, 0.03);
        }
    }
    // Sidings fan off the main line beside the apron, as many as the yard
    // is long for, under a gantry that spans them all.
    let [pa, pb] = link.platform;
    let along = link.along();
    let out = Vec2::new(-along.y, along.x) * link.side;
    let length = pa.distance(pb);
    let lot_world: Vec<Vec2> = c.lot_poly.iter().map(|p| f.to_world(p.x, p.y)).collect();
    let lay = yard_layout(link, &lot_world);
    let main = lay.main;
    let sidings = lay.sidings;
    // The points where a siding rejoins the main line are clamped to the
    // lot's own extent along the track, so a yard does not lay rail through
    // its neighbours: a short lot gets short, steep turnouts.
    let (lot_lo, lot_hi) = c.lot_poly.iter().map(|p| (f.to_world(p.x, p.y) - pa).dot(along)).fold((f32::MAX, f32::MIN), |(lo, hi), s| (lo.min(s), hi.max(s)));
    let (lot_lo, lot_hi) = (lot_lo.min(-2.0), lot_hi.max(length + 2.0));
    for k in 1..=sidings {
        let off = main + 4.6 * k as f32;
        let reach = 26.0 + 8.0 * k as f32;
        let s0 = (-reach).max(lot_lo);
        let s1 = (length + reach).min(lot_hi);
        let a0 = pa + along * s0 + out * main;
        let a1 = pa + along * (s0 * 0.25) + out * (main + (off - main) * 0.55);
        let b1 = pa + along * (length + (s1 - length) * 0.75) + out * (main + (off - main) * 0.55);
        let b0 = pa + along * s1 + out * main;
        let ctrl = [a0, a1, pa + out * off, pb + out * off, b1, b0];
        let pts = nd_sim::plan::sample_curve(&ctrl);
        railway_track(g, pal, &pts, 4.4);
    }
    let (apron_in, apron_w) = (lay.apron_in, lay.apron_w);
    g.part(Concrete).path_box(&[pa + out * (apron_in + apron_w * 0.5), pb + out * (apron_in + apron_w * 0.5)], 0.0, apron_w, 0.0, 0.3, rgba(mul(pal.concrete(0), 1.1)));
    g.part(Glow).path_strip(&[pa + out * (apron_in + 0.3), pb + out * (apron_in + 0.3)], 0.0, 0.12, 0.32, rgba(pal.platform_line));
    // Gantry: legs on the apron and beyond the main line carrying the two
    // runway beams along the track. [r4] (K33) The crane's bridge, trolley
    // and spreader ride them, drawn and moved by the renderer from the
    // train's real load (`YardLayout`).
    let steel = mul(pal.metal_paint(0), 1.1);
    let (far_off, near_off) = (lay.far_off, lay.near_off);
    for t in [0.25f32, 0.75] {
        let p = pa.lerp(pb, t);
        let near = p + out * near_off;
        let far = p + out * far_off;
        for q in [near, far] {
            g.part(Metal).boxed(Vec3::new(q.x, 4.0, q.y), Vec3::new(0.5, 8.0, 0.5), 0.0, rgba(steel));
        }
    }
    for off in [near_off, far_off] {
        g.part(Metal).beam(Vec3::new(pa.x + out.x * off, YARD_RUNWAY_Y, pa.y + out.y * off), Vec3::new(pb.x + out.x * off, YARD_RUNWAY_Y, pb.y + out.y * off), 0.4, rgba(steel));
    }
    aviation_light(g, pal, Vec3::new(pa.x + out.x * near_off, 8.9, pa.y + out.y * near_off), 0.35);
    // Container stacks on the apron's far edge, towards the shed.
    let n = ((length / 7.0) as i32).clamp(1, 10);
    for k in 0..n {
        let p = pa.lerp(pb, (k as f32 + 0.5) / n as f32) + out * (apron_in + apron_w + 1.8);
        let lp = f.to_local(p);
        if !poly_inside(c, &rect_yaw(lp.x, lp.y, 3.1, 1.3, along.y.atan2(along.x) - f.yaw())) {
            continue;
        }
        if c.real_boxes {
            // [K34] The yard's real boxes stand here.
            for level in 0..3 {
                box_slot(g, c, lp.x, lp.y, along.y.atan2(along.x) - f.yaw(), 0.0, level, false);
            }
            continue;
        }
        let stack = rng.random_range(1..=3);
        for level in 0..stack {
            // [r4] (K33) The standard box, in a line's colours.
            let line = shipping_line(rng.random());
            iso_container(g, c, rng, lp.x, lp.y, along.y.atan2(along.x) - f.yaw(), false, level as f32 * CONTAINER_H, line, BoxLook::Dry);
        }
    }
    // Forklift and lorries at the doors.
    let fp = pa.lerp(pb, 0.5) + out * (apron_in + apron_w + 5.5);
    let yaw = rng.random_range(0.0..3.0);
    if vehicle_fits(c, f.to_local(fp), yaw, Vehicle::Van) {
        vehicle(g, f, pal, rng, f.to_local(fp), 0.0, yaw, Vehicle::Van, [0.9, 0.7, 0.1]);
    }
    let front = c.front_face(&shed);
    for s in [-1.0f32, 1.0] {
        let p = front.at(s * front.len * 0.25, 5.5);
        if vehicle_fits(c, p, FRAC_PI_2, Vehicle::Truck) {
            let col = pick(rng, &pal.car_colors);
            vehicle(g, f, pal, rng, p, 0.0, FRAC_PI_2, Vehicle::Truck, col);
        }
    }
    // Fence, gate, floodlights, weighbridge.
    let gate = c.front_face(&yard);
    fence_lot(g, c, &yard, 2.4, true);
    barrier(g, c, &gate, gate.len * 0.2, -1.0, 3.5);
    for p in [gate.at(-gate.len * 0.4, -1.5), Vec2::new(lo.x + 2.0, hi.y - 2.0), Vec2::new(hi.x - 2.0, hi.y - 2.0)] {
        if c.inside_local(p) {
            floodlight(g, c, p, 11.0);
        }
    }
    let wb = gate.at(-gate.len * 0.2, -5.0);
    if poly_inside(c, &rect(wb.x, wb.y, 1.6, 4.6)) {
        f.bx(g, Metal, wb.x, 0.08, wb.y, Vec3::new(3.0, 0.16, 9.0), mul(pal.steel, 0.5));
    }
}

/// [r4] (K33) Draw with `draw` in a bare world frame (no lot): for the
/// containers the renderer puts on vehicles.
fn draw_free(seed: u64, wear: f32, pal: &Palette, draw: impl FnOnce(&mut Geometry, &Ctx)) -> Geometry {
    let rules = BuildingRules::default();
    let sq = vec![Vec2::new(-40.0, -40.0), Vec2::new(40.0, -40.0), Vec2::new(40.0, 40.0), Vec2::new(-40.0, 40.0)];
    let c = Ctx {
        frame: Frame { center: Vec2::ZERO, along: Vec2::X, inward: Vec2::Y, width: 80.0, depth: 80.0 },
        rules: rules.for_land_use(LandUse::Industrial),
        style: rules.style(Tier::Mid),
        palette: pal,
        land_use: LandUse::Industrial,
        tier: Tier::Mid,
        floors: 1,
        wear: wear.clamp(0.0, 1.0),
        fw: 80.0,
        fd: 80.0,
        lot_poly: sq.clone(),
        poly: sq,
        seed,
        parking: None,
        powered: 1.0,
        park: None,
        rail: None,
        animated_rotors: false,
        field: None,
        animated_cranes: false,
        dynamic_lights: false,
        battery: None,
        plant_scale: 1.0,
        shore: None,
        pier: None,
        park_look: None,
        boxes: Vec::new(),
        real_boxes: false,
        overhead: Vec::new(),
        waste_fill: 0.0,
        site: None,
    };
    let mut g = Geometry::default();
    draw(&mut g, &c);
    g
}

/// [r4] (K33) One standard container on its own, for the renderer to put
/// on a train, a lorry or a crane's spreader: centred on the origin, its
/// floor at height 0, its length along +x, drawn exactly as the yards and
/// stacks draw theirs ([`iso_container`]), in the line and paint `seed`
/// picks, as worn as `wear`.
pub fn container_geometry(forty: bool, look: BoxLook, seed: u64, wear: f32, pal: &Palette) -> Geometry {
    draw_free(seed, wear, pal, |g, c| {
        let mut rng = rng_for(seed, 0xC0E7);
        iso_container(g, c, &mut rng, 0.0, 0.0, 0.0, forty, 0.0, shipping_line(seed), look);
    })
}

/// [K34] One real box on its own, as its record says (line, build, wear,
/// marks; its number on when `numbered`), placed like
/// [`container_geometry`]'s.
pub fn real_box_geometry(face: &nd_sim::containers::BoxFace, numbered: bool, pal: &Palette) -> Geometry {
    draw_free(face.seed, face.wear, pal, |g, c| real_box(g, c, 0.0, 0.0, 0.0, 0.0, face, numbered))
}

/// [K34] A box's number alone (both sides and the door), in the same
/// place: the renderer lays it over the shared box mesh close up.
pub fn box_number_mesh(face: &nd_sim::containers::BoxFace) -> crate::mesh::MeshData {
    let f = Frame { center: Vec2::ZERO, along: Vec2::X, inward: Vec2::Y, width: 80.0, depth: 80.0 };
    let mut m = crate::mesh::MeshData::default();
    if face.kind != nd_sim::rail::BoxKind::Tank {
        box_number(&mut m, &f, 0.0, 0.0, 0.0, 0.0, face, box_life(face).door);
    }
    m
}

/// [r4] (K33) A container ship's deck cargo: bays of forty-foot boxes across
/// the deck between the bow and the deckhouse, one to three high, each in
/// its own line's colours. In the ship's frame (bow along +x) with the
/// lowest tier on `deck`; `len` and `beam` are the hull's.
pub fn freighter_cargo(seed: u64, len: f32, beam: f32, deck: f32, pal: &Palette) -> Geometry {
    draw_free(seed, 0.35, pal, |g, c| {
        let mut rng = rng_for(seed, 0x5419);
        let half = len * 0.5;
        let rows = ((beam * 0.86) / (CONTAINER_W + 0.06)).floor().max(1.0) as i32;
        let pitch = CONTAINER_40 + 0.5;
        let bays = (((len - 26.0) / pitch).floor() as i32).max(1);
        for k in 0..bays {
            let x = -half + 16.0 + pitch * 0.5 + k as f32 * pitch;
            for r in 0..rows {
                let z = (r as f32 - (rows - 1) as f32 * 0.5) * (CONTAINER_W + 0.06);
                let tiers = 1 + ((k * 7 + r * 3 + seed as i32) % 3);
                for t in 0..tiers {
                    let line = shipping_line(rng.random());
                    let look = if coin(&mut rng, 0.12) { BoxLook::Reefer } else { BoxLook::Dry };
                    iso_container(g, c, &mut rng, x, z, 0.0, true, deck + t as f32 * CONTAINER_H, line, look);
                }
            }
        }
    })
}
