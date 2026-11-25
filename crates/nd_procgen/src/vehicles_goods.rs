//! [r17-goods] The new goods' lorries: a drinks lorry with roller-shutter
//! bays, a cement tanker, the nuclear flask carrier, and the logs on a
//! log lorry's deck. A child of `vehicles` so it shares its builder (`B`)
//! and the site lorries' cab and running gear.

use super::{body_length, site_cab, site_running_gear, B, AMBER, TAIL, TRIM};
use crate::color::{mix, mul, rgba as c4};
use glam::Vec3;
use nd_sim::vehicle::Livery;

/// A drinks lorry: the cab and a box body whose sides are four roller
/// shutters, two of them up to show the kegs and crates stacked inside.
pub(super) fn drinks_truck(b: &mut B, col: [f32; 3]) {
    let m = *b.m;
    let len = body_length(m.body);
    let (fx, rx) = (len * 0.5, -len * 0.5);
    let hw = 1.22;
    let cab_col = if m.livery == Livery::None { mix(col, [0.8, 0.8, 0.82], 0.5) } else { col };
    let cr = site_cab(b, fx, hw, cab_col);
    site_running_gear(b, fx, rx, hw, 1.1);
    let (x0, x1, y0, top) = (rx, cr - 0.12, 0.95, 3.1);
    let body = if m.livery == Livery::None { [0.62, 0.08, 0.07] } else { col };
    // The frame: floor, roof, the posts between the bays, front and back walls.
    b.bx(((x0 + x1) * 0.5, y0, 0.0), (x1 - x0, 0.14, hw * 2.0), mul(body, 0.6));
    b.bx(((x0 + x1) * 0.5, top, 0.0), (x1 - x0, 0.16, hw * 2.04), body);
    b.bx((x1 - 0.06, (y0 + top) * 0.5, 0.0), (0.12, top - y0, hw * 2.0), body);
    b.bx((x0 + 0.06, (y0 + top) * 0.5, 0.0), (0.12, top - y0, hw * 2.0), mul(body, 0.85));
    let bays = 4;
    let bw = (x1 - x0) / bays as f32;
    let gold = [0.95, 0.72, 0.2];
    for k in 0..bays {
        let x = x0 + (k as f32 + 0.5) * bw;
        b.bx((x0 + k as f32 * bw, (y0 + top) * 0.5, 0.0), (0.12, top - y0, hw * 2.02), mul(body, 0.8));
        // The stock inside: kegs (silver) low, crates (coloured) above.
        b.bx((x, y0 + 0.85, 0.0), (bw - 0.25, 1.55, hw * 1.7), [[0.7, 0.72, 0.74], [0.15, 0.35, 0.12], [0.75, 0.55, 0.12], [0.6, 0.12, 0.08]][k % 4]);
        // Shutters: bays 0 and 2 down on the left, 1 and 3 on the right.
        for s in [-1.0f32, 1.0] {
            let down = (k % 2 == 0) == (s < 0.0);
            if down {
                b.bx((x, (y0 + top) * 0.5, s * (hw - 0.03)), (bw - 0.14, top - y0 - 0.15, 0.06), mul(body, 0.95));
                if b.detail {
                    // The shutter's handle rail.
                    b.bx((x, y0 + 0.35, s * hw), (bw - 0.2, 0.06, 0.03), mul(body, 0.6));
                }
            } else {
                // Rolled up under the roof.
                b.bx((x, top - 0.3, s * (hw - 0.05)), (bw - 0.14, 0.35, 0.1), mul(body, 0.7));
            }
        }
    }
    // A gold band along the roof line.
    b.sides((x0 + x1) * 0.5, top - 0.08, hw + 0.01, x1 - x0, 0.1, gold);
    b.lamps(rx, 1.0, hw - 0.15, 0.25, 0.14, -1.0, TAIL);
    b.wear_marks(x0 + 0.3, x1 - 0.3, y0 + 0.2, top - 0.2, hw);
}

