//! Industrial plant: tanks, chimneys, pipe runs, shipping containers,
//! steel beams and parked vehicles.

#[allow(unused_imports)]
use super::*;

#[allow(clippy::too_many_arguments)]
pub fn tank_vertical(g: &mut Geometry, f: &Frame, p: Vec2, r: f32, y0: f32, h: f32, s: Surface, color: [f32; 3]) {
    let w = f.to_world(p.x, p.y);
    // The shell's top lies under the rim's (and the cupola's bottom on the
    // rim's top): those caps are left out.
    g.part(s).cylinder_capped(w, r, r, y0, y0 + h, 14, true, false, rgba(color));
    g.part(Matte).cylinder(w, r * 1.03, r * 1.03, y0 + h, y0 + h + 0.15, 14, rgba(CHARCOAL));
    // ladder and top rail
    g.part(Matte).boxed(Vec3::new(w.x + r, y0 + h * 0.5, w.y), Vec3::new(0.08, h, 0.5), 0.0, rgba(CHARCOAL));
    g.part(Matte).cylinder_capped(w, r * 0.95, r * 0.95, y0 + h + 0.15, y0 + h + 1.0, 14, false, true, rgba(mul(CHARCOAL, 0.5)));
}

pub fn tank_horizontal(g: &mut Geometry, f: &Frame, p: Vec2, angle: f32, len: f32, r: f32, y: f32, color: [f32; 3]) {
    let w = f.to_world(p.x, p.y);
    let a = f.yaw() + angle;
    let dir = Vec2::new(a.cos(), a.sin());
    g.part(Metal).pipe(w, dir, len, r, y, 12, rgba(color));
    for t in [-0.35f32, 0.35] {
        let q = w + dir * (len * t);
        g.part(Concrete).boxed(Vec3::new(q.x, (y - r) * 0.5, q.y), Vec3::new(0.8, y - r, r * 1.6), a, rgba(mul(color, 0.6)));
    }
}

pub fn chimney(g: &mut Geometry, f: &Frame, p: Vec2, r: f32, y0: f32, h: f32, color: [f32; 3], light: [f32; 3]) {
    let w = f.to_world(p.x, p.y);
    // Stood on a roof or the ground, and capped by the rim: no end caps.
    g.part(Metal).cylinder_capped(w, r, r * 0.75, y0, y0 + h, 10, false, false, rgba(color));
    g.part(Matte).cylinder(w, r * 0.8, r * 0.8, y0 + h, y0 + h + 0.4, 10, rgba(DARK));
    g.part(Glow).boxed(Vec3::new(w.x, y0 + h + 0.6, w.y), Vec3::new(0.35, 0.35, 0.35), 0.0, rgba(light));
}

/// Pipe along a sequence of lot-local (u, y, v) points with elbows.
pub fn pipe_run(g: &mut Geometry, f: &Frame, pts: &[Vec3], r: f32, color: [f32; 3]) {
    for w in pts.windows(2) {
        let a = f.at(w[0].x, w[0].y, w[0].z);
        let b = f.at(w[1].x, w[1].y, w[1].z);
        g.part(Metal).tube(a, b, r, 8, rgba(color));
        g.part(Metal).boxed(b, Vec3::splat(r * 2.4), f.yaw(), rgba(mul(color, 0.85)));
    }
}

/// A container's paint as it looks after years at sea: the line colour
/// jittered in brightness and hue, and on some units bleached by the sun
/// toward a chalky, desaturated version of itself.
pub fn container_paint(rng: &mut impl Rng, base: [f32; 3], wear: f32) -> [f32; 3] {
    let k = rng.random_range(0.86..1.1);
    let mut c = [0usize, 1, 2].map(|i| base[i] * k * rng.random_range(0.93..1.07));
    if coin(rng, 0.15 + 0.35 * wear) {
        let lum = 0.3 * c[0] + 0.55 * c[1] + 0.15 * c[2];
        let chalk = [lum * 1.25 + 0.07, lum * 1.22 + 0.07, lum * 1.18 + 0.07];
        c = mix(c, chalk, rng.random_range(0.25..0.6));
    }
    c
}

