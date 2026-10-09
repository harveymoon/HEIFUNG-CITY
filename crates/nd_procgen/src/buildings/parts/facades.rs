//! Facades: windows, window bands, curtain-wall lights, shopfronts,
//! entrances, signs, awnings, balconies, grime and the other wall detail.

#[allow(unused_imports)]
use super::*;

// ── facades ──────────────────────────────────────────────────────────────────

#[derive(Clone, Copy, Debug)]
pub struct WindowSpec {
    pub w: f32,
    pub h: f32,
    pub spacing: f32,
    /// Height of the sill above the floor.
    pub sill: f32,
    pub lit: f32,
    /// Fraction of windows boarded with plywood.
    pub boarded: f32,
    /// Chance of an air conditioner under a window.
    pub ac: f32,
    pub frame: [f32; 3],
}

impl WindowSpec {
    pub fn from_ctx(c: &Ctx) -> Self {
        Self {
            w: c.rules.window_size[0],
            h: c.rules.window_size[1],
            spacing: c.rules.window_spacing,
            sill: (c.fh() - c.rules.window_size[1]) * 0.5,
            lit: c.lit_chance(),
            boarded: c.wear * 0.5,
            ac: c.style.clutter * 0.3,
            frame: c.style.trim_color,
        }
    }
}

/// Punched windows on one face for `floors` storeys starting at `y0`.
#[allow(clippy::too_many_arguments)]
pub fn windows(g: &mut Geometry, c: &Ctx, rng: &mut impl Rng, face: &Face, y0: f32, floors: i32, fh: f32, spec: &WindowSpec) {
    let usable = face.len - 1.4;
    let count = (usable / spec.spacing).floor() as i32;
    if count < 1 || floors < 1 {
        return;
    }
    let step = usable / count as f32;
    let w = spec.w.min(step * 0.8);
    for k in 0..floors {
        let y = y0 + k as f32 * fh + spec.sill + spec.h * 0.5;
        for i in 0..count {
            let t = -usable * 0.5 + (i as f32 + 0.5) * step;
            face.window_frame(g, c, t, y, w, spec.h, 0.24, spec.frame);
            let r: f32 = rng.random();
            if r < spec.boarded {
                face.glaze(g, c, t, y, PANE, w, spec.h, Pane::Boarded);
            } else if coin(rng, spec.lit) {
                let wc = pick(rng, &c.style.window_colors);
                let col = mul(wc, rng.random_range(0.55..1.0));
                face.glaze(g, c, t, y, PANE, w, spec.h, Pane::Lit(col));
            } else {
                face.glaze(g, c, t, y, PANE, w, spec.h, Pane::Dark);
            }
            if coin(rng, spec.ac) {
                ac_unit(g, c, face, t + w * 0.25, y - spec.h * 0.5 - 0.35, 0.32);
            }
        }
    }
}

/// Continuous recessed window band per floor with lit segments.
pub fn band_windows(g: &mut Geometry, c: &Ctx, rng: &mut impl Rng, face: &Face, y0: f32, floors: i32, fh: f32) {
    let f = &c.frame;
    let len = face.len * 0.92;
    let lit = c.lit_chance();
    let segs = (len / 3.0).floor().max(1.0) as i32;
    let seg = len / segs as f32;
    for k in 0..floors {
        let y = y0 + k as f32 * fh + fh * 0.55;
        face.glaze(g, c, 0.0, y, FRAME, len, fh * 0.46, Pane::Dark);
        face.bx(g, f, Matte, 0.0, y - fh * 0.26, 0.05, Vec3::new(len, 0.12, 0.12), c.style.trim_color);
        for i in 0..segs {
            if !coin(rng, lit) {
                continue;
            }
            let t = -len * 0.5 + (i as f32 + 0.5) * seg;
            let wc = pick(rng, &c.style.window_colors);
            let col = mul(wc, rng.random_range(0.5..0.95));
            face.glaze(g, c, t, y, PANE, seg * 0.9, fh * 0.38, Pane::Lit(col));
        }
    }
}