/// A cement tanker: the cab and a long pale tank on its frame, two
/// discharge cones underneath, a walkway and manholes on top, the blower
/// behind the cab.
pub(super) fn cement_tanker(b: &mut B, col: [f32; 3]) {
    let m = *b.m;
    let len = body_length(m.body);
    let (fx, rx) = (len * 0.5, -len * 0.5);
    let hw = 1.22;
    let cab_col = if m.livery == Livery::None { mix(col, [0.8, 0.8, 0.82], 0.5) } else { col };
    let cr = site_cab(b, fx, hw, cab_col);
    site_running_gear(b, fx, rx, hw, 1.2);
    let tank = [0.84, 0.84, 0.82];
    b.bx(((rx + cr) * 0.5, 0.95, 0.0), (cr - rx - 0.3, 0.25, hw * 1.4), TRIM);
    // The blower and its pipe behind the cab.
    b.bx((cr - 0.35, 1.45, 0.0), (0.5, 0.7, hw * 1.2), [0.3, 0.3, 0.32]);
    let (a, z) = (Vec3::new(cr - 0.7, 2.05, 0.0), Vec3::new(rx + 0.3, 2.05, 0.0));
    b.body.tube_capped(a, z, 0.95, 10, true, true, c4(tank));
    if b.detail {
        // Discharge cones under the tank and the hatches on top.
        for t in [0.3f32, 0.7] {
            let p = a.lerp(z, t);
            b.body.tube_capped(p - Vec3::Y * 0.6, p - Vec3::Y * 1.0, 0.35, 8, false, true, c4(mul(tank, 0.8)));
            b.body.tube_capped(p + Vec3::Y * 0.9, p + Vec3::Y * 1.05, 0.28, 8, false, true, c4(mul(tank, 0.7)));
        }
        // The walkway rail and the company stripe.
        b.bx(((a.x + z.x) * 0.5, 3.05, 0.0), (a.x - z.x, 0.04, 0.6), [0.5, 0.5, 0.52]);
    }
    let stripe = if m.livery == Livery::None { [0.1, 0.3, 0.6] } else { col };
    b.sides((a.x + z.x) * 0.5, 2.05, 0.96, a.x - z.x - 0.4, 0.25, stripe);
    b.wear_marks(rx + 0.3, cr - 0.3, 1.2, 2.9, 0.96);
}

/// The nuclear flask carrier: a heavy tractor and a long low-loader with
/// a fat yellow-banded cask strapped down in a cradle, marker lights and
/// amber beacons; it never travels alone (`World::send_fuel_convoy`).
pub(super) fn flask_carrier(b: &mut B, col: [f32; 3]) {
    let m = *b.m;
    let len = body_length(m.body);
    let (fx, rx) = (len * 0.5, -len * 0.5);
    let hw = 1.25;
    let cab = if m.livery == Livery::None { mix(col, [0.85, 0.85, 0.88], 0.6) } else { [0.85, 0.86, 0.9] };
    let cr = site_cab(b, fx, hw, cab);
    // The low deck: drops between the cab's axle and the bogies at the back.
    let deck = 0.8;
    b.bx(((rx + cr) * 0.5 - 0.2, deck, 0.0), (cr - rx - 0.6, 0.25, hw * 2.0), [0.18, 0.18, 0.2]);
    b.bx((cr - 0.4, 1.1, 0.0), (0.5, 0.7, hw * 1.6), [0.18, 0.18, 0.2]);
    b.axle(fx - 0.95, hw + 0.02, 0.5, 0.3);
    for k in 0..3 {
        b.axle(rx + 0.8 + k as f32 * 0.9, hw + 0.02, 0.42, 0.32);
    }
    // The cradle and the cask: grey steel with cooling fins, yellow bands.
    let (c0, c1) = (rx + 0.6, cr - 1.0);
    let mid = (c0 + c1) * 0.5;
    for x in [c0 + 0.5, c1 - 0.5] {
        b.bx((x, deck + 0.45, 0.0), (0.35, 0.7, hw * 1.7), [0.25, 0.25, 0.28]);
    }
    let r = 0.95;
    let (a, z) = (Vec3::new(c1, deck + 0.25 + r, 0.0), Vec3::new(c0, deck + 0.25 + r, 0.0));
    b.body.tube_capped(a, z, r, 12, true, true, c4([0.55, 0.57, 0.6]));
    let yellow = [1.0, 0.78, 0.05];
    for t in [0.12f32, 0.5, 0.88] {
        let p = a.lerp(z, t);
        b.body.tube_capped(p - Vec3::X * 0.12, p + Vec3::X * 0.12, r + 0.04, 12, false, false, c4(yellow));
    }
    if b.detail {
        for t in [0.3f32, 0.7] {
            let p = a.lerp(z, t);
            b.body.tube_capped(p - Vec3::X * 0.03, p + Vec3::X * 0.03, r + 0.08, 12, false, false, c4([0.42, 0.44, 0.47]));
        }
        // Straps over the cask.
        for x in [mid - 0.9, mid + 0.9] {
            b.bx((x, deck + 0.25 + r, 0.0), (0.06, r * 2.0 + 0.1, hw * 1.95), [0.9, 0.5, 0.1]);
        }
    }
    // A black-and-yellow placard on each side of the cask, lit faintly.
    b.lit_sides(mid, deck + 0.25 + r, r + 0.1, 0.7, 0.7, [1.4, 1.1, 0.1]);
    // Amber beacons on the cab and the back of the deck, marker lamps.
    b.lights.boxed(Vec3::new(fx - 1.0, 2.58, 0.0), Vec3::new(0.18, 0.14, 1.4), 0.0, c4(AMBER));
    b.lights.boxed(Vec3::new(rx + 0.2, 1.3, 0.0), Vec3::new(0.12, 0.12, 2.2), 0.0, c4(AMBER));
    b.lamps(rx, 1.0, hw - 0.15, 0.25, 0.14, -1.0, TAIL);
    b.wear_marks(rx + 0.3, cr - 0.3, deck, deck + 1.0, hw);
}