/// Gradient decal: `top` at its upper edge fading to `bottom` at its lower
/// one. Rust running down from a rail, a stain under a casting.
#[allow(clippy::too_many_arguments)]
pub(super) fn streak_decal(m: &mut MeshData, centre: Vec3, along: Vec3, w: f32, h: f32, facing: Vec3, top: [f32; 3], bottom: [f32; 3]) {
    let n0 = m.positions.len();
    m.decal(centre, along, Vec3::Y, w, h, facing, rgba(top));
    for i in n0..m.positions.len() {
        if m.positions[i][1] < centre.y {
            m.colors[i] = rgba(bottom);
        }
    }
}

/// Four side quads of a box, open top and bottom: rails, flanges, sleeves.
/// `corner(x, y, z)` maps unit corners (x, z in -1..1) at height y.
pub(super) fn box_sides(m: &mut MeshData, corner: impl Fn(f32, f32, f32) -> Vec3, ya: f32, yb: f32, color: [f32; 4]) {
    let centre = (corner(-1.0, ya, -1.0) + corner(1.0, yb, 1.0)) * 0.5;
    for (a, b) in [((-1.0f32, -1.0f32), (1.0f32, -1.0f32)), ((1.0, -1.0), (1.0, 1.0)), ((1.0, 1.0), (-1.0, 1.0)), ((-1.0, 1.0), (-1.0, -1.0))] {
        let q = [corner(a.0, ya, a.1), corner(b.0, ya, b.1), corner(b.0, yb, b.1), corner(a.0, yb, a.1)];
        let mid = (q[0] + q[2]) * 0.5;
        let out = Vec3::new(mid.x - centre.x, 0.0, mid.z - centre.z);
        m.quad_facing(q, out, color);
    }
}

/// Most triangles one container may spend, details and wear included.
pub const CONTAINER_TRIS: usize = 70;
/// [r4] (K33) Most a liveried, reefer, tank or open-top box may spend (a
/// tank's frame and barrel are solid: it has no sheet to decal on).
pub const CONTAINER_TRIS_EX: usize = 160;

/// [r4] (K33) The ISO container, the one standard every container in town
/// is drawn to (stacks homes, yards, cargo hubs, trains, ships, lorries,
/// cranes): a twenty-foot (TEU) and a forty-foot box, 8 ft wide and 8 ft
/// 6 in high, metres.
pub const CONTAINER_20: f32 = 6.06;
pub const CONTAINER_40: f32 = 12.19;
pub const CONTAINER_W: f32 = 2.44;
pub const CONTAINER_H: f32 = 2.59;

/// [r4] (K33) A shipping line (all fictional): the paint its boxes carry,
/// the colour of its mark and whether the mark is a band along the upper
/// side or a logo block towards one end.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ShippingLine {
    pub name: &'static str,
    pub body: [f32; 3],
    pub mark: [f32; 3],
    pub band: bool,
}

/// [r4] (K33) The lines whose boxes come through town.
pub const SHIPPING_LINES: [ShippingLine; 10] = [
    ShippingLine { name: "Kestrel Ocean Lines", body: [0.06, 0.16, 0.36], mark: [0.8, 0.8, 0.78], band: true },
    ShippingLine { name: "Ardent Maritime", body: [0.46, 0.1, 0.07], mark: [0.8, 0.8, 0.78], band: false },
    ShippingLine { name: "Tidewell Freight", body: [0.06, 0.22, 0.22], mark: [0.75, 0.55, 0.08], band: true },
    ShippingLine { name: "Hanzo Kaiun", body: [0.34, 0.35, 0.36], mark: [0.5, 0.08, 0.06], band: false },
    ShippingLine { name: "Meridian Box Co.", body: [0.7, 0.28, 0.05], mark: [0.05, 0.08, 0.18], band: false },
    ShippingLine { name: "Corvid Logistics", body: [0.1, 0.1, 0.11], mark: [0.75, 0.55, 0.08], band: true },
    ShippingLine { name: "Sable Line", body: [0.1, 0.26, 0.14], mark: [0.8, 0.8, 0.78], band: true },
    ShippingLine { name: "Lumen Pacific", body: [0.24, 0.42, 0.56], mark: [0.72, 0.66, 0.5], band: false },
    ShippingLine { name: "Oro Leasing", body: [0.62, 0.44, 0.07], mark: [0.04, 0.04, 0.05], band: false },
    ShippingLine { name: "Brine & Basalt", body: [0.52, 0.17, 0.08], mark: [0.72, 0.66, 0.5], band: true },
];

