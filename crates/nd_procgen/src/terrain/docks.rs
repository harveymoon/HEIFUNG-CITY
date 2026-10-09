//! [economy] Waterfront docks and boats: moorings, pontoon layouts and
//! the boat, trawler and ship meshes.

use super::*;

// ── [economy] waterfront docks and boats ────────────────────────────────────

/// Height of a marina's floating pontoon decks above the water.
pub const PONTOON_DECK: f32 = 0.45;

/// A boat tied up at a dock: where (world x, z), which way its bow points
/// (radians, `atan2` of the bow direction, the convention of `Ship` poses)
/// and what kind of boat.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Mooring {
    pub pos: Vec2,
    pub yaw: f32,
    pub kind: ShipKind,
}

/// The water side of a marina or fishing port beyond its main pier: extra
/// deck pieces (finger pontoons, a T-head quay), each `(from, to, width)`,
/// and the boats moored at them.
#[derive(Clone, Debug, Default)]
pub struct DockLayout {
    pub decks: Vec<(Vec2, Vec2, f32)>,
    pub boats: Vec<Mooring>,
}

/// Length and beam of a boat kind, metres (matches [`boat_mesh`]).
pub fn boat_size(kind: ShipKind) -> (f32, f32) {
    match kind {
        ShipKind::Freighter => (70.0, 12.0),
        ShipKind::Ferry => (28.0, 8.0),
        ShipKind::FishingBoat => (16.0, 5.0),
        ShipKind::Yacht => (22.0, 6.0),
        ShipKind::Sailboat => (9.0, 3.0),
    }
}

/// Where the boats of a marina or fishing port lie at their berths, up to
/// `PortKind::berths()` of them (fewer where the water is too narrow or
/// the pier too short): small boats on finger pontoons off a marina's
/// pontoon (yachts too, on longer fingers, at a large marina), trawlers
/// alongside a fishing port's quay and its T-head. Deterministic from the
/// port. Other kinds of dock have none.
pub fn moorings(port: &nd_sim::ports::Port, land: &nd_sim::landscape::Landscape) -> Vec<Mooring> {
    dock_layout(port, land).boats
}

/// The decks and moorings of a waterfront dock (see [`moorings`]).
pub fn dock_layout(port: &nd_sim::ports::Port, land: &nd_sim::landscape::Landscape) -> DockLayout {
    let mut out = DockLayout::default();
    let kind = port.kind;
    if !(kind.is_marina() || kind.is_fishing()) {
        return out;
    }
    let berths = kind.berths() as usize;
    let (_, width) = kind.pier();
    let [root, tip] = port.pier;
    let len = root.distance(tip);
    if len < 2.0 {
        return out;
    }
    let along = (tip - root) / len;
    let across = Vec2::new(-along.y, along.x);
    // Where the pier leaves the bank.
    let mut s_water = 0.0;
    while s_water < len && !land.is_water(root + along * s_water) {
        s_water += 1.0;
    }
    // A boat fits where its middle, ends and sides are all on water.
    let afloat = |p: Vec2, dir: Vec2, k: ShipKind| -> bool {
        let (l, b) = boat_size(k);
        let n = Vec2::new(-dir.y, dir.x);
        [p, p + dir * l * 0.5, p - dir * l * 0.5, p + n * b * 0.5, p - n * b * 0.5].iter().all(|q| land.is_water(*q))
    };
    let yaw_of = |d: Vec2| d.y.atan2(d.x);
    if kind.is_marina() {
        // Finger pontoons both sides of the main pontoon, boats bow-in
        // between them; the large marina puts a yacht in every third pair.
        let mut s = s_water + 1.5;
        let mut pair = 0;
        while out.boats.len() < berths && pair < 40 {
            let k = if kind == nd_sim::ports::PortKind::MarinaLarge && pair % 3 == 0 { ShipKind::Yacht } else { ShipKind::Sailboat };
            let (l, b) = boat_size(k);
            let spacing = b + 1.6;
            let mid = s + spacing * 0.5;
            if mid + spacing * 0.5 > len {
                break;
            }
            for side in [1.0f32, -1.0] {
                if out.boats.len() >= berths {
                    break;
                }
                let base = root + along * mid;
                let p = base + across * side * (width * 0.5 + l * 0.5 + 0.4);
                if afloat(p, across, k) {
                    out.boats.push(Mooring { pos: p, yaw: yaw_of(-across * side), kind: k });
                    // The finger beside it.
                    let fs = root + along * (mid + spacing * 0.5);
                    let a = fs + across * side * width * 0.5;
                    let fb = a + across * side * (l * 0.8);
                    if land.is_water(fb) {
                        out.decks.push((a, fb, 1.0));
                    }
                }
            }
            s += spacing;
            pair += 1;
        }
        // Any berths left: a T-head across the tip, boats along its far side.
        if out.boats.len() < berths {
            let head = 24.0;
            out.decks.push((tip - across * head * 0.5, tip + across * head * 0.5, width));
            let (l, b) = boat_size(ShipKind::Sailboat);
            let mut x = -head * 0.5 + l * 0.5;
            while x + l * 0.5 <= head * 0.5 && out.boats.len() < berths {
                let p = tip + along * (width * 0.5 + b * 0.5 + 0.4) + across * x;
                if afloat(p, across, ShipKind::Sailboat) {
                    out.boats.push(Mooring { pos: p, yaw: yaw_of(across), kind: ShipKind::Sailboat });
                }
                x += l + 1.0;
            }
        }
    } else {
        // A quay with a T-head; the trawlers' berths are the sim's
        // (`nd_sim::fishing::quay_moorings`), so a boat out at sea leaves
        // its own berth empty.
        let head = nd_sim::fishing::quay_head(kind);
        out.decks.push((tip - across * head * 0.5, tip + across * head * 0.5, width));
        out.boats = nd_sim::fishing::quay_moorings(port, land).into_iter().map(|(pos, yaw)| Mooring { pos, yaw, kind: ShipKind::FishingBoat }).collect();
    }
    out
}

