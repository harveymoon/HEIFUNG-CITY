//! Lot ground, the zoning wash, parks and vacant-lot dressing.
//!
//! [refactor/app] Split out of `render.rs`.

use super::*;

/// Park land is a park as soon as it is planned, built or not, and park
/// backland with no road is reached through the park itself.
pub(super) fn is_park_lot(lot: &nd_sim::Lot) -> bool {
    lot.building.is_none() && lot.park.is_some()
}

/// A lot with nothing on it and no park plan: it gets the vacant dressing.
pub(super) fn is_vacant_lot(lot: &nd_sim::Lot) -> bool {
    lot.building.is_none() && lot.park.is_none() && !lot.landlocked
}

/// What floors a lot under whatever stands on it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(super) enum LotFill {
    /// Unusable ground: plain earth, no outline.
    Earth,
    /// A built lot's gravel.
    Gravel,
    None,
}

/// The floor of a lot (not a park's): plain earth for unusable ground with
/// nothing on it, gravel under a building; nothing on a vacant lot (the
/// zoning layer shows what it is zoned for), under a strip mine (its own
/// yard round the pit) or what floors its own ground (`own_floor`): a
/// landmark (its big, often notched site: a gravel fill lifted onto the
/// ground vertex by vertex rose over its lawns and paths), a beach or a
/// built park. [r4-flags-look] (K19, flag day 27 at (470,314)) A beach on
/// backland was floored with the unusable ground's green earth, a few
/// centimetres under its sand on a raised pad and drawn over it: the
/// umbrellas and loungers stood in "a layer of grass".
pub(super) fn lot_fill(landlocked: bool, built: bool, holed: bool, own_floor: bool) -> LotFill {
    match (landlocked, built) {
        (true, false) => LotFill::Earth,
        (_, false) => LotFill::None,
        _ if holed || own_floor => LotFill::None,
        _ => LotFill::Gravel,
    }
}

