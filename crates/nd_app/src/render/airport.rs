//! [airport] Airports drawn: the field (runways with their markings,
//! taxiways, the apron and its stands, the terminal with jet bridges, the
//! tower, hangars, the cargo shed, car parks, the fuel farm, the fence),
//! the runway, taxiway and approach lights with the sequenced flashers
//! running in to each runway end, the tower's rotating beacon, the planes
//! (one shared mesh per type and airline, gear down when it should be),
//! and the ground vehicles round the planes on their stands.
//!
//! The field is one batch of meshes per airport, made again only when the
//! airport changes; planes are a handful of entities each, moved every
//! frame from `nd_sim::airport::plane_pose` (the sim's own reckoning, so
//! what is drawn is where the sim says the plane is).

use super::*;
use nd_sim::airport::{self as ap, Airport, AirportId, PlaneKind, PlanePhase, Rect};

#[derive(Component)]
pub(super) struct AirportTag;

/// The entities of one plane: its body (one per surface), the gear, the
/// tug that pushes it back.
struct Plane3d {
    body: Vec<Entity>,
    gear: Vec<Entity>,
    tug: Vec<Entity>,
}

struct Field {
    key: u64,
    ents: Vec<Entity>,
    beacon: Vec<Entity>,
    /// The sequenced flashers: entity, runway end (world), outward (unit).
    rabbits: Vec<(Entity, SVec2, SVec2, f32)>,
    /// The apron's ground vehicles, by the stands taken.
    apron_key: u64,
    apron: Vec<Entity>,
    base: f32,
}

#[derive(Resource, Default)]
pub(super) struct AirportRender {
    fields: HashMap<AirportId, Field>,
    planes: HashMap<(AirportId, u32), Plane3d>,
    bodies: HashMap<(PlaneKind, u8), Vec<(Surface, Handle<Mesh>)>>,
    gear: HashMap<PlaneKind, Vec<(Surface, Handle<Mesh>)>>,
    tug: Vec<(Surface, Handle<Mesh>)>,
    beacon_mesh: Option<(Handle<Mesh>, Handle<Mesh>)>,
    beacon_mats: Option<(Handle<StandardMaterial>, Handle<StandardMaterial>)>,
    rabbit_mesh: Option<Handle<Mesh>>,
    rabbit_mat: Option<Handle<StandardMaterial>>,
    spin: f32,
}

// ── colours ────────────────────────────────────────────────────────────────

const RUNWAY: [f32; 4] = [0.12, 0.12, 0.13, 1.0];
const TAXIWAY: [f32; 4] = [0.16, 0.16, 0.17, 1.0];
const APRON: [f32; 4] = [0.21, 0.21, 0.205, 1.0];
const WHITE_PAINT: [f32; 4] = [0.92, 0.92, 0.9, 1.0];
const YELLOW_PAINT: [f32; 4] = [0.95, 0.72, 0.12, 1.0];
const LIGHT_WHITE: [f32; 4] = [5.0, 4.8, 4.2, 1.0];
const LIGHT_BLUE: [f32; 4] = [0.35, 0.8, 6.0, 1.0];
const LIGHT_GREEN: [f32; 4] = [0.3, 6.0, 0.8, 1.0];
const LIGHT_RED: [f32; 4] = [6.0, 0.35, 0.25, 1.0];
const LIGHT_AMBER: [f32; 4] = [6.0, 3.4, 0.7, 1.0];
const NEON_CYAN: [f32; 4] = [0.3, 3.2, 4.5, 1.0];
const NEON_MAGENTA: [f32; 4] = [4.2, 0.4, 3.0, 1.0];
const WARM_WINDOW: [f32; 4] = [3.4, 2.6, 1.6, 1.0];

/// Airline liveries (fictional): tail and cheatline colours.
fn livery(airline: u8) -> [f32; 4] {
    const C: [[f32; 4]; 8] = [
        [0.08, 0.32, 0.36, 1.0],
        [0.62, 0.12, 0.42, 1.0],
        [0.72, 0.34, 0.1, 1.0],
        [0.1, 0.5, 0.72, 1.0],
        [0.2, 0.5, 0.22, 1.0],
        [0.16, 0.2, 0.55, 1.0],
        [0.42, 0.32, 0.16, 1.0],
        [0.68, 0.14, 0.12, 1.0],
    ];
    C[airline as usize % C.len()]
}

fn hash2(a: u64, b: u64) -> u64 {
    ap::mix(a, b)
}

/// Builds in the airport's own frame (x along the runways, y across, h up
/// from the levelled field).
struct Frame<'a> {
    a: &'a Airport,
    base: f32,
    yaw: f32,
}

impl Frame<'_> {
    fn p(&self, x: f32, y: f32) -> SVec2 {
        self.a.world(SVec2::new(x, y))
    }
    fn v(&self, x: f32, y: f32, h: f32) -> Vec3 {
        let p = self.p(x, y);
        Vec3::new(p.x, self.base + h, p.y)
    }
    fn flat(&self, m: &mut MeshData, r: Rect, h: f32, c: [f32; 4]) {
        m.ground_polygon(&[self.p(r.x0, r.y0), self.p(r.x1, r.y0), self.p(r.x1, r.y1), self.p(r.x0, r.y1)], self.base + h, c);
    }
    /// A box from `h0` up, `sx` along by `sy` across by `sz` high.
    fn bx(&self, m: &mut MeshData, cx: f32, cy: f32, h0: f32, sx: f32, sy: f32, sz: f32, c: [f32; 4]) {
        m.boxed(self.v(cx, cy, h0 + sz * 0.5), Vec3::new(sx, sz, sy), self.yaw, c);
    }
    fn bx_open(&self, m: &mut MeshData, cx: f32, cy: f32, h0: f32, sx: f32, sy: f32, sz: f32, c: [f32; 4]) {
        m.boxed_open(self.v(cx, cy, h0 + sz * 0.5), Vec3::new(sx, sz, sy), self.yaw, c);
    }
    /// A vertical face along y = `y`, from x0 to x1, h0 to h1, facing `out` (+1: +y).
    fn face_y(&self, m: &mut MeshData, y: f32, x0: f32, x1: f32, h0: f32, h1: f32, out: f32, c: [f32; 4]) {
        let n = self.a.across() * out;
        m.quad_facing([self.v(x0, y, h0), self.v(x1, y, h0), self.v(x1, y, h1), self.v(x0, y, h1)], Vec3::new(n.x, 0.0, n.y), c);
    }
    fn light(&self, m: &mut MeshData, x: f32, y: f32, h: f32, s: f32, c: [f32; 4]) {
        m.boxed(self.v(x, y, h + s * 0.3), Vec3::new(s, s * 0.6, s), self.yaw, c);
    }
}

// ── the field ──────────────────────────────────────────────────────────────

fn field_key(a: &Airport, base: f32) -> u64 {
    let mut h = DefaultHasher::new();
    (a.size, a.center.x.to_bits(), a.center.y.to_bits(), a.along.x.to_bits(), a.along.y.to_bits(), base.to_bits(), a.building).hash(&mut h);
    h.finish()
}

