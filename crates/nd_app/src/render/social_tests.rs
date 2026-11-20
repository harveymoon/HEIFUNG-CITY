//! [r11-adopt-frames] Murals and gardens drawn by cell.

use super::*;

/// The demo district, with free-standing murals and gardens on its empty
/// lots, spread over several cells.
fn city() -> World {
    let mut w = World::new(nd_sim::WorldConfig::default());
    nd_sim::scenarios::demo_district(&mut w);
    let mut lots: Vec<(nd_sim::LotId, nd_sim::Lot)> = w.lots.iter().filter(|(_, l)| l.usable() && l.building.is_none()).map(|(id, l)| (id, l.clone())).collect();
    lots.sort_by_key(|(id, _)| *id);
    for (i, (id, lot)) in lots.into_iter().enumerate() {
        let along = (lot.front_edge[1] - lot.front_edge[0]).normalize_or_zero();
        if i % 2 == 0 {
            w.social.murals.push(nd_sim::social::Mural { group: 0, lot: id, building: None, seed: i as u64, day: 1, quality: 0.2, pos: lot.entrance(), along, facing: lot.front_dir, width: 6.0, height: 3.0 });
        } else {
            w.social.gardens.push(nd_sim::social::Garden { group: 0, lot: id, seed: i as u64, day: 1, pos: lot.center });
        }
    }
    w
}

fn kit_parts() -> (nd_procgen::BuildingRules, nd_procgen::Palette) {
    (nd_procgen::BuildingRules::default(), nd_procgen::Palette::default())
}

fn items(w: &World) -> Vec<(ChunkKey, u64, Source)> {
    let (rules, pal) = kit_parts();
    social_items(w, &StyleBook::default(), super::super::buildings::Kit { rules: &rules, palette: &pal }, &HashSet::new(), &mut HashMap::new()).0
}

/// Every vertex of a geometry's parts, as bits, sorted.
fn vertices(g: &Geometry) -> Vec<[u32; 3]> {
    let mut v: Vec<[u32; 3]> = g.parts.iter().flat_map(|p| p.positions.iter().map(|q| q.map(f32::to_bits))).collect();
    v.sort_unstable();
    v
}

#[test]
fn the_cells_draw_what_the_whole_drawing_drew() {
    let w = city();
    let pal = nd_procgen::Palette::default();
    let items = items(&w);
    let hashes = social_cell_hashes(&items);
    assert!(hashes.len() > 1, "several cells: {}", hashes.len());
    let todo: HashSet<ChunkKey> = hashes.keys().copied().collect();
    let cells = cell_pieces(&w, &pal, &items, &todo, &mut HashMap::new());
    let (mut day, mut night) = (Geometry::default(), Geometry::default());
    for list in cells.values() {
        let (d, n) = merge_cell(list);
        day.append(&d);
        night.append(&n);
    }
    // As the whole city was drawn: every wall, then every garden.
    let (mut whole, mut whole_night) = (Geometry::default(), Geometry::default());
    for m in &w.social.murals {
        let (d, n) = mural_wall(m.pos, m.along, m.facing, m.width, m.height, m.quality, m.seed, m.group as u64, "", &pal);
        whole.append(&d);
        whole_night.append(&n);
    }
    for g in &w.social.gardens {
        let lot = &w.lots[g.lot];
        whole.append(&garden(lot.center, lot.front_dir, lot.width().min(lot.depth()), g.seed));
    }
    // Raised onto the built ground after, as one.
    if !w.landscape.is_flat() {
        for geo in [&mut whole, &mut whole_night] {
            for part in geo.parts.iter_mut() {
                for p in part.positions.iter_mut() {
                    p[1] += w.surface_height(SVec2::new(p[0], p[2]));
                }
            }
        }
    }
    assert!(whole.triangle_count() > 0);
    assert_eq!(day.triangle_count(), whole.triangle_count());
    assert_eq!(night.triangle_count(), whole_night.triangle_count());
    assert_eq!(vertices(&day), vertices(&whole));
    assert_eq!(vertices(&night), vertices(&whole_night));
}

#[test]
fn a_new_garden_redraws_only_its_cell_and_ageing_paint_nothing() {
    let mut w = city();
    let before = social_cell_hashes(&items(&w));
    let key = social_key(&w);
    // Paint wearing within its band: the same drawing.
    w.social.murals[0].quality = 0.3;
    assert_eq!(social_key(&w), key);
    assert_eq!(social_cell_hashes(&items(&w)), before);
    // A garden planted: its cell only.
    let g = w.social.gardens[0].clone();
    w.social.gardens.push(nd_sim::social::Garden { seed: 999, ..g.clone() });
    let after = social_cell_hashes(&items(&w));
    let cell = cell_of(w.lots[g.lot].center);
    let moved: Vec<ChunkKey> = after.iter().filter(|(c, h)| before.get(*c) != Some(*h)).map(|(c, _)| *c).collect();
    assert_eq!(moved, vec![cell]);
}

#[test]
fn ground_pieces_are_kept_once_built() {
    let w = city();
    let pal = nd_procgen::Palette::default();
    let items = items(&w);
    let todo: HashSet<ChunkKey> = social_cell_hashes(&items).keys().copied().collect();
    let mut pieces = HashMap::new();
    let first = cell_pieces(&w, &pal, &items, &todo, &mut pieces);
    assert_eq!(pieces.len(), items.len(), "every wall and garden kept");
    let again = cell_pieces(&w, &pal, &items, &todo, &mut pieces);
    for (c, list) in &first {
        for ((_, a), (_, b)) in list.iter().zip(&again[c]) {
            assert!(Arc::ptr_eq(a, b), "the same piece, not built again");
        }
    }
}
