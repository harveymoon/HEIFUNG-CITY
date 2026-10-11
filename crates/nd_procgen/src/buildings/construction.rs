//! A building site. Drawn in place of the finished style while the sim's
//! `Construction` is in progress: hoarding round the lot with a gate, a
//! concrete raft, the footprint rising floor by floor in bare concrete with
//! scaffolding climbing beside it, a tower crane on the taller jobs, and the
//! clutter of pallets, a site cabin, a mixer truck and the crew.

use super::kit::hoarding;
use super::parts::*;
use super::{Ctx, Face};
use crate::surface::{Geometry, Surface::*};
use glam::{Vec2, Vec3};
use nd_sim::zoning::{point_in_polygon, polygon_area};
use nd_sim::LandUse;
use rand::Rng;

use super::kit::HOARDING_H;

/// A stream of its own for one block of a site, so the blocks whose draw
/// counts change with progress (the structure, the crane, the stacks that
/// thin out) never shift what the others draw: the yard keeps its cabin,
/// truck and heaps where they were from one rebuild to the next.
fn sub(c: &Ctx, salt: u64) -> nd_sim::SimRng {
    nd_sim::rng_for(c.seed, 0x517E_0000 + salt)
}

/// Draw the site at `progress` in `0..1`.
pub fn construction_site(g: &mut Geometry, c: &Ctx, rng: &mut impl Rng, progress: f32) {
    let f = &c.frame;
    let pal = c.palette;
    let progress = progress.clamp(0.0, 1.0);
    let landscaping = c.land_use == LandUse::Park;
    let fh = c.fh();
    let height = if landscaping { 0.0 } else { c.height() };
    // The structure goes up over the middle of the job: groundworks first,
    // fit-out last.
    let built = if landscaping { 0.0 } else { (((progress - 0.08) / 0.84).clamp(0.0, 1.0) * height).min(height) };
    let floors_done = if fh > 0.0 { (built / fh).floor() as i32 } else { 0 };
    let slab_top = floors_done as f32 * fh;
    let concrete = mul(pick(rng, &pal.concrete), 0.85);
    let pale = mul(concrete, 1.15);
    let steel = mul(pal.steel, 0.9);

    // Bare earth across the lot, churned to mud by the tracks.
    ground(g, f, Dirt, &c.lot_poly, 0.02, pal.dirt);
    for _ in 0..rng.random_range(2..5) {
        if let Some(p) = c.random_inside(rng, 1.0) {
            let patch = fit_inside(c, &ngon(p.x, p.y, rng.random_range(2.0..4.5), 7, rng.random_range(0.0..1.0)));
            ground(g, f, Dirt, &patch, 0.04, pal.gravel);
        }
    }

    let front = site_perimeter(g, c, rng);

    // The raft, and the structure on it.
    let poly = &c.poly;
    if poly.len() >= 3 {
        extrude(g, f, Concrete, poly, 0.0, 0.35, mul(pal.concrete(0), 0.7));
        if !landscaping {
            structure(g, c, &mut sub(c, 1), poly, built, floors_done, fh, concrete, pale, steel);
            if progress > 0.02 && progress < 0.97 && built > 0.3 {
                scaffolding(g, c, &mut sub(c, 2), poly, (built + 2.5).min(height + 1.5), steel);
            }
            if c.floors >= 6 && progress > 0.06 && progress < 0.98 {
                tower_crane(g, c, &mut sub(c, 3), poly, built, height);
            }
        } else {
            // Landscaping: the raft is a plaza base; spoil heaps around it.
            for k in 0..3 {
                let rng = &mut sub(c, 10 + k);
                if let Some(p) = c.random_inside(rng, 1.5) {
                    let r = rng.random_range(1.5..3.0);
                    spoil_heap(g, c, rng, p, r);
                }
            }
        }
    }

    // Site clutter: everything stays off the footprint and inside the fence.
    let keep_out = offset_poly(poly, -1.2);
    let clear = |p: Vec2| c.inside_local(p) && distance_to_boundary(p, &c.lot_poly) > 1.4 && !(keep_out.len() >= 3 && point_in_polygon(p, &keep_out));
    let yard_front = front.out * -1.0;
    // Cabin and mixer by the gate.
    let cabin_at = front.at(-front.len * 0.32, -3.6);
    if clear(cabin_at) && clear(cabin_at + yard_front * 1.6) {
        site_cabin(g, c, &mut sub(c, 4), cabin_at, front.yaw());
    }
    let truck_at = front.at(front.len * 0.22, -5.2);
    if progress < 0.97 && clear(truck_at) && clear(truck_at + front.tangent * 3.5) && clear(truck_at - front.tangent * 3.5) {
        let rng = &mut sub(c, 5);
        let side = if rng.random_bool(0.5) { 1.0 } else { -1.0 };
        mixer_truck(g, c, rng, truck_at, front.yaw() + std::f32::consts::FRAC_PI_2 * side);
    }
    let loo = front.at(front.len * 0.42, -2.2);
    if clear(loo) {
        f.bx(g, Matte, loo.x, 1.15, loo.y, Vec3::new(1.1, 2.3, 1.1), [0.12, 0.30, 0.55]);
    }
    // Stacks of materials, fewer as they are built in.
    let stacks = (2.0 + 5.0 * (1.0 - progress)).round() as i32 + c.floors as i32 / 8;
    for k in 0..stacks.max(0) {
        let rng = &mut sub(c, 100 + k as u64);
        let Some(p) = c.random_inside(rng, 2.0) else { continue };
        if !clear(p) {
            continue;
        }
        let yaw = rng.random_range(-0.4..0.4);
        material_stack(g, c, rng, p, yaw);
    }
    if progress < 0.5 {
        for k in 0..sub(c, 6).random_range(1..3) {
            let rng = &mut sub(c, 200 + k as u64);
            if let Some(p) = c.random_inside(rng, 2.0) {
                if clear(p) {
                    let r = rng.random_range(1.2..2.4);
                    spoil_heap(g, c, rng, p, r);
                }
            }
        }
    }

    // The crew: on the ground around the yard, and up on the top slab.
    if (0.05..=0.95).contains(&progress) {
        let n = 3 + (c.floors as usize / 6).min(3);
        for k in 0..n {
            let rng = &mut sub(c, 300 + k as u64);
            if k % 2 == 0 && floors_done > 0 && poly.len() >= 3 {
                if let Some(p) = random_in_poly(rng, poly, 0.0) {
                    worker(g, c, rng, p, slab_top + 0.35);
                    continue;
                }
            }
            if let Some(p) = c.random_inside(rng, 1.5) {
                if clear(p) {
                    worker(g, c, rng, p, 0.04);
                }
            }
        }
    }
}

/// The building proper: solid shell up to the last complete floor, slabs
/// proud of it, and the floor under way as bare columns and rebar.
#[allow(clippy::too_many_arguments)]
fn structure(g: &mut Geometry, c: &Ctx, rng: &mut impl Rng, poly: &[Vec2], built: f32, floors_done: i32, fh: f32, concrete: [f32; 3], pale: [f32; 3], steel: [f32; 3]) {
    let f = &c.frame;
    let total = c.floors as i32;
    if built <= 0.35 {
        // Groundworks: starter bars and formwork lying about on the raft.
        for face in c.faces(poly) {
            if face.len < 2.0 {
                continue;
            }
            let n = (face.len / 1.6) as i32;
            for k in 0..n {
                let t = -face.len * 0.5 + (k as f32 + 0.5) * (face.len / n.max(1) as f32);
                face.bx(g, f, Rust, t, 0.35 + 0.45, -0.25, Vec3::new(0.04, 0.9, 0.04), c.palette.rust);
            }
        }
        return;
    }
    let slab_top = floors_done as f32 * fh;
    // Shell: finished floors, with dark openings where the windows will go.
    if slab_top > 0.4 {
        extrude(g, f, Concrete, poly, 0.35, slab_top, concrete);
        for face in c.faces(poly) {
            if face.len < 3.0 {
                continue;
            }
            let spacing = c.rules.window_spacing.max(2.0);
            let n = ((face.len - 1.2) / spacing).floor().max(1.0) as i32;
            let [ww, wh] = c.rules.window_size;
            for fl in 0..floors_done {
                let y = fl as f32 * fh + 0.35 + fh * 0.55;
                for k in 0..n {
                    let t = -face.len * 0.5 + 0.6 + spacing * 0.5 + k as f32 * spacing;
                    if t.abs() > face.len * 0.5 - ww * 0.6 {
                        continue;
                    }
                    face.decal(g, f, Matte, t, y, 0.02, ww, wh.min(fh - 0.8), DARK);
                }
            }
        }
    }
    // Slabs standing a little proud, one per finished floor.
    for k in 1..=floors_done {
        cornice(g, c, poly, k as f32 * fh + 0.35, 0.28, 0.18, Concrete, pale);
    }
    // The floor under way: columns up to the next slab, partially poured.
    if floors_done < total {
        let y0 = slab_top + 0.35;
        let part = ((built - slab_top) / fh).clamp(0.0, 1.0);
        let y1 = y0 + (fh * part).max(0.6);
        pilotis(g, c, poly, y0, y1, 4.0, 0.45, Concrete, pale);
        // Formwork and rebar on the columns still to be poured.
        for face in c.faces(poly) {
            if face.len < 3.0 {
                continue;
            }
            let n = (face.len / 4.0).floor().max(1.0) as i32;
            for k in 0..=n {
                let t = (-face.len * 0.5 + k as f32 * (face.len / n as f32)).clamp(-face.len * 0.5 + 0.5, face.len * 0.5 - 0.5);
                face.bx(g, f, Rust, t, y1 + 0.45, -0.27, Vec3::new(0.05, 0.9, 0.05), c.palette.rust);
                if rng.random_bool(0.4) {
                    face.bx(g, f, Wood, t, (y0 + y1) * 0.5, -0.27, Vec3::new(0.7, y1 - y0, 0.7), mul(c.palette.plywood, 0.9));
                }
            }
        }
        // A deck of formwork where the next slab goes, when the pour is near.
        if part > 0.7 {
            extrude(g, f, Wood, &offset_poly(poly, 0.1), y0 + fh - 0.1, y0 + fh + 0.05, mul(c.palette.plywood, 0.95));
            parapet(g, c, poly, y0 + fh + 0.05, 1.0, 0.06, Matte, steel);
        } else {
            // Edge protection on the top slab.
            parapet(g, c, poly, y0, 1.0, 0.06, Matte, steel);
        }
        // Props and loose kit on the slab.
        for _ in 0..rng.random_range(2..5) {
            if let Some(p) = random_in_poly(rng, poly, 0.8) {
                f.bx_yaw(g, Wood, p.x, y0 + 0.07, p.y, Vec3::new(1.2, 0.14, 1.0), c.palette.wood, rng.random_range(-0.5..0.5));
                f.bx(g, Concrete, p.x, y0 + 0.14 + 0.3, p.y, Vec3::new(1.0, 0.6, 0.8), pale);
            }
        }
    } else {
        // Topped out: roof slab, parapet edge still raw.
        extrude(g, f, Concrete, poly, slab_top + 0.35, slab_top + 0.6, pale);
        parapet(g, c, poly, slab_top + 0.6, 0.9, 0.25, Concrete, concrete);
    }
}