/// A hull from keel to gunwale with a pointed bow, bow along +x, and a
/// paler deck: `len` long, `beam` wide, from `y0` to `y1`.
fn hull(m: &mut MeshData, len: f32, beam: f32, bow: f32, y0: f32, y1: f32, color: [f32; 3], deck: [f32; 3]) {
    use glam::Vec3;
    let half = len * 0.5;
    let ring = |hb: f32, back: f32, y: f32| vec![Vec3::new(-half + back, y, -hb), Vec3::new(half - bow, y, -hb), Vec3::new(half, y, 0.0), Vec3::new(half - bow, y, hb), Vec3::new(-half + back, y, hb)];
    let bottom = ring(beam * 0.32, len * 0.06, y0);
    let top = ring(beam * 0.5, 0.0, y1);
    m.frustum(&bottom, &top, rgba(color));
    let d: Vec<Vec3> = ring(beam * 0.46, 0.25, y1 + 0.05);
    m.cap(&d, Vec3::Y, rgba(deck));
}

/// [fishing] A trawler, bow along +x, waterline at 0: wheelhouse forward,
/// a gantry over the stern. Steaming or in port its outrigger booms stand
/// up; with `nets_out` they are lowered out over the water either side and
/// the warps run from the gantry down into the sea astern.
pub fn trawler_mesh(nets_out: bool, palette: &Palette) -> MeshData {
    use glam::Vec3;
    let mut m = MeshData::default();
    let white = [0.92, 0.92, 0.9];
    let glass = palette.glass_dark;
    let (len, beam) = boat_size(ShipKind::FishingBoat);
    let red = [0.55, 0.13, 0.1];
    hull(&mut m, len, beam, 3.5, -1.2, 1.4, red, [0.45, 0.42, 0.38]);
    // White sheer strake along the top of the hull.
    m.boxed(Vec3::new(-1.5, 1.25, 0.0), Vec3::new(len - 5.0, 0.3, beam * 1.005), 0.0, rgba(white));
    // Wheelhouse forward, windows round it.
    m.boxed(Vec3::new(2.4, 2.75, 0.0), Vec3::new(3.6, 2.7, 3.4), 0.0, rgba(white));
    m.boxed(Vec3::new(2.4, 3.55, 0.0), Vec3::new(3.65, 0.65, 3.45), 0.0, rgba(glass));
    m.boxed(Vec3::new(2.4, 4.2, 0.0), Vec3::new(3.9, 0.15, 3.7), 0.0, rgba([0.3, 0.3, 0.32]));
    // Mast with a crosstree and the outrigger booms.
    let spar = rgba([0.8, 0.8, 0.78]);
    m.boxed(Vec3::new(3.2, 6.5, 0.0), Vec3::new(0.2, 4.6, 0.2), 0.0, spar);
    m.boxed(Vec3::new(3.2, 7.6, 0.0), Vec3::new(0.15, 0.15, 2.0), 0.0, spar);
    for s in [-1.0f32, 1.0] {
        if nets_out {
            // Lowered nearly flat, a line from each boom end to the water.
            let end = Vec3::new(0.0, 3.4, s * 9.5);
            m.beam(Vec3::new(3.0, 4.5, s * 0.3), end, 0.16, rgba([0.75, 0.75, 0.72]));
            m.beam(end, Vec3::new(-6.0, -0.2, s * 11.0), 0.06, rgba([0.25, 0.25, 0.25]));
        } else {
            m.beam(Vec3::new(3.0, 4.5, 0.0), Vec3::new(1.0, 6.5, s * 5.5), 0.14, rgba([0.75, 0.75, 0.72]));
        }
    }
    // Stern gantry (A-frame) and the net drum.
    let yellow = [0.9, 0.62, 0.1];
    for s in [-1.0f32, 1.0] {
        m.beam(Vec3::new(-6.6, 1.4, s * 2.0), Vec3::new(-6.0, 6.2, s * 1.6), 0.25, rgba(yellow));
    }
    m.boxed(Vec3::new(-6.0, 6.2, 0.0), Vec3::new(0.35, 0.35, 3.6), 0.0, rgba(yellow));
    if nets_out {
        // The drum paid out: warps from the gantry into the water astern,
        // and the net's floats strung out behind.
        for s in [-1.0f32, 1.0] {
            m.beam(Vec3::new(-6.0, 6.0, s * 1.5), Vec3::new(-26.0, -0.3, s * 4.0), 0.07, rgba([0.2, 0.2, 0.2]));
        }
        for k in 0..6 {
            let x = -27.0 - k as f32 * 2.2;
            let z = (k as f32 - 2.5) * 1.3;
            m.boxed(Vec3::new(x, 0.12, z), Vec3::new(0.7, 0.35, 0.7), 0.0, rgba([0.95, 0.45, 0.08]));
        }
        m.boxed(Vec3::new(-4.0, 1.85, 0.0), Vec3::new(1.6, 0.9, 3.0), 0.0, rgba([0.15, 0.35, 0.2]));
    } else {
        m.boxed(Vec3::new(-4.0, 2.1, 0.0), Vec3::new(1.6, 1.4, 3.0), 0.0, rgba([0.15, 0.35, 0.2]));
    }
    m.boxed(Vec3::new(-1.0, 1.75, 0.0), Vec3::new(1.4, 0.7, 1.4), 0.0, rgba([0.2, 0.4, 0.75]));
    m
}

