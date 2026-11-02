//! [r9-stacks-look] The Stacks hold together: no box floats or runs into
//! another, every flight runs from a landing at a door down to the ground or
//! a roof over open air, every landing is reached and hangs off the unit
//! whose door it serves, every door opens onto something people stand on,
//! planks rest on two roofs and every wire is tied at both ends to the pole
//! or a mast on an open roof, clear of the boxes. Checked on narrow, wide,
//! corner and odd lots, at every height, with stand-in and real boxes.

use super::stacks::*;
use super::sweep_tests::{faults, lot_of, odd_lots, rect};
use super::*;
use glam::Vec3;
use nd_sim::containers::{BoxFace, Marks};
use nd_sim::rail::BoxKind;

fn ctx<'a>(lot: &Lot, rules: &'a BuildingRules, pal: &'a Palette, floors: u8, tier: Tier, cond: f32, seed: u64) -> Ctx<'a> {
    let finish = Finish { rules: rules.for_land_use(LandUse::Residential), style: rules.style(tier), palette: pal };
    Ctx::new(lot, Style::Stacks, LandUse::Residential, floors, tier, cond, seed, finish, &Extras::default())
}

/// A home's boxes as a stack developer buys them: mostly dry boxes, a
/// reefer, an open top and a tank among them.
fn real_faces(n: usize, seed: u64) -> Vec<BoxFace> {
    (0..n)
        .map(|k| {
            let kind = match (k as u64 + seed) % 7 {
                3 => BoxKind::Reefer,
                5 => BoxKind::OpenTop,
                6 => BoxKind::Tank,
                _ => BoxKind::Dry,
            };
            BoxFace { id: Default::default(), line: (k % 10) as u8, forty: (k + seed as usize) % 3 == 0, kind, wear: 0.5, marks: Marks::default(), number: *b"ABCU1234567", seed: seed * 101 + k as u64 }
        })
        .collect()
}

fn lots() -> Vec<(String, Lot)> {
    let at = Vec2::new(-40.0, 25.0);
    let mut v: Vec<(String, Lot)> = odd_lots().into_iter().map(|(n, l)| (n.to_string(), l)).collect();
    v.push(("narrow 8x36".into(), lot_of(&rect(8.0, 36.0), 1.1, at)));
    v.push(("wide 60x18".into(), lot_of(&rect(60.0, 18.0), -0.4, at)));
    v.push(("square 30x30".into(), lot_of(&rect(30.0, 30.0), 0.0, at)));
    v.push(("deep 20x45".into(), lot_of(&rect(20.0, 45.0), 2.3, at)));
    v.push(("corner".into(), lot_of(&[Vec2::new(-18.0, 0.0), Vec2::new(18.0, 0.0), Vec2::new(18.0, 14.0), Vec2::new(4.0, 30.0), Vec2::new(-18.0, 30.0)], 0.8, at)));
    for (i, l) in super::coplanar_tests::test_lots(LandUse::Residential).into_iter().take(10).enumerate() {
        v.push((format!("street lot {i}"), l));
    }
    v
}