/// Tube-and-plank scaffold around the footprint, climbing with the work.
fn scaffolding(g: &mut Geometry, c: &Ctx, rng: &mut impl Rng, poly: &[Vec2], top: f32, steel: [f32; 3]) {
    let f = &c.frame;
    let lift = 2.0;
    let lifts = (top / lift).floor().max(1.0) as i32;
    // Decks, transoms and rails only on the lifts being worked; below them
    // the scaffold is bare tube, which is what a tall site looks like and
    // keeps a thirty-floor shell from costing more than the building.
    let worked = 6;
    let d_in = 0.35;
    let d_out = 1.25;
    for face in c.faces(poly) {
        // A stream per face, its netting drawn first: the lifts above
        // change with every floor, the sheeting on a side does not.
        let rng = &mut nd_sim::rng_for(rng.random::<u64>(), 0x5CAF);
        let net = rng.random_bool(0.55).then(|| pick(rng, &c.palette.tarp_colors));
        if face.len < 3.0 {
            continue;
        }
        // Skip a side where the scaffold would stand outside the fence.
        if !c.inside_local(face.at(-face.len * 0.5, d_out)) || !c.inside_local(face.at(face.len * 0.5, d_out)) || !c.inside_local(face.at(0.0, d_out)) {
            continue;
        }
        let bays = (face.len / 2.2).ceil().max(1.0) as i32;
        let step = face.len / bays as f32;
        for k in 0..=bays {
            let t = -face.len * 0.5 + k as f32 * step;
            for d in [d_in, d_out] {
                face.bx(g, f, Metal, t, top * 0.5, d, Vec3::new(0.09, top, 0.09), steel);
            }
            // Transoms between the two standards, every other bay, on the
            // working lifts.
            if k % 2 == 0 {
                for l in (lifts - worked + 1).max(1)..=lifts {
                    face.bx(g, f, Metal, t, l as f32 * lift, (d_in + d_out) * 0.5, Vec3::new(0.07, 0.07, d_out - d_in), steel);
                }
            }
        }
        for l in 1..=lifts {
            let y = l as f32 * lift;
            for d in [d_in, d_out] {
                face.bx(g, f, Metal, 0.0, y, d, Vec3::new(face.len, 0.07, 0.07), steel);
            }
            if l <= lifts - worked {
                continue; // bare tube below the working lifts
            }
            // The plank deck: one run per lift with one or two planks
            // missing, drawn as the runs between the gaps rather than a box
            // per plank (a tall site has hundreds of lifts of planking).
            let pw = step * 0.5;
            let gaps = rng.random_range(0..=2);
            let mut cuts: Vec<f32> = (0..gaps).map(|_| rng.random_range(-face.len * 0.45..face.len * 0.45)).collect();
            cuts.sort_by(|a, b| a.total_cmp(b));
            let mut start = -face.len * 0.5;
            for cut in cuts.iter().chain(std::iter::once(&(face.len * 0.5 + pw))) {
                let end = (cut - pw * 0.5).min(face.len * 0.5);
                if end - start > 0.3 {
                    face.bx(g, f, Wood, (start + end) * 0.5, y + 0.06, (d_in + d_out) * 0.5, Vec3::new(end - start, 0.05, d_out - d_in), mul(c.palette.wood, rng.random_range(0.85..1.1)));
                }
                start = cut + pw * 0.5;
            }
            face.bx(g, f, Metal, 0.0, y + 1.0, d_out, Vec3::new(face.len, 0.06, 0.06), steel);
            face.bx(g, f, Wood, 0.0, y + 0.18, d_out - 0.04, Vec3::new(face.len, 0.2, 0.03), mul(c.palette.wood, 0.9));
        }
        // Diagonal brace, and a sheet of netting on some faces.
        let a = f.at2(face.at(-face.len * 0.5, d_out), 0.1);
        let b = f.at2(face.at((face.len * 0.5).min(-face.len * 0.5 + lift * 2.5), d_out), top.min(lift * 2.5));
        g.part(Metal).tube(a, b, 0.04, 5, rgba(steel));
        if let Some(tarp) = net {
            face.bx(g, f, Tarp, 0.0, (top + 0.25) * 0.5, d_out + 0.08, Vec3::new(face.len, top - 0.25, 0.02), tarp);
        }
    }
}

/// [fix/buildings-ui] A demolition's siding: debris netting hung on a
/// light scaffold (standards every few metres, a ledger every three lifts)
/// round the footprint up to `top`, a side left open where the machine
/// works (`open`, its index in `c.faces(poly)`) and any side whose scaffold
/// would stand outside the fence. Far lighter than a building site's
/// scaffold: a tall tower coming down is mostly sheeting.
fn siding(g: &mut Geometry, c: &Ctx, poly: &[Vec2], top: f32, steel: [f32; 3], open: Option<usize>) {
    let f = &c.frame;
    let d = 1.0;
    let rng = &mut sub(c, 0x51D);
    for (fi, face) in c.faces(poly).into_iter().enumerate() {
        let net = if rng.random_bool(0.5) { DEBRIS_NET } else { pick(rng, &c.palette.tarp_colors) };
        if face.len < 3.0 || open == Some(fi) {
            continue;
        }
        if !c.inside_local(face.at(-face.len * 0.5, d + 0.2)) || !c.inside_local(face.at(face.len * 0.5, d + 0.2)) || !c.inside_local(face.at(0.0, d + 0.2)) {
            continue;
        }
        let bays = (face.len / 4.0).ceil().max(1.0) as i32;
        for k in 0..=bays {
            let t = -face.len * 0.5 + k as f32 * face.len / bays as f32;
            face.bx(g, f, Metal, t, top * 0.5, d - 0.1, Vec3::new(0.1, top, 0.1), steel);
        }
        let mut y = 6.0;
        while y < top {
            face.bx(g, f, Metal, 0.0, y, d - 0.1, Vec3::new(face.len, 0.08, 0.08), steel);
            y += 6.0;
        }
        face.bx(g, f, Tarp, 0.0, (top + 0.25) * 0.5, d + 0.02, Vec3::new(face.len, top - 0.25, 0.02), net);
    }
}

/// Paints tower cranes come in: the orange and red of the big rental
/// fleets, the classic yellow, and white. Picked per site from its seed.
const CRANE_PAINTS: [[f32; 3]; 4] = [[0.95, 0.36, 0.05], [0.72, 0.07, 0.05], [0.92, 0.70, 0.08], [0.82, 0.83, 0.80]];
/// Share of cranes carrying an LED billboard on the counter-jib.
const CRANE_BILLBOARD_CHANCE: f64 = 0.45;
/// Ad looks for the counter-jib billboards: background, lettering, logo.
/// Glow colours, so above 1 for the bright parts.
const CRANE_ADS: [([f32; 3], [f32; 3], [f32; 3]); 5] = [
    ([1.6, 0.10, 0.85], [2.2, 2.2, 2.2], [0.2, 1.6, 2.0]),
    ([0.10, 1.10, 1.70], [0.05, 0.05, 0.12], [2.2, 1.5, 0.2]),
    ([1.9, 0.55, 0.05], [2.3, 2.2, 2.0], [0.10, 0.10, 0.12]),
    ([0.08, 0.12, 0.85], [2.2, 1.9, 0.3], [2.0, 0.2, 0.4]),
    ([0.05, 0.05, 0.06], [0.3, 2.0, 0.9], [2.0, 2.0, 2.0]),
];