/// [economy] The mesh of a boat of any kind, bow along +x, waterline at 0:
/// a trawler with its wheelhouse forward and a gantry over the stern, a
/// white motor yacht with stepped decks, or a small sailing boat with a
/// mast and sails up. Freighters and ferries are [`ship_mesh`]'s.
pub fn boat_mesh(kind: ShipKind, palette: &Palette) -> MeshData {
    use glam::Vec3;
    let mut m = MeshData::default();
    let white = [0.92, 0.92, 0.9];
    let glass = palette.glass_dark;
    match kind {
        ShipKind::Freighter => return ship_mesh(true, palette),
        ShipKind::Ferry => return ship_mesh(false, palette),
        ShipKind::FishingBoat => return trawler_mesh(false, palette),
        ShipKind::Yacht => {
            let (len, beam) = boat_size(kind);
            hull(&mut m, len, beam, beam * 1.4, -1.2, 2.0, white, [0.72, 0.6, 0.45]);
            // A dark stripe along the topsides, a swim platform at the stern.
            m.boxed(Vec3::new(-1.5, 1.2, 0.0), Vec3::new(len - beam * 1.4 - 1.5, 0.35, beam * 1.005), 0.0, rgba([0.1, 0.14, 0.25]));
            m.boxed(Vec3::new(-len * 0.5 - 0.6, 0.35, 0.0), Vec3::new(1.4, 0.2, beam * 0.8), 0.0, rgba([0.72, 0.6, 0.45]));
            // Saloon deck and its window band.
            m.boxed(Vec3::new(-1.5, 3.0, 0.0), Vec3::new(len * 0.5, 2.0, beam * 0.78), 0.0, rgba(white));
            m.boxed(Vec3::new(-1.5, 3.2, 0.0), Vec3::new(len * 0.5 + 0.05, 0.8, beam * 0.785), 0.0, rgba(glass));
            // Upper deck, set back, then the flybridge and radar arch.
            m.boxed(Vec3::new(-2.0, 4.85, 0.0), Vec3::new(len * 0.32, 1.7, beam * 0.65), 0.0, rgba(white));
            m.boxed(Vec3::new(-0.9, 5.0, 0.0), Vec3::new(len * 0.1, 0.7, beam * 0.655), 0.0, rgba(glass));
            m.boxed(Vec3::new(-3.0, 5.85, 0.0), Vec3::new(len * 0.22, 0.3, beam * 0.7), 0.0, rgba([0.85, 0.85, 0.83]));
            for s in [-1.0f32, 1.0] {
                m.beam(Vec3::new(-3.5, 5.7, s * beam * 0.3), Vec3::new(-3.0, 7.4, s * beam * 0.15), 0.2, rgba(white));
            }
            m.boxed(Vec3::new(-3.0, 7.45, 0.0), Vec3::new(0.6, 0.25, beam * 0.35), 0.0, rgba(white));
        }
        ShipKind::Sailboat => {
            let (len, beam) = boat_size(kind);
            hull(&mut m, len, beam, 2.2, -0.8, 0.7, white, [0.7, 0.6, 0.45]);
            m.boxed(Vec3::new(-0.6, 1.05, 0.0), Vec3::new(3.2, 0.7, 1.8), 0.0, rgba(white));
            m.boxed(Vec3::new(-0.6, 1.1, 0.0), Vec3::new(3.25, 0.25, 1.85), 0.0, rgba(glass));
            // Mast, boom, mainsail and jib (both faces of each sail).
            let mast = 0.9;
            let top = 11.0;
            m.boxed(Vec3::new(mast, (0.7 + top) * 0.5, 0.0), Vec3::new(0.14, top - 0.7, 0.14), 0.0, rgba([0.8, 0.8, 0.8]));
            m.boxed(Vec3::new(mast - 1.9, 1.9, 0.0), Vec3::new(3.8, 0.12, 0.12), 0.0, rgba([0.8, 0.8, 0.8]));
            let sail = rgba([0.96, 0.95, 0.92]);
            let (a, b, c) = (Vec3::new(mast - 0.1, top - 0.3, 0.0), Vec3::new(mast - 0.1, 2.0, 0.0), Vec3::new(mast - 3.7, 2.0, 0.0));
            m.tri_facing(a, b, c, Vec3::Z, sail);
            m.tri_facing(a, b, c, -Vec3::Z, sail);
            let (a, b, c) = (Vec3::new(mast + 0.1, top - 1.5, 0.0), Vec3::new(len * 0.5 - 0.3, 0.8, 0.0), Vec3::new(mast + 0.2, 1.2, 0.0));
            m.tri_facing(a, b, c, Vec3::Z, sail);
            m.tri_facing(a, b, c, -Vec3::Z, sail);
        }
    }
    m
}