/// Lot ground fills and (with the zoning overlay, toggled with O) the zone
/// outline glows and vacant-lot outlines, looked at again when a lot
/// appears, disappears, or changes between built, vacant and park: the
/// fills one mesh per chunk ([r11-adopt-frames]: only the chunks whose
/// fills changed are built again), the zoning layer's two for the whole
/// city. Land value does not reach them, so the periodic land-value
/// recompute no longer touches any ground mesh.
pub(super) fn sync_lots(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    sim: Res<Sim>,
    assets: Res<GameAssets>,
    mats: Res<Materials>,
    overlays: Res<Overlays>,
    mut state: ResMut<RenderState>,
    tools: Res<crate::tools::ToolState>,
) {
    let _t = crate::frame_stats::span("sync_lots");
    let w = &sim.world;
    // Unzoned parcels are outlined grey with the zoning overlay, and always
    // while the Zone tool is out (they are what it paints).
    let show_parcels = overlays.zoning || tools.tool == crate::tools::Tool::Zone;
    let fresh = !assets.palette_changed && !sim.full_rebuild;
    // [p2-look] A strip mine's plots are floored by the mine (its yard
    // round the pit dug into them), not with the lot gravel; a mine opening
    // or growing changes no lot, so they are in the gate.
    let holed = nd_procgen::holes::holed_lots(w);
    let holed_sig = {
        let mut h = DefaultHasher::new();
        holed.hash(&mut h);
        h.finish()
    };
    if state.lots_gate.same((sim.generation, w.lots.version(), w.parcels.version, w.zones.len(), w.roads.version, overlays.zoning, show_parcels, holed_sig)) && state.lots_signature.is_some() && fresh {
        return;
    }
    let mut h = DefaultHasher::new();
    holed_sig.hash(&mut h);
    for (id, lot) in w.lots.iter() {
        id.hash(&mut h);
        (lot.building.is_some(), lot.park.is_some(), lot.landlocked).hash(&mut h);
    }
    (w.parcels.version, show_parcels).hash(&mut h);
    let signature = (w.lots.len(), w.zones.len(), w.roads.version, overlays.zoning, h.finish());
    if state.lots_signature == Some(signature) && !assets.palette_changed && !sim.full_rebuild {
        return;
    }
    state.lots_signature = Some(signature);
    for e in state.lot_overlays.drain(..) {
        commands.entity(e).despawn();
    }
    let pal = &assets.palette;
    let outlines = overlays.zoning;
    let mut glow = MeshData::default();
    // The zoning layer: every zoned lot washed in its zone's colour (bright,
    // translucent, above the ground and pavements), built or not, with a
    // crisp edge; built lots a little fainter so the city still reads.
    let mut wash = MeshData::default();
    if outlines {
        // The drawn zone polygons (painted lots carry zones of their own
        // outline, which the lot outlines below already show).
        for (_, polygon, lu) in w.master.zones() {
            if polygon.len() < 3 {
                continue;
            }
            let c = pal.zone_color(lu);
            let mut outline = polygon.to_vec();
            if let Some(&first) = polygon.first() { outline.push(first); }
            glow.ground_strip(&outline, 0.5, 0.04, scale(c, 1.0));
        }
    }
    if show_parcels {
        let grey = [0.5f32, 0.5, 0.56];
        // [island-lots] On a hillside the outline is lifted onto the ground
        // vertex by vertex: long edges get points every few metres so they
        // follow the slope rather than cut through it (dark dashes and
        // spikes on the island hills).
        let step = if w.landscape.is_flat() { f32::INFINITY } else { 4.0 };
        for p in w.parcels.parcels.iter().filter(|p| p.lot.is_none() && p.polygon.len() >= 3) {
            let mut closed = p.polygon.clone();
            if let Some(&first) = p.polygon.first() { closed.push(first); }
            let outline = densify(&closed, step);
            let k = if p.landlocked() { 0.18 } else { 0.32 };
            glow.ground_strip(&outline, 0.16, 0.03, scale(grey, k));
        }
    }
    if outlines {
        for lot in w.lots.values() {
            if lot.polygon.len() < 3 || lot.landlocked {
                continue;
            }
            // [sweep-sim] A dock or a station stands on its own unzoned
            // footprint: drawn as itself (a pale transport outline), not as
            // the civic/industrial use its lot carries for the economy.
            let transport = lot.building.and_then(|b| w.buildings.get(b)).is_some_and(|b| b.port.is_some() || b.rail.is_some() || b.airport.is_some());
            let c = if lot.shore.beach { BEACH_TINT } else if transport { TRANSPORT_TINT } else { zone_layer_color(lot.land_use) };
            let alpha = if lot.building.is_some() { 0.5 } else { 0.72 };
            // A plant or battery field is a flat yard: the wash would flood
            // it in the zone's colour (a solar farm on a residential lot
            // read as a green block), so it keeps only the outline.
            let power = lot.building.and_then(|b| w.buildings.get(b)).is_some_and(|b| b.is_power());
            if !power && !transport {
                wash.ground_polygon(&lot.polygon, 0.35, [c[0], c[1], c[2], alpha]);
            }
            // A park's lots are one park: no zone edge down the seams
            // between them, only round the outside.
            // [r8-parks] Nor across a footpath through it.
            let shared: &[usize] = lot.park.as_ref().map_or(&[], |p| p.shared_edges.as_slice());
            if let Some(p) = lot.park.as_ref().filter(|p| !p.inner_edges.is_empty()) {
                for (a, b) in p.outer_stretches(&lot.polygon) {
                    glow.ground_strip(&[a, b], 0.35, 0.4, scale(c, 1.3));
                }
            } else if shared.is_empty() {
                let mut edge = lot.polygon.clone();
                if let Some(&first) = lot.polygon.first() { edge.push(first); }
                glow.ground_strip(&edge, 0.35, 0.4, scale(c, 1.3));
            } else {
                let n = lot.polygon.len();
                for i in (0..n).filter(|i| !shared.contains(i)) {
                    glow.ground_strip(&[lot.polygon[i], lot.polygon[(i + 1) % n]], 0.35, 0.4, scale(c, 1.3));
                }
            }
        }
    }
    // [r11-adopt-frames] The fills by chunk: only chunks whose lots' fills
    // changed are built again.
    let fills = lot_fills(w, &holed);
    let hashes = fill_hashes(w, &fills);
    if assets.palette_changed || sim.full_rebuild {
        state.lot_fills.retain(|_, (_, e)| {
            if let Some(e) = e {
                commands.entity(*e).despawn();
            }
            false
        });
    }
    let gone: Vec<ChunkKey> = state.lot_fills.keys().filter(|k| !hashes.contains_key(*k)).copied().collect();
    for k in gone {
        if let Some((_, Some(e))) = state.lot_fills.remove(&k) {
            commands.entity(e).despawn();
        }
    }
    for (k, h) in &hashes {
        if state.lot_fills.get(k).map(|d| d.0) == Some(*h) {
            continue;
        }
        if let Some((_, Some(e))) = state.lot_fills.remove(k) {
            commands.entity(e).despawn();
        }
        let fill = fill_mesh_in(w, &fills, *k, pal);
        let e = (!fill.is_empty()).then(|| {
            let mut e = commands.spawn((Mesh3d(meshes.add(to_mesh(&fill, true))), MeshMaterial3d(mats.surface(Surface::Dirt)), Transform::IDENTITY, LotsTag));
            // [r12-grime] Weeds and soil in from the lots' edges.
            if weathering::has_channel(&fill) {
                e.insert(weathering::Weathered(Surface::Dirt));
            }
            e.id()
        });
        state.lot_fills.insert(*k, (*h, e));
    }
    if !glow.is_empty() {
        state.lot_overlays.push(commands.spawn((Mesh3d(meshes.add(to_mesh(&glow, false))), MeshMaterial3d(mats.glow()), Transform::IDENTITY, NotShadowCaster, NotShadowReceiver, LotsTag)).id());
    }
    if !wash.is_empty() {
        state.lot_overlays.push(commands.spawn((Mesh3d(meshes.add(to_mesh(&wash, false))), MeshMaterial3d(mats.overlay.clone()), Transform::IDENTITY, NotShadowCaster, NotShadowReceiver, LotsTag)).id());
    }
}