/// Everything static on the field, in world coordinates.
fn field_geometry(w: &World, a: &Airport, base: f32, grass: [f32; 4]) -> Geometry {
    let f = Frame { a, base, yaw: a.along.y.atan2(a.along.x) };
    let lay = a.layout();
    let intl = a.size == ap::AirportSize::International;
    let mut g = Geometry::default();
    // The infield: rough grass over the levelled ground.
    let (fw, fd) = a.size.size();
    f.flat(g.part(Surface::Foliage), Rect::new(-fw * 0.5, -fd * 0.5, fw * 0.5, fd * 0.5), 0.03, grass);
    // Runways, their markings and lights.
    for r in &lay.runways {
        let (hl, hw) = (r.length * 0.5, r.width * 0.5);
        f.flat(g.part(Surface::Asphalt), Rect::new(-hl - 30.0, r.y - hw - 6.0, hl + 30.0, r.y + hw + 6.0), 0.05, [0.3, 0.31, 0.29, 1.0]);
        f.flat(g.part(Surface::Asphalt), Rect::new(-hl, r.y - hw, hl, r.y + hw), 0.07, RUNWAY);
        let paint = g.part(Surface::Paint);
        // Edge lines and the centreline.
        for s in [-1.0f32, 1.0] {
            f.flat(paint, Rect::new(-hl + 2.0, r.y + s * (hw - 1.4) - 0.45, hl - 2.0, r.y + s * (hw - 1.4) + 0.45), 0.09, WHITE_PAINT);
        }
        let mut x = -hl + 70.0;
        while x < hl - 70.0 {
            f.flat(paint, Rect::new(x, r.y - 0.45, x + 30.0, r.y + 0.45), 0.09, WHITE_PAINT);
            x += 50.0;
        }
        for end in [-1.0f32, 1.0] {
            let xe = end * hl;
            let inward = -end;
            // Threshold piano keys.
            let n = if intl { 8 } else { 6 };
            for side in [-1.0f32, 1.0] {
                for k in 0..n {
                    let yc = r.y + side * (3.4 + k as f32 * ((hw - 5.0) / n as f32));
                    let (xa, xb) = (xe + inward * 6.0, xe + inward * 36.0);
                    f.flat(paint, Rect::new(xa.min(xb), yc - 0.9, xa.max(xb), yc + 0.9), 0.09, WHITE_PAINT);
                }
            }
            // Aiming point and touchdown zone bars.
            for side in [-1.0f32, 1.0] {
                let (xa, xb) = (xe + inward * 300.0, xe + inward * 345.0);
                let yc = r.y + side * (hw * 0.42);
                f.flat(paint, Rect::new(xa.min(xb), yc - 3.0, xa.max(xb), yc + 3.0), 0.09, WHITE_PAINT);
                for (d, bars) in [(150.0f32, 3), (450.0, 2), (600.0, 1)] {
                    for b in 0..bars {
                        let yb = r.y + side * (hw * 0.42 - 3.0 + b as f32 * 3.0);
                        let (xa, xb) = (xe + inward * d, xe + inward * (d + 22.0));
                        f.flat(paint, Rect::new(xa.min(xb), yb - 0.9, xa.max(xb), yb + 0.9), 0.09, WHITE_PAINT);
                    }
                }
            }
        }
        let glow = g.part(Surface::Glow);
        // Edge lights, centreline lights, thresholds.
        let mut x = -hl;
        while x <= hl + 0.1 {
            for s in [-1.0f32, 1.0] {
                f.light(glow, x, r.y + s * (hw + 1.5), 0.1, 1.4, LIGHT_WHITE);
            }
            x += 60.0;
        }
        let mut x = -hl + 15.0;
        while x < hl {
            f.light(glow, x, r.y, 0.07, 0.5, LIGHT_WHITE);
            x += 30.0;
        }
        for end in [-1.0f32, 1.0] {
            let mut y = r.y - hw;
            while y <= r.y + hw + 0.1 {
                f.light(glow, end * (hl + 1.0), y, 0.1, 0.8, LIGHT_GREEN);
                f.light(glow, end * (hl - 2.0), y, 0.1, 0.6, LIGHT_RED);
                y += 3.0;
            }
            // PAPI on the left of the touchdown zone.
            for k in 0..4 {
                let c = if k < 2 { LIGHT_WHITE } else { LIGHT_RED };
                f.light(glow, -end * (hl - 300.0), r.y + hw + 14.0 + k as f32 * 9.0, 0.3, 1.2, c);
            }
        }
    }
    // Taxiways, links, their lines and edge lights.
    for &(y, x0, x1, wd) in &lay.taxiways {
        f.flat(g.part(Surface::Asphalt), Rect::new(x0, y - wd * 0.5, x1, y + wd * 0.5), 0.06, TAXIWAY);
        f.flat(g.part(Surface::Paint), Rect::new(x0, y - 0.25, x1, y + 0.25), 0.085, YELLOW_PAINT);
        let mut x = x0;
        while x <= x1 {
            for s in [-1.0f32, 1.0] {
                f.light(g.part(Surface::Glow), x, y + s * (wd * 0.5 + 1.0), 0.06, 0.55, LIGHT_BLUE);
            }
            x += 45.0;
        }
    }
    let twy_w = lay.taxiways.first().map_or(18.0, |t| t.3);
    for &(x, y0, y1) in &lay.links {
        f.flat(g.part(Surface::Asphalt), Rect::new(x - twy_w * 0.5, y0, x + twy_w * 0.5, y1), 0.055, TAXIWAY);
        f.flat(g.part(Surface::Paint), Rect::new(x - 0.25, y0, x + 0.25, y1), 0.083, YELLOW_PAINT);
        let mut y = y0;
        while y <= y1 {
            f.light(g.part(Surface::Glow), x, y, 0.05, 0.4, LIGHT_GREEN);
            y += 15.0;
        }
    }
    // The apron, its lane and the stands' lead-in lines.
    f.flat(g.part(Surface::Concrete), lay.apron, 0.06, APRON);
    f.flat(g.part(Surface::Paint), Rect::new(lay.apron.x0 + 4.0, lay.apron_lane - 0.25, lay.apron.x1 - 4.0, lay.apron_lane + 0.25), 0.085, YELLOW_PAINT);
    for st in &lay.stands {
        let paint = g.part(Surface::Paint);
        f.flat(paint, Rect::new(st.at.x - 0.25, lay.apron_lane, st.at.x + 0.25, st.at.y + 22.0), 0.086, YELLOW_PAINT);
        f.flat(paint, Rect::new(st.at.x - 4.0, st.at.y + 21.0, st.at.x + 4.0, st.at.y + 22.0), 0.087, YELLOW_PAINT);
        // The stand's safety box.
        let half = if intl && !st.cargo { 29.0 } else { 25.0 };
        for s in [-1.0f32, 1.0] {
            f.flat(paint, Rect::new(st.at.x + s * half - 0.2, st.at.y - 22.0, st.at.x + s * half + 0.2, st.at.y + 26.0), 0.086, [0.9, 0.2, 0.15, 1.0]);
        }
        // Idle kit at the stand's edge: a tug, carts, a stairs truck.
        let k = hash2(st.at.x.to_bits() as u64, 3);
        let gse = g.part(Surface::Panel);
        f.bx(gse, st.at.x - half + 4.0, st.at.y + 20.0, 0.0, 3.2, 1.8, 1.6, [0.9, 0.72, 0.12, 1.0]);
        if k % 2 == 0 {
            for c in 0..3 {
                f.bx(gse, st.at.x - half + 9.0 + c as f32 * 3.4, st.at.y + 20.0, 0.0, 2.8, 1.7, 1.5, [0.5, 0.52, 0.55, 1.0]);
            }
        }
    }
    // Apron floodlight masts.
    let mut x = lay.apron.x0 + 10.0;
    while x < lay.apron.x1 {
        let y = lay.apron.y0 - 4.0;
        f.bx(g.part(Surface::Matte), x, y, 0.0, 0.7, 0.7, 26.0, [0.25, 0.26, 0.28, 1.0]);
        f.bx(g.part(Surface::Matte), x, y, 26.0, 4.0, 1.2, 1.2, [0.2, 0.2, 0.22, 1.0]);
        f.bx(g.part(Surface::Glow), x, y + 0.7, 26.0, 3.6, 0.2, 1.0, LIGHT_AMBER);
        x += if intl { 130.0 } else { 120.0 };
    }
    terminal(&f, &mut g, &lay, intl);
    tower(&f, &mut g, &lay);
    for (i, h) in lay.hangars.iter().enumerate() {
        hangar(&f, &mut g, *h, if intl { 24.0 } else { 14.0 }, i);
    }
    cargo_shed(&f, &mut g, lay.cargo_shed, intl);
    for (i, (r, levels)) in lay.car_parks.iter().enumerate() {
        car_park(&f, &mut g, *r, *levels, i as u64);
    }
    fuel_farm(&f, &mut g, lay.fuel_farm, intl);
    // A windsock by the runway.
    let r = lay.runways[lay.departures];
    let (sx, sy) = (60.0, r.y + r.width * 0.5 + 30.0);
    f.bx(g.part(Surface::Matte), sx, sy, 0.0, 0.25, 0.25, 6.5, [0.3, 0.3, 0.3, 1.0]);
    f.bx(g.part(Surface::Tarp), sx + 2.4, sy, 5.8, 4.5, 0.9, 0.9, [0.95, 0.4, 0.08, 1.0]);
    f.light(g.part(Surface::Glow), sx, sy, 6.5, 0.4, LIGHT_RED);
    // The fence round the field (not across the forecourt).
    let (wd, dd) = a.size.size();
    let (hw, hd) = (wd * 0.5 - 2.0, dd * 0.5 - 2.0);
    let fence = g.part(Surface::Metal);
    for (p, q) in [((-hw, -hd), (hw, -hd)), ((hw, -hd), (hw, hd)), ((-hw, hd), (-hw, -hd))] {
        let (a0, b0) = (f.p(p.0, p.1), f.p(q.0, q.1));
        fence.strip_box(a0, b0, 0.0, 0.08, base, base + 2.4, [0.42, 0.44, 0.45, 1.0]);
    }
    approach_lights(w, &f, &mut g, &lay, intl);
    g
}