/// A convoy escort's marks on its car: a dark blue body, a white band, a
/// blue-and-amber light bar on the roof.
pub(super) fn escort_marks(b: &mut B, roof: f32, hw: f32) {
    b.sides(0.0, 0.75, hw + 0.01, 3.2, 0.25, [0.85, 0.85, 0.88]);
    b.lights.boxed(Vec3::new(-0.2, roof + 0.12, -0.35), Vec3::new(0.3, 0.16, 0.6), 0.0, c4([0.2, 0.5, 3.0]));
    b.lights.boxed(Vec3::new(-0.2, roof + 0.12, 0.35), Vec3::new(0.3, 0.16, 0.6), 0.0, c4(AMBER));
}

/// [r17-goods] Logs between the stanchions of a log lorry's deck, pale ends
/// showing: drawn by `flatbed` for `Load::Logs`.
pub(super) fn logs_on_deck(b: &mut B, d0: f32, d1: f32, deck: f32, hw: f32) {
    let bark = [0.32, 0.22, 0.14];
    let end = [0.78, 0.62, 0.42];
    let rows: [(f32, f32, usize); 3] = [(0.32, 0.0, 3), (0.86, 0.25, 2), (1.38, 0.5, 1)];
    for (y, off, n) in rows {
        for k in 0..n {
            let z = if n == 1 { 0.0 } else { -hw * 0.66 + off + k as f32 * (hw * 1.32 - 2.0 * off) / (n - 1) as f32 };
            let r = 0.36 + 0.03 * ((k + n) % 3) as f32;
            // Bark along the log, the pale sawn end at the back.
            b.body.tube_capped(Vec3::new(d1 - 0.1, deck + y, z), Vec3::new(d0 + 0.15, deck + y, z), r, 6, false, false, c4(bark));
            b.body.tube_capped(Vec3::new(d0 + 0.15, deck + y, z), Vec3::new(d0 + 0.1, deck + y, z), r * 0.95, 6, false, true, c4(end));
        }
    }
    // Stanchions up both sides.
    let n = 3;
    for i in 0..n {
        let x = d0 + 0.3 + (d1 - d0 - 0.6) * i as f32 / (n - 1) as f32;
        for s in [-1.0f32, 1.0] {
            b.bx((x, deck + 0.95, s * (hw - 0.05)), (0.12, 1.9, 0.1), [0.15, 0.15, 0.16]);
        }
    }
}