/// [r11-adopt-frames] Every lot's fill (not a park's) with its chunk, in
/// the order the world lists them: a landmark, a beach or a built park
/// floors its own ground; [airport] an airport's field is drawn by
/// `render/airport.rs`.
pub(super) fn lot_fills(w: &World, holed: &[LotId]) -> Vec<(ChunkKey, LotId, LotFill)> {
    let mut out = Vec::new();
    for (id, lot) in w.lots.iter() {
        if is_park_lot(lot) || lot.polygon.is_empty() {
            continue;
        }
        let own_floor = lot.land_use == nd_sim::LandUse::Park || lot.building.and_then(|b| w.buildings.get(b)).is_some_and(|b| b.service.is_some_and(|s| s.kind.is_landmark()) || b.airport.is_some());
        let fill = lot_fill(lot.landlocked, lot.building.is_some(), holed.contains(&id), own_floor);
        if fill != LotFill::None {
            out.push((chunk_of(lot.center), id, fill));
        }
    }
    out
}

/// [r11-adopt-frames] Each chunk's fills hashed: the lots, their fill and
/// their outlines, and the ground they are set on (`lift_spawned` lifts a
/// fill onto the ground only when it is spawned: a regraded ground draws
/// every chunk again, as the one mesh was).
pub(super) fn fill_hashes(w: &World, fills: &[(ChunkKey, LotId, LotFill)]) -> HashMap<ChunkKey, u64> {
    let mut hs: HashMap<ChunkKey, DefaultHasher> = HashMap::new();
    let ground = (w.ground.version(), w.landscape.version(), w.relief);
    for (k, id, fill) in fills {
        let h = hs.entry(*k).or_insert_with(|| {
            let mut h = DefaultHasher::new();
            ground.hash(&mut h);
            h
        });
        (id, fill).hash(h);
        hash_f32s(h, w.lots[*id].polygon.iter().copied());
    }
    hs.into_iter().map(|(k, h)| (k, h.finish())).collect()
}