/// Everything wrong with a plan.
fn plan_faults(c: &Ctx, p: &StackPlan) -> Vec<String> {
    let mut bad = Vec::new();
    let u = &p.units;
    // Boxes: none through another, every upper one resting on a roof.
    for i in 0..u.len() {
        for j in (i + 1)..u.len() {
            let (a, b) = (&u[i], &u[j]);
            let vertical = a.top().min(b.top()) - a.y0.max(b.y0);
            if vertical > 0.02 && overlap(a, b) {
                bad.push(format!("units {i} and {j} run into each other ({vertical:.2} m)"));
            }
        }
        if u[i].level > 0 && !u.iter().any(|o| (o.top() - u[i].y0).abs() < 0.03 && overlap(o, &u[i])) {
            bad.push(format!("unit {i} floats at {:.2}", u[i].y0));
        }
    }
    // Doors open onto ground, a roof at their floor, or their own landing.
    for d in &p.doors {
        let unit = &u[d.unit];
        let step = unit.local_to_lot(d.lu, d.side * (unit.wid * 0.5 + 0.7));
        match d.onto {
            Onto::Ground if unit.level != 0 => bad.push(format!("an upper door of unit {} opens onto the ground", d.unit)),
            Onto::Ground | Onto::Roof if !p.walkable(c, step, unit.y0) => bad.push(format!("unit {}'s door opens onto nothing at {step:?} y {:.2}", d.unit, unit.y0)),
            Onto::Landing(l) => {
                let l = &p.landings[l];
                if l.unit != d.unit || (l.y - unit.y0).abs() > 0.01 || l.side != d.side || !(l.lu0..=l.lu1).contains(&d.lu) {
                    bad.push(format!("unit {}'s door is not at its landing", d.unit));
                }
            }
            _ => {}
        }
    }
    // Landings: on their unit's wall at its floor, in open air, reached.
    for (li, l) in p.landings.iter().enumerate() {
        let unit = &u[l.unit];
        if (unit.y0 - l.y).abs() > 0.01 || unit.level == 0 {
            bad.push(format!("landing {li} is not at its unit's floor"));
        }
        if !p.doors.iter().any(|d| d.onto == Onto::Landing(li)) {
            bad.push(format!("landing {li} has no door"));
        }
        let flights = p.flights.iter().filter(|f| f.landing == li).count();
        let ladders = p.ladders.iter().filter(|f| f.landing == li).count();
        if flights + ladders != 1 {
            bad.push(format!("landing {li} reached by {flights} flights and {ladders} ladders"));
        }
        for a in [0.0f32, 0.5, 1.0] {
            for out in [0.1f32, 0.5, 0.95] {
                let q = l.at(u, l.lu0 + (l.lu1 - l.lu0) * a, out);
                if !c.inside_local(q) || u.iter().any(|o| o.contains(q) && o.top() > l.y - 0.2 && o.y0 < l.y + HEADROOM) {
                    bad.push(format!("landing {li} runs into a box or off the lot at {q:?}"));
                }
            }
        }
    }
    // Flights: from their landing's end, over open air, to a foot people
    // stand on.
    for (fi, f) in p.flights.iter().enumerate() {
        let l = &p.landings[f.landing];
        let (t2, f2) = (Vec2::new(f.top.x, f.top.z), Vec2::new(f.foot.x, f.foot.z));
        if (f.top.y - l.y).abs() > 0.01 || !l.covers(u, t2, 0.05) {
            bad.push(format!("flight {fi} does not start on its landing"));
        }
        let drop = f.top.y - f.foot.y;
        let slope = drop / t2.distance(f2).max(1e-3);
        if drop < 1.0 || !(0.6..=0.95).contains(&slope) {
            bad.push(format!("flight {fi} drops {drop:.2} m at slope {slope:.2}"));
        }
        let ahead = (f2 - t2).normalize_or_zero();
        if !p.walkable(c, f2 + ahead * 0.4, f.foot.y) {
            bad.push(format!("flight {fi} lands on nothing at {f2:?} y {:.2}", f.foot.y));
        }
        for k in 0..=20 {
            let t = k as f32 / 20.0;
            let h = f.top.y - drop * t;
            for off in [-0.4f32, 0.0, 0.4] {
                let q = t2.lerp(f2, t) + f.out * off;
                if !c.inside_local(q) || u.iter().any(|o| o.contains(q) && o.top() > h + 0.02) {
                    bad.push(format!("flight {fi} runs through a box or off the lot at {q:?} y {h:.2}"));
                }
            }
        }
    }
    for (k, ld) in p.ladders.iter().enumerate() {
        let l = &p.landings[ld.landing];
        let q = Vec2::new(ld.foot.x, ld.foot.z);
        if (ld.top - l.y - 1.0).abs() > 0.01 || !l.covers(u, q, 0.25) {
            bad.push(format!("ladder {k} does not reach its landing"));
        }
        if !p.walkable(c, q, ld.foot.y) || u.iter().any(|o| o.contains(q) && o.top() > ld.foot.y + 0.02 && o.y0 < ld.top) {
            bad.push(format!("ladder {k} stands on nothing or runs through a box"));
        }
    }
    // Planks: both ends on a roof at their height.
    for (k, pl) in p.planks.iter().enumerate() {
        for (end, i) in [(pl.a, pl.units.0), (pl.b, pl.units.1)] {
            let q = Vec2::new(end.x, end.z);
            if !u[i].contains(q) || (u[i].top() - end.y).abs() > 0.01 || !p.walkable(c, q, end.y) {
                bad.push(format!("plank {k} has an end off its roof"));
            }
        }
    }
    // Masts on open roof; wires tied at both ends, through clear air.
    for (k, m) in p.masts.iter().enumerate() {
        let unit = &u[m.unit];
        if !unit.contains(m.at) || (unit.top() - m.y0).abs() > 0.01 || u.iter().any(|o| o.y0 > unit.top() - 0.05 && o.contains(m.at)) {
            bad.push(format!("mast {k} stands on no open roof"));
        }
    }
    for (k, w) in p.wires.iter().enumerate() {
        for a in [w.a, w.b] {
            match a {
                Anchor::Pole(_) if p.pole.is_none() => bad.push(format!("wire {k} tied to a pole that is not there")),
                Anchor::Mast(i) if i >= p.masts.len() => bad.push(format!("wire {k} tied to a missing mast")),
                _ => {}
            }
        }
        let (a, b) = (p.anchor(w.a), p.anchor(w.b));
        if a.distance(b) < 2.0 {
            bad.push(format!("wire {k} is a stub"));
        }
        let len = a.distance(b);
        let steep = ((b - a).y / len.max(1e-3)).abs();
        let sag = if steep > 1e-3 { w.sag.min(0.8 * len / (4.0 * steep)) } else { w.sag };
        for i in 1..30 {
            let t = i as f32 / 30.0;
            let q = a.lerp(b, t) - Vec3::Y * (sag * 4.0 * t * (1.0 - t));
            let q2 = Vec2::new(q.x, q.z);
            if !c.inside_local(q2) || u.iter().any(|o| o.contains(q2) && o.y0 < q.y + 0.03 && o.top() > q.y - 0.03) {
                bad.push(format!("wire {k} runs through a box or off the lot at {q:?}"));
                break;
            }
        }
    }
    if let Some(pole) = p.pole {
        if !c.inside_local(pole.at) || u.iter().any(|o| o.contains_m(pole.at, 0.2) && o.y0 < pole.h) {
            bad.push(format!("the pole stands in a box or off the lot at {:?}", pole.at));
        }
    }
    bad
}

