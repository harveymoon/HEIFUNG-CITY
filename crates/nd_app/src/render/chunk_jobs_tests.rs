//! [r10-chunk-hitch] Building chunk layers and jobs.

use super::*;
use crate::render::buildings::changed_layers;

/// The demo district built out: every empty zoned lot built on, most
/// finished, a few left as sites; one shop made a car park.
fn city() -> (World, BuildingId, Vec<BuildingId>) {
    let mut w = World::new(nd_sim::WorldConfig::default());
    nd_sim::scenarios::demo_district(&mut w);
    let mut lots: Vec<nd_sim::LotId> = w.lots.iter().filter(|(_, l)| l.usable() && l.building.is_none()).map(|(id, _)| id).collect();
    lots.sort();
    let mut sites = Vec::new();
    for (i, lot) in lots.into_iter().enumerate() {
        w.build_on(lot);
        if let Some(b) = w.lots.get(lot).and_then(|l| l.building) {
            if w.buildings.get(b).is_some_and(|x| x.construction.is_some()) {
                if i % 5 == 0 {
                    sites.push(b);
                } else {
                    w.finish_construction(b);
                }
            }
        }
    }
    let shop = w.buildings.iter().filter(|(_, b)| b.land_use == nd_sim::LandUse::Commercial && b.construction.is_none() && !b.is_tenant()).map(|(id, _)| id).min().expect("a shop");
    let b = w.buildings.get_mut(shop).unwrap();
    b.business = Some(nd_sim::Business::new(nd_sim::Business::PARKING_KIND, 2));
    b.parking_capacity = 40;
    b.parking_used = 10;
    w.drain_events();
    (w, shop, sites)
}

fn kit() -> DrawKit {
    DrawKit { rules: nd_procgen::BuildingRules::default(), palette: nd_procgen::Palette::default() }
}

/// Every building's layer, as the renderer would have drawn them.
fn drawn(w: &World) -> HashMap<BuildingId, LayerKey> {
    w.buildings.keys().filter_map(|id| Some((id, building_layer(w, id)?))).collect()
}

/// The buildings of each layer, in world order.
fn groups(w: &World) -> Vec<(LayerKey, Vec<BuildingId>)> {
    let mut m: HashMap<LayerKey, Vec<BuildingId>> = HashMap::new();
    for id in w.buildings.keys() {
        if let Some(k) = building_layer(w, id) {
            m.entry(k).or_default().push(id);
        }
    }
    let mut g: Vec<_> = m.into_iter().collect();
    g.sort_unstable_by_key(|(k, _)| (k.0 .0, k.0 .1, k.1));
    g
}

#[test]
fn a_car_park_filling_up_redraws_only_its_chunks_live_layer() {
    let (mut w, shop, _) = city();
    let before = drawn(&w);
    let k = building_layer(&w, shop).unwrap();
    assert_eq!(k.1, Layer::Live, "a car park is live");
    // The sim parks a car: the band changes, the car park is redrawn.
    w.buildings.get_mut(shop).unwrap().parking_used = 30;
    let marked = changed_layers(&w, &before, &[SimEvent::BuildingChanged(shop)]);
    assert_eq!(marked, vec![(k, "parking_fill")], "only the live layer of its chunk");
    assert!(before.iter().any(|(id, l)| *id != shop && l.0 == k.0 && l.1 == Layer::Settled), "its chunk has settled buildings, left alone");
}

#[test]
fn a_site_moving_on_redraws_only_the_live_layer_and_its_finish_both() {
    let (mut w, _, sites) = city();
    let site = sites[0];
    let before = drawn(&w);
    let k = building_layer(&w, site).unwrap();
    assert_eq!(k.1, Layer::Live, "a building site is live");
    assert_eq!(changed_layers(&w, &before, &[SimEvent::BuildingChanged(site)]), vec![(k, "construction")]);
    // Finished: drawn with the settled buildings now, gone from the live layer.
    w.finish_construction(site);
    let marked = changed_layers(&w, &before, &[SimEvent::BuildingChanged(site)]);
    assert_eq!(marked, vec![((k.0, Layer::Settled), "changed_other"), (k, "left_layer")]);
}