/// The approach lighting off both ends of every runway: a barrette every
/// 30 m out along the centreline on poles standing on the ground, and a
/// crossbar at 300 m.
fn approach_lights(w: &World, f: &Frame, g: &mut Geometry, lay: &ap::Layout, intl: bool) {
    let reach = if intl { 720.0 } else { 420.0 };
    for r in &lay.runways {
        for end in [-1.0f32, 1.0] {
            let mut out = 30.0;
            while out <= reach {
                let x = end * (r.length * 0.5 + out);
                let p = f.p(x, r.y);
                let ground = w.surface_height(p).min(f.base + 30.0);
                let top = (f.base + 0.8).max(ground + 0.8);
                let pole = g.part(Surface::Matte);
                pole.boxed(Vec3::new(p.x, (ground + top) * 0.5, p.y), Vec3::new(0.25, (top - ground).max(0.1), 0.25), f.yaw, [0.3, 0.3, 0.32, 1.0]);
                let width = if (out - 300.0).abs() < 1.0 { 30.0 } else { 4.5 };
                let glow = g.part(Surface::Glow);
                glow.boxed(Vec3::new(p.x, top + 0.2, p.y), Vec3::new(0.6, 0.4, width), f.yaw, LIGHT_WHITE);
                out += 30.0;
            }
        }
    }
}

fn terminal(f: &Frame, g: &mut Geometry, lay: &ap::Layout, intl: bool) {
    let t = lay.terminal;
    let h = lay.terminal_height;
    let wall = [0.32, 0.34, 0.37, 1.0];
    f.bx_open(g.part(Surface::Panel), t.center().x, t.center().y, 0.0, t.x1 - t.x0, t.y1 - t.y0, h, wall);
    // Glazed faces, airside and landside, with the lit halls behind.
    for (y, out) in [(t.y0 - 0.15, -1.0f32), (t.y1 + 0.15, 1.0)] {
        f.face_y(g.part(Surface::Glass), y, t.x0 + 3.0, t.x1 - 3.0, 1.5, h - 3.0, out, [0.22, 0.27, 0.32, 1.0]);
        // Mullions, and thin lit bands of the halls behind the glass.
        let mut x = t.x0 + 3.0;
        while x < t.x1 - 3.0 {
            f.face_y(g.part(Surface::Matte), y + out * 0.1, x, x + 0.5, 1.5, h - 3.0, out, [0.15, 0.16, 0.18, 1.0]);
            x += 6.0;
        }
        for band in [0.18f32, 0.52] {
            let z = 1.5 + (h - 4.5) * band;
            f.face_y(g.part(Surface::Glow), y + out * 0.12, t.x0 + 4.0, t.x1 - 4.0, z, z + 1.3, out, WARM_WINDOW);
        }
    }
    // The roof: a deep overhang in two tiers, plant on top, a neon band.
    let roof = [0.2, 0.21, 0.23, 1.0];
    f.bx(g.part(Surface::WornPanel), t.center().x, t.center().y, h, t.x1 - t.x0 + 16.0, t.y1 - t.y0 + 14.0, 1.4, roof);
    f.bx(g.part(Surface::WornPanel), t.center().x, t.center().y, h + 1.4, (t.x1 - t.x0) * 0.7, (t.y1 - t.y0) * 0.55, 3.0, [0.16, 0.17, 0.19, 1.0]);
    // Skylight strips across the lower roof, lit from the halls at night.
    let mut x = t.x0 + 12.0;
    while x < t.x1 - 12.0 {
        for (ya, yb) in [(t.y0 - 4.0, t.center().y - (t.y1 - t.y0) * 0.3), (t.center().y + (t.y1 - t.y0) * 0.3, t.y1 + 4.0)] {
            f.flat(g.part(Surface::Glass), Rect::new(x, ya, x + 3.0, yb), h + 1.42, [0.2, 0.25, 0.3, 1.0]);
            f.flat(g.part(Surface::Glow), Rect::new(x + 1.2, ya + 1.0, x + 1.8, yb - 1.0), h + 1.45, [1.1, 0.9, 0.6, 1.0]);
        }
        x += 18.0;
    }
    let mut x = t.x0 + 30.0;
    while x < t.x1 - 30.0 {
        f.bx(g.part(Surface::Metal), x, t.center().y, h + 4.4, 8.0, 6.0, 2.5, [0.5, 0.5, 0.52, 1.0]);
        x += 55.0;
    }
    for (y, out) in [(t.y0 - 7.1, -1.0f32), (t.y1 + 7.1, 1.0)] {
        f.face_y(g.part(Surface::Glow), y, t.x0 - 7.0, t.x1 + 7.0, h + 0.4, h + 0.9, out, if intl { NEON_CYAN } else { NEON_MAGENTA });
    }
    // The sign over the landside doors.
    let cx = t.center().x;
    f.face_y(g.part(Surface::Matte), t.y1 + 7.3, cx - 32.0, cx + 32.0, h + 1.4, h + 5.4, 1.0, [0.06, 0.06, 0.08, 1.0]);
    let mut gx = cx - 29.0;
    let mut k = 0u64;
    while gx < cx + 28.0 {
        let wd = 1.2 + (hash2(k, 77) % 4) as f32 * 0.7;
        let (z0, z1) = if hash2(k, 5) % 5 == 0 { (h + 2.4, h + 3.6) } else { (h + 2.0, h + 4.6) };
        f.face_y(g.part(Surface::Glow), t.y1 + 7.4, gx, gx + wd, z0, z1, 1.0, if k % 7 == 3 { NEON_CYAN } else { NEON_MAGENTA });
        gx += wd + 0.9 + if hash2(k, 9) % 6 == 0 { 2.0 } else { 0.0 };
        k += 1;
    }
    f.face_y(g.part(Surface::Glow), t.y1 + 7.4, cx - 32.0, cx + 32.0, h + 1.4, h + 1.6, 1.0, NEON_CYAN);
    // The forecourt canopy on columns.
    let cy = t.y1 + 9.0;
    f.bx(g.part(Surface::Panel), cx, cy, 6.0, t.x1 - t.x0 - 20.0, 12.0, 0.6, roof);
    let mut x = t.x0 + 15.0;
    while x < t.x1 - 10.0 {
        f.bx(g.part(Surface::Concrete), x, cy + 4.5, 0.0, 0.6, 0.6, 6.0, [0.6, 0.6, 0.6, 1.0]);
        x += 24.0;
    }
    f.face_y(g.part(Surface::Glow), cy + 6.05, t.x0 - 0.0 + 10.0, t.x1 - 10.0, 5.7, 6.0, 1.0, WARM_WINDOW);
    // Jet bridges to the stands that have them: from the terminal face
    // out to a rotunda, then on to the plane's front door.
    for st in lay.stands.iter().filter(|s| s.bridge) {
        let (bx, by) = (st.at.x - 15.0, (t.y0 + st.at.y) * 0.5 + 12.0);
        let door = SVec2::new(st.at.x - 3.2, st.at.y + 13.5);
        let body = [0.62, 0.63, 0.65, 1.0];
        let hb = 5.2;
        // Fixed link from the terminal to the rotunda.
        f.bx(g.part(Surface::Panel), bx, (t.y0 + by) * 0.5, hb - 1.6, 3.6, t.y0 - by, 3.2, body);
        f.bx(g.part(Surface::Concrete), bx, by, 0.0, 1.4, 1.4, hb - 1.6, [0.5, 0.5, 0.5, 1.0]);
        // The movable tunnel to the door.
        let (a0, b0) = (f.v(bx, by, hb), f.v(door.x, door.y, hb - 0.4));
        g.part(Surface::Panel).slab(a0 + Vec3::Y * 1.6, b0 + Vec3::Y * 1.6, 3.2, 3.2, body);
        g.part(Surface::Glow).slab(a0 + Vec3::Y * 1.68, b0 + Vec3::Y * 1.68, 3.3, 0.06, [1.2, 1.0, 0.7, 1.0]);
        f.bx(g.part(Surface::Matte), door.x + 1.5, door.y + 1.5, 0.0, 1.0, 1.0, hb - 2.0, [0.25, 0.25, 0.25, 1.0]);
    }
    if !intl {
        // A small apron canopy for the walk out to the planes.
        f.bx(g.part(Surface::Panel), t.center().x, t.y0 - 4.0, 3.5, (t.x1 - t.x0) * 0.6, 8.0, 0.4, roof);
    }
}