/// [r11-adopt-frames] One chunk's lot fill mesh.
pub(super) fn fill_mesh_in(w: &World, fills: &[(ChunkKey, LotId, LotFill)], chunk: ChunkKey, pal: &nd_procgen::Palette) -> MeshData {
    let mut fill = MeshData::default();
    for (k, id, f) in fills {
        if *k != chunk {
            continue;
        }
        let lot = &w.lots[*id];
        fill_lot(&mut fill, lot, *f, pal, lot_wear(w, lot, *f));
    }
    fill
}

/// [r12-grime] How worn a lot's floor looks: its building's grime (by
/// age, condition, tier and use), or well worn for unusable ground.
pub(super) fn lot_wear(w: &World, lot: &nd_sim::Lot, f: LotFill) -> f32 {
    match (f, lot.building.and_then(|b| w.buildings.get(b))) {
        (LotFill::Earth, _) => 0.7,
        (_, Some(b)) => {
            let age_days = w.clock.tick.saturating_sub(b.built_tick) as f32 / nd_sim::clock::MINUTES_PER_DAY as f32;
            nd_procgen::weathering::building_grime(b.tier, b.condition, age_days, b.land_use, None)
        }
        _ => 0.0,
    }
}

fn seg_dist(p: SVec2, a: SVec2, b: SVec2) -> f32 {
    let ab = b - a;
    let l2 = ab.length_squared();
    let t = if l2 < 1e-6 { 0.0 } else { ((p - a).dot(ab) / l2).clamp(0.0, 1.0) };
    p.distance(a + ab * t)
}

/// One lot's floor. [r12-grime] A convex lot is laid as a fan from its
/// middle, the middle marked with how far in it is and the rim with
/// nothing, so the weathering's weeds and soil creep in from the lot's
/// edge (furthest into its corners); any other lot as one polygon.
pub(super) fn fill_lot(fill: &mut MeshData, lot: &nd_sim::Lot, f: LotFill, pal: &nd_procgen::Palette, wear: f32) {
    let color = match f {
        LotFill::Earth => scale(pal.ground, 1.15),
        LotFill::Gravel => rgba(pal.gravel),
        LotFill::None => return,
    };
    let poly = &lot.polygon;
    if wear > 0.0 && poly.len() >= 3 && nd_procgen::mesh::is_convex(poly) {
        let c = poly.iter().copied().fold(SVec2::ZERO, |a, p| a + p) / poly.len() as f32;
        let inr = (0..poly.len()).map(|i| seg_dist(c, poly[i], poly[(i + 1) % poly.len()])).fold(f32::MAX, f32::min);
        if inr > 0.5 {
            use nd_procgen::weathering::{edge_aux, pack_edge};
            let start = fill.positions.len();
            let ring: Vec<Vec3> = poly.iter().map(|p| Vec3::new(p.x, 0.02, p.y)).collect();
            fill.ground_fan(Vec3::new(c.x, 0.02, c.y), &ring, color);
            fill.set_aux(start, edge_aux(wear, pack_edge(0.0, 0.0)));
            fill.aux[start] = edge_aux(wear, pack_edge(inr, 0.0));
            return;
        }
    }
    fill.ground_polygon(poly, 0.02, color);
}

/// [waterfront] Public beaches on the zoning layer and in previews: sand.
pub(super) const BEACH_TINT: [f32; 3] = [1.15, 0.9, 0.45];
/// [sweep-sim] Docks and stations on the zoning layer: their own use, an
/// outline in pale steel, no zone wash.
pub(super) const TRANSPORT_TINT: [f32; 3] = [0.75, 0.8, 0.95];

