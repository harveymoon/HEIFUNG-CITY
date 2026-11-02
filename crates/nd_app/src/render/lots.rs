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
#[derive(Debug, PartialEq, Eq)]
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
/// outline glows and vacant-lot outlines: three meshes for the whole city,
/// rebuilt when a lot appears, disappears, or changes between built, vacant
/// and park. Land value does not reach them, so the periodic land-value
/// recompute no longer touches any ground mesh.
pub(super) fn sync_lots(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    sim: Res<Sim>,
    assets: Res<GameAssets>,
    mats: Res<Materials>,
    overlays: Res<Overlays>,
    mut state: ResMut<RenderState>,
    existing: Query<Entity, With<LotsTag>>,
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
    despawn_all(&mut commands, &existing);
    let pal = &assets.palette;
    let outlines = overlays.zoning;
    let mut glow = MeshData::default();
    let mut fill = MeshData::default();
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
            let shared: &[usize] = lot.park.as_ref().map_or(&[], |p| p.shared_edges.as_slice());
            if shared.is_empty() {
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
    for (id, lot) in w.lots.iter() {
        if is_park_lot(lot) || lot.polygon.is_empty() {
            continue;
        }
        // A landmark, a beach or a built park floors its own ground.
        // [airport] An airport's field is drawn by `render/airport.rs`.
        let own_floor = lot.land_use == nd_sim::LandUse::Park || lot.building.and_then(|b| w.buildings.get(b)).is_some_and(|b| b.service.is_some_and(|s| s.kind.is_landmark()) || b.airport.is_some());
        match lot_fill(lot.landlocked, lot.building.is_some(), holed.contains(&id), own_floor) {
            LotFill::Earth => fill.ground_polygon(&lot.polygon, 0.02, scale(pal.ground, 1.15)),
            LotFill::Gravel => fill.ground_polygon(&lot.polygon, 0.02, rgba(pal.gravel)),
            LotFill::None => {}
        }
    }
    if !fill.is_empty() {
        commands.spawn((Mesh3d(meshes.add(to_mesh(&fill, true))), MeshMaterial3d(mats.surface(Surface::Dirt)), Transform::IDENTITY, LotsTag));
    }
    if !glow.is_empty() {
        spawn_glow(&mut commands, &mut meshes, &mats, &glow, LotsTag);
    }
    if !wash.is_empty() {
        spawn_overlay(&mut commands, &mut meshes, &mats, &wash, LotsTag);
    }
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
    if state.vacant_gate.same((sim.generation, w.lots.version(), w.land_value.version(), w.config.seed)) && !rebuild_all {
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
            // Dormant, going wild or kept as a parklet, by what the land is worth.
            let seed = w.config.seed ^ ((lot.center.x * 31.0 + lot.center.y * 17.0).abs() as u64);
            geo.append(&generate_vacant(lot, w.land_value.at(lot.center), seed, pal));
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
}