/// The paint of the tower crane on a site with this seed.
pub fn crane_paint(seed: u64) -> [f32; 3] {
    let mut rng = nd_sim::rng_for(seed, 91);
    let r: f32 = rng.random_range(0.0..1.0);
    // Orange and red are what most of the skyline flies; yellow and white less.
    let i = if r < 0.35 { 0 } else if r < 0.6 { 1 } else if r < 0.85 { 2 } else { 3 };
    CRANE_PAINTS[i]
}

/// The slewing head of a tower crane, handed to the renderer instead of
/// being baked into the site's mesh (see `Ctx::animated_cranes`) so it can turn:
/// the jib, counter-jib with its ballast and billboard, cab, tower top,
/// trolley and hook, drawn about the mast's axis with the jib along +x.
#[derive(Clone, Debug, PartialEq)]
pub struct CraneRig {
    /// Where the mast stands, world x/z.
    pub pos: Vec2,
    /// World heading of the jib as drawn (radians from +x towards +z): the
    /// rest heading the jib swings about.
    pub yaw: f32,
    /// The head, in mast coordinates; heights are above the lot pad.
    pub head: Geometry,
    /// The site's seed: the rhythm of its lifts.
    pub seed: u64,
}

/// Lift cycle of a working crane, in working minutes (`SimClock::work_minutes`).
pub const CRANE_LIFT_MINUTES: u64 = 12;
/// Furthest a crane swings from its rest heading, radians.
pub const CRANE_SLEW_RANGE: f32 = 1.2;

/// Where a crane's jib is aimed, relative to its rest heading, after
/// `work_minutes` of working time: a new heading for every lift, picked
/// from the site's seed and the lift's number, so every viewer sees the
/// same crane do the same thing, and nothing moves while work is off. The
/// renderer eases the jib towards it at a slow, capped rate.
pub fn crane_slew(seed: u64, work_minutes: u64) -> f32 {
    let lift = work_minutes / CRANE_LIFT_MINUTES;
    let mut rng = nd_sim::rng_for(seed ^ lift.wrapping_mul(0x9E37_79B9_7F4A_7C15), 92);
    // Mostly working over the slab, now and then a long swing to the
    // stacks by the gate.
    if rng.random_bool(0.2) {
        rng.random_range(-CRANE_SLEW_RANGE..CRANE_SLEW_RANGE)
    } else {
        rng.random_range(-0.45..0.45)
    }
}

/// Turn every vertex of `head` about the vertical through `pivot` by
/// `-yaw`, moving `pivot` to the origin: world to mast coordinates.
fn to_mast_frame(head: &mut Geometry, pivot: Vec2, yaw: f32) {
    let (s, c) = (-yaw).sin_cos();
    let rot = |x: f32, z: f32| (x * c - z * s, x * s + z * c);
    for surface in crate::surface::Surface::ALL {
        let m = head.part(surface);
        for p in &mut m.positions {
            let (x, z) = rot(p[0] - pivot.x, p[2] - pivot.y);
            p[0] = x;
            p[2] = z;
        }
        for n in &mut m.normals {
            let (x, z) = rot(n[0], n[2]);
            n[0] = x;
            n[2] = z;
        }
        for t in &mut m.tangents {
            let (x, z) = rot(t[0], t[2]);
            t[0] = x;
            t[2] = z;
        }
    }
}

/// An LED billboard standing on the counter-jib by the ballast, lit on
/// both faces: a bright ad (background, two lines of lettering and a logo)
/// in a dark frame. `centre` and `along` are world (along the counter-jib).
fn crane_billboard(g: &mut Geometry, rng: &mut impl Rng, centre: Vec3, along: Vec3, w: f32, h: f32) {
    let across = Vec3::new(-along.z, 0.0, along.x);
    let (bg, ink, logo) = CRANE_ADS[rng.random_range(0..CRANE_ADS.len())];
    // Frame and the two posts it stands on.
    g.part(Matte).boxed(centre, Vec3::new(w + 0.3, h + 0.3, 0.3), along.z.atan2(along.x), rgba(DARK));
    for t in [-0.35f32, 0.35] {
        let p = centre + along * (t * w) - Vec3::Y * (h * 0.5 + 0.4);
        g.part(Matte).boxed(p, Vec3::new(0.2, 0.8, 0.2), 0.0, rgba(DARK));
    }
    let lines = [rng.random_range(0.45f32..0.62), rng.random_range(0.3f32..0.5)];
    for side in [-1.0f32, 1.0] {
        let facing = across * side;
        let face = |u: f32, v: f32| centre + facing * 0.17 + along * u + Vec3::Y * v;
        // Mirrored on the far face, so the copy reads left to right there too.
        let a = along * side;
        g.part(Glow).decal(face(0.0, 0.0), a, Vec3::Y, w * 0.96, h * 0.92, facing, rgba(bg));
        let lift = facing * 0.02;
        for (k, len) in lines.iter().enumerate() {
            let lw = w * len;
            let u = (-w * 0.42 + lw * 0.5) * side;
            g.part(Glow).decal(face(u, h * (0.16 - 0.3 * k as f32)) + lift, a, Vec3::Y, lw, h * 0.16, facing, rgba(ink));
        }
        g.part(Glow).decal(face(w * 0.3 * side, 0.0) + lift, a, Vec3::Y, h * 0.55, h * 0.55, facing, rgba(logo));
    }
}