/// The zoning layer's colours: fixed, saturated and far apart (as city
/// builders use them), not the palette's subdued tints, so zones read at a
/// glance over any ground. Values just above 1 glow a touch at night.
pub(super) fn zone_layer_color(lu: nd_sim::LandUse) -> [f32; 3] {
    match lu {
        nd_sim::LandUse::Residential => [0.15, 1.15, 0.25],
        nd_sim::LandUse::Commercial => [0.2, 0.55, 1.25],
        nd_sim::LandUse::Industrial => [1.2, 0.95, 0.05],
        // [r4-leisure] commercial land of the leisure sub-zone: commercial's
        // blue leaning violet, one family on the map
        nd_sim::LandUse::Entertainment => [0.55, 0.35, 1.3],
        nd_sim::LandUse::Civic => [0.05, 1.05, 1.15],
        nd_sim::LandUse::Park => [0.1, 0.55, 0.15],
    }
}

/// What a park lot's drawing depends on: its tier and feature set, its
/// paths and benches, and the parcel outline itself.
pub(super) fn park_key(lot: &nd_sim::Lot, layout: &nd_sim::ParkLayout) -> u64 {
    let mut h = DefaultHasher::new();
    layout.tier.hash(&mut h);
    hash_f32s(&mut h, lot.polygon.iter().copied());
    for f in &layout.features {
        f.kind.hash(&mut h);
        hash_f32s(&mut h, [f.pos, SVec2::new(f.radius, f.heading)]);
    }
    for path in &layout.paths {
        hash_f32s(&mut h, path.iter().copied());
    }
    for &(p, facing) in &layout.benches {
        hash_f32s(&mut h, [p]);
        facing.to_bits().hash(&mut h);
    }
    layout.shared_edges.hash(&mut h);
    layout.anchor.hash(&mut h);
    // [r8-parks] The features reaching over it, its car park, its seams.
    for (p, r) in &layout.nearby {
        hash_f32s(&mut h, [*p, SVec2::new(*r, 0.0)]);
    }
    if let Some(c) = &layout.car_park {
        hash_f32s(&mut h, c.outline.iter().copied());
        c.spaces.len().hash(&mut h);
    }
    for (a, b) in &layout.inner_edges {
        hash_f32s(&mut h, [*a, *b]);
    }
    h.finish()
}

/// Park layouts, one set of entities per park lot. A park is regenerated
/// only when its own layout or outline changes; neighbouring parks and the
/// land-value recompute leave it alone.
pub(super) fn sync_parks(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    sim: Res<Sim>,
    assets: Res<GameAssets>,
    mats: Res<Materials>,
    mut state: ResMut<RenderState>,
) {
    let w = &sim.world;
    let rebuild_all = assets.palette_changed || assets.rules_changed || sim.full_rebuild;
    if state.parks_gate.same((sim.generation, w.lots.version())) && !rebuild_all {
        return;
    }
    // Drop parks that are gone, built on, or stale.
    let mut wanted: HashMap<LotId, u64> = HashMap::new();
    for (id, lot) in w.lots.iter() {
        if let Some(layout) = lot.park.as_ref().filter(|_| is_park_lot(lot)) {
            wanted.insert(id, park_key(lot, layout));
        }
    }
    let stale: Vec<LotId> = state.parks.iter().filter(|(id, (key, _))| rebuild_all || wanted.get(id) != Some(key)).map(|(id, _)| *id).collect();
    for id in stale {
        if let Some((_, ents)) = state.parks.remove(&id) {
            for e in ents {
                commands.entity(e).despawn();
            }
        }
    }
    for (id, key) in wanted {
        if state.parks.contains_key(&id) {
            continue;
        }
        let lot = &w.lots[id];
        let Some(layout) = &lot.park else { continue };
        let seed = (lot.center.x * 31.0 + lot.center.y * 17.0).abs() as u64;
        // [r5-layered] No trees under a raised deck over the park.
        let overhead = nd_procgen::undercroft::deck_quads(w, &lot.polygon);
        let extras = Extras { park: Some(layout), overhead: &overhead, ..Extras::default() };
        let mut geo = generate_building(lot, nd_sim::LandUse::Park, 1, layout.tier, 1.0, seed, &assets.rules, &assets.palette, &extras);
        nd_procgen::climate::snow_cover(&mut geo, assets.palette.snow);
        let ents = spawn_geometry(&mut commands, &mut meshes, &mats, &geo, ParkTag);
        state.parks.insert(id, (key, ents));
    }
}

