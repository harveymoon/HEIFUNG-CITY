//! [r11-adopt-frames] Skid row and the trash drawn by cell, on the pool.

use super::*;
use bevy::tasks::AsyncComputeTaskPool;

/// The demo district built out, every building's bins overflowing (a
/// step apiece by building), a camp on the pavement and a van at the kerb.
fn city() -> World {
    let mut w = World::new(nd_sim::WorldConfig::default());
    nd_sim::scenarios::demo_district(&mut w);
    let mut lots: Vec<nd_sim::LotId> = w.lots.iter().filter(|(_, l)| l.usable() && l.building.is_none()).map(|(id, _)| id).collect();
    lots.sort();
    for lot in lots {
        w.build_on(lot);
        if let Some(b) = w.lots.get(lot).and_then(|l| l.building) {
            if w.buildings.get(b).is_some_and(|x| x.construction.is_some()) {
                w.finish_construction(b);
            }
        }
    }
    let ids: Vec<BuildingId> = w.buildings.keys().collect();
    for (i, id) in ids.into_iter().enumerate() {
        let bin = nd_sim::trash::Bin { stored: 100.0 + (i % 5) as f32 * 40.0, capacity: 100.0, ..Default::default() };
        w.society.trash.bins.insert(id, bin);
    }
    let lot = w.lots.values().find(|l| l.usable()).expect("a lot").clone();
    w.society.homeless.camps.push(nd_sim::homeless::Camp { id: 1, pos: lot.entrance(), dir: SVec2::X, lot: None, out: lot.front_dir, sidewalk: true, households: Vec::new(), people: 5, founded: 0, under: false });
    w.drain_events();
    w
}

/// The cells drawn from scratch, as the renderer has them after a full draw.
fn drawn_all(w: &World) -> (HashMap<ChunkKey, (Option<u64>, Vec<Entity>)>, HashMap<u64, Arc<Geometry>>) {
    let items = society_items(w);
    let hashes = cell_hashes(&items);
    let work = cells_to_draw(&HashMap::new(), items, &hashes, &HashMap::new());
    let mut pieces = HashMap::new();
    let mut cells = HashMap::new();
    for d in draw_cells(w, &nd_procgen::Palette::default(), &work, &|| false).expect("not halted") {
        pieces.extend(d.built);
        cells.insert(d.cell, (Some(d.hash), Vec::new()));
    }
    (cells, pieces)
}

#[test]
fn a_bin_crossing_a_step_redraws_only_its_cell() {
    let mut w = city();
    let (cells, pieces) = drawn_all(&w);
    assert!(cells.len() > 1, "the district spans several cells: {}", cells.len());
    // Nothing changed: nothing to draw.
    let items = society_items(&w);
    let hashes = cell_hashes(&items);
    assert!(cells_to_draw(&cells, items, &hashes, &pieces).is_empty());
    // One bin fills a step further.
    let (&id, _) = w.society.trash.bins.iter().next().expect("a bin");
    let before = overflow_step(w.society.trash.bins[&id].overflow());
    w.society.trash.bins.get_mut(&id).unwrap().stored += 200.0;
    assert_ne!(overflow_step(w.society.trash.bins[&id].overflow()), before);
    let items = society_items(&w);
    let hashes = cell_hashes(&items);
    let work = cells_to_draw(&cells, items, &hashes, &pieces);
    assert_eq!(work.len(), 1, "only its cell");
    // Only the changed pile is built again: the rest come from the cache.
    let fresh: Vec<u64> = work[0].pieces.iter().filter(|p| p.2.is_none()).map(|p| p.0).collect();
    assert_eq!(fresh.len(), 1, "one piece built again");
}

#[test]
fn a_cell_from_cached_pieces_is_the_cell_built_afresh() {
    let w = city();
    let pal = nd_procgen::Palette::default();
    let (_, pieces) = drawn_all(&w);
    let items = society_items(&w);
    let hashes = cell_hashes(&items);
    let fresh = cells_to_draw(&HashMap::new(), items.clone(), &hashes, &HashMap::new());
    let cached = cells_to_draw(&HashMap::new(), items.clone(), &hashes, &pieces);
    assert!(cached.iter().all(|c| c.pieces.iter().all(|p| p.2.is_some())), "every piece cached");
    for (a, b) in fresh.iter().zip(&cached) {
        let (ga, gb) = (cell_geometry(&w, &pal, a).0, cell_geometry(&w, &pal, b).0);
        // As the whole-city path drew it: the cell's pieces built in order and merged.
        let mut whole = Geometry::default();
        for (cell, _, piece) in &items {
            if *cell == a.cell {
                whole.append(&build_piece(&w, piece, &pal));
            }
        }
        for g in [&ga, &gb] {
            assert_eq!(g.parts.len(), whole.parts.len());
            for (p, q) in g.parts.iter().zip(&whole.parts) {
                assert_eq!(p.positions, q.positions);
                assert_eq!(p.indices, q.indices);
                assert_eq!(p.colors, q.colors);
            }
        }
    }
}

#[test]
fn a_job_on_the_pool_draws_what_the_main_thread_draws() {
    AsyncComputeTaskPool::get_or_init(bevy::tasks::TaskPool::default);
    let core = crate::sim_thread::SimCore::new(city());
    let pal = nd_procgen::Palette::default();
    let items = society_items(&core);
    let hashes = cell_hashes(&items);
    let work = cells_to_draw(&HashMap::new(), items.clone(), &hashes, &HashMap::new());
    let here = draw_cells(&core, &pal, &work, &|| false).expect("not halted");
    let work2 = cells_to_draw(&HashMap::new(), items, &hashes, &HashMap::new());
    let p2 = pal.clone();
    let mut job = CellJob::start(core.lend(), move |w, halt| draw_cells(w, &p2, &work2, halt));
    while !job.finished() {
        std::thread::yield_now();
    }
    let there = job.take().expect("not recalled");
    assert_eq!(here.len(), there.len());
    for (a, b) in here.iter().zip(&there) {
        assert_eq!((a.cell, a.hash), (b.cell, b.hash));
        assert_eq!(a.meshes.len(), b.meshes.len());
        for ((sa, ma), (sb, mb)) in a.meshes.iter().zip(&b.meshes) {
            assert_eq!(sa, sb);
            assert_eq!(ma.count_vertices(), mb.count_vertices());
            assert_eq!(format!("{:?}", ma.attribute(Mesh::ATTRIBUTE_POSITION)), format!("{:?}", mb.attribute(Mesh::ATTRIBUTE_POSITION)));
        }
    }
}

#[test]
fn a_recalled_job_gives_up() {
    AsyncComputeTaskPool::get_or_init(bevy::tasks::TaskPool::default);
    let mut core = crate::sim_thread::SimCore::new(city());
    let pal = nd_procgen::Palette::default();
    let items = society_items(&core);
    let hashes = cell_hashes(&items);
    let work = cells_to_draw(&HashMap::new(), items, &hashes, &HashMap::new());
    let gate = Arc::new(std::sync::Barrier::new(2));
    let g2 = gate.clone();
    let mut job = CellJob::start(core.lend(), move |w, halt| {
        g2.wait();
        // Held until the edit below has recalled the lease.
        while !halt() {
            std::thread::yield_now();
        }
        draw_cells(w, &pal, &work, halt)
    });
    gate.wait();
    core.grant_treasury(1.0);
    while !job.finished() {
        std::thread::yield_now();
    }
    assert!(job.take().is_none(), "an edit recalls the job");
}