/// [r4] (K33) The line a box with this seed belongs to.
pub fn shipping_line(seed: u64) -> &'static ShippingLine {
    &SHIPPING_LINES[(crate::hash::hash2(seed, 0x5157) % SHIPPING_LINES.len() as u64) as usize]
}

/// [r4] (K33) What a box is built as: a dry box, a reefer (white, with its
/// refrigeration unit in the front end), an ISO tank (a barrel in a
/// twenty-foot frame) or an open-top box under a tarp.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BoxLook {
    Dry,
    Reefer,
    Tank,
    OpenTop,
}

/// [r4] (K33) How a box is painted: anyhow from the place's palette (the
/// old way, with the odd livery), or in a shipping line's colours.
#[derive(Clone, Copy, Debug)]
pub enum Livery<'a> {
    Any([f32; 3]),
    Line(&'a ShippingLine),
}

/// An ISO shipping container placed in parcel coordinates: corrugated
/// walls and roof (the `Container` sheet), top and bottom side rails, corner
/// posts with castings, a door end with hinge posts, four locking bars and
/// the door seam, and a front wall recessed between its posts at the other
/// end. Each unit is painted its own way (see [`container_paint`]) and may
/// carry a shipping-line band or logo block, an ID marking, rust running
/// down from the top rail and castings, dents and repainted patches, more
/// of them the more worn the place is. Everything flat is a decal: a unit
/// stays within [`CONTAINER_TRIS`].
#[allow(clippy::too_many_arguments)]
pub fn container_unit(g: &mut Geometry, c: &Ctx, rng: &mut impl Rng, u: f32, v: f32, yaw: f32, len: f32, wid: f32, h: f32, y0: f32, color: [f32; 3]) {
    container_unit_ex(g, c, rng, u, v, yaw, len, wid, h, y0, Livery::Any(color), BoxLook::Dry);
}

/// [r4] (K33) A standard box ([`CONTAINER_20`] or [`CONTAINER_40`] long)
/// in a shipping line's livery: [`container_unit_ex`] at the ISO size.
#[allow(clippy::too_many_arguments)]
pub fn iso_container(g: &mut Geometry, c: &Ctx, rng: &mut impl Rng, u: f32, v: f32, yaw: f32, forty: bool, y0: f32, line: &ShippingLine, look: BoxLook) {
    let len = if forty && look != BoxLook::Tank { CONTAINER_40 } else { CONTAINER_20 };
    container_unit_ex(g, c, rng, u, v, yaw, len, CONTAINER_W, CONTAINER_H, y0, Livery::Line(line), look);
}

/// [`container_unit`] with its livery and build given: a line's boxes
/// always carry its mark on both sides; a reefer is white with its
/// machinery in the front end; a tank is a barrel in a frame; an open-top
/// box has a tarp for a roof. Within [`CONTAINER_TRIS_EX`].
#[allow(clippy::too_many_arguments)]
pub fn container_unit_ex(g: &mut Geometry, c: &Ctx, rng: &mut impl Rng, u: f32, v: f32, yaw: f32, len: f32, wid: f32, h: f32, y0: f32, livery: Livery, look: BoxLook) {
    container_core(g, c, rng, u, v, yaw, len, wid, h, y0, livery, look, None);
}

/// [K34] A real box's life as its paint shows it: how worn it is, its
/// patched panels and dents, and which end its doors are (+1 or -1 along
/// its length). Drawn by [`container_core`] instead of the dice.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct BoxLife {
    pub wear: f32,
    pub patches: u8,
    pub dents: u8,
    pub door: f32,
}