/// Lit segments around a glass tower, per floor, standing just proud of the
/// facets so they read through the glazing.
pub fn curtain_lights(g: &mut Geometry, c: &Ctx, rng: &mut impl Rng, poly: &[Vec2], y0: f32, floors: i32, fh: f32) {
    let lit = c.lit_chance();
    for face in c.faces(poly) {
        let segs = (face.len / 3.2).floor().max(1.0) as i32;
        let seg = face.len / segs as f32;
        for k in 0..floors {
            let y = y0 + k as f32 * fh + fh * 0.5;
            for i in 0..segs {
                if !coin(rng, lit) {
                    continue;
                }
                let t = -face.len * 0.5 + (i as f32 + 0.5) * seg;
                let wc = pick(rng, &c.style.window_colors);
                let col = mul(wc, rng.random_range(0.35..0.8));
                face.glaze(g, c, t, y, PANE, seg * 0.85, fh * 0.42, Pane::Lit(col));
            }
        }
    }
}

/// Glass shopfront with a lit interior, a fascia sign band and shutters where
/// the building is run down.
pub fn storefront(g: &mut Geometry, c: &Ctx, rng: &mut impl Rng, face: &Face, y0: f32, h: f32) {
    let f = &c.frame;
    let len = face.len * 0.9;
    let units = (len / 6.0).floor().max(1.0) as i32;
    let unit = len / units as f32;
    for i in 0..units {
        let t = -len * 0.5 + (i as f32 + 0.5) * unit;
        let shuttered = coin(rng, c.wear * 0.7);
        if shuttered {
            face.decal(g, f, Metal, t, y0 + h * 0.45, 0.06, unit * 0.9, h * 0.9, mul(pick(rng, &c.palette.metal_paint), 0.8));
            grime(g, c, rng, face, y0, y0 + h, 2.0);
        } else {
            face.glaze(g, c, t, y0 + h * 0.45, 0.04, unit * 0.9, h * 0.8, Pane::Dark);
            if coin(rng, 0.5 + 0.4 * c.lit_chance()) {
                let wc = pick(rng, &c.palette.glow_colors);
                face.glaze(g, c, t, y0 + h * 0.45, 0.06, unit * 0.86, h * 0.72, Pane::Lit(mul(wc, 0.6)));
            }
            // door
            face.decal(g, f, Matte, t + unit * 0.3, y0 + 1.1, 0.08, 1.0, 2.2, DARK);
        }
        // fascia and sign
        face.bx(g, f, Matte, t, y0 + h * 0.93, 0.12, Vec3::new(unit * 0.96, h * 0.14, 0.2), mul(c.style.trim_color, 0.8));
        if coin(rng, c.rules.sign_chance) {
            sign_board(g, c, rng, face, t, y0 + h * 0.93, unit * 0.7, h * 0.1);
        }
        if rng.random_bool(0.35) {
            awning(g, c, rng, face, t, y0 + h * 0.78, unit * 0.8);
        }
    }
}

/// Roll-up door with a lit lamp above it.
pub fn roller_door(g: &mut Geometry, c: &Ctx, face: &Face, t: f32, w: f32, h: f32, color: [f32; 3]) {
    let f = &c.frame;
    face.decal(g, f, Metal, t, h * 0.5, 0.05, w, h, color);
    face.bx(g, f, Matte, t, h + 0.15, 0.08, Vec3::new(w + 0.4, 0.3, 0.14), CHARCOAL);
    face.bx(g, f, Glow, t, h + 0.5, 0.25, Vec3::new(0.5, 0.18, 0.3), c.palette.street_lamp);
}