/// A tower crane standing clear of the footprint, jib reaching over it.
/// The mast is drawn into `g`; the head that slews goes to the renderer
/// in `Geometry::cranes` when it animates them ([`Ctx::animated_cranes`]),
/// into `g` otherwise.
fn tower_crane(g: &mut Geometry, c: &Ctx, rng: &mut impl Rng, poly: &[Vec2], built: f32, height: f32) {
    let f = &c.frame;
    let paint = crane_paint(c.seed);
    let centre = poly_center(poly);
    // Stand by the footprint on the side with the most room.
    let mut best: Option<(f32, Vec2)> = None;
    for face in c.faces(poly) {
        let p = face.at(0.0, 2.6);
        if !c.inside_local(p) {
            continue;
        }
        let room = distance_to_boundary(p, &c.lot_poly);
        if room > 1.2 && best.map_or(true, |(r, _)| room > r) {
            best = Some((room, p));
        }
    }
    // No room beside it: the crane climbs inside the core.
    let base = best.map(|(_, p)| p).unwrap_or(centre);
    let mast_h = (built + 14.0).min(height + 16.0).max(18.0);
    let w = 1.5;
    // Lattice mast: corner legs and a lighter core.
    for (du, dv) in [(-1.0f32, -1.0f32), (1.0, -1.0), (1.0, 1.0), (-1.0, 1.0)] {
        f.bx(g, Matte, base.x + du * w * 0.45, mast_h * 0.5, base.y + dv * w * 0.45, Vec3::new(0.14, mast_h, 0.14), paint);
    }
    let bands = (mast_h / 2.5) as i32;
    for k in 0..=bands {
        let y = k as f32 * 2.5;
        f.bx(g, Matte, base.x, y, base.y, Vec3::new(w, 0.08, 0.08), paint);
        f.bx(g, Matte, base.x, y, base.y, Vec3::new(0.08, 0.08, w), paint);
    }
    f.bx(g, Matte, base.x, mast_h * 0.5, base.y, Vec3::new(w * 0.5, mast_h, w * 0.5), mul(paint, 0.8));
    // Foundation block and ballast.
    f.bx(g, Concrete, base.x, 0.4, base.y, Vec3::new(w + 2.0, 0.8, w + 2.0), mul(c.palette.concrete(0), 0.75));
    // Everything from the slewing unit up turns with the jib.
    let mut head = Geometry::default();
    let h = &mut head;
    let top = mast_h + 0.6;
    f.bx(h, Matte, base.x, top, base.y, Vec3::new(w + 0.4, 1.2, w + 0.4), mul(paint, 0.9));
    let dir = {
        let d = centre - base;
        if d.length() > 0.5 { d.normalize() } else { Vec2::new(rng.random_range(-1.0..1.0), rng.random_range(-1.0..1.0)).normalize_or(Vec2::X) }
    };
    let far = poly.iter().map(|p| (*p - base).dot(dir)).fold(0.0, f32::max) + 4.0;
    // Reach over the footprint, but never past the fence.
    let reach = |d: Vec2, want: f32, least: f32| {
        let mut len = want;
        while len > least && !c.inside_local(base + d * len) {
            len -= 0.5;
        }
        len.max(least)
    };
    let jib_len = reach(dir, far.min(40.0), 3.0);
    let tip = base + dir * jib_len;
    let angle = dir.y.atan2(dir.x);
    let jib_y = top + 1.4;
    let jib_mid = base + dir * jib_len * 0.5;
    f.bx_yaw(h, Matte, jib_mid.x, jib_y, jib_mid.y, Vec3::new(jib_len, 0.9, 0.9), paint, angle);
    f.bx_yaw(h, Matte, jib_mid.x, jib_y + 0.6, jib_mid.y, Vec3::new(jib_len * 0.98, 0.1, 0.1), mul(paint, 0.8), angle);
    // Counter-jib with ballast blocks. A crane the renderer slews swings it
    // over the street anyway, so there it is not cut short at the fence: a
    // real counter-jib is about a third of the jib, long enough for the
    // ballast (and, on some, a billboard). A crane drawn standing still
    // keeps it inside the fence.
    let back_len = if c.animated_cranes { (jib_len * 0.35).clamp(5.0, 12.0) } else { reach(-dir, jib_len * 0.4, 2.0) };
    let back_mid = base - dir * back_len * 0.5;
    f.bx_yaw(h, Matte, back_mid.x, jib_y, back_mid.y, Vec3::new(back_len, 0.8, 1.0), paint, angle);
    let ballast = base - dir * (back_len - 1.0);
    f.bx_yaw(h, Concrete, ballast.x, jib_y - 0.9, ballast.y, Vec3::new(1.8, 1.4, 1.6), mul(c.palette.concrete(0), 0.8), angle);
    // Cab beside the mast, facing the jib.
    let cab = base + dir * (w * 0.5 + 1.0);
    f.bx_yaw(h, Matte, cab.x, top + 0.2, cab.y, Vec3::new(1.8, 1.9, 1.6), mul(paint, 0.95), angle);
    f.bx_yaw(h, Glass, cab.x + dir.x * 0.5, top + 0.4, cab.y + dir.y * 0.5, Vec3::new(1.0, 1.1, 1.7), c.palette.glass_dark, angle);
    // Tower top and pendant lines.
    f.bx(h, Matte, base.x, jib_y + 3.0, base.y, Vec3::new(0.4, 5.0, 0.4), paint);
    let apex = f.at2(base, jib_y + 5.5);
    h.part(Matte).tube(apex, f.at2(base + dir * jib_len * 0.9, jib_y + 0.5), 0.03, 4, rgba(DARK));
    h.part(Matte).tube(apex, f.at2(base - dir * back_len * 0.9, jib_y + 0.4), 0.03, 4, rgba(DARK));
    // Trolley, hoist rope and hook with a load swinging over the slab.
    let t = rng.random_range(0.45..0.85);
    let trolley = base + dir * jib_len * t;
    f.bx_yaw(h, Matte, trolley.x, jib_y - 0.6, trolley.y, Vec3::new(1.2, 0.4, 1.1), DARK, angle);
    let hook_y = (built + 4.0 + rng.random_range(0.0..5.0)).min(jib_y - 2.0);
    let rope = f.at2(trolley, jib_y - 0.8);
    h.part(Matte).tube(rope, f.at2(trolley, hook_y), 0.025, 4, rgba(DARK));
    f.bx(h, Matte, trolley.x, hook_y - 0.3, trolley.y, Vec3::new(0.5, 0.7, 0.3), DARK);
    if rng.random_bool(0.7) {
        let load = hook_y - 1.6;
        // A swinging steel bundle is 3.6 m long: only where it clears the fence.
        if rng.random_bool(0.5) && poly_inside(c, &rect_yaw(trolley.x, trolley.y, 1.9, 0.6, angle + 0.4)) {
            f.bx_yaw(h, Metal, trolley.x, load, trolley.y, Vec3::new(3.6, 0.5, 1.0), mul(c.palette.steel, 0.8), angle + 0.4);
        } else {
            f.bx_yaw(h, Concrete, trolley.x, load, trolley.y, Vec3::new(1.4, 1.2, 1.2), mul(c.palette.concrete(0), 0.9), angle);
        }
    }
    // Aviation lights.
    aviation_light_local(h, f, c.palette, base.x, jib_y + 5.8, base.y, 0.3);
    aviation_light_local(h, f, c.palette, tip.x, jib_y + 0.6, tip.y, 0.3);
    // Some carry an LED billboard on the counter-jib, by the counterweight:
    // its own random stream, so the rest of the site is drawn as before.
    let mut ad = nd_sim::rng_for(c.seed, 93);
    if back_len >= 4.0 && ad.random_bool(CRANE_BILLBOARD_CHANCE) {
        let bw = (back_len * 0.8).min(7.0);
        let bh = (bw * 0.42).clamp(1.6, 2.8);
        let mid = base - dir * (back_len - 1.2 - bw * 0.5).max(bw * 0.5 + 0.6);
        crane_billboard(h, &mut ad, f.at2(mid, jib_y + 0.4 + 0.45 + bh * 0.5 + 0.35), f.dir3(-dir), bw, bh);
    }
    if c.animated_cranes {
        let pivot = f.to_world(base.x, base.y);
        let d3 = f.dir3(dir);
        let yaw = d3.z.atan2(d3.x);
        to_mast_frame(&mut head, pivot, yaw);
        g.cranes.push(CraneRig { pos: pivot, yaw, head, seed: c.seed });
    } else {
        g.append(&head);
    }
}

/// Portable site office: a container-like cabin with a lit window and steps.
fn site_cabin(g: &mut Geometry, c: &Ctx, rng: &mut impl Rng, p: Vec2, yaw: f32) {
    let f = &c.frame;
    let col = pick(rng, &[[0.78, 0.78, 0.76], [0.55, 0.62, 0.68], [0.70, 0.66, 0.58]]);
    container_unit(g, c, rng, p.x, p.y, yaw, 6.0, 2.5, 2.6, 0.25, col);
    // Blocks under it, a lit window on the long side, a door and steps.
    let (s, co) = yaw.sin_cos();
    let at = |lu: f32, lv: f32| Vec2::new(p.x + lu * co - lv * s, p.y + lu * s + lv * co);
    for lu in [-2.6f32, 2.6] {
        let q = at(lu, 0.0);
        f.bx_yaw(g, Concrete, q.x, 0.12, q.y, Vec3::new(0.6, 0.25, 2.4), mul(c.palette.concrete(0), 0.7), yaw);
    }
    let win = at(0.8, -1.27);
    f.bx_yaw(g, Glow, win.x, 1.7, win.y, Vec3::new(1.6, 0.9, 0.03), mul(c.palette.window_warm, 0.9), yaw);
    let door = at(-1.8, -1.27);
    f.bx_yaw(g, Matte, door.x, 1.25, door.y, Vec3::new(0.9, 2.0, 0.04), mul(col, 0.6), yaw);
    let step = at(-1.8, -1.75);
    f.bx_yaw(g, Metal, step.x, 0.12, step.y, Vec3::new(1.0, 0.24, 0.8), mul(c.palette.steel, 0.7), yaw);
    // Aircon and a lamp over the door.
    let ac = at(2.0, 1.3);
    f.bx_yaw(g, Matte, ac.x, 2.0, ac.y, Vec3::new(0.7, 0.5, 0.3), [0.7, 0.7, 0.68], yaw);
    f.bx_yaw(g, Glow, door.x, 2.5, door.y, Vec3::new(0.3, 0.1, 0.1), c.palette.street_lamp, yaw);
}

/// A truck with a mixer drum on the back.
fn mixer_truck(g: &mut Geometry, c: &Ctx, rng: &mut impl Rng, p: Vec2, yaw: f32) {
    let f = &c.frame;
    let col = pick(rng, &[[0.85, 0.85, 0.82], [0.75, 0.25, 0.1], [0.2, 0.3, 0.45]]);
    vehicle(g, f, c.palette, rng, p, 0.0, yaw, Vehicle::Truck, col);
    // Drum: a tilted tube over the body, hopper at the back.
    let w = f.to_world(p.x, p.y);
    let a = f.yaw() + yaw;
    let (sn, cs) = a.sin_cos();
    let at = |dx: f32, y: f32, dz: f32| Vec3::new(w.x + dx * cs - dz * sn, y, w.y + dx * sn + dz * cs);
    let drum = pick(rng, &[[0.9, 0.55, 0.1], [0.8, 0.8, 0.78], [0.3, 0.55, 0.3]]);
    g.part(Metal).tube(at(-3.6, 3.9, 0.0), at(0.6, 3.0, 0.0), 1.1, 10, rgba(drum));
    g.part(Metal).tube(at(-3.6, 3.9, 0.0), at(-4.4, 4.1, 0.0), 0.6, 8, rgba(mul(drum, 0.8)));
    g.part(Matte).boxed(at(-4.6, 4.3, 0.0), Vec3::new(0.8, 0.9, 1.0), a, rgba(DARK));
    g.part(Matte).boxed(at(-4.4, 2.3, 0.0), Vec3::new(0.3, 1.6, 0.3), a, rgba(mul(c.palette.steel, 0.6)));
    // Wet concrete splashed down the chute.
    g.part(Concrete).boxed(at(-4.9, 0.3, 0.6), Vec3::new(1.2, 0.08, 1.0), a + 0.3, rgba(mul(c.palette.concrete(0), 0.6)));
}