/// [`container_unit_ex`], or with `life` a real box drawn as it is.
#[allow(clippy::too_many_arguments)]
pub(crate) fn container_core(g: &mut Geometry, c: &Ctx, rng: &mut impl Rng, u: f32, v: f32, yaw: f32, len: f32, wid: f32, h: f32, y0: f32, livery: Livery, look: BoxLook, life: Option<BoxLife>) {
    let f = &c.frame;
    let pal = c.palette;
    let (s, co) = yaw.sin_cos();
    let at = |lu: f32, y: f32, lv: f32| f.at2(Vec2::new(u + lu * co - lv * s, v + lu * s + lv * co), y);
    let long = f.dir3(Vec2::new(co, s));
    let across = f.dir3(Vec2::new(-s, co));
    let (hl, hw) = (len * 0.5, wid * 0.5);
    let top = y0 + h;
    let (rail_b, rail_t) = (0.16f32.min(h * 0.1), 0.14f32.min(h * 0.1));
    let (wall_a, wall_b) = (y0 + rail_b, top - rail_t);
    let wall_mid = (wall_a + wall_b) * 0.5;
    let wall_h = wall_b - wall_a;
    // Rails stand proud of the sheet, posts proud of the rails.
    let (rail_out, post_out) = (0.025, 0.045);
    let tier_wear = if c.tier == nd_sim::Tier::Low { 0.2 } else { 0.0 };
    let wear = (c.wear * 0.75 + tier_wear + rng.random_range(-0.1..0.2)).clamp(0.0, 1.0);
    let wear = life.map_or(wear, |l| l.wear.clamp(0.0, 1.0));
    let base = match (livery, look) {
        (_, BoxLook::Reefer) => [0.78, 0.78, 0.75],
        (Livery::Any(col), _) => col,
        (Livery::Line(l), _) => l.body,
    };
    let paint = container_paint(rng, base, wear);
    let frame_col = mul(paint, 0.72);
    let door = if rng.random_bool(0.5) { -1.0f32 } else { 1.0 };
    let door = life.map_or(door, |l| l.door.signum());

    if look == BoxLook::Tank {
        // [r4] The ISO tank: a barrel slung in a twenty-foot frame of
        // corner posts and end rails, a walkway along the top.
        let barrel = mul(container_paint(rng, if matches!(livery, Livery::Line(_)) { [0.6, 0.62, 0.64] } else { base }, wear), 1.0);
        let r = (wid.min(h) * 0.46).max(0.4);
        let mid = Vec2::new(u, v);
        let dir = Vec2::new(co, s);
        let m = g.part(Metal);
        let w0 = f.to_world(mid.x, mid.y);
        let wdir = (f.to_world(mid.x + dir.x, mid.y + dir.y) - w0).normalize_or_zero();
        m.pipe(w0, wdir, len - 0.9, r, y0 + h * 0.5, 12, rgba(barrel));
        let m = g.part(Matte);
        for end in [-1.0f32, 1.0] {
            for side in [-1.0f32, 1.0] {
                m.boxed(at(end * (hl - 0.09), y0 + h * 0.5, side * (hw - 0.09)), Vec3::new(0.18, h, 0.18), f.yaw() + yaw, rgba(frame_col));
            }
            m.boxed(at(end * (hl - 0.09), top - 0.1, 0.0), Vec3::new(0.16, 0.2, wid), f.yaw() + yaw, rgba(frame_col));
            m.boxed(at(end * (hl - 0.09), y0 + 0.1, 0.0), Vec3::new(0.16, 0.2, wid), f.yaw() + yaw, rgba(frame_col));
        }
        m.boxed(at(0.0, y0 + h * 0.5 + r + 0.05, 0.0), Vec3::new(len * 0.6, 0.08, 0.6), f.yaw() + yaw, rgba(mul(frame_col, 0.8)));
        if let Livery::Line(l) = livery {
            for side in [-1.0f32, 1.0] {
                g.part(Metal).decal(at(0.0, y0 + h * 0.5, side * (r + 0.02)), long, Vec3::Y, len * 0.3, r * 0.6, across * side, rgba(l.mark));
            }
        }
        return;
    }

    // Corrugated sheet: both sides, the door leaves, the recessed front
    // wall and the roof (a tarp on an open-top box).
    {
        let m = g.part(Container);
        for side in [-1.0f32, 1.0] {
            let q = [at(-hl, wall_a, side * hw), at(hl, wall_a, side * hw), at(hl, wall_b, side * hw), at(-hl, wall_b, side * hw)];
            m.quad_facing(q, across * side, rgba(paint));
        }
        for (end, inset) in [(door, 0.0f32), (-door, 0.08)] {
            let x = end * (hl - inset);
            let q = [at(x, wall_a, -hw), at(x, wall_a, hw), at(x, wall_b, hw), at(x, wall_b, -hw)];
            m.quad_facing(q, long * end, rgba(if inset > 0.0 { mul(paint, 0.9) } else { paint }));
        }
        let (rl, rw) = (hl + rail_out, hw + rail_out);
        if look != BoxLook::OpenTop {
            m.quad_facing([at(-rl, top, -rw), at(rl, top, -rw), at(rl, top, rw), at(-rl, top, rw)], Vec3::Y, rgba(mul(paint, 0.94)));
        }
    }
    if look == BoxLook::OpenTop {
        // The tarp, sagging a little between the roof bows.
        let tarp = pick(rng, &pal.tarp_colors);
        let (rl, rw) = (hl - 0.05, hw - 0.02);
        g.part(Tarp).quad_facing([at(-rl, top - 0.08, -rw), at(rl, top - 0.08, -rw), at(rl, top - 0.08, rw), at(-rl, top - 0.08, rw)], Vec3::Y, rgba(mul(tarp, 0.9)));
        for k in 1..4 {
            let x = -rl + 2.0 * rl * k as f32 / 4.0;
            g.part(Matte).decal(at(x, top - 0.06, 0.0), across, long, wid - 0.1, 0.06, Vec3::Y, rgba(mul(tarp, 0.6)));
        }
    }
    // Top and bottom rails all round (headers and sills at the ends), and
    // the underside where the unit is off the ground.
    {
        let m = g.part(Matte);
        let (rl, rw) = (hl + rail_out, hw + rail_out);
        let corner = |a: f32, y: f32, b: f32| at(a * rl, y, b * rw);
        box_sides(m, corner, wall_b, top, rgba(frame_col));
        box_sides(m, corner, y0, wall_a, rgba(mul(frame_col, 0.85)));
        if y0 > 0.05 {
            m.quad_facing([at(-rl, y0, -rw), at(-rl, y0, rw), at(rl, y0, rw), at(rl, y0, -rw)], Vec3::NEG_Y, rgba(mul(frame_col, 0.5)));
        }
        // Corner posts on the long sides, with a casting at the top of each.
        for side in [-1.0f32, 1.0] {
            for end in [-1.0f32, 1.0] {
                m.decal(at(end * (hl - 0.09), (y0 + top) * 0.5, side * (hw + post_out)), long, Vec3::Y, 0.18, h, across * side, rgba(frame_col));
                m.decal(at(end * (hl - 0.09), top - 0.09, side * (hw + post_out + 0.006)), long, Vec3::Y, 0.2, 0.18, across * side, rgba(mul(frame_col, 0.75)));
            }
        }
        // Front end: two posts framing the recessed wall.
        for side in [-1.0f32, 1.0] {
            m.decal(at(-door * (hl + post_out), (y0 + top) * 0.5, side * (hw - 0.09)), across, Vec3::Y, 0.18, h, long * -door, rgba(frame_col));
        }
        // Door end: hinge posts at the outer edges, four locking bars, the
        // seam between the leaves.
        let dx = door * (hl + post_out);
        for side in [-1.0f32, 1.0] {
            m.decal(at(dx, (y0 + top) * 0.5, side * (hw - 0.11)), across, Vec3::Y, 0.22, h, long * door, rgba(mul(frame_col, 0.8)));
            for k in [0.3f32, 0.72] {
                m.decal(at(door * (hl + 0.03), wall_mid, side * hw * k), across, Vec3::Y, 0.05, wall_h + 0.1, long * door, rgba(mix(mul(paint, 0.6), pal.steel, 0.25)));
            }
        }
        m.decal(at(door * (hl + 0.02), wall_mid, 0.0), across, Vec3::Y, 0.035, wall_h, long * door, rgba(DARK));
        // ID marking stencilled on one leaf (a real box's number is
        // stencilled on by `box_number`).
        if life.is_none() {
            m.decal(at(door * (hl + 0.025), wall_b - 0.32, -door * hw * 0.5), across, Vec3::Y, 0.7, 0.18, long * door, rgba(mix(pal.chalk, paint, 0.25)));
        }
        if look == BoxLook::Reefer {
            // [r4] The refrigeration unit filling the front end: a dark
            // grille, the fan behind it and the control box.
            let fx = -door * (hl + post_out + 0.01);
            m.decal(at(fx, wall_mid + wall_h * 0.12, 0.0), across, Vec3::Y, wid - 0.5, wall_h * 0.62, long * -door, rgba(mul(CHARCOAL, 0.8)));
            m.decal(at(fx - door * 0.005, wall_mid + wall_h * 0.18, -hw * 0.3), across, Vec3::Y, 0.75, 0.75, long * -door, rgba(DARK));
            m.decal(at(fx - door * 0.005, wall_a + wall_h * 0.18, hw * 0.35), across, Vec3::Y, 0.5, 0.36, long * -door, rgba(mix(pal.steel, paint, 0.3)));
        }
    }
    // Livery and wear: at most three more decals per unit (a real box
    // shows all its patches and dents).
    let mut extras = 0;
    let cap = if life.is_some() { 8 } else { 3 };
    let side_decal = |g: &mut Geometry, side: f32, lu: f32, y: f32, off: f32, w: f32, hh: f32, col: [f32; 3]| {
        g.part(Container).decal(at(lu, y, side * (hw + off)), long, Vec3::Y, w, hh, across * side, rgba(col));
    };
    let lum = |c: [f32; 3]| 0.3 * c[0] + 0.55 * c[1] + 0.15 * c[2];
    match livery {
        Livery::Line(l) => {
            // A line paints its mark on both sides, the same on every box.
            let mark = mix(if look == BoxLook::Reefer { l.body } else { l.mark }, paint, 0.15 * wear);
            let bh = (wall_h * 0.18).min(0.42);
            let (bw, bh2) = ((len * 0.3).clamp(1.2, 3.2), (wall_h * 0.36).min(0.95));
            for side in [-1.0f32, 1.0] {
                if l.band {
                    side_decal(g, side, 0.0, wall_b - bh * 0.5 - 0.3, 0.01, len - 0.6, bh, mark);
                } else {
                    side_decal(g, side, side * (hl - bw * 0.5 - 0.5) * door, wall_mid + 0.2, 0.01, bw, bh2, mark);
                }
            }
            extras += 1;
        }
        Livery::Any(_) => {
            let marks = &pal.container_marks;
            if coin(rng, 0.4 * (1.0 - 0.4 * wear)) && !marks.is_empty() {
                // A contrasting colour from the line's livery list.
                let mut mark = pick(rng, marks);
                if (lum(mark) - lum(paint)).abs() < 0.12 {
                    mark = pick(rng, marks);
                }
                let mark = mix(mark, paint, 0.15 * wear);
                let side = if rng.random_bool(0.5) { 1.0f32 } else { -1.0 };
                if rng.random_bool(0.5) {
                    // A band along the upper part of the side.
                    let bh = rng.random_range(0.25..0.5f32).min(wall_h * 0.25);
                    side_decal(g, side, 0.0, wall_b - bh * 0.5 - rng.random_range(0.1..0.5), 0.01, len - 0.6, bh, mark);
                } else {
                    // A logo block towards one end.
                    let (bw, bh) = (rng.random_range(1.0..2.2f32).min(len * 0.4), rng.random_range(0.5..1.0f32).min(wall_h * 0.5));
                    let lu = rng.random_range(-1.0..1.0f32) * (hl - bw * 0.5 - 0.4).max(0.0);
                    side_decal(g, side, lu, wall_mid + rng.random_range(0.0..0.3), 0.01, bw, bh, mark);
                }
                extras += 1;
            }
        }
    }
    let patches = life.map_or(if coin(rng, 0.1 + 0.25 * wear) { 1 } else { 0 }, |l| l.patches.min(3));
    for _ in 0..patches {
        if extras >= cap {
            break;
        }
        // A repainted patch in not-quite-matching paint, or bare primer.
        let col = if rng.random_bool(0.7) { container_paint(rng, paint, 0.0) } else { mix([0.42, 0.40, 0.37], paint, 0.2) };
        let (pw, ph) = (rng.random_range(0.6..2.4f32).min(len * 0.4), rng.random_range(0.5..1.6f32).min(wall_h * 0.8));
        let lu = rng.random_range(-1.0..1.0f32) * (hl - pw * 0.5 - 0.3).max(0.0);
        let side = if rng.random_bool(0.5) { 1.0f32 } else { -1.0 };
        let y = wall_a + ph * 0.5 + rng.random_range(0.0..(wall_h - ph).max(0.01));
        side_decal(g, side, lu, y, 0.013, pw, ph, col);
        extras += 1;
    }
    let dents = life.map_or(if coin(rng, 0.15 + 0.4 * wear) { 1 } else { 0 }, |l| l.dents.min(3));
    for _ in 0..dents {
        if extras >= cap {
            break;
        }
        // A dent from a twistlock or a forklift: a darker patch low down.
        let (dw, dh) = (rng.random_range(0.3..0.9f32), rng.random_range(0.2..0.6f32));
        let lu = rng.random_range(-1.0..1.0f32) * (hl - dw * 0.5 - 0.3).max(0.0);
        let side = if rng.random_bool(0.5) { 1.0f32 } else { -1.0 };
        side_decal(g, side, lu, wall_a + dh * 0.5 + rng.random_range(0.0..0.6), 0.016, dw, dh, mul(paint, 0.68));
        extras += 1;
    }
    // Rust bleeding down from the top rail and out of the castings.
    let room = if life.is_some() { 4 } else { 3usize.saturating_sub(extras) };
    let streaks = ((wear * 2.6 + rng.random_range(0.0..0.8)) as usize).min(room);
    for _ in 0..streaks {
        let side = if rng.random_bool(0.5) { 1.0f32 } else { -1.0 };
        let at_casting = rng.random_bool(0.4);
        let sw = if at_casting { rng.random_range(0.2..0.4f32) } else { rng.random_range(0.12..0.5f32) };
        let sh = rng.random_range(0.4..1.0f32) * wall_h * (0.4 + 0.6 * wear);
        let lu = if at_casting { (hl - 0.32) * if rng.random_bool(0.5) { 1.0 } else { -1.0 } } else { rng.random_range(-1.0..1.0f32) * (hl - 0.5).max(0.0) };
        let rust = mix(pal.rust_stain, pal.rust, rng.random_range(0.0..0.6));
        streak_decal(g.part(Container), at(lu, wall_b - sh * 0.5, side * (hw + 0.02)), long, sw, sh, across * side, rust, mix(rust, paint, 0.85));
    }
}