/// Recessed doorway with glass doors, a canopy and lamps.
pub fn entrance(g: &mut Geometry, c: &Ctx, rng: &mut impl Rng, face: &Face, t: f32, w: f32, h: f32) {
    let f = &c.frame;
    face.bx(g, f, Matte, t, h * 0.5, -0.3, Vec3::new(w + 0.6, h, 0.8), DARK);
    // Half a centimetre off the grid: mullions 12 cm deep (podium office)
    // put their faces at exactly 12 cm, and the door glass flickered on them.
    face.glaze(g, c, t, h * 0.5, 0.125, w, h * 0.95, Pane::Dark);
    let lit = coin(rng, 0.6 + 0.4 * c.lit_chance());
    if lit {
        let col = mul(pick(rng, &c.style.window_colors), 0.7);
        face.glaze(g, c, t, h * 0.5, 0.145, w * 0.9, h * 0.85, Pane::Lit(col));
    }
    canopy(g, c, face, t, h + 0.3, w + 1.6, 2.0, mul(c.style.trim_color, 0.9));
    // Doorstep paving, as deep as the forecourt allows.
    let step = (2.2f32).min(c.reach_out(face) - 0.2);
    if step > 0.4 {
        face.bx(g, f, Matte, t, 0.08, step * 0.5, Vec3::new(w + 1.2, 0.16, step), mul(c.palette.paving, 1.1));
    }
}

/// Canopy over a face, `depth` out from it but never past the lot edge.
pub fn canopy(g: &mut Geometry, c: &Ctx, face: &Face, t: f32, y: f32, w: f32, depth: f32, color: [f32; 3]) {
    let f = &c.frame;
    let depth = depth.min(c.reach_out(face) - 0.2);
    if depth < 0.5 {
        return;
    }
    face.bx(g, f, Panel, t, y, depth * 0.5, Vec3::new(w, 0.25, depth), color);
    face.bx(g, f, Glow, t, y - 0.16, depth * 0.5, Vec3::new(w * 0.5, 0.06, depth * 0.3), mul(c.palette.lamp, 0.8));
}

pub fn awning(g: &mut Geometry, c: &Ctx, rng: &mut impl Rng, face: &Face, t: f32, y: f32, w: f32) {
    let f = &c.frame;
    let col = pick(rng, &c.palette.awning_colors);
    let col = mul(col, 1.0 - 0.4 * c.wear);
    face.bx(g, f, Tarp, t, y, 0.7, Vec3::new(w, 0.1, 1.4), col);
    face.bx(g, f, Tarp, t, y - 0.2, 1.4, Vec3::new(w, 0.4, 0.06), col);
    for side in [-1.0f32, 1.0] {
        face.bx(g, f, Matte, t + side * w * 0.48, y - 0.1, 0.7, Vec3::new(0.05, 0.2, 1.4), CHARCOAL);
    }
}

/// Sign: dark backing with blocks of glowing lettering.
pub fn sign_board(g: &mut Geometry, c: &Ctx, rng: &mut impl Rng, face: &Face, t: f32, y: f32, w: f32, h: f32) {
    let f = &c.frame;
    let sc = pick(rng, &c.palette.sign_colors);
    let k = 1.0 - 0.6 * c.wear * rng.random::<f32>();
    face.bx(g, f, Matte, t, y, 0.16, Vec3::new(w + 0.2, h + 0.2, 0.12), DARK);
    let letters = ((w / (h * 0.8)).floor() as i32).clamp(2, 12);
    let lw = w / letters as f32;
    for i in 0..letters {
        if coin(rng, 0.15 + 0.5 * c.wear) && i % 3 == 1 {
            continue; // dead letters
        }
        let lt = t - w * 0.5 + (i as f32 + 0.5) * lw;
        face.decal(g, f, Glow, lt, y, 0.23, lw * 0.7, h * 0.7, mul(sc, k));
    }
}