/// Pallet of blocks, bundle of steel, stack of pipe or drums.
fn material_stack(g: &mut Geometry, c: &Ctx, rng: &mut impl Rng, p: Vec2, yaw: f32) {
    let f = &c.frame;
    let pal = c.palette;
    match rng.random_range(0..5) {
        0 | 1 => {
            // Pallet of blocks under a tarp sometimes.
            f.bx_yaw(g, Wood, p.x, 0.08, p.y, Vec3::new(1.2, 0.14, 1.0), pal.wood, yaw);
            let layers = rng.random_range(2..5);
            for k in 0..layers {
                f.bx_yaw(g, Concrete, p.x, 0.15 + 0.25 + k as f32 * 0.5, p.y, Vec3::new(1.1, 0.5, 0.9), mul(pick(rng, &pal.concrete), 0.95), yaw);
            }
            if rng.random_bool(0.4) {
                f.bx_yaw(g, Tarp, p.x, 0.15 + layers as f32 * 0.5 + 0.03, p.y, Vec3::new(1.25, 0.06, 1.05), pick(rng, &pal.tarp_colors), yaw);
            }
        }
        2 => {
            // Steel bundle on bearers.
            for lv in [-0.4f32, 0.4] {
                let (s, co) = yaw.sin_cos();
                let q = Vec2::new(p.x - lv * s, p.y + lv * co);
                f.bx_yaw(g, Wood, q.x, 0.1, q.y, Vec3::new(0.15, 0.2, 1.2), pal.wood, yaw);
            }
            f.bx_yaw(g, Metal, p.x, 0.35, p.y, Vec3::new(6.0, 0.3, 0.9), mul(pal.steel, 0.9), yaw);
            f.bx_yaw(g, Metal, p.x, 0.62, p.y, Vec3::new(6.0, 0.24, 0.6), mul(pal.steel, 0.8), yaw);
        }
        3 => {
            // Rebar: long rusty rods.
            f.bx_yaw(g, Rust, p.x, 0.15, p.y, Vec3::new(5.0, 0.3, 0.7), c.palette.rust, yaw);
            f.bx_yaw(g, Rust, p.x, 0.38, p.y, Vec3::new(5.0, 0.18, 0.45), mul(c.palette.rust, 1.1), yaw);
        }
        _ => {
            // Drums and a gas bottle cage.
            for k in 0..rng.random_range(2..5) {
                let q = p + Vec2::new((k % 2) as f32 * 0.7, (k / 2) as f32 * 0.7);
                let w = f.to_world(q.x, q.y);
                g.part(Metal).cylinder(w, 0.3, 0.3, 0.0, 0.9, 10, rgba(pick(rng, &[[0.2, 0.3, 0.6], [0.7, 0.1, 0.1], c.palette.rust, [0.8, 0.8, 0.75]])));
            }
            f.bx_yaw(g, Matte, p.x + 1.4, 0.7, p.y, Vec3::new(1.0, 1.4, 0.8), mul(pal.steel, 0.6), yaw);
        }
    }
}

/// Heap of spoil or sand.
fn spoil_heap(g: &mut Geometry, c: &Ctx, rng: &mut impl Rng, p: Vec2, r: f32) {
    let f = &c.frame;
    let rot = rng.random_range(0.0..1.0);
    let bottom = fit_inside(c, &ngon(p.x, p.y, r, 8, rot));
    let top = ngon(poly_center(&bottom).x, poly_center(&bottom).y, r * 0.25, 8, rot);
    let col = if rng.random_bool(0.5) { c.palette.dirt } else { mix(c.palette.gravel, [0.6, 0.5, 0.35], 0.5) };
    loft(g, f, Dirt, &bottom, &top, 0.0, r * 0.55, mul(col, 0.9));
}

/// A labourer: hi-vis body, head, helmet.
fn worker(g: &mut Geometry, c: &Ctx, rng: &mut impl Rng, p: Vec2, y: f32) {
    let f = &c.frame;
    let yaw = rng.random_range(0.0..std::f32::consts::TAU);
    f.bx_yaw(g, Matte, p.x, y + 0.45, p.y, Vec3::new(0.3, 0.9, 0.42), [0.18, 0.18, 0.22], yaw);
    f.bx_yaw(g, Matte, p.x, y + 1.15, p.y, Vec3::new(0.34, 0.5, 0.5), c.palette.hi_vis, yaw);
    f.bx_yaw(g, Matte, p.x, y + 1.52, p.y, Vec3::new(0.24, 0.24, 0.24), c.palette.citizen, yaw);
    f.bx_yaw(g, Matte, p.x, y + 1.7, p.y, Vec3::new(0.3, 0.14, 0.32), c.palette.helmet, yaw);
}

/// [fix/buildings-ui] Netting for a demolition's scaffold when the face's
/// own pick drew none: the dull green of debris mesh.
const DEBRIS_NET: [f32; 3] = [0.16, 0.30, 0.20];

/// [fix/buildings-ui] Paint of the X on a condemned roof.
const CONDEMNED_X: [f32; 3] = [0.85, 0.08, 0.06];

/// The top of a building's mass as drawn (walls, glass and roofs; not
/// masts, signs or lights).
pub fn mass_top(g: &Geometry) -> f32 {
    [Concrete, Plaster, Brick, Stone, Panel, Glass, WornConcrete, WornPlaster, WornBrick, WornPanel]
        .iter()
        .filter_map(|s| g.parts.get(s.index()))
        .flat_map(|m| m.positions.iter().map(|p| p[1]))
        .fold(0.0, f32::max)
}

/// The main roof of a building as drawn: the highest level of flat mass
/// surface that covers a fair share of the whole roofed area (a parapet's
/// cap or a plant room does not), and the world (x, z) corners on it.
pub fn roof_plane(g: &Geometry) -> Option<(f32, Vec<Vec2>)> {
    use std::collections::BTreeMap;
    const BUCKET: f32 = 0.25;
    let mut levels: BTreeMap<i32, (f32, Vec<Vec2>)> = BTreeMap::new();
    for s in [Concrete, Plaster, Brick, Stone, Panel, Glass, Metal, WornConcrete, WornPlaster, WornBrick, WornPanel, WornMetal] {
        let Some(m) = g.parts.get(s.index()) else { continue };
        for t in m.indices.chunks_exact(3) {
            let [a, b, c] = [t[0], t[1], t[2]].map(|i| Vec3::from(m.positions[i as usize]));
            let n = (b - a).cross(c - a);
            let area = n.length() * 0.5;
            if area < 1e-3 || n.normalize().y.abs() < 0.95 || (a.y - b.y).abs() > 0.05 || (a.y - c.y).abs() > 0.05 {
                continue;
            }
            let e = levels.entry((a.y / BUCKET).round() as i32).or_default();
            e.0 += area;
            e.1.extend([a, b, c].map(|p| Vec2::new(p.x, p.z)));
        }
    }
    let total: f32 = levels.values().map(|l| l.0).sum::<f32>();
    let biggest = levels.values().map(|l| l.0).fold(0.0, f32::max);
    if total <= 0.0 {
        return None;
    }
    let (k, (_, pts)) = levels.into_iter().rev().find(|(k, l)| *k > 4 && l.0 >= biggest * 0.3)?;
    Some((k as f32 * BUCKET, pts))
}

/// Red X's painted over a roof outline (lot-local) at height `y`: one per
/// square of about 14 m that fits, or one smaller over the middle.
fn roof_crosses(g: &mut Geometry, c: &Ctx, outline: &[Vec2], y: f32) {
    if outline.len() < 3 {
        return;
    }
    let f = &c.frame;
    // Inside the roof's outline (a hull) and the footprint itself, which
    // an L-shaped building's hull overreaches.
    let on_roof = |q: Vec2| point_in_polygon(q, outline) && (c.poly.len() < 3 || point_in_polygon(q, &c.poly)) && c.inside_local(q);
    let fits = |p: Vec2, r: f32| on_roof(p) && (0..4).all(|k| {
        let a = std::f32::consts::FRAC_PI_4 + k as f32 * std::f32::consts::FRAC_PI_2;
        on_roof(p + Vec2::new(a.cos(), a.sin()) * r * 1.45)
    });
    let mut x = |p: Vec2, r: f32| {
        for a in [std::f32::consts::FRAC_PI_4, -std::f32::consts::FRAC_PI_4] {
            f.bx_yaw(g, Matte, p.x, y + 0.06, p.y, Vec3::new(r * 2.6, 0.06, (r * 0.22).clamp(0.5, 1.4)), CONDEMNED_X, a);
        }
    };
    let (lo, hi) = bounds(outline);
    let cell = 14.0;
    let r = cell * 0.32;
    let (nx, ny) = (((hi.x - lo.x) / cell).floor() as i32, ((hi.y - lo.y) / cell).floor() as i32);
    let mut drawn = 0;
    if nx >= 2 || ny >= 2 {
        let start = lo + Vec2::new(((hi.x - lo.x) - nx.max(1) as f32 * cell) * 0.5, ((hi.y - lo.y) - ny.max(1) as f32 * cell) * 0.5);
        for i in 0..nx.max(1) {
            for j in 0..ny.max(1) {
                let p = start + Vec2::new((i as f32 + 0.5) * cell, (j as f32 + 0.5) * cell);
                if drawn < 12 && fits(p, r) {
                    x(p, r);
                    drawn += 1;
                }
            }
        }
    }
    if drawn == 0 {
        let p = poly_center(outline);
        let mut r = (inradius(outline) * 0.62).min(6.0);
        while r >= 1.2 && !fits(p, r) {
            r *= 0.8;
        }
        if r >= 1.2 {
            x(p, r);
        }
    }
}