/// A steel flat bar from `a` to `b`, `width` wide in the plane whose
/// normal is `n`: a cross brace. Two faces, no edges.
pub fn flat_bar(m: &mut MeshData, a: Vec3, b: Vec3, width: f32, n: Vec3, color: [f32; 3]) {
    let side = (b - a).cross(n).normalize_or_zero() * (width * 0.5);
    if side == Vec3::ZERO {
        return;
    }
    let q = [a - side, b - side, b + side, a + side];
    m.quad_facing(q, n, rgba(color));
    m.quad_facing(q, -n, rgba(mul(color, 0.8)));
}

/// Section of the I-beam props: flange width and section depth 0.2 m.
pub const IBEAM_FLANGE: f32 = 0.2;

/// A steel I-beam prop: two flanges and a web from `base` to `top`, at
/// lot-local `p`, flanges parallel to `yaw` (relative to the frame). On
/// the ground it stands on a base plate on a small concrete pad; on a roof,
/// on the plate alone. A cap plate takes the load at the top. Rust-brown,
/// each one its own shade; 22 triangles, 24 with the pad.
#[allow(clippy::too_many_arguments)]
pub fn ibeam_support(g: &mut Geometry, c: &Ctx, rng: &mut impl Rng, p: Vec2, yaw: f32, base: f32, top: f32, on_ground: bool) {
    let f = &c.frame;
    let pal = c.palette;
    let a = f.yaw() + yaw;
    let w = f.to_world(p.x, p.y);
    let (s, co) = a.sin_cos();
    let at = |x: f32, y: f32, z: f32| Vec3::new(w.x + x * co - z * s, y, w.y + x * s + z * co);
    let rust = mul(mix(pal.rust, pal.rust_stain, rng.random_range(0.0..0.7)), rng.random_range(0.8..1.1));
    let plate = mul(pal.rust_stain, 0.8);
    let mut y = base;
    if on_ground {
        // A pad cast flush with the yard, just proud of the dirt.
        y += 0.07;
        g.part(Concrete).quad_facing([at(-0.3, y, -0.3), at(0.3, y, -0.3), at(0.3, y, 0.3), at(-0.3, y, 0.3)], Vec3::Y, rgba(mul(pal.concrete(0), rng.random_range(0.7..0.9))));
    }
    // Base plate: a thin sheet, its top is all that shows.
    y += 0.02;
    g.part(Matte).quad_facing([at(-0.18, y, -0.18), at(0.18, y, -0.18), at(0.18, y, 0.18), at(-0.18, y, 0.18)], Vec3::Y, rgba(plate));
    // Cap plate bolted under the container: its top is hidden against the
    // floor, its underside only seen from below, so just the edges.
    let y1 = top - 0.05;
    box_sides(g.part(Matte), |x, yy, z| at(x * 0.17, yy, z * 0.17), y1, top, rgba(plate));
    if y1 <= y {
        return;
    }
    // Section: flanges 0.025 thick, a 0.018 web.
    let (b, d, tf, tw) = (IBEAM_FLANGE, IBEAM_FLANGE, 0.025f32, 0.018f32);
    // Each flange is its outer and inner face (its 25 mm edges are a pixel
    // at street distance), the web its two faces: six quads.
    let m = g.part(Rust);
    let across = Vec3::new(-s, 0.0, co);
    for side in [-1.0f32, 1.0] {
        for (z, face) in [(side * d * 0.5, side), (side * (d * 0.5 - tf), -side)] {
            let col = if face == side { rust } else { mul(rust, 0.75) };
            m.quad_facing([at(-b * 0.5, y, z), at(b * 0.5, y, z), at(b * 0.5, y1, z), at(-b * 0.5, y1, z)], across * face, rgba(col));
        }
    }
    for side in [-1.0f32, 1.0] {
        let x = side * tw * 0.5;
        let z = d * 0.5 - tf;
        m.quad_facing([at(x, y, -z), at(x, y, z), at(x, y1, z), at(x, y1, -z)], Vec3::new(co, 0.0, s) * side, rgba(mul(rust, 0.85)));
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Vehicle {
    Car,
    Van,
    Truck,
    Wreck,
    Bus,
    /// [economy] A garbage truck, never a panel lorry.
    GarbageTruck,
    /// [economy] A box lorry, never a garbage truck.
    Lorry,
}

impl Vehicle {
    /// Footprint: half-length and half-width, plus how far its centre sits
    /// ahead of the anchor point.
    fn footprint(self) -> (f32, f32, f32) {
        match self {
            Vehicle::Car => (2.1, 0.95, 0.0),
            Vehicle::Van => (2.6, 1.0, 0.0),
            Vehicle::Truck | Vehicle::GarbageTruck | Vehicle::Lorry => (4.9, 1.2, -0.3),
            Vehicle::Wreck => (2.0, 0.9, 0.0),
            Vehicle::Bus => (5.75, 1.25, 0.0),
        }
    }
}

/// Whether a vehicle at `p` facing `yaw` (relative to the frame) stands
/// wholly on the lot. Checking only the centre parked lorries in the road.
pub fn vehicle_fits(c: &Ctx, p: Vec2, yaw: f32, kind: Vehicle) -> bool {
    let (hl, hw, ahead) = kind.footprint();
    let d = Vec2::new(yaw.cos(), yaw.sin());
    let n = Vec2::new(-d.y, d.x);
    let centre = p + d * ahead;
    c.all_inside(&[centre + d * hl + n * hw, centre + d * hl - n * hw, centre - d * hl + n * hw, centre - d * hl - n * hw, centre])
}

/// A vehicle standing on a surface at height `y` (a deck, the ground),
/// facing `yaw` relative to the frame. Parking decks, depots and yards all
/// draw their cars through here; the generator is `crate::vehicles`.
#[allow(clippy::too_many_arguments)]
pub fn vehicle(g: &mut Geometry, f: &Frame, pal: &Palette, rng: &mut impl Rng, p: Vec2, y: f32, yaw: f32, kind: Vehicle, color: [f32; 3]) {
    use crate::vehicles::Parked;
    let kind = match kind {
        Vehicle::Car => Parked::Car,
        Vehicle::Van => Parked::Van,
        Vehicle::Truck => Parked::Truck,
        Vehicle::Wreck => Parked::Wreck,
        Vehicle::Bus => Parked::Bus,
        Vehicle::GarbageTruck => Parked::GarbageTruck,
        Vehicle::Lorry => Parked::Lorry,
    };
    // Trucks sat a little behind their anchor (see the footprint).
    let back = if matches!(kind, Parked::Truck | Parked::GarbageTruck | Parked::Lorry) { -0.3 } else { 0.0 };
    let a = f.yaw() + yaw;
    let w = f.to_world(p.x, p.y) + Vec2::new(a.cos(), a.sin()) * back;
    crate::vehicles::draw_parked(g, pal, rng, w, y, a, kind, color);
}