fn tower(f: &Frame, g: &mut Geometry, lay: &ap::Layout) {
    let h = lay.tower_height;
    let p = f.p(lay.tower.x, lay.tower.y);
    let b = f.base;
    g.part(Surface::Concrete).cylinder_capped(p, 4.2, 3.2, b, b + h - 7.0, 12, false, false, [0.62, 0.62, 0.6, 1.0]);
    g.part(Surface::Concrete).cylinder_capped(p, 7.5, 7.5, b, b + 6.0, 10, false, true, [0.55, 0.55, 0.54, 1.0]);
    g.part(Surface::Panel).cylinder_capped(p, 3.4, 6.4, b + h - 8.0, b + h - 6.6, 12, true, false, [0.4, 0.42, 0.45, 1.0]);
    g.part(Surface::Glass).cylinder_capped(p, 6.0, 6.9, b + h - 6.6, b + h - 2.6, 12, false, false, [0.35, 0.5, 0.55, 1.0]);
    g.part(Surface::Glow).cylinder_capped(p, 5.7, 6.5, b + h - 6.2, b + h - 4.8, 12, false, false, [0.4, 1.6, 1.4, 1.0]);
    g.part(Surface::Panel).cylinder_capped(p, 7.2, 6.2, b + h - 2.6, b + h - 1.8, 12, false, true, [0.3, 0.32, 0.35, 1.0]);
    g.part(Surface::Matte).cylinder_capped(p, 0.25, 0.12, b + h - 1.8, b + h + 7.0, 6, false, true, [0.3, 0.3, 0.3, 1.0]);
    g.part(Surface::Glow).cylinder_capped(p, 0.35, 0.35, b + h + 7.0, b + h + 7.6, 6, true, true, LIGHT_RED);
}

fn hangar(f: &Frame, g: &mut Geometry, r: Rect, h: f32, i: usize) {
    let c = r.center();
    let (sx, sy) = (r.x1 - r.x0, r.y1 - r.y0);
    let wall = if i % 2 == 0 { [0.42, 0.45, 0.47, 1.0] } else { [0.5, 0.44, 0.36, 1.0] };
    f.bx_open(g.part(Surface::Metal), c.x, c.y, 0.0, sx, sy, h * 0.7, wall);
    // A barrel roof along y.
    let n = 8;
    let roof = g.part(Surface::WornMetal);
    for k in 0..n {
        let a0 = std::f32::consts::PI * k as f32 / n as f32;
        let a1 = std::f32::consts::PI * (k + 1) as f32 / n as f32;
        let x0 = c.x - sx * 0.5 * a0.cos();
        let x1 = c.x - sx * 0.5 * a1.cos();
        let z0 = h * 0.7 + h * 0.3 * a0.sin();
        let z1 = h * 0.7 + h * 0.3 * a1.sin();
        roof.quad_facing([f.v(x0, r.y0, z0), f.v(x1, r.y0, z1), f.v(x1, r.y1, z1), f.v(x0, r.y1, z0)], Vec3::Y, wall);
    }
    // The doors face the apron (−y), a lit strip above them.
    f.face_y(g.part(Surface::Matte), r.y0 - 0.1, r.x0 + 4.0, r.x1 - 4.0, 0.0, h * 0.62, -1.0, [0.18, 0.19, 0.2, 1.0]);
    f.face_y(g.part(Surface::Glow), r.y0 - 0.2, r.x0 + 6.0, r.x1 - 6.0, h * 0.64, h * 0.66, -1.0, LIGHT_AMBER);
}

fn cargo_shed(f: &Frame, g: &mut Geometry, r: Rect, intl: bool) {
    let c = r.center();
    let h = if intl { 15.0 } else { 9.0 };
    f.bx_open(g.part(Surface::Metal), c.x, c.y, 0.0, r.x1 - r.x0, r.y1 - r.y0, h, [0.36, 0.38, 0.36, 1.0]);
    f.bx(g.part(Surface::Panel), c.x, c.y, h, r.x1 - r.x0 + 2.0, r.y1 - r.y0 + 2.0, 0.6, [0.48, 0.5, 0.5, 1.0]);
    f.face_y(g.part(Surface::Glow), r.y0 - 0.15, r.x0 + 5.0, r.x0 + 40.0, h - 3.0, h - 1.2, -1.0, LIGHT_AMBER);
    // Truck docks on the landside.
    let mut x = r.x0 + 6.0;
    while x < r.x1 - 6.0 {
        f.face_y(g.part(Surface::Matte), r.y1 + 0.1, x, x + 3.6, 0.0, 4.2, 1.0, [0.2, 0.22, 0.24, 1.0]);
        f.light(g.part(Surface::Glow), x + 1.8, r.y1 + 0.3, 4.6, 0.4, LIGHT_AMBER);
        x += 9.0;
    }
    // Air freight containers waiting on the apron side.
    let mut k = 0u64;
    let mut x = r.x0 + 4.0;
    while x < r.x1 - 4.0 {
        let stack = 1 + (hash2(k, 11) % 2) as i32;
        for s in 0..stack {
            let col = [[0.62, 0.62, 0.6, 1.0], [0.5, 0.3, 0.18, 1.0], [0.24, 0.32, 0.4, 1.0]][(hash2(k, s as u64) % 3) as usize];
            f.bx(g.part(Surface::Container), x, r.y0 - 6.0, s as f32 * 1.65, 2.4, 1.6, 1.6, col);
        }
        k += 1;
        x += 3.1;
    }
}