/// [fix/buildings-ui] A building with its power off for good (stripped out
/// for demolition): its window lights go, leaving the dark glass behind
/// them, and its signs, neon and lamps are dead fittings, drawn matte.
pub fn lights_out(g: &mut Geometry) {
    let glow = std::mem::take(g.part(Glow));
    let mut dead = crate::mesh::MeshData::default();
    for t in glow.indices.chunks_exact(3) {
        if t.iter().all(|&i| glow.colors[i as usize][3] >= WINDOW_LIGHT_MARK) {
            continue;
        }
        for &i in t {
            let i = i as usize;
            let [r, gr, b, _] = glow.colors[i];
            let dim = |v: f32| (v * 0.25).min(0.35);
            dead.positions.push(glow.positions[i]);
            dead.normals.push(glow.normals[i]);
            dead.colors.push([dim(r), dim(gr), dim(b), 1.0]);
            dead.uvs.push(glow.uvs[i]);
            dead.tangents.push(glow.tangents[i]);
            dead.indices.push(dead.positions.len() as u32 - 1);
        }
    }
    g.part(Matte).append(&dead);
}

// ── demolition ──────────────────────────────────────────────────────────

/// Share of the job spent on the structure itself; the rest is the last of
/// the rubble and the slab being broken out.
const WRECK_END: f32 = 0.8;
/// Share spent putting the hoarding up and stripping out before the
/// machines start on the structure.
const STRIP_OUT: f32 = 0.08;
/// Above this the excavator cannot reach the cut: a small machine works on
/// the top slab and a tower crane lowers it down.
const HIGH_REACH: f32 = 26.0;

/// A demolition site at `progress` in `0..1`. `g` holds the building as it
/// stood: it is cut down from the top as the work goes on (triangles above
/// the working level are dropped, those across it are clipped), and the
/// site is drawn around what is left: hoarding with a gate, the broken top
/// slab with rebar, a high-reach excavator (or a small one on the roof and
/// a tower crane for tall jobs), rubble heaps, a dump truck being loaded
/// and the crew. After the structure is down the heaps are cleared until
/// bare earth is left.
/// A site's perimeter: plywood hoarding round the lot with a gate on the
/// street side, a sign board on it and floodlights on two corners. The
/// street face, for what stands by the gate.
fn site_perimeter(g: &mut Geometry, c: &Ctx, rng: &mut impl Rng) -> Face {
    let hoard = {
        let h = offset_poly(&c.lot_poly, 0.3);
        if h.len() >= 3 { h } else { c.lot_poly.clone() }
    };
    let faces = c.faces(&hoard);
    let front_i = faces
        .iter()
        .enumerate()
        .max_by(|(_, a), (_, b)| (a.out.dot(Vec2::new(0.0, -1.0)) * a.len).total_cmp(&(b.out.dot(Vec2::new(0.0, -1.0)) * b.len)))
        .map(|(i, _)| i)
        .unwrap_or(0);
    for (i, face) in faces.iter().enumerate() {
        hoarding(g, c, rng, face, if i == front_i { Some((face.len * 0.5).min(6.0)) } else { None });
    }
    let front = faces[front_i];
    if front.len > 8.0 {
        sign_board(g, c, rng, &front, -front.len * 0.3, HOARDING_H * 0.55, (front.len * 0.25).min(4.0), 1.1);
    }
    // Floodlights on two corners.
    for k in [0usize, hoard.len() / 2] {
        let corner = hoard[k % hoard.len()];
        let p = corner.lerp(poly_center(&hoard), 0.08);
        if c.inside_local(p) {
            floodlight(g, c, p, 6.5);
        }
    }
    front
}

pub fn demolition_site(g: &mut Geometry, c: &Ctx, rng: &mut impl Rng, progress: f32) {
    let f = &c.frame;
    let pal = c.palette;
    let s = progress.clamp(0.0, 1.0);
    let top = g.parts.iter().flat_map(|m| m.positions.iter().map(|p| p[1])).fold(0.0f32, f32::max);
    let wreck = ((s - STRIP_OUT) / (WRECK_END - STRIP_OUT)).clamp(0.0, 1.0);
    let cut = top * (1.0 - wreck);
    let cut_points = if wreck > 0.0 { clip_above(g, cut) } else { Vec::new() };
    let concrete = mul(pick(rng, &pal.concrete), 0.8);
    let poly = &c.poly;

    if s >= WRECK_END {
        // The structure is down: broken ground where it stood.
        ground(g, f, Dirt, &c.lot_poly, 0.02, pal.dirt);
    }

    let front = site_perimeter(g, c, rng);

    // The broken top: a raw slab over what the cut left, rubble on it and
    // rebar sticking up round its edge.
    let hull: Vec<Vec2> = convex_hull(&cut_points.iter().map(|p| f.to_local(*p)).collect::<Vec<_>>());
    // A sliver of a hull (a cut through one thin wall) offsets into a
    // bow-tie whose cap folds over: no slab on it.
    let hull = if hull.len() >= 3 && polygon_area(&hull).abs() > 6.0 && inradius(&hull) > 0.8 && poly_inside(c, &hull) { hull } else { Vec::new() };
    broken_top(g, c, &hull, cut, concrete);

    // [fix/buildings-ui] Condemned: a big X on the roof (on the broken top
    // once the machines are at it), read from the air.
    if s < WRECK_END {
        let roof = if wreck > 0.0 { (!hull.is_empty()).then(|| (cut, hull.clone())) } else { roof_plane(g).map(|(y, pts)| (y, convex_hull(&pts.iter().map(|p| f.to_local(*p)).collect::<Vec<_>>()))) };
        if let Some((y, outline)) = roof {
            roof_crosses(g, c, &outline, y);
        }
    }

    // Site clutter stays inside the fence and, while the structure stands,
    // off its footprint and its scaffold.
    let standing = s < WRECK_END;
    let keep_out = if standing { offset_poly(poly, -1.6) } else { Vec::new() };
    let clear = |p: Vec2| c.inside_local(p) && distance_to_boundary(p, &c.lot_poly) > 1.4 && !(keep_out.len() >= 3 && point_in_polygon(p, &keep_out));

    // The machine.
    let working = demolition_machine(g, c, s, &hull, cut, top, standing, &clear);

    // [fix/buildings-ui] The siding: scaffold sheeted in debris netting
    // round what stands, going up while the building is stripped out and
    // coming down with the cut, open on the side the machine bites.
    if standing && poly.len() >= 3 {
        let mass = mass_top(g);
        let h = if wreck > 0.0 { cut } else { mass * (s / STRIP_OUT).clamp(0.0, 1.0) };
        if h > 2.0 {
            siding(g, c, poly, h.min(mass + 1.0), mul(c.palette.steel, 0.9), working);
        }
    }

    // Rubble heaps: growing while the structure comes down, carted away after.
    let heaps = if standing { (1.0 + wreck * 5.0).round() as i32 } else { (6.0 * (1.0 - (s - WRECK_END) / (1.0 - WRECK_END))).round() as i32 };
    for k in 0..heaps.max(0) {
        let rng = &mut sub(c, 400 + k as u64);
        let at = if standing { c.random_inside(rng, 2.0) } else { random_in_poly(rng, poly, 1.0).or_else(|| c.random_inside(rng, 2.0)) };
        let Some(p) = at else { continue };
        if !clear(p) {
            continue;
        }
        let r = rng.random_range(1.4..2.8) * (0.6 + 0.4 * wreck);
        rubble_heap(g, c, rng, p, r, concrete);
    }

    // A dump truck by the gate being loaded.
    let truck_at = front.at(front.len * 0.22, -5.2);
    let rng = &mut sub(c, 22);
    let side = if rng.random_bool(0.5) { 1.0 } else { -1.0 };
    let truck_yaw = front.yaw() + std::f32::consts::FRAC_PI_2 * side;
    let (axis, across) = (Vec2::new(truck_yaw.cos(), truck_yaw.sin()), front.tangent);
    let truck_room = [axis * 4.3, axis * -4.3, across * 1.4, across * -1.4, axis * 4.3 + across * 1.3, axis * -4.3 - across * 1.3].iter().all(|o| clear(truck_at + *o));
    if s > STRIP_OUT && s < 0.97 && clear(truck_at) && truck_room {
        dump_truck(g, c, rng, truck_at, truck_yaw, concrete);
    }
    let cabin_at = front.at(-front.len * 0.32, -3.6);
    let cabin_room = [front.tangent * 3.3, front.tangent * -3.3, front.out * 1.4, front.out * -1.6].iter().all(|o| clear(cabin_at + *o));
    if clear(cabin_at) && cabin_room {
        site_cabin(g, c, &mut sub(c, 4), cabin_at, front.yaw());
    }

    // The crew, one of them on a hose keeping the dust down.
    if (0.03..=0.97).contains(&s) {
        let n = 2 + (c.floors as usize / 8).min(3);
        for k in 0..n {
            let rng = &mut sub(c, 300 + k as u64);
            if let Some(p) = c.random_inside(rng, 1.5) {
                if clear(p) {
                    worker(g, c, rng, p, 0.04);
                }
            }
        }
    }
}