/// Footprints overlapping by more than a couple of centimetres.
fn overlap(a: &Unit, b: &Unit) -> bool {
    let ca = a.corners();
    let cb = b.corners();
    for axis in [ca[1] - ca[0], ca[3] - ca[0], cb[1] - cb[0], cb[3] - cb[0]] {
        let axis = axis.normalize_or_zero();
        let proj = |cs: &[Vec2; 4]| cs.iter().map(|p| p.dot(axis)).fold((f32::MAX, f32::MIN), |(lo, hi), x| (lo.min(x), hi.max(x)));
        let ((a0, a1), (b0, b1)) = (proj(&ca), proj(&cb));
        if a1 - 0.02 < b0 || b1 - 0.02 < a0 {
            return false;
        }
    }
    true
}

#[test]
fn stacks_stairs_wires_and_boxes_attach_to_the_pile() {
    let rules = BuildingRules::default();
    let pal = Palette::default();
    let mut bad: Vec<String> = Vec::new();
    let (mut flights, mut ladders, mut landings, mut roof_doors, mut upper_doors, mut uppers, mut wires, mut planks, mut cases) = (0, 0, 0, 0, 0, 0, 0, 0, 0);
    for (li, (name, lot)) in lots().iter().enumerate() {
        for (fi, floors) in [1u8, 2, 3, 5, 8].into_iter().enumerate() {
            for seed in 0..4u64 {
                for real in [false, true] {
                    let k = li as u64 * 97 + fi as u64 * 13 + seed;
                    let tier = Tier::ALL[(k % 3) as usize];
                    let mut c = ctx(lot, &rules, &pal, floors, tier, [0.2, 0.6, 1.0][(k % 3) as usize], k);
                    if real {
                        c.boxes = real_faces(6 + (k % 14) as usize, k);
                    }
                    let p = plan(&c, &mut nd_sim::rng_for(k, 0x57AC));
                    let again = plan(&c, &mut nd_sim::rng_for(k, 0x57AC));
                    assert_eq!(format!("{p:?}"), format!("{again:?}"), "{name}: the same seed plans the same pile");
                    cases += 1;
                    for f in plan_faults(&c, &p) {
                        bad.push(format!("{name} floors {floors} seed {seed} real {real}: {f}"));
                    }
                    flights += p.flights.len();
                    ladders += p.ladders.len();
                    landings += p.landings.len();
                    wires += p.wires.len();
                    planks += p.planks.len();
                    uppers += p.units.iter().filter(|x| x.level > 0 && x.shell != Shell::Tank).count();
                    upper_doors += p.doors.iter().filter(|d| p.units[d.unit].level > 0).count();
                    roof_doors += p.doors.iter().filter(|d| d.onto == Onto::Roof).count();
                }
            }
        }
    }
    eprintln!("{cases} piles: {uppers} upper units, {upper_doors} upper doors ({roof_doors} onto roofs, {landings} landings: {flights} flights, {ladders} ladders), {planks} planks, {wires} wires");
    assert!(bad.is_empty(), "{} faults:\n{}", bad.len(), bad.iter().take(40).cloned().collect::<Vec<_>>().join("\n"));
    assert!(flights > cases / 2, "upper units are reached by stairs ({flights} flights in {cases} piles)");
    // Units buried in the middle of a pile are reached through their
    // neighbours; the ones with an open wall get a door.
    assert!(upper_doors * 100 > uppers * 45, "upper units have doors people reach ({upper_doors} of {uppers})");
    assert!(wires > cases, "the piles are wired ({wires} wires in {cases} piles)");
    assert!(ladders > 0 && roof_doors > 0 && planks > 0, "ladders {ladders}, roof doors {roof_doors}, planks {planks}");
}

/// The drawn Stacks stay clean meshes on their lots (the sweep's checks).
#[test]
fn stacks_draw_clean_on_every_lot() {
    let rules = BuildingRules::default();
    let pal = Palette::default();
    let mut bad: Vec<String> = Vec::new();
    for (li, (name, lot)) in lots().iter().enumerate() {
        for (fi, floors) in [1u8, 3, 6].into_iter().enumerate() {
            let k = li as u64 * 7 + fi as u64;
            let g = generate_styled(lot, LandUse::Residential, floors, Tier::ALL[(k % 3) as usize], [0.2, 0.6, 1.0][(k % 3) as usize], k, Style::Stacks, &rules, &pal);
            for (kind, what) in faults(&g, lot, Style::Stacks) {
                bad.push(format!("{name} floors {floors}: {kind} {what}"));
            }
        }
    }
    assert!(bad.is_empty(), "{} faults:\n{}", bad.len(), bad.join("\n"));
}
