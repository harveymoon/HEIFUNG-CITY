//! A building site. Drawn in place of the finished style while the sim's
//! `Construction` is in progress: hoarding round the lot with a gate, a
//! concrete raft, the footprint rising floor by floor in bare concrete with
//! scaffolding climbing beside it, a tower crane on the taller jobs, and the
//! clutter of pallets, a site cabin, a mixer truck and the crew.

use super::parts::*;
use super::{Ctx, Face};
use crate::surface::{Geometry, Surface::*};
use glam::{Vec2, Vec3};
use nd_sim::zoning::point_in_polygon;
use nd_sim::LandUse;
use rand::Rng;

const HOARDING_H: f32 = 2.3;

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

    // Hoarding with a gate on the street side.
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

    // The raft, and the structure on it.
    let poly = &c.poly;
    if poly.len() >= 3 {
        extrude(g, f, Concrete, poly, 0.0, 0.35, mul(pal.concrete(0), 0.7));
        if !landscaping {
            structure(g, c, rng, poly, built, floors_done, fh, concrete, pale, steel);
            if progress > 0.02 && progress < 0.97 && built > 0.3 {
                scaffolding(g, c, rng, poly, (built + 2.5).min(height + 1.5), steel);
            }
            if c.floors >= 6 && progress > 0.06 && progress < 0.98 {
                tower_crane(g, c, rng, poly, built, height);
            }
        } else {
            // Landscaping: the raft is a plaza base; spoil heaps around it.
            for _ in 0..3 {
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
        site_cabin(g, c, rng, cabin_at, front.yaw());
    }
    let truck_at = front.at(front.len * 0.22, -5.2);
    if progress < 0.97 && clear(truck_at) && clear(truck_at + front.tangent * 3.5) && clear(truck_at - front.tangent * 3.5) {
        let side = if rng.random_bool(0.5) { 1.0 } else { -1.0 };
        mixer_truck(g, c, rng, truck_at, front.yaw() + std::f32::consts::FRAC_PI_2 * side);
    }
    let loo = front.at(front.len * 0.42, -2.2);
    if clear(loo) {
        f.bx(g, Matte, loo.x, 1.15, loo.y, Vec3::new(1.1, 2.3, 1.1), [0.12, 0.30, 0.55]);
    }
    // Stacks of materials, fewer as they are built in.
    let stacks = (2.0 + 5.0 * (1.0 - progress)).round() as i32 + c.floors as i32 / 8;
    for _ in 0..stacks {
        let Some(p) = c.random_inside(rng, 2.0) else { continue };
        if !clear(p) {
            continue;
        }
        let yaw = rng.random_range(-0.4..0.4);
        material_stack(g, c, rng, p, yaw);
    }
    if progress < 0.5 {
        for _ in 0..rng.random_range(1..3) {
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

/// Plywood hoarding along one edge, with a gate gap of `gate` metres in the
/// middle when given.
fn hoarding(g: &mut Geometry, c: &Ctx, rng: &mut impl Rng, face: &Face, gate: Option<f32>) {
    let f = &c.frame;
    if face.len < 0.6 {
        return;
    }
    let ply = mul(c.palette.plywood, rng.random_range(0.85..1.05));
    let post = mul(c.palette.steel, 0.55);
    let h = HOARDING_H;
    let spans: Vec<(f32, f32)> = match gate {
        Some(gw) if face.len > gw + 2.0 => vec![(-face.len * 0.5, -gw * 0.5), (gw * 0.5, face.len * 0.5)],
        _ => vec![(-face.len * 0.5, face.len * 0.5)],
    };
    for (a, b) in &spans {
        let len = b - a;
        let mid = (a + b) * 0.5;
        face.bx(g, f, Wood, mid, h * 0.5, 0.0, Vec3::new(len, h, 0.06), ply);
        // Rail along the top and posts behind.
        face.bx(g, f, Matte, mid, h + 0.03, -0.05, Vec3::new(len, 0.08, 0.14), post);
        let posts = (len / 2.4).ceil().max(1.0) as i32;
        for k in 0..=posts {
            let t = a + k as f32 * (len / posts as f32);
            face.bx(g, f, Matte, t, h * 0.5, -0.08, Vec3::new(0.1, h, 0.1), post);
        }
        // Posters and a warning stripe.
        if rng.random_bool(0.5) && len > 4.0 {
            let t = rng.random_range(a + 1.0..b - 1.0);
            face.decal(g, f, Glow, t, 1.3, 0.04, 1.2, 1.6, mul(pick(rng, &c.palette.sign_colors), 0.6));
        }
        face.decal(g, f, Matte, mid, 0.45, 0.035, len, 0.25, c.palette.hi_vis);
    }
    if let Some(gw) = gate {
        if face.len > gw + 2.0 {
            // Gate posts and a sliding mesh gate, part open.
            for side in [-1.0f32, 1.0] {
                face.bx(g, f, Matte, side * gw * 0.5, (h + 0.3) * 0.5, 0.0, Vec3::new(0.22, h + 0.3, 0.22), post);
            }
            face.bx(g, f, Matte, -gw * 0.5 - gw * 0.3, h * 0.5, -0.25, Vec3::new(gw * 0.6, h * 0.92, 0.04), [0.30, 0.31, 0.33]);
            face.bx(g, f, Matte, gw * 0.5 - gw * 0.15, h * 0.5, 0.25, Vec3::new(gw * 0.3, h * 0.92, 0.04), [0.30, 0.31, 0.33]);
            // A barrier across the opening.
            face.bx(g, f, Matte, 0.0, 0.9, 0.4, Vec3::new(gw * 0.8, 0.12, 0.08), c.palette.hi_vis);
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
        if rng.random_bool(0.55) {
            let tarp = pick(rng, &c.palette.tarp_colors);
            face.bx(g, f, Tarp, 0.0, top * 0.5, d_out + 0.08, Vec3::new(face.len, top * 0.96, 0.02), tarp);
        }
    }
}

/// A tower crane standing clear of the footprint, jib reaching over it.
fn tower_crane(g: &mut Geometry, c: &Ctx, rng: &mut impl Rng, poly: &[Vec2], built: f32, height: f32) {
    let f = &c.frame;
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
    let mast_h = (built + 14.0).clamp(18.0, height + 16.0);
    let w = 1.5;
    // Lattice mast: corner legs and a lighter core.
    for (du, dv) in [(-1.0f32, -1.0f32), (1.0, -1.0), (1.0, 1.0), (-1.0, 1.0)] {
        f.bx(g, Matte, base.x + du * w * 0.45, mast_h * 0.5, base.y + dv * w * 0.45, Vec3::new(0.14, mast_h, 0.14), c.palette.crane);
    }
    let bands = (mast_h / 2.5) as i32;
    for k in 0..=bands {
        let y = k as f32 * 2.5;
        f.bx(g, Matte, base.x, y, base.y, Vec3::new(w, 0.08, 0.08), c.palette.crane);
        f.bx(g, Matte, base.x, y, base.y, Vec3::new(0.08, 0.08, w), c.palette.crane);
    }
    f.bx(g, Matte, base.x, mast_h * 0.5, base.y, Vec3::new(w * 0.5, mast_h, w * 0.5), mul(c.palette.crane, 0.8));
    // Foundation block and ballast.
    f.bx(g, Concrete, base.x, 0.4, base.y, Vec3::new(w + 2.0, 0.8, w + 2.0), mul(c.palette.concrete(0), 0.75));
    // Slewing unit, cab, and the jib out over the footprint.
    let top = mast_h + 0.6;
    f.bx(g, Matte, base.x, top, base.y, Vec3::new(w + 0.4, 1.2, w + 0.4), mul(c.palette.crane, 0.9));
    let dir = {
        let d = centre - base;
        if d.length() > 0.5 { d.normalize() } else { Vec2::new(rng.random_range(-1.0..1.0), rng.random_range(-1.0..1.0)).normalize_or_zero().max(Vec2::X) }
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
    f.bx_yaw(g, Matte, jib_mid.x, jib_y, jib_mid.y, Vec3::new(jib_len, 0.9, 0.9), c.palette.crane, angle);
    f.bx_yaw(g, Matte, jib_mid.x, jib_y + 0.6, jib_mid.y, Vec3::new(jib_len * 0.98, 0.1, 0.1), mul(c.palette.crane, 0.8), angle);
    // Counter-jib with ballast blocks, kept inside the fence too.
    let back_len = reach(-dir, jib_len * 0.4, 2.0);
    let back_mid = base - dir * back_len * 0.5;
    f.bx_yaw(g, Matte, back_mid.x, jib_y, back_mid.y, Vec3::new(back_len, 0.8, 1.0), c.palette.crane, angle);
    let ballast = base - dir * (back_len - 1.0);
    f.bx_yaw(g, Concrete, ballast.x, jib_y - 0.9, ballast.y, Vec3::new(1.8, 1.4, 1.6), mul(c.palette.concrete(0), 0.8), angle);
    // Cab beside the mast, facing the jib.
    let cab = base + dir * (w * 0.5 + 1.0);
    f.bx_yaw(g, Matte, cab.x, top + 0.2, cab.y, Vec3::new(1.8, 1.9, 1.6), mul(c.palette.crane, 0.95), angle);
    f.bx_yaw(g, Glass, cab.x + dir.x * 0.5, top + 0.4, cab.y + dir.y * 0.5, Vec3::new(1.0, 1.1, 1.7), c.palette.glass_dark, angle);
    // Tower top and pendant lines.
    f.bx(g, Matte, base.x, jib_y + 3.0, base.y, Vec3::new(0.4, 5.0, 0.4), c.palette.crane);
    let apex = f.at2(base, jib_y + 5.5);
    g.part(Matte).tube(apex, f.at2(base + dir * jib_len * 0.9, jib_y + 0.5), 0.03, 4, rgba(DARK));
    g.part(Matte).tube(apex, f.at2(base - dir * back_len * 0.9, jib_y + 0.4), 0.03, 4, rgba(DARK));
    // Trolley, hoist rope and hook with a load swinging over the slab.
    let t = rng.random_range(0.45..0.85);
    let trolley = base + dir * jib_len * t;
    f.bx_yaw(g, Matte, trolley.x, jib_y - 0.6, trolley.y, Vec3::new(1.2, 0.4, 1.1), DARK, angle);
    let hook_y = (built + 4.0 + rng.random_range(0.0..5.0)).min(jib_y - 2.0);
    let rope = f.at2(trolley, jib_y - 0.8);
    g.part(Matte).tube(rope, f.at2(trolley, hook_y), 0.025, 4, rgba(DARK));
    f.bx(g, Matte, trolley.x, hook_y - 0.3, trolley.y, Vec3::new(0.5, 0.7, 0.3), DARK);
    if rng.random_bool(0.7) {
        let load = hook_y - 1.6;
        // A swinging steel bundle is 3.6 m long: only where it clears the fence.
        if rng.random_bool(0.5) && poly_inside(c, &rect_yaw(trolley.x, trolley.y, 1.9, 0.6, angle + 0.4)) {
            f.bx_yaw(g, Metal, trolley.x, load, trolley.y, Vec3::new(3.6, 0.5, 1.0), mul(c.palette.steel, 0.8), angle + 0.4);
        } else {
            f.bx_yaw(g, Concrete, trolley.x, load, trolley.y, Vec3::new(1.4, 1.2, 1.2), mul(c.palette.concrete(0), 0.9), angle);
        }
    }
    // Aviation lights.
    f.bx(g, Glow, base.x, jib_y + 5.8, base.y, Vec3::new(0.3, 0.3, 0.3), c.palette.aviation_light);
    f.bx(g, Glow, tip.x, jib_y + 0.6, tip.y, Vec3::new(0.3, 0.3, 0.3), c.palette.aviation_light);
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