/// Tall narrow sign hung off the facade.
pub fn vertical_sign(g: &mut Geometry, c: &Ctx, rng: &mut impl Rng, face: &Face, t: f32, y0: f32, h: f32) {
    let f = &c.frame;
    let sc = pick(rng, &c.palette.sign_colors);
    let w = 1.1;
    face.bx(g, f, Matte, t, y0 + h * 0.5, 0.8, Vec3::new(w + 0.2, h + 0.2, 0.3), DARK);
    for k in 0..2 {
        face.bx(g, f, Matte, t, y0 + h * (0.25 + 0.5 * k as f32), 0.35, Vec3::new(0.08, 0.08, 0.7), CHARCOAL);
    }
    let cells = (h / (w * 0.9)).floor().max(1.0) as i32;
    for i in 0..cells {
        if coin(rng, c.wear * 0.4) {
            continue;
        }
        let y = y0 + (i as f32 + 0.5) * (h / cells as f32);
        face.decal(g, f, Glow, t, y, 0.96, w * 0.7, (h / cells as f32) * 0.7, mul(sc, 0.9));
    }
}

/// A framed LED billboard: a grid of brightly lit cells in two colours plus a
/// dim frame. Dead pixels appear with wear.
pub fn led_screen(g: &mut Geometry, c: &Ctx, rng: &mut impl Rng, face: &Face, t: f32, y: f32, w: f32, h: f32) {
    let f = &c.frame;
    face.bx(g, f, Matte, t, y, 0.18, Vec3::new(w + 0.4, h + 0.4, 0.36), DARK);
    let a = pick(rng, &c.palette.led_colors);
    let b = pick(rng, &c.palette.led_colors);
    let cols = 5;
    let rows = 3;
    let cw = w / cols as f32;
    let ch = h / rows as f32;
    for i in 0..cols {
        for j in 0..rows {
            if coin(rng, c.wear * 0.5) {
                continue;
            }
            let tt = ((i as f32 / cols as f32 + j as f32 / rows as f32) * 0.5 + rng.random_range(-0.2..0.2)).clamp(0.0, 1.0);
            let col = mix(a, b, tt);
            let k = rng.random_range(0.6..1.1);
            face.decal(g, f, Glow, t - w * 0.5 + (i as f32 + 0.5) * cw, y - h * 0.5 + (j as f32 + 0.5) * ch, 0.37, cw * 0.96, ch * 0.96, mul(col, k));
        }
    }
}

/// Roof billboard on steel legs.
pub fn billboard(g: &mut Geometry, c: &Ctx, rng: &mut impl Rng, p: Vec2, yaw: f32, y_base: f32, w: f32, h: f32) {
    let f = &c.frame;
    let face = Face::from_edge(p - Vec2::new(yaw.cos(), yaw.sin()) * w * 0.5, p + Vec2::new(yaw.cos(), yaw.sin()) * w * 0.5);
    let legs = 1.2;
    for side in [-0.4f32, 0.4] {
        face.bx(g, f, Matte, side * w, y_base + legs * 0.5, 0.0, Vec3::new(0.25, legs, 0.25), c.palette.steel);
    }
    led_screen(g, c, rng, &face, 0.0, y_base + legs + h * 0.5, w, h);
}

/// Rust and soot streaks on a face between two heights.
pub fn grime(g: &mut Geometry, c: &Ctx, rng: &mut impl Rng, face: &Face, y0: f32, y1: f32, amount: f32) {
    let f = &c.frame;
    let n = ((c.style.grime + c.wear) * amount * rng.random_range(0.5..1.5)) as i32;
    for _ in 0..if face.len < 0.5 { 0 } else { n } {
        let t = rng.random_range(-face.len * 0.45..face.len * 0.45);
        let sh = rng.random_range(0.8..(y1 - y0).max(1.0));
        face.decal(g, f, Rust, t, y1 - sh * 0.5, 0.02, rng.random_range(0.3..1.2), sh, c.palette.rust_stain);
    }
    // A run-down building gets tagged at street level whatever its style.
    if c.wear > 0.5 && y0 < 2.5 {
        graffiti(g, c, rng, face, y0, (y0 + 3.0).min(y1), (amount * 0.5).clamp(0.6, 2.5));
    }
}