/// The raw slab over what a demolition's cut left, rebar round its edge
/// and rubble on it.
fn broken_top(g: &mut Geometry, c: &Ctx, hull: &[Vec2], cut: f32, concrete: [f32; 3]) {
    let (f, pal) = (&c.frame, c.palette);
    if cut > 0.6 && !hull.is_empty() {
        extrude(g, f, Concrete, &offset_poly(hull, 0.1), (cut - 0.3).max(0.0), cut, mul(concrete, 0.75));
        let rng = &mut sub(c, 20);
        for face in c.faces(hull) {
            if face.len < 1.5 {
                continue;
            }
            let n = (face.len / 1.2) as i32;
            for k in 0..n {
                let t = -face.len * 0.5 + (k as f32 + 0.5) * (face.len / n.max(1) as f32);
                let h = rng.random_range(0.4..1.3);
                face.bx(g, f, Rust, t, cut + h * 0.5, -0.3, Vec3::new(0.05, h, 0.05), pal.rust);
            }
        }
        for _ in 0..rng.random_range(2..5) {
            if let Some(p) = random_in_poly(rng, hull, 1.0) {
                let r = rng.random_range(0.6..1.4);
                let bottom = ngon(p.x, p.y, r, 7, rng.random_range(0.0..1.0));
                if polygon_inside(&bottom, hull) {
                    loft(g, f, Concrete, &bottom, &ngon(p.x, p.y, r * 0.3, 7, 0.3), cut, cut + r * 0.5, mul(concrete, 0.9));
                }
            }
        }
    }
}

/// The demolition's machine at stage `s`: on the roof of a tall job under
/// a tower crane, a high-reach excavator beside a standing one, or loading
/// out the last heaps.
#[allow(clippy::too_many_arguments)]
fn demolition_machine(g: &mut Geometry, c: &Ctx, s: f32, hull: &[Vec2], cut: f32, top: f32, standing: bool, clear: &impl Fn(Vec2) -> bool) -> Option<usize> {
    let poly = &c.poly;
    let mut working = None;
    if s > STRIP_OUT * 0.5 && s < 0.98 {
        let rng = &mut sub(c, 21);
        // A small excavator on the top slab, turned so it and its arm
        // stay on the slab.
        let on_roof = |g: &mut Geometry, rng: &mut _| -> bool {
            if hull.is_empty() || cut < 3.0 {
                return false;
            }
            let p = poly_center(hull);
            let inner = offset_poly(hull, 1.0);
            if inner.len() < 3 {
                return false;
            }
            let yaw0 = rng_angle(rng);
            let on_slab = |yaw: f32| {
                let (d, n) = (Vec2::new(yaw.cos(), yaw.sin()), Vec2::new(-yaw.sin(), yaw.cos()));
                [d * 5.6, d * -2.6, n * 1.8, n * -1.8, d * 2.6 + n * 1.8, d * 2.6 - n * 1.8, d * -2.6 + n * 1.8, d * -2.6 - n * 1.8].iter().all(|o| point_in_polygon(p + *o, &inner))
            };
            match (0..8).map(|k| yaw0 + k as f32 * std::f32::consts::FRAC_PI_4).find(|&y| on_slab(y)) {
                Some(yaw) => {
                    excavator(g, c, rng, p, cut, yaw, None);
                    true
                }
                None => false,
            }
        };
        if cut > HIGH_REACH && !hull.is_empty() {
            // Tall job: the machine works on the roof, a tower crane lowers
            // the pieces.
            on_roof(g, rng);
            tower_crane(g, c, &mut sub(c, 3), poly, cut, top);
        } else if standing {
            // A high-reach excavator on the ground facing the building, its
            // arm at the cut; on a lot with no room beside the building, a
            // small machine on the roof instead.
            let target_y = (cut + 0.6).max(1.6);
            let mut spot: Option<(Vec2, Vec2, f32)> = None;
            'faces: for (fi, face) in c.faces(poly).into_iter().enumerate() {
                let yaw = (-face.out).y.atan2(-face.out.x);
                for d in [5.5f32, 4.5, 3.6] {
                    let p = face.at(0.0, d);
                    if fits(c, p, yaw, clear) {
                        spot = Some((p, face.mid(), yaw));
                        working = Some(fi);
                        break 'faces;
                    }
                }
            }
            match spot {
                Some((p, target, yaw)) => excavator(g, c, rng, p, 0.0, yaw, Some((target, target_y))),
                None => {
                    on_roof(g, rng);
                }
            }
        } else if let Some(p) = random_in_poly(rng, poly, 3.0) {
            // Loading out the last heaps from where the building was.
            let yaw = rng_angle(rng);
            let tip = p + Vec2::new(yaw.cos(), yaw.sin()) * 5.6;
            if fits(c, p, yaw, clear) && clear(tip) && distance_to_boundary(tip, &c.lot_poly) > 0.6 {
                excavator(g, c, rng, p, 0.0, yaw, None);
            }
        }
    }
    working
}

/// Drop everything above `cut` from the geometry; triangles crossing it are
/// clipped at it. Returns the world (x, z) points where walls were cut.
pub fn clip_above(g: &mut Geometry, cut: f32) -> Vec<Vec2> {
    let mut points = Vec::new();
    for m in g.parts.iter_mut() {
        if m.is_empty() {
            continue;
        }
        let old = std::mem::take(m);
        let mut remap: Vec<u32> = vec![u32::MAX; old.positions.len()];
        let mut keep = |m: &mut crate::mesh::MeshData, i: u32| -> u32 {
            let k = i as usize;
            if remap[k] == u32::MAX {
                remap[k] = m.positions.len() as u32;
                m.positions.push(old.positions[k]);
                m.normals.push(old.normals[k]);
                m.colors.push(old.colors[k]);
                m.uvs.push(old.uvs[k]);
                m.tangents.push(old.tangents[k]);
            }
            remap[k]
        };
        for tri in old.indices.chunks_exact(3) {
            let ys = [old.positions[tri[0] as usize][1], old.positions[tri[1] as usize][1], old.positions[tri[2] as usize][1]];
            if ys.iter().all(|&y| y <= cut) {
                for &i in tri {
                    let n = keep(m, i);
                    m.indices.push(n);
                }
                continue;
            }
            if ys.iter().all(|&y| y > cut) {
                continue;
            }
            // Sutherland–Hodgman against the plane y = cut, keeping below.
            let mut out: Vec<u32> = Vec::with_capacity(4);
            for k in 0..3 {
                let (a, b) = (tri[k], tri[(k + 1) % 3]);
                let (ya, yb) = (ys[k], ys[(k + 1) % 3]);
                if ya <= cut {
                    out.push(keep(m, a));
                }
                if (ya <= cut) != (yb <= cut) {
                    let t = (cut - ya) / (yb - ya);
                    let (ia, ib) = (a as usize, b as usize);
                    let lerp3 = |p: [f32; 3], q: [f32; 3]| [p[0] + (q[0] - p[0]) * t, p[1] + (q[1] - p[1]) * t, p[2] + (q[2] - p[2]) * t];
                    let lerp4 = |p: [f32; 4], q: [f32; 4]| [p[0] + (q[0] - p[0]) * t, p[1] + (q[1] - p[1]) * t, p[2] + (q[2] - p[2]) * t, p[3] + (q[3] - p[3]) * t];
                    let pos = lerp3(old.positions[ia], old.positions[ib]);
                    points.push(Vec2::new(pos[0], pos[2]));
                    m.positions.push(pos);
                    m.normals.push(Vec3::from(lerp3(old.normals[ia], old.normals[ib])).normalize_or(Vec3::from(old.normals[ia])).to_array());
                    m.colors.push(lerp4(old.colors[ia], old.colors[ib]));
                    let (ua, ub) = (old.uvs[ia], old.uvs[ib]);
                    m.uvs.push([ua[0] + (ub[0] - ua[0]) * t, ua[1] + (ub[1] - ua[1]) * t]);
                    m.tangents.push(lerp4(old.tangents[ia], old.tangents[ib]));
                    out.push(m.positions.len() as u32 - 1);
                }
            }
            // A triangle grazing the cut leaves a sliver whose winding is
            // rounding noise: dropped.
            for k in 1..out.len().saturating_sub(1) {
                let [a, b, c] = [out[0], out[k], out[k + 1]].map(|i| Vec3::from(m.positions[i as usize]));
                if (b - a).cross(c - a).length() > 1e-5 {
                    m.indices.extend_from_slice(&[out[0], out[k], out[k + 1]]);
                }
            }
        }
    }
    points
}