/// A ship's hull and deckhouse as one small mesh, bow along +x, waterline
/// at 0: a freighter with container stacks, or a ferry with a cabin.
pub fn ship_mesh(freighter: bool, palette: &Palette) -> MeshData {
    use glam::Vec3;
    let mut m = MeshData::default();
    let (len, beam) = if freighter { (70.0, 12.0) } else { (28.0, 8.0) };
    let hull = if freighter { [0.18, 0.2, 0.24] } else { [0.85, 0.86, 0.88] };
    let half = len * 0.5;
    let bottom = [Vec3::new(-half, -1.5, -beam * 0.5), Vec3::new(half - beam, -1.5, -beam * 0.5), Vec3::new(half, -1.5, 0.0), Vec3::new(half - beam, -1.5, beam * 0.5), Vec3::new(-half, -1.5, beam * 0.5)];
    let top: Vec<Vec3> = bottom.iter().map(|p| *p + Vec3::Y * 4.0).collect();
    m.frustum(&bottom, &top, rgba(hull));
    // The deck, paler than the hull. `frustum` already capped the top in
    // the hull colour; a second cap in the same plane z-fought with it, so
    // the deck stands a few centimetres proud of it (and hides it).
    let deck: Vec<Vec3> = top.iter().map(|p| *p + Vec3::Y * 0.06).collect();
    m.cap(&deck, Vec3::Y, rgba(mul(hull, 1.3)));
    if freighter {
        m.boxed(Vec3::new(-half + 6.0, 6.5, 0.0), Vec3::new(7.0, 7.0, beam * 0.8), 0.0, rgba([0.9, 0.9, 0.88]));
        let colors = &palette.container_colors;
        for k in 0..6 {
            let x = -half + 16.0 + k as f32 * 7.5;
            let c = if colors.is_empty() { [0.7, 0.3, 0.2] } else { colors[k % colors.len()] };
            let tiers = 1 + (k % 3) as i32;
            m.boxed(Vec3::new(x, 2.5 + tiers as f32 * 1.3, 0.0), Vec3::new(6.8, tiers as f32 * 2.6, beam * 0.85), 0.0, rgba(c));
        }
    } else {
        m.boxed(Vec3::new(-1.0, 4.0, 0.0), Vec3::new(len * 0.6, 3.0, beam * 0.8), 0.0, rgba([0.92, 0.92, 0.9]));
        m.boxed(Vec3::new(-2.0, 6.2, 0.0), Vec3::new(len * 0.35, 1.8, beam * 0.6), 0.0, rgba(palette.glass_dark));
        m.boxed(Vec3::new(-2.0, 4.0, 0.0), Vec3::new(len * 0.62, 0.6, beam * 0.82), 0.0, rgba([0.9, 0.25, 0.2]));
    }
    m
}
