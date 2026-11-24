//! [r16-tidy] A container yard's room comes from its own layout, in the
//! sim, not from the renderer (todo "Container yard room comes from the
//! renderer"): with the layout installed (`nd_procgen::install_sim_hooks`,
//! as the game does at start-up) the sim lays each depot out itself, keeps
//! it within what its stacks hold, and lays it out again only when its lot
//! or look changes. Its own test binary: the layout is installed once for
//! the whole process.
use nd_procgen::{BoxSlot, BuildingRules, Extras, Palette, BAY_TIER};
use nd_sim::world::DepotKind;
use nd_sim::{BusinessKind, LandUse, Vec2, World, WorldConfig};

fn square(x0: f32, y0: f32, w: f32, h: f32) -> Vec<Vec2> {
    vec![Vec2::new(x0, y0), Vec2::new(x0 + w, y0), Vec2::new(x0 + w, y0 + h), Vec2::new(x0, y0 + h)]
}

#[test]
fn a_container_yard_takes_its_room_from_its_layout_in_the_sim() {
    let mut w = World::new(WorldConfig { emergencies: false, society: false, dead_ends_lead_elsewhere: true, ..WorldConfig::default() });
    w.economy.treasury = 1_000_000.0;
    w.add_road(&[Vec2::new(-400.0, 0.0), Vec2::new(400.0, 0.0)], 1, 1);
    w.add_zone(square(-300.0, 10.0, 600.0, 110.0), LandUse::Industrial);
    let lot = w.lots.iter().filter(|(_, l)| l.usable() && l.land_use == LandUse::Industrial && l.car_access() && l.building.is_none()).max_by(|a, b| a.1.area().total_cmp(&b.1.area()).then(b.0.cmp(&a.0))).map(|(id, _)| id).expect("a free industrial lot");
    let y = w.open_business(lot, BusinessKind::ContainerYard, true).expect("a container yard");
    w.containers.live = true;
    assert_eq!(w.box_depot(y), Some(DepotKind::Yard));
    let ground = w.box_capacity(y, DepotKind::Yard);
    // Without a layout the ground alone says (and nothing is laid out).
    w.refresh_yard_rooms();
    assert!(w.containers.layout_room.get(&y).is_none());
    assert_eq!(w.box_room(y, DepotKind::Yard), ground);

    nd_procgen::install_sim_hooks();
    assert!(nd_sim::world::yard_layout_installed());
    w.refresh_yard_rooms();
    let room = *w.containers.layout_room.get(&y).expect("laid out by the sim") as usize;
    // What the renderer draws: the style's stacks for real boxes, bays left out.
    let bld = &w.buildings[y];
    let lot = w.footprint(y).unwrap();
    let extras = Extras { ..nd_procgen::style_extras(&w, y, &[]) };
    assert!(extras.real_boxes, "a live depot draws real boxes");
    let g = nd_procgen::generate_building(&lot, bld.land_use, bld.floors, bld.tier, bld.condition, bld.seed, &BuildingRules::default(), &Palette::default(), &extras);
    let slots: Vec<BoxSlot> = g.box_slots.iter().copied().filter(|s| s.tier != BAY_TIER).collect();
    assert!(!slots.is_empty(), "a container yard draws stacks");
    assert_eq!(room, nd_procgen::buildings::parts::stack_room(&slots), "the room is what its drawn stacks hold");
    assert_eq!(w.box_room(y, DepotKind::Yard), ground.min(room));

    // Laid out again only when its lot or look changes: a stale value
    // stands while the key holds, and is replaced once the look changes.
    w.containers.layout_room.insert(y, 1);
    w.refresh_yard_rooms();
    assert_eq!(w.containers.layout_room.get(&y), Some(&1), "not laid out again with nothing changed");
    w.buildings[y].seed ^= 0x55;
    w.refresh_yard_rooms();
    assert_ne!(w.containers.layout_room.get(&y), Some(&1), "laid out again once its look changed");

    // No longer a depot: its room goes.
    w.buildings[y].business.as_mut().unwrap().closed = true;
    w.refresh_yard_rooms();
    assert!(w.containers.layout_room.get(&y).is_none());
    assert!(w.containers.layout_keys.get(&y).is_none());
}