/// Convex hull, counter-clockwise (Andrew's monotone chain).
fn convex_hull(points: &[Vec2]) -> Vec<Vec2> {
    let mut p: Vec<Vec2> = points.to_vec();
    p.sort_by(|a, b| a.x.total_cmp(&b.x).then(a.y.total_cmp(&b.y)));
    p.dedup_by(|a, b| a.distance_squared(*b) < 1e-4);
    if p.len() < 3 {
        return p;
    }
    let cross = |o: Vec2, a: Vec2, b: Vec2| (a - o).perp_dot(b - o);
    let mut hull: Vec<Vec2> = Vec::with_capacity(p.len() * 2);
    for pass in 0..2 {
        let start = hull.len();
        let iter: Box<dyn Iterator<Item = &Vec2>> = if pass == 0 { Box::new(p.iter()) } else { Box::new(p.iter().rev()) };
        for &q in iter {
            while hull.len() >= start + 2 && cross(hull[hull.len() - 2], hull[hull.len() - 1], q) <= 0.0 {
                hull.pop();
            }
            hull.push(q);
        }
        hull.pop();
    }
    hull
}

fn polygon_inside(inner: &[Vec2], outer: &[Vec2]) -> bool {
    inner.iter().all(|p| point_in_polygon(*p, outer))
}

fn rng_angle(rng: &mut impl Rng) -> f32 {
    rng.random_range(0.0..std::f32::consts::TAU)
}

/// Room for an excavator's body (tracks 4.6 × 3.0 m, turning) at `p`.
fn fits(c: &Ctx, p: Vec2, yaw: f32, clear: &impl Fn(Vec2) -> bool) -> bool {
    let d = Vec2::new(yaw.cos(), yaw.sin());
    let n = Vec2::new(-d.y, d.x);
    // Corners, sides and the bucket's reach, all clear of the fence.
    let pts = [d * 2.6 + n * 1.8, d * 2.6 - n * 1.8, -d * 2.6 + n * 1.8, -d * 2.6 - n * 1.8, n * 1.8, -n * 1.8, -d * 2.6, d * 2.6];
    clear(p) && pts.iter().all(|o| c.inside_local(p + *o) && distance_to_boundary(p + *o, &c.lot_poly) > 0.4)
}

/// A tracked excavator at `p`, standing at height `y`, facing `yaw`
/// (lot-local). With `reach`, a long demolition arm runs from the cab to
/// that point and height, ending in a crusher; without, a short digging arm.
fn excavator(g: &mut Geometry, c: &Ctx, rng: &mut impl Rng, p: Vec2, y: f32, yaw: f32, reach: Option<(Vec2, f32)>) {
    let f = &c.frame;
    let body = c.palette.crane;
    let dark = mul(c.palette.steel, 0.4);
    let d = Vec2::new(yaw.cos(), yaw.sin());
    let n = Vec2::new(-d.y, d.x);
    // Tracks, turntable, house with counterweight, cab.
    for side in [-1.0f32, 1.0] {
        let q = p + n * side * 1.15;
        f.bx_yaw(g, Matte, q.x, y + 0.45, q.y, Vec3::new(4.4, 0.9, 0.7), dark, yaw);
    }
    f.bx_yaw(g, Metal, p.x, y + 1.0, p.y, Vec3::new(1.6, 0.25, 1.6), dark, yaw);
    f.bx_yaw(g, Matte, p.x, y + 1.7, p.y, Vec3::new(3.0, 1.2, 2.5), body, yaw);
    let back = p - d * 1.6;
    f.bx_yaw(g, Matte, back.x, y + 1.6, back.y, Vec3::new(0.6, 1.0, 2.4), mul(body, 0.8), yaw);
    let cab = p + d * 0.7 + n * 0.75;
    f.bx_yaw(g, Matte, cab.x, y + 2.9, cab.y, Vec3::new(1.2, 1.4, 0.95), body, yaw);
    f.bx_yaw(g, Glass, cab.x + d.x * 0.4, y + 3.0, cab.y + d.y * 0.4, Vec3::new(0.5, 1.0, 0.9), c.palette.glass_dark, yaw);
    let pivot = f.at2(p + d * 1.2 - n * 0.4, y + 2.1);
    let steel = rgba(mul(body, 0.95));
    match reach {
        Some((target, ty)) => {
            // Stop the crusher just short of the wall.
            let to = target - (target - p).normalize_or_zero() * 0.8;
            let tip = f.at2(to, ty);
            let span = (tip - pivot).length();
            let elbow = pivot.lerp(tip, 0.45) + Vec3::Y * (span * 0.28).max(1.5);
            g.part(Matte).tube(pivot, elbow, 0.32, 6, steel);
            g.part(Matte).tube(elbow, tip, 0.24, 6, steel);
            g.part(Matte).tube(pivot + Vec3::Y * 0.3, elbow - (elbow - pivot).normalize_or_zero() * 2.0, 0.09, 5, rgba(c.palette.steel));
            // Crusher jaws.
            g.part(Metal).boxed(tip - Vec3::Y * 0.4, Vec3::new(0.7, 1.1, 0.5), f.yaw() + yaw, rgba(dark));
            g.part(Metal).boxed(tip - Vec3::Y * 0.9 + f.dir3(d) * 0.2, Vec3::new(0.25, 0.6, 0.45), f.yaw() + yaw + 0.4, rgba(dark));
        }
        None => {
            let elbow = pivot + f.dir3(d) * 2.6 + Vec3::Y * 1.6;
            let tip = pivot + f.dir3(d) * 4.0 - Vec3::Y * 1.2;
            g.part(Matte).tube(pivot, elbow, 0.2, 6, steel);
            g.part(Matte).tube(elbow, tip, 0.16, 6, steel);
            let bucket = if rng.random_bool(0.5) { 0.5 } else { 0.7 };
            g.part(Metal).boxed(tip, Vec3::new(0.7, bucket, 0.8), f.yaw() + yaw, rgba(dark));
        }
    }
}

/// A heap of broken concrete, brick and bent steel.
fn rubble_heap(g: &mut Geometry, c: &Ctx, rng: &mut impl Rng, p: Vec2, r: f32, concrete: [f32; 3]) {
    let f = &c.frame;
    let rot = rng.random_range(0.0..1.0);
    let bottom = fit_inside(c, &ngon(p.x, p.y, r, 8, rot));
    if bottom.len() < 3 {
        return;
    }
    let cen = poly_center(&bottom);
    let col = mix(concrete, pick(rng, &c.palette.brick), rng.random_range(0.0..0.5));
    loft(g, f, Concrete, &bottom, &ngon(cen.x, cen.y, r * 0.3, 8, rot + 0.2), 0.0, r * 0.5, col);
    for _ in 0..3 {
        let q = cen + Vec2::new(rng.random_range(-0.5..0.5), rng.random_range(-0.5..0.5)) * r;
        if point_in_polygon(q, &bottom) {
            let yaw = rng.random_range(0.0..std::f32::consts::TAU);
            f.bx_yaw(g, Concrete, q.x, r * 0.35, q.y, Vec3::new(0.9, 0.35, 0.6), mul(concrete, 1.1), yaw);
        }
    }
    if rng.random_bool(0.6) {
        let a = f.at2(cen, r * 0.3);
        let b = f.at2(cen + Vec2::new(rng.random_range(-0.6..0.6), rng.random_range(-0.6..0.6)) * r, r * 0.5 + 0.8);
        g.part(Rust).tube(a, b, 0.03, 4, rgba(c.palette.rust));
    }
}

/// A tipper lorry with its body heaped with rubble.
fn dump_truck(g: &mut Geometry, c: &Ctx, rng: &mut impl Rng, p: Vec2, yaw: f32, concrete: [f32; 3]) {
    let f = &c.frame;
    let col = pick(rng, &[[0.85, 0.55, 0.1], [0.75, 0.2, 0.1], [0.3, 0.35, 0.4]]);
    vehicle(g, f, c.palette, rng, p, 0.0, yaw, Vehicle::Truck, col);
    let w = f.to_world(p.x, p.y);
    let a = f.yaw() + yaw;
    let (sn, cs) = a.sin_cos();
    let at = |dx: f32, y: f32| Vec3::new(w.x + dx * cs, y, w.y + dx * sn);
    // Steel tipper body over the bed, and the load showing over its sides.
    g.part(Metal).boxed(at(-1.8, 2.3), Vec3::new(4.2, 1.2, 2.3), a, rgba(mul(col, 0.8)));
    g.part(Concrete).boxed(at(-1.8, 3.0), Vec3::new(3.8, 0.4, 2.0), a, rgba(concrete));
    g.part(Concrete).boxed(at(-2.2, 3.3), Vec3::new(1.6, 0.4, 1.2), a + 0.3, rgba(mul(concrete, 0.9)));
}