/// Which look a vacant lot gets for a land value: dormant, wild or parklet.
pub(super) fn vacant_bucket(land_value: f32) -> u8 {
    if land_value < nd_procgen::vacant::LOW {
        0
    } else if land_value < nd_procgen::vacant::HIGH {
        1
    } else {
        2
    }
}

/// Vacant-lot dressing, merged into one mesh per surface per chunk. A chunk
/// is regenerated when the set of vacant lots in it changes, a lot's outline
/// changes, or a lot crosses a land-value bucket; the land-value recompute
/// itself (every 120 ticks) changes nothing until a lot crosses a threshold.
pub(super) fn sync_vacant(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    sim: Res<Sim>,
    assets: Res<GameAssets>,
    mats: Res<Materials>,
    mut state: ResMut<RenderState>,
) {
    let w = &sim.world;
    let rebuild_all = assets.palette_changed || sim.full_rebuild;
    // [r11-lot-memory] Old slabs follow the lot history and fade by the day.
    if state.vacant_gate.same((sim.generation, w.lots.version(), w.land_value.version(), w.config.seed, w.lot_memory.version, w.clock.day())) && !rebuild_all {
        return;
    }
    // Per chunk: the lots in it and a hash of what their dressing depends on.
    let mut per_chunk: HashMap<ChunkKey, (DefaultHasher, Vec<LotId>)> = HashMap::new();
    for (id, lot) in w.lots.iter() {
        if !is_vacant_lot(lot) {
            continue;
        }
        let (h, ids) = per_chunk.entry(chunk_of(lot.center)).or_insert_with(|| (DefaultHasher::new(), Vec::new()));
        id.hash(h);
        vacant_bucket(w.land_value.at(lot.center)).hash(h);
        hash_f32s(h, lot.polygon.iter().copied());
        nd_procgen::lot_memory::slab_trace(w, lot).map(|(t, s)| (t.id, s)).hash(h);
        ids.push(id);
    }
    let wanted: HashMap<ChunkKey, (u64, Vec<LotId>)> = per_chunk.into_iter().map(|(k, (h, ids))| (k, (h.finish(), ids))).collect();
    let stale: Vec<ChunkKey> = state.vacant_chunks.iter().filter(|(k, (key, _))| rebuild_all || wanted.get(k).map(|(h, _)| h) != Some(key)).map(|(k, _)| *k).collect();
    for k in stale {
        if let Some((_, ents)) = state.vacant_chunks.remove(&k) {
            for e in ents {
                commands.entity(e).despawn();
            }
        }
    }
    let pal = &assets.palette;
    for (k, (key, ids)) in wanted {
        if state.vacant_chunks.contains_key(&k) {
            continue;
        }
        let mut geo = Geometry::default();
        for id in ids {
            let lot = &w.lots[id];
            let seed = w.config.seed ^ ((lot.center.x * 31.0 + lot.center.y * 17.0).abs() as u64);
            // [r11-lot-memory] The slab of what stood here, under the weeds.
            // On dear land the owner has it turfed into a parklet after two
            // weeks; until then the lot is the bare cleared ground round it.
            let value = w.land_value.at(lot.center);
            let slab = nd_procgen::lot_memory::slab_trace(w, lot).filter(|(_, s)| vacant_bucket(value) < 2 || *s < 2);
            let value = if slab.is_some() && vacant_bucket(value) == 2 { (nd_procgen::vacant::LOW + nd_procgen::vacant::HIGH) * 0.5 } else { value };
            // Dormant, going wild or kept as a parklet, by what the land is worth.
            geo.append(&generate_vacant(lot, value, seed, pal));
            if let Some((t, s)) = slab {
                nd_procgen::lot_memory::slab(&mut geo, &lot.polygon, t, s, pal);
            }
        }
        nd_procgen::climate::snow_cover(&mut geo, pal.snow);
        let ents = if geo.triangle_count() > 0 { spawn_geometry(&mut commands, &mut meshes, &mats, &geo, VacantTag) } else { Vec::new() };
        state.vacant_chunks.insert(k, (key, ents));
    }
}