/// The two layers of a chunk draw exactly what the one merged chunk drew:
/// the same triangles per surface, the same detail, the same pads and
/// lamps, the same rigs.
#[test]
fn the_two_layers_draw_what_the_merged_chunk_drew() {
    let (w, _, _) = city();
    let kit = kit();
    let styles = {
        let mut s = StyleBook::default();
        s.refresh(&w, &kit.rules);
        s
    };
    let g = groups(&w);
    let chunks: std::collections::BTreeSet<ChunkKey> = g.iter().map(|(k, _)| k.0).collect();
    let mut both = 0;
    for c in chunks {
        let layers: Vec<&(LayerKey, Vec<BuildingId>)> = g.iter().filter(|(k, _)| k.0 == c).collect();
        both += (layers.len() == 2) as usize;
        let all: Vec<BuildingId> = w.buildings.keys().filter(|id| layers.iter().any(|(_, ids)| ids.contains(id))).collect();
        let merged = generate_layer(&w, kit.kit(), &styles, (c, Layer::Settled), &all, &|| false).unwrap();
        let parts: Vec<LayerGeo> = layers.iter().map(|(k, ids)| generate_layer(&w, kit.kit(), &styles, *k, ids, &|| false).unwrap()).collect();
        let tris = |g: &Geometry| g.parts.iter().map(|m| m.indices.len()).collect::<Vec<_>>();
        let sum = |gs: &[&Geometry]| -> Vec<usize> { (0..Surface::ALL.len()).map(|i| gs.iter().map(|g| g.parts[i].indices.len()).sum()).collect() };
        assert_eq!(tris(&merged.geo), sum(&parts.iter().map(|p| &p.geo).collect::<Vec<_>>()), "chunk {c:?}: base triangles per surface");
        for cell in 0..merged.cells.len() {
            assert_eq!(merged.cells[cell].triangle_count(), parts.iter().map(|p| p.cells[cell].triangle_count()).sum::<usize>(), "chunk {c:?} detail cell {cell}");
        }
        let n = |f: fn(&Geometry) -> usize| (f(&merged.geo), parts.iter().map(|p| f(&p.geo)).sum::<usize>());
        let (a, b) = n(|g| g.pads.len());
        assert_eq!(a, b, "pads");
        let (a, b) = n(|g| g.lamps.len());
        assert_eq!(a, b, "lamps");
        let (a, b) = n(|g| g.cranes.len());
        assert_eq!(a, b, "cranes");
        // Every building's own triangles are the same wherever it is drawn:
        // the vertex positions of the layers are those of the merged chunk.
        let mut m: Vec<[i64; 3]> = merged.geo.parts.iter().flat_map(|p| p.positions.iter()).map(|p| p.map(|x| (x * 1000.0).round() as i64)).collect();
        let mut l: Vec<[i64; 3]> = parts.iter().flat_map(|g| g.geo.parts.iter().flat_map(|p| p.positions.iter())).map(|p| p.map(|x| (x * 1000.0).round() as i64)).collect();
        m.sort_unstable();
        l.sort_unstable();
        assert!(m == l, "chunk {c:?}: the same vertices");
    }
    assert!(both > 0, "some chunk has both a live and a settled layer");
}

/// A chunk built by a job on the async pool, from a lease on the screen's
/// world, is the chunk built on the main thread.
#[test]
fn a_job_builds_what_the_main_thread_builds() {
    AsyncComputeTaskPool::get_or_init(bevy::tasks::TaskPool::default);
    let (w, shop, _) = city();
    let kit = Arc::new(kit());
    let mut styles = StyleBook::default();
    styles.refresh(&w, &kit.rules);
    let core = crate::sim_thread::SimCore::new(w);
    let chunk = building_layer(&core, shop).unwrap().0;
    let mine: Vec<(LayerKey, Vec<BuildingId>)> = groups(&core).into_iter().filter(|(k, _)| k.0 == chunk).collect();
    assert_eq!(mine.len(), 2, "its chunk has both layers");
    let sync = build_chunk(&core, kit.kit(), &styles, &mine, &|| false).unwrap();
    // The geometry itself, generated on the pool.
    let lease = core.lend();
    let (s2, m2) = (styles.clone(), mine.clone());
    let geo: Vec<LayerGeo> = block_on(AsyncComputeTaskPool::get().spawn(async move {
        let k = DrawKit { rules: nd_procgen::BuildingRules::default(), palette: nd_procgen::Palette::default() };
        m2.iter().map(|(key, ids)| generate_layer(lease.world(), k.kit(), &s2, *key, ids, &|| false).unwrap()).collect()
    }));
    let here: Vec<LayerGeo> = mine.iter().map(|(key, ids)| generate_layer(&core, kit.kit(), &styles, *key, ids, &|| false).unwrap()).collect();
    assert!(geo == here, "the same geometry on the pool");
    // And the whole job: the same layers, triangles and buildings.
    let ids: Vec<BuildingId> = mine.iter().flat_map(|(_, i)| i.iter().copied()).collect();
    let mut job = Job::start(core.lend(), kit.clone(), styles.subset(&ids), mine.clone());
    while !job.finished() {
        std::thread::yield_now();
    }
    let out = job.take().expect("not recalled");
    assert_eq!(out.len(), sync.len());
    for (a, b) in out.iter().zip(&sync) {
        assert_eq!(a.key, b.key);
        assert_eq!(a.triangles, b.triangles);
        assert_eq!(a.drawn, b.drawn);
        assert_eq!(a.base.len(), b.base.len());
        assert_eq!(a.detail.len(), b.detail.len());
    }
    assert_eq!(core.leases_out(), 0, "the job let the world go");
}

/// An edit while a job holds the world recalls the job, which stops and
/// lets go: the edit writes the screen's own copy, never a copy of it.
#[test]
fn an_edit_recalls_a_job_instead_of_copying_the_city() {
    let (w, _, _) = city();
    let mut core = crate::sim_thread::SimCore::new(w);
    let lease = core.lend();
    let reader = std::thread::spawn(move || {
        let mut steps = 0u64;
        while !lease.recalled() {
            steps += lease.world().buildings.len() as u64;
            std::thread::yield_now();
        }
        steps
    });
    let before: *const World = &*core;
    core.grant_treasury(1.0);
    let after: *const World = &*core;
    // (The reader may have seen the recall before reading anything.)
    let _ = reader.join().unwrap();
    assert_eq!(before, after, "written in place, not copied");
    assert_eq!(core.stats.recalls, 1);
    assert_eq!(core.leases_out(), 0);
}