/// Vertical downpipes with a rust patch where they leak.
pub fn downpipes(g: &mut Geometry, c: &Ctx, rng: &mut impl Rng, face: &Face, y_top: f32) {
    let f = &c.frame;
    let n = (face.len / 9.0).floor() as i32 + 1;
    for i in 0..n {
        let t = -face.len * 0.5 + 0.6 + i as f32 * (face.len - 1.2) / n.max(1) as f32;
        face.bx(g, f, Metal, t, y_top * 0.5, 0.12, Vec3::new(0.14, y_top, 0.14), mul(c.style.trim_color, 0.7));
        if coin(rng, c.wear) {
            face.decal(g, f, Rust, t, y_top * 0.3, 0.03, 0.5, y_top * 0.4, c.palette.rust_stain);
        }
    }
}

/// Zigzag fire escape down a face: landings per floor, diagonal stair runs.
#[allow(clippy::too_many_arguments)]
pub fn fire_escape(g: &mut Geometry, c: &Ctx, rng: &mut impl Rng, face: &Face, t: f32, y0: f32, floors: i32, fh: f32) {
    let f = &c.frame;
    let steel = mul(c.palette.steel, 0.4 + 0.2 * rng.random::<f32>());
    // Rusted through once the building is run down.
    let (s, steel) = outdoor_steel(c, steel);
    let rail = if s == Rust { Rust } else { Matte };
    let w = 4.0;
    let d = 1.1;
    for k in 1..floors {
        let y = y0 + k as f32 * fh;
        face.bx(g, f, s, t, y, d * 0.5, Vec3::new(w, 0.08, d), steel);
        face.bx(g, f, rail, t, y + 0.5, d - 0.03, Vec3::new(w, 0.04, 0.04), steel);
        face.bx(g, f, rail, t, y + 1.0, d - 0.03, Vec3::new(w, 0.04, 0.04), steel);
        for side in [-1.0f32, 1.0] {
            face.bx(g, f, rail, t + side * w * 0.5, y + 0.5, d - 0.03, Vec3::new(0.05, 1.0, 0.05), steel);
        }
        // stair run to the floor below, alternating direction
        let dir = if k % 2 == 0 { 1.0 } else { -1.0 };
        let a = f.at2(face.at(t - dir * w * 0.45, d * 0.5), y);
        let b = f.at2(face.at(t + dir * w * 0.45, d * 0.5), y - fh + 0.1);
        g.part(s).beam(a, b, 0.5, rgba(steel));
    }
    // ladder down from the first landing
    face.bx(g, f, rail, t + w * 0.4, y0 + fh * 0.6, d * 0.5, Vec3::new(0.5, fh * 0.9, 0.05), steel);
}

#[allow(clippy::too_many_arguments)]
pub fn balcony(g: &mut Geometry, c: &Ctx, face: &Face, t: f32, y: f32, w: f32, depth: f32, slab: [f32; 3], rail: [f32; 3]) {
    let f = &c.frame;
    face.bx(g, f, Concrete, t, y, depth * 0.5, Vec3::new(w, 0.22, depth), slab);
    // Rail panels are decals: the front faces the street, each side faces
    // along the wall. Nobody stands on the balcony to see their backs, and a
    // tower has hundreds of them.
    face.decal(g, f, Matte, t, y + 0.6, depth - 0.05, w, 1.0, rail);
    let along = f.dir3(face.tangent);
    let out = f.dir3(face.out);
    for side in [-1.0f32, 1.0] {
        let centre = f.at2(face.at(t + side * (w * 0.5 - 0.03), depth * 0.5), y + 0.6);
        g.part(Matte).decal(centre, out, Vec3::Y, depth, 1.0, along * side, rgba(rail));
    }
}

pub fn ac_unit(g: &mut Geometry, c: &Ctx, face: &Face, t: f32, y: f32, d: f32) {
    // Under a ground-floor window on a low storey it would sink into the
    // ground.
    if y < 0.35 {
        return;
    }
    let f = &c.frame;
    face.bx(g, f, Metal, t, y, d, Vec3::new(0.8, 0.6, 0.35), mul(c.palette.steel, 0.9));
    face.decal(g, f, Matte, t, y, d + 0.18, 0.5, 0.5, DARK);
}