fn car_park(f: &Frame, g: &mut Geometry, r: Rect, levels: u8, seed: u64) {
    let deck = 3.2;
    let colours = [[0.15, 0.15, 0.17, 1.0], [0.6, 0.6, 0.62, 1.0], [0.42, 0.1, 0.1, 1.0], [0.82, 0.82, 0.8, 1.0], [0.1, 0.18, 0.35, 1.0], [0.3, 0.32, 0.3, 1.0]];
    for lv in 0..levels.max(1) {
        let h = lv as f32 * deck;
        if lv == 0 {
            f.flat(g.part(Surface::Asphalt), r, 0.06, [0.3, 0.3, 0.31, 1.0]);
        } else {
            f.bx(g.part(Surface::Concrete), r.center().x, r.center().y, h - 0.35, r.x1 - r.x0, r.y1 - r.y0, 0.35, [0.58, 0.58, 0.56, 1.0]);
            // Parapets.
            for (y, out) in [(r.y0, -1.0f32), (r.y1, 1.0)] {
                f.face_y(g.part(Surface::Concrete), y, r.x0, r.x1, h, h + 1.0, out, [0.55, 0.55, 0.53, 1.0]);
            }
        }
        // Rows of bays: two back to back every 16 m, cars in most of them.
        let mut y = r.y0 + 3.0;
        let mut row = 0u64;
        while y + 5.0 < r.y1 {
            let mut x = r.x0 + 2.0;
            let mut i = 0u64;
            while x + 2.5 < r.x1 {
                f.flat(g.part(Surface::Paint), Rect::new(x, y, x + 0.12, y + 5.0), h + 0.08, WHITE_PAINT);
                let k = hash2(seed * 1000 + lv as u64 * 97 + row, i);
                if k % 10 < 7 {
                    let c = colours[(k >> 8) as usize % colours.len()];
                    f.bx_open(g.part(Surface::Matte), x + 1.3, y + 2.5, h + 0.25, 1.8, 4.3, 0.75, c);
                    f.bx_open(g.part(Surface::Glass), x + 1.3, y + 2.3, h + 1.0, 1.55, 2.2, 0.5, [0.12, 0.13, 0.15, 1.0]);
                }
                x += 2.6;
                i += 1;
            }
            row += 1;
            y += if row % 2 == 1 { 5.2 } else { 10.8 };
        }
        if levels > 1 {
            // Columns between the decks.
            let mut x = r.x0 + 1.0;
            while x < r.x1 {
                let mut y = r.y0 + 1.0;
                while y < r.y1 {
                    f.bx(g.part(Surface::Concrete), x, y, h, 0.5, 0.5, deck - 0.35, [0.55, 0.55, 0.53, 1.0]);
                    y += 16.0;
                }
                x += 16.0;
            }
        }
    }
    if levels > 1 {
        let h = levels as f32 * deck;
        f.bx(g.part(Surface::Concrete), r.center().x, r.center().y, h - 0.35, r.x1 - r.x0, r.y1 - r.y0, 0.35, [0.58, 0.58, 0.56, 1.0]);
        f.face_y(g.part(Surface::Glow), r.y1 + 0.1, r.x0 + 4.0, r.x0 + 26.0, h + 0.3, h + 2.6, 1.0, NEON_CYAN);
    }
}

fn fuel_farm(f: &Frame, g: &mut Geometry, r: Rect, intl: bool) {
    let n = if intl { 4 } else { 2 };
    let rad = ((r.x1 - r.x0) / (n as f32 + 0.5) * 0.5).min((r.y1 - r.y0) * 0.22);
    for k in 0..n {
        let x = r.x0 + (r.x1 - r.x0) * (k as f32 + 0.5) / n as f32;
        for (j, y) in [r.y0 + (r.y1 - r.y0) * 0.3, r.y0 + (r.y1 - r.y0) * 0.72].into_iter().enumerate() {
            if !intl && j == 1 {
                continue;
            }
            let p = f.p(x, y);
            g.part(Surface::WornMetal).cylinder_capped(p, rad, rad, f.base, f.base + rad * 0.9, 16, false, true, [0.78, 0.78, 0.74, 1.0]);
        }
    }
    let wall = g.part(Surface::Concrete);
    let corners = [f.p(r.x0, r.y0), f.p(r.x1, r.y0), f.p(r.x1, r.y1), f.p(r.x0, r.y1)];
    for i in 0..4 {
        wall.strip_box(corners[i], corners[(i + 1) % 4], 0.0, 0.5, f.base, f.base + 1.4, [0.5, 0.5, 0.48, 1.0]);
    }
}

// ── planes ─────────────────────────────────────────────────────────────────

/// A ring across the fuselage at `x` (the model's nose is +x, up +y).
fn ring_x(x: f32, r: f32, yc: f32, n: usize) -> Vec<Vec3> {
    (0..n).map(|i| {
        let a = i as f32 / n as f32 * std::f32::consts::TAU;
        Vec3::new(x, yc + a.sin() * r, a.cos() * r)
    }).collect()
}

/// A wing-like plate: root chord from `ra` to `rb` (x) at z = `z0`, tip from
/// `ta` to `tb` at z = `z1`, at height y, `thick` thick.
fn plate(m: &mut MeshData, ra: f32, rb: f32, z0: f32, ta: f32, tb: f32, z1: f32, y0: f32, y1: f32, thick: f32, c: [f32; 4]) {
    let top = [Vec3::new(rb, y0 + thick, z0), Vec3::new(ra, y0 + thick, z0), Vec3::new(ta, y1 + thick, z1), Vec3::new(tb, y1 + thick, z1)];
    let bot = top.map(|p| p - Vec3::Y * thick);
    m.quad_facing(top, Vec3::Y, c);
    m.quad_facing(bot, -Vec3::Y, c);
    m.quad_facing([bot[1], bot[2], top[2], top[1]], Vec3::X, c);
    m.quad_facing([bot[0], bot[3], top[3], top[0]], -Vec3::X, c);
}