#[cfg(test)]
mod lot_fill_tests {
    use super::*;

    /// [r4-flags-look] (K19) A built lot on backland is floored like any
    /// built lot, not with the unusable ground's earth.
    #[test]
    fn built_backland_is_not_floored_as_unusable_ground() {
        assert_eq!(lot_fill(true, false, false, false), LotFill::Earth, "empty backland");
        assert_eq!(lot_fill(true, true, false, false), LotFill::Gravel, "a company's building on backland");
        assert_eq!(lot_fill(true, true, false, true), LotFill::None, "a beach on backland floors its own sand");
        assert_eq!(lot_fill(false, true, false, false), LotFill::Gravel);
        assert_eq!(lot_fill(false, false, false, false), LotFill::None, "vacant");
        assert_eq!(lot_fill(false, true, true, false), LotFill::None, "a strip mine");
        assert_eq!(lot_fill(false, true, false, true), LotFill::None, "a landmark or a park");
    }

    /// Every vertex of a mesh, as bits, sorted.
    fn vertices(m: &MeshData) -> Vec<[u32; 3]> {
        let mut v: Vec<[u32; 3]> = m.positions.iter().map(|p| p.map(f32::to_bits)).collect();
        v.sort_unstable();
        v
    }

    /// [r11-adopt-frames] A lot built on redraws the fill of its chunk, not
    /// the city's; the chunks' fills together are the one fill drawn before.
    #[test]
    fn a_lot_built_on_redraws_only_its_chunks_fill() {
        let mut w = World::new(nd_sim::WorldConfig::default());
        nd_sim::scenarios::demo_district(&mut w);
        let pal = nd_procgen::Palette::default();
        let holed = nd_procgen::holes::holed_lots(&w);
        let before = fill_hashes(&w, &lot_fills(&w, &holed));
        assert!(before.len() > 2, "fills in several chunks: {}", before.len());
        let lot = w.lots.iter().filter(|(_, l)| l.usable() && l.building.is_none()).map(|(id, _)| id).min().expect("an empty lot");
        w.build_on(lot);
        assert!(w.lots[lot].building.is_some());
        let fills = lot_fills(&w, &holed);
        let after = fill_hashes(&w, &fills);
        let moved: Vec<ChunkKey> = after.iter().filter(|(k, h)| before.get(*k) != Some(*h)).map(|(k, _)| *k).collect();
        assert_eq!(moved, vec![chunk_of(w.lots[lot].center)], "its chunk only");
        // The whole fill as one mesh, as it was drawn before.
        let mut whole = MeshData::default();
        for (id, lot) in w.lots.iter() {
            if is_park_lot(lot) || lot.polygon.is_empty() {
                continue;
            }
            let own_floor = lot.land_use == nd_sim::LandUse::Park || lot.building.and_then(|b| w.buildings.get(b)).is_some_and(|b| b.service.is_some_and(|s| s.kind.is_landmark()) || b.airport.is_some());
            let f = lot_fill(lot.landlocked, lot.building.is_some(), holed.contains(&id), own_floor);
            fill_lot(&mut whole, lot, f, &pal, lot_wear(&w, lot, f));
        }
        let mut parts = MeshData::default();
        for k in after.keys() {
            parts.append(&fill_mesh_in(&w, &fills, *k, &pal));
        }
        assert!(!whole.positions.is_empty());
        assert_eq!(parts.indices.len(), whole.indices.len());
        assert_eq!(vertices(&parts), vertices(&whole));
    }
}