/// A plane's body in its livery, nose along +x, wheels at y = 0.
fn plane_geometry(kind: PlaneKind, airline: u8) -> Geometry {
    let (len, span) = kind.dims();
    let r = len * if kind == PlaneKind::Widebody { 0.054 } else { 0.056 };
    let yc = r + if kind == PlaneKind::Turboprop { 1.2 } else { 1.7 };
    let body = [0.86, 0.87, 0.88, 1.0];
    let belly = [0.6, 0.62, 0.64, 1.0];
    let tail = livery(airline);
    let mut g = Geometry::default();
    let n = 14;
    let m = g.part(Surface::Panel);
    let xf = len * 0.34;
    let xb = -len * 0.3;
    m.frustum_capped(&ring_x(xb, r, yc, n), &ring_x(xf, r, yc, n), false, false, body);
    // Nose and tail cones.
    m.frustum_capped(&ring_x(xf, r, yc, n), &ring_x(len * 0.46, r * 0.6, yc - r * 0.15, n), false, false, body);
    m.frustum_capped(&ring_x(len * 0.46, r * 0.6, yc - r * 0.15, n), &ring_x(len * 0.5, r * 0.12, yc - r * 0.3, n), false, true, body);
    m.frustum_capped(&ring_x(-len * 0.5, r * 0.18, yc + r * 0.55, n), &ring_x(xb, r, yc, n), true, false, body);
    // The cheatline and the belly.
    for s in [-1.0f32, 1.0] {
        let z = s * (r + 0.03);
        m.quad_facing([Vec3::new(xb, yc - r * 0.05, z), Vec3::new(xf, yc - r * 0.05, z), Vec3::new(xf, yc - r * 0.25, z), Vec3::new(xb, yc - r * 0.25, z)], Vec3::Z * s, tail);
        m.quad_facing([Vec3::new(xb, yc - r * 0.55, z * 0.98), Vec3::new(xf, yc - r * 0.55, z * 0.98), Vec3::new(xf, yc - r * 0.8, z * 0.86), Vec3::new(xb, yc - r * 0.8, z * 0.86)], Vec3::Z * s - Vec3::Y * 0.4, belly);
    }
    // Wings.
    let jet = kind.is_jet();
    let sweep = if jet { len * 0.17 } else { len * 0.02 };
    let (wroot_a, wroot_b) = (len * 0.1, -len * 0.12);
    let wy = if jet { yc - r * 0.6 } else { yc + r * 0.85 };
    let tip_y = wy + if jet { span * 0.035 } else { 0.0 };
    for s in [-1.0f32, 1.0] {
        plate(m, wroot_a, wroot_b, s * r * 0.9, wroot_a - sweep, wroot_a - sweep - len * 0.07, s * span * 0.5, wy, tip_y, 0.45, body);
    }
    // Tailplane and fin.
    let hs = span * 0.32;
    for s in [-1.0f32, 1.0] {
        plate(m, -len * 0.38, -len * 0.47, s * r * 0.3, -len * 0.45, -len * 0.5, s * hs * 0.5, yc + r * 0.5, yc + r * 0.7, 0.25, body);
    }
    let fin_h = len * 0.17 + r;
    let fin = [Vec3::new(-len * 0.34, yc + r * 0.6, 0.0), Vec3::new(-len * 0.49, yc + r * 0.6, 0.0), Vec3::new(-len * 0.52, yc + fin_h, 0.0), Vec3::new(-len * 0.43, yc + fin_h, 0.0)];
    for s in [-1.0f32, 1.0] {
        m.quad_facing(fin.map(|p| p + Vec3::Z * 0.22 * s), Vec3::Z * s, tail);
    }
    // Engines.
    let metal = g.part(Surface::Metal);
    let er = if kind == PlaneKind::Widebody { r * 0.42 } else { r * 0.36 };
    let engines: Vec<f32> = match kind {
        PlaneKind::Freighter => vec![span * 0.18, span * 0.32],
        PlaneKind::Turboprop => vec![span * 0.17],
        _ => vec![span * 0.18],
    };
    for s in [-1.0f32, 1.0] {
        for &z in &engines {
            let x_at = wroot_a - sweep * (z / (span * 0.5)) + 1.5;
            let y = if jet { wy - er * 0.9 } else { wy };
            metal.tube_capped(Vec3::new(x_at - er * 4.4, y, s * z), Vec3::new(x_at, y, s * z), er, 10, true, true, [0.7, 0.71, 0.73, 1.0]);
            if !jet {
                // The propeller disc.
                metal.boxed(Vec3::new(x_at + 0.2, y, s * z), Vec3::new(0.12, er * 4.6, 0.3), 0.0, [0.12, 0.12, 0.12, 1.0]);
                metal.boxed(Vec3::new(x_at + 0.2, y, s * z), Vec3::new(0.12, 0.3, er * 4.6), 0.0, [0.12, 0.12, 0.12, 1.0]);
            }
        }
    }
    // Cockpit, windows and nav lights.
    let glass = g.part(Surface::Glass);
    for s in [-1.0f32, 1.0] {
        glass.quad_facing([Vec3::new(len * 0.42, yc + r * 0.2, s * r * 0.55), Vec3::new(len * 0.455, yc + r * 0.05, s * r * 0.35), Vec3::new(len * 0.455, yc + r * 0.25, s * r * 0.2), Vec3::new(len * 0.42, yc + r * 0.45, s * r * 0.4)], Vec3::new(0.5, 0.4, s), [0.08, 0.1, 0.12, 1.0]);
    }
    if kind != PlaneKind::Freighter {
        let glow = g.part(Surface::Glow);
        for s in [-1.0f32, 1.0] {
            let z = s * (r + 0.05);
            glow.quad_facing([Vec3::new(xb + 1.0, yc + r * 0.28, z), Vec3::new(xf - 1.0, yc + r * 0.28, z), Vec3::new(xf - 1.0, yc + r * 0.42, z), Vec3::new(xb + 1.0, yc + r * 0.42, z)], Vec3::Z * s, [1.6, 1.4, 1.0, 1.0]);
        }
    }
    let glow = g.part(Surface::Glow);
    let tip_x = wroot_a - sweep - len * 0.035;
    glow.boxed(Vec3::new(tip_x, tip_y + 0.3, -span * 0.5), Vec3::splat(0.5), 0.0, LIGHT_RED);
    glow.boxed(Vec3::new(tip_x, tip_y + 0.3, span * 0.5), Vec3::splat(0.5), 0.0, LIGHT_GREEN);
    glow.boxed(Vec3::new(-len * 0.5, yc + r * 0.55, 0.0), Vec3::splat(0.4), 0.0, LIGHT_WHITE);
    glow.boxed(Vec3::new(0.0, yc + r + 0.1, 0.0), Vec3::new(0.5, 0.25, 0.5), 0.0, LIGHT_RED);
    // The airline's mark on the fin: a neon slash.
    glow.quad_facing([Vec3::new(-len * 0.42, yc + fin_h * 0.45, 0.25), Vec3::new(-len * 0.46, yc + fin_h * 0.45, 0.25), Vec3::new(-len * 0.49, yc + fin_h * 0.8, 0.25), Vec3::new(-len * 0.45, yc + fin_h * 0.8, 0.25)], Vec3::Z, scale4(tail, 3.0));
    glow.quad_facing([Vec3::new(-len * 0.42, yc + fin_h * 0.45, -0.25), Vec3::new(-len * 0.46, yc + fin_h * 0.45, -0.25), Vec3::new(-len * 0.49, yc + fin_h * 0.8, -0.25), Vec3::new(-len * 0.45, yc + fin_h * 0.8, -0.25)], -Vec3::Z, scale4(tail, 3.0));
    g
}

fn scale4(c: [f32; 4], k: f32) -> [f32; 4] {
    [c[0] * k + 0.2, c[1] * k + 0.2, c[2] * k + 0.2, 1.0]
}

/// The landing gear (drawn while it is down) and the landing lights.
fn gear_geometry(kind: PlaneKind) -> Geometry {
    let (len, _) = kind.dims();
    let r = len * 0.056;
    let yc = r + if kind == PlaneKind::Turboprop { 1.2 } else { 1.7 };
    let mut g = Geometry::default();
    let m = g.part(Surface::Matte);
    let dark = [0.12, 0.12, 0.13, 1.0];
    let strut = [0.55, 0.56, 0.58, 1.0];
    m.beam(Vec3::new(len * 0.36, 0.5, 0.0), Vec3::new(len * 0.36, yc - r * 0.8, 0.0), 0.25, strut);
    m.boxed(Vec3::new(len * 0.36, 0.45, 0.0), Vec3::new(0.9, 0.9, 0.7), 0.0, dark);
    for s in [-1.0f32, 1.0] {
        m.beam(Vec3::new(-len * 0.05, 0.6, s * r * 1.1), Vec3::new(-len * 0.05, yc - r * 0.6, s * r * 0.9), 0.35, strut);
        m.boxed(Vec3::new(-len * 0.05, 0.6, s * r * 1.1), Vec3::new(2.2, 1.2, 1.0), 0.0, dark);
    }
    g.part(Surface::Glow).boxed(Vec3::new(len * 0.36, yc - r * 0.7, 0.0), Vec3::new(0.3, 0.4, 0.8), 0.0, [9.0, 9.0, 8.0, 1.0]);
    g
}

/// A pushback tug, nose along +x.
fn tug_geometry() -> Geometry {
    let mut g = Geometry::default();
    g.part(Surface::Panel).boxed(Vec3::new(0.0, 0.8, 0.0), Vec3::new(5.0, 1.2, 2.6), 0.0, [0.9, 0.7, 0.1, 1.0]);
    g.part(Surface::Panel).boxed(Vec3::new(-1.5, 1.8, 0.0), Vec3::new(1.4, 0.9, 1.6), 0.0, [0.25, 0.25, 0.27, 1.0]);
    g.part(Surface::Glow).boxed(Vec3::new(-1.5, 2.35, 0.0), Vec3::new(0.3, 0.2, 0.3), 0.0, LIGHT_AMBER);
    g
}

fn meshes_of(meshes: &mut Assets<Mesh>, g: &Geometry) -> Vec<(Surface, Handle<Mesh>)> {
    g.iter().filter(|(_, m)| !m.is_empty()).map(|(s, m)| (s, meshes.add(super::shared_mesh(m, !s.is_glow())))).collect()
}

fn spawn_parts(commands: &mut Commands, mats: &Materials, parts: &[(Surface, Handle<Mesh>)], shadows: bool) -> Vec<Entity> {
    parts
        .iter()
        .map(|(s, mesh)| {
            let mut e = commands.spawn((Mesh3d(mesh.clone()), MeshMaterial3d(mats.surface(*s)), super::offstage(), Visibility::Hidden, AirportTag));
            if s.is_glow() || !shadows {
                e.insert((NotShadowCaster, NotShadowReceiver));
            }
            e.id()
        })
        .collect()
}

/// The ground vehicles round the planes on their stands: a fuel bowser,
/// a belt loader and a train of baggage carts, catering, a power unit.
fn apron_geometry(a: &Airport, base: f32) -> Geometry {
    let f = Frame { a, base, yaw: a.along.y.atan2(a.along.x) };
    let lay = a.layout();
    let mut g = Geometry::default();
    for p in a.planes.iter().filter(|p| p.phase == PlanePhase::AtStand) {
        let st = lay.stands[(p.stand as usize).min(lay.stands.len() - 1)];
        let (len, span) = p.kind.dims();
        let (x, y) = (st.at.x, st.at.y);
        let m = g.part(Surface::Panel);
        if p.kind == PlaneKind::Freighter {
            // A container loader at the side door, dollies of containers.
            f.bx(m, x + 6.0, y + len * 0.25, 0.0, 4.0, 7.0, 2.6, [0.9, 0.72, 0.12, 1.0]);
            for k in 0..4 {
                f.bx(g.part(Surface::Panel), x + 13.0, y + len * 0.25 - k as f32 * 3.6, 0.0, 2.8, 3.0, 0.5, [0.3, 0.3, 0.32, 1.0]);
                f.bx(g.part(Surface::Container), x + 13.0, y + len * 0.25 - k as f32 * 3.6, 0.5, 2.4, 2.8, 1.6, [0.6, 0.6, 0.58, 1.0]);
            }
        } else {
            let m = g.part(Surface::Panel);
            // Belt loader to the rear hold, a tug and its carts.
            f.bx(m, x + 3.5, y - len * 0.22, 0.0, 2.0, 7.0, 1.6, [0.9, 0.72, 0.12, 1.0]);
            f.bx(m, x + 9.0, y - len * 0.3, 0.0, 2.0, 3.2, 1.6, [0.9, 0.72, 0.12, 1.0]);
            for k in 0..3 {
                f.bx(m, x + 9.0, y - len * 0.3 - 4.0 - k as f32 * 3.6, 0.0, 1.8, 2.8, 1.5, [0.45, 0.47, 0.5, 1.0]);
            }
            // Catering truck at the forward right door.
            f.bx(m, x + 5.5, y + len * 0.3, 0.0, 2.6, 7.0, 3.2, [0.72, 0.75, 0.8, 1.0]);
        }
        let m = g.part(Surface::Panel);
        // The fuel bowser under the left wing.
        f.bx(m, x - span * 0.22, y - 3.0, 0.0, 3.0, 10.0, 3.0, [0.82, 0.82, 0.8, 1.0]);
        f.bx(m, x - span * 0.22, y + 3.5, 0.0, 2.6, 2.6, 2.4, [0.6, 0.12, 0.1, 1.0]);
        // A power unit by the nose.
        f.bx(m, x + 3.0, y + len * 0.5 - 4.0, 0.0, 1.6, 2.4, 1.3, [0.3, 0.32, 0.35, 1.0]);
        f.light(g.part(Surface::Glow), x + 3.0, y + len * 0.5 - 4.0, 1.3, 0.3, LIGHT_AMBER);
        f.light(g.part(Surface::Glow), x - span * 0.22, y + 3.5, 2.4, 0.3, LIGHT_AMBER);
    }
    g
}

#[allow(clippy::too_many_arguments)]
pub(super) fn sync_airports(
    mut commands: Commands,
    sim: Res<Sim>,
    mats: Res<Materials>,
    assets: Res<GameAssets>,
    lighting: Res<Lighting>,
    time: Res<Time>,
    mut state: ResMut<AirportRender>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut q: Query<(&mut Transform, &mut Visibility), With<AirportTag>>,
) {
    let w = &sim.world;
    let st = &mut *state;
    if sim.full_rebuild {
        for (_, f) in st.fields.drain() {
            for e in f.ents.into_iter().chain(f.beacon).chain(f.apron).chain(f.rabbits.into_iter().map(|r| r.0)) {
                commands.entity(e).despawn();
            }
        }
        for (_, p) in st.planes.drain() {
            for e in p.body.into_iter().chain(p.gear).chain(p.tug) {
                commands.entity(e).despawn();
            }
        }
    }
    if w.airports.is_empty() && st.fields.is_empty() && st.planes.is_empty() {
        return;
    }
    if st.beacon_mesh.is_none() {
        // The beacon: two beams sweeping round, green and white.
        let mut beam = MeshData::default();
        let ring = |x: f32, r: f32| ring_x(x, r, 0.0, 10);
        beam.frustum(&ring(0.4, 0.3), &ring(26.0, 3.2), [0.5, 0.5, 0.5, 1.0]);
        let mut back = MeshData::default();
        back.frustum(&ring(-0.4, 0.3), &ring(-26.0, 3.2), [0.5, 0.5, 0.5, 1.0]);
        st.beacon_mesh = Some((meshes.add(super::shared_mesh(&beam, false)), meshes.add(super::shared_mesh(&back, false))));
        let mk = |c: Color| StandardMaterial { base_color: c, unlit: true, alpha_mode: AlphaMode::Add, cull_mode: None, ..default() };
        st.beacon_mats = Some((materials.add(mk(Color::linear_rgba(0.1, 0.9, 0.35, 1.0))), materials.add(mk(Color::linear_rgba(0.9, 0.9, 0.8, 1.0)))));
        let mut dot = MeshData::default();
        dot.boxed(Vec3::ZERO, Vec3::new(1.4, 1.4, 1.4), 0.0, [1.0, 1.0, 1.0, 1.0]);
        st.rabbit_mesh = Some(meshes.add(super::shared_mesh(&dot, false)));
        st.rabbit_mat = Some(materials.add(StandardMaterial { base_color: Color::WHITE, emissive: LinearRgba::rgb(60.0, 60.0, 55.0), unlit: true, ..default() }));
        st.tug = meshes_of(&mut meshes, &tug_geometry());
    }
    let night = super::night_strength(&sim, &lighting);
    let t_now = time.elapsed_secs();
    // The fields.
    let ids: Vec<AirportId> = w.airports.airports.keys().collect();
    for &id in &ids {
        let a = &w.airports.airports[id];
        let base = a.lot.map(|l| w.lot_pad(l)).unwrap_or(a.level);
        let key = field_key(a, base);
        if st.fields.get(&id).is_some_and(|f| f.key == key) {
            continue;
        }
        if let Some(f) = st.fields.remove(&id) {
            for e in f.ents.into_iter().chain(f.beacon).chain(f.apron).chain(f.rabbits.into_iter().map(|r| r.0)) {
                commands.entity(e).despawn();
            }
        }
        // Rough grass between the runways, a little drier than the parks.
        let pal = &assets.palette;
        let k = |i: usize| (pal.foliage[i] * 0.45 + pal.foliage_dry[i] * 0.55) * 0.8;
        let g = field_geometry(w, a, base, [k(0), k(1), k(2), 1.0]);
        let mut ents = Vec::new();
        for (s, m) in g.iter() {
            if m.is_empty() {
                continue;
            }
            let mut e = commands.spawn((Mesh3d(meshes.add(super::shared_mesh(m, !s.is_glow()))), MeshMaterial3d(mats.surface(s)), Transform::IDENTITY, Visibility::Visible, AirportTag));
            if s.is_glow() || matches!(s, Surface::Paint | Surface::Asphalt | Surface::WornAsphalt | Surface::Concrete) {
                e.insert(NotShadowCaster);
            }
            ents.push(e.id());
        }
        let lay = a.layout();
        let top = a.world(lay.tower);
        let at = Vec3::new(top.x, base + lay.tower_height + 7.3, top.y);
        let (bm, bk) = st.beacon_mesh.clone().unwrap();
        let (gm, wm) = st.beacon_mats.clone().unwrap();
        let beacon = vec![
            commands.spawn((Mesh3d(bm), MeshMaterial3d(gm), Transform::from_translation(at), Visibility::Hidden, NotShadowCaster, NotShadowReceiver, AirportTag)).id(),
            commands.spawn((Mesh3d(bk), MeshMaterial3d(wm), Transform::from_translation(at), Visibility::Hidden, NotShadowCaster, NotShadowReceiver, AirportTag)).id(),
        ];
        let mut rabbits = Vec::new();
        for r in &lay.runways {
            for end in [-1.0f32, 1.0] {
                let e = commands.spawn((Mesh3d(st.rabbit_mesh.clone().unwrap()), MeshMaterial3d(st.rabbit_mat.clone().unwrap()), super::offstage(), Visibility::Hidden, NotShadowCaster, NotShadowReceiver, AirportTag)).id();
                let p = a.world(SVec2::new(end * r.length * 0.5, r.y));
                rabbits.push((e, p, a.along * end, if a.size == ap::AirportSize::International { 720.0 } else { 420.0 }));
            }
        }
        st.fields.insert(id, Field { key, ents, beacon, rabbits, apron_key: 0, apron: Vec::new(), base });
    }
    let gone: Vec<AirportId> = st.fields.keys().copied().filter(|id| !w.airports.airports.contains_key(*id)).collect();
    for id in gone {
        if let Some(f) = st.fields.remove(&id) {
            for e in f.ents.into_iter().chain(f.beacon).chain(f.apron).chain(f.rabbits.into_iter().map(|r| r.0)) {
                commands.entity(e).despawn();
            }
        }
    }
    // The beacon turns at night; the flashers run in towards the runway.
    if !sim.paused {
        st.spin = (st.spin + time.delta_secs().min(0.1) * 2.4) % std::f32::consts::TAU;
    }
    let lit = night > 0.35;
    for (id, f) in st.fields.iter_mut() {
        for (k, e) in f.beacon.iter().enumerate() {
            if let Ok((mut tf, mut vis)) = q.get_mut(*e) {
                tf.rotation = Quat::from_rotation_y(st.spin + k as f32 * 0.0);
                *vis = if lit { Visibility::Visible } else { Visibility::Hidden };
            }
        }
        let landing_dir = w.airports.airports.get(*id).map_or(1.0, |a| a.dir);
        for (i, (e, end, out, reach)) in f.rabbits.iter().enumerate() {
            // Only the end planes land at: the one they come in over.
            let lands_here = out.dot(w.airports.airports.get(*id).map_or(SVec2::X, |a| a.along)) * -landing_dir > 0.0;
            if let Ok((mut tf, mut vis)) = q.get_mut(*e) {
                let phase = ((t_now * 2.0 + i as f32 * 0.13) % 1.0).clamp(0.0, 1.0);
                let d = reach * (1.0 - phase);
                let p = *end + *out * d.max(30.0);
                let ground = w.surface_height(p).min(f.base + 30.0).max(f.base);
                tf.translation = Vec3::new(p.x, ground + 1.4, p.y);
                *vis = if lit && lands_here { Visibility::Visible } else { Visibility::Hidden };
            }
        }
        // The ground vehicles round the planes on stands.
        let Some(a) = w.airports.airports.get(*id) else { continue };
        let mut h = DefaultHasher::new();
        for p in a.planes.iter().filter(|p| p.phase == PlanePhase::AtStand) {
            (p.stand, p.kind.index()).hash(&mut h);
        }
        let key = h.finish();
        if key != f.apron_key {
            for e in f.apron.drain(..) {
                commands.entity(e).despawn();
            }
            let g = apron_geometry(a, f.base);
            for (s, m) in g.iter() {
                if m.is_empty() {
                    continue;
                }
                let mut e = commands.spawn((Mesh3d(meshes.add(super::shared_mesh(m, !s.is_glow()))), MeshMaterial3d(mats.surface(s)), Transform::IDENTITY, Visibility::Visible, AirportTag));
                if s.is_glow() {
                    e.insert((NotShadowCaster, NotShadowReceiver));
                }
                f.apron.push(e.id());
            }
            f.apron_key = key;
        }
    }
    // The planes.
    let t = w.clock.tick as f32 + sim.frac();
    let mut seen: HashSet<(AirportId, u32)> = HashSet::new();
    for &id in &ids {
        let a = &w.airports.airports[id];
        let base = st.fields.get(&id).map_or(a.level, |f| f.base);
        for p in &a.planes {
            let k = (id, p.flight);
            seen.insert(k);
            if !st.planes.contains_key(&k) {
                if !st.bodies.contains_key(&(p.kind, p.airline)) {
                    let parts = meshes_of(&mut meshes, &plane_geometry(p.kind, p.airline));
                    st.bodies.insert((p.kind, p.airline), parts);
                }
                if !st.gear.contains_key(&p.kind) {
                    let parts = meshes_of(&mut meshes, &gear_geometry(p.kind));
                    st.gear.insert(p.kind, parts);
                }
                let body = spawn_parts(&mut commands, &mats, &st.bodies[&(p.kind, p.airline)], true);
                let gear = spawn_parts(&mut commands, &mats, &st.gear[&p.kind], true);
                let tug = spawn_parts(&mut commands, &mats, &st.tug, true);
                st.planes.insert(k, Plane3d { body, gear, tug });
            }
            let pose = ap::plane_pose(a, p, t);
            let ground = if p.phase.airborne() { base } else { base + 0.06 };
            let at = Vec3::new(pose.pos.x, ground + pose.alt, pose.pos.y);
            let rot = Quat::from_rotation_y(-pose.heading) * Quat::from_rotation_z(pose.pitch);
            let pushing = ap::pushing_back(a, p, t);
            let p3 = &st.planes[&k];
            for e in &p3.body {
                if let Ok((mut tf, mut vis)) = q.get_mut(*e) {
                    tf.translation = at;
                    tf.rotation = rot;
                    *vis = Visibility::Visible;
                }
            }
            for e in &p3.gear {
                if let Ok((mut tf, mut vis)) = q.get_mut(*e) {
                    tf.translation = at;
                    tf.rotation = rot;
                    *vis = if pose.gear_down { Visibility::Visible } else { Visibility::Hidden };
                }
            }
            let (len, _) = p.kind.dims();
            let nose = SVec2::new(pose.heading.cos(), pose.heading.sin());
            let tug_at = pose.pos + nose * (len * 0.5 + 3.5);
            for e in &p3.tug {
                if let Ok((mut tf, mut vis)) = q.get_mut(*e) {
                    tf.translation = Vec3::new(tug_at.x, base + 0.06, tug_at.y);
                    tf.rotation = Quat::from_rotation_y(-pose.heading + std::f32::consts::PI);
                    *vis = if pushing { Visibility::Visible } else { Visibility::Hidden };
                }
            }
        }
    }
    let gone: Vec<(AirportId, u32)> = st.planes.keys().copied().filter(|k| !seen.contains(k)).collect();
    for k in gone {
        if let Some(p) = st.planes.remove(&k) {
            for e in p.body.into_iter().chain(p.gear).chain(p.tug) {
                commands.entity(e).despawn();
            }
        }
    }
}
