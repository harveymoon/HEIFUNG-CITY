//! Lots cut from one zone polygon on its own ([`generate_lots`]), and lots
//! made from parcels.

use super::*;

/// Carve `zone` into parcels: subtract every road corridor and the ground
/// taken by the `covering` zones (drawn later, so they win), slice the
/// blocks along their frontages, and keep parcels whose centre is not inside
/// a covering zone or an existing lot. Parcels with no road frontage or a
/// sliver shape are kept as landlocked, unusable ground. New lots are
/// appended to `lots`. A self-crossing outline is not a region and yields
/// nothing: boolean ops on it would produce garbage parcels.
pub fn generate_lots(
    zone_id: ZoneId,
    zone: &Zone,
    roads: &RoadNetwork,
    lots: &mut SlotMap<LotId, Lot>,
    covering: &[Zone],
) -> Vec<LotId> {
    generate_lots_wet(zone_id, zone, roads, lots, covering, &WaterMask::default())
}

/// [`generate_lots`] that also keeps parcels off the water.
pub fn generate_lots_wet(
    zone_id: ZoneId,
    zone: &Zone,
    roads: &RoadNetwork,
    lots: &mut SlotMap<LotId, Lot>,
    covering: &[Zone],
    water: &WaterMask,
) -> Vec<LotId> {
    let lu = zone.land_use;
    let mut created = Vec::new();
    if zone.polygon.len() < 3 || roads.is_empty() || geom::self_intersects(&zone.polygon) {
        return created;
    }
    let mut rng = crate::rng_for(polygon_seed(&zone.polygon), 5);
    let zone_poly = to_geo(&ccw(&zone.polygon));
    let (zlo, zhi) = bounds_of(&zone.polygon);

    // Blocks: the zone minus every nearby road corridor (one smooth polygon
    // per chain of road pieces).
    let mut corridors: Option<MultiPolygon<f64>> = None;
    for (pts, hw, ext_a, ext_b, _) in road_chains(roads) {
        let (lo, hi) = pts.iter().fold((Vec2::splat(f32::MAX), Vec2::splat(f32::MIN)), |(lo, hi), p| (lo.min(*p), hi.max(*p)));
        let (lo, hi) = (lo - Vec2::splat(hw * 2.0), hi + Vec2::splat(hw * 2.0));
        if hi.x < zlo.x || hi.y < zlo.y || lo.x > zhi.x || lo.y > zhi.y {
            continue;
        }
        let Some(c) = chain_corridor(&pts, hw, ext_a, ext_b) else { continue };
        corridors = Some(match corridors {
            None => MultiPolygon(vec![c]),
            Some(m) => m.union(&c),
        });
    }
    let mut blocks: MultiPolygon<f64> = match &corridors {
        Some(m) => zone_poly.difference(m),
        None => MultiPolygon(vec![zone_poly]),
    };
    // Ground under a newer zone is theirs: take it out before slicing, so
    // the parcels along the shared boundary stop at it instead of straddling
    // it. (The centre test below still guards against slivers of overlap.)
    let mut covered: Option<MultiPolygon<f64>> = None;
    for z in covering {
        if z.polygon.len() < 3 || geom::self_intersects(&z.polygon) || !boxes_overlap(bounds_of(&z.polygon), (zlo, zhi), 0.0) {
            continue;
        }
        let c = to_geo(&z.polygon);
        covered = Some(match covered {
            None => MultiPolygon(vec![c]),
            Some(m) => m.union(&c),
        });
    }
    if let Some(c) = &covered {
        blocks = blocks.difference(c);
    }
    // Rivers, lakes and the sea are nobody's parcel.
    if let Some(w) = water.near(zlo, zhi) {
        blocks = blocks.difference(&w);
    }

    let mut parcels: Vec<Vec<Vec2>> = Vec::new();
    for block in blocks.0 {
        subdivide(block, lu, roads, &mut rng, 0, &mut parcels);
    }
    let parcels = merge_backland(parcels, lu, roads);

    // Lots already there (other zones) by bounding box, so each parcel is
    // tested against its neighbours only. Lots made by this call are few
    // and checked directly.
    let existing: Vec<(LotId, (Vec2, Vec2))> = lots.iter().map(|(id, l)| (id, bounds_of(&l.polygon))).collect();
    let existing_grid = Grid::new(60.0, existing.iter().map(|(_, b)| *b));
    let mut mine: Vec<(LotId, (Vec2, Vec2))> = Vec::new();
    let shares_ground = |l: &Lot, center: Vec2, poly: &[Vec2]| l.contains(center) || point_in_polygon(l.center, poly);

    for poly in parcels {
        let poly = ccw(&poly);
        if poly.len() < 3 {
            continue;
        }
        let center = inner_center(&poly);
        if covering.iter().any(|z| z.contains(center)) {
            continue;
        }
        let pbox = bounds_of(&poly);
        let taken = existing_grid.in_box(pbox.0, pbox.1).into_iter().any(|i| shares_ground(&lots[existing[i as usize].0], center, &poly))
            || mine.iter().any(|(id, b)| boxes_overlap(*b, pbox, 0.0) && shares_ground(&lots[*id], center, &poly));
        if taken {
            continue;
        }
        let Some(lot) = make_lot(zone_id, lu, poly, center, roads) else { continue };
        let id = lots.insert(lot);
        mine.push((id, pbox));
        created.push(id);
    }
    created
}

/// A lot of land use `lu` on a parcel outline (counter-clockwise) with its
/// interior centre: frontage, the frontage-aligned rectangle, the street a
/// car reaches it from, and whether it is landlocked for that use (no
/// frontage it may use, a sharp inward corner, a sliver, too narrow).
/// None when there is no road anywhere to hang it on.
pub fn make_lot(zone_id: ZoneId, lu: LandUse, poly: Vec<Vec2>, center: Vec2, roads: &RoadNetwork) -> Option<Lot> {
    let area = polygon_area(&poly);
    let compact = area / perimeter(&poly).powi(2);
    let (fi, road) = frontage_edge(&poly, roads, lu.fronts_paths());
    let n = poly.len();
    let fa = poly[fi];
    let fb = poly[(fi + 1) % n];
    let along = (fb - fa).normalize_or_zero();
    if along == Vec2::ZERO {
        return None;
    }
    let inward = Vec2::new(-along.y, along.x);
    let (u0, u1) = extents(&poly, along);
    let (v0, v1) = extents(&poly, inward);
    let corners = [
        along * u0 + inward * v0,
        along * u1 + inward * v0,
        along * u1 + inward * v1,
        along * u0 + inward * v1,
    ];
    let has_frontage = road.is_some() && fa.distance(fb) >= MIN_FRONTAGE.min(lu.lot_width() * 0.45);
    let sharp = reflex_vertex(&poly, 0.6).is_some();
    let landlocked = !has_frontage || sharp || compact < MIN_COMPACTNESS || (u1 - u0) < 8.0 || (v1 - v0) < 8.0;
    #[cfg(test)]
    if landlocked {
        let reason = if !has_frontage { "nofront" } else if sharp { "sharp" } else if compact < MIN_COMPACTNESS { "compact" } else { "narrow" };
        DEAD_REASONS.with(|r| r.borrow_mut().push((center, reason, area, compact)));
    }
    let (sid, t) = match road {
        Some(hit) => hit,
        None => {
            let (s, t, _) = nearest_road(roads, center, true)?;
            (s, t)
        }
    };
    let fronts_street = roads.segment(sid).is_street();
    let entrance = fa.lerp(fb, 0.5);
    let (car_segment, car_point) = if fronts_street {
        (Some(sid), entrance)
    } else {
        match roads.nearest_street(entrance) {
            Some((cs, _, q)) if q.distance(entrance) < 260.0 => (Some(cs), q),
            _ => (None, entrance),
        }
    };
    Some(Lot {
        zone: zone_id,
        land_use: lu,
        polygon: poly,
        corners,
        front_edge: [fa, fb],
        center,
        front_dir: -inward,
        front_segment: sid,
        front_t: t,
        access_node: roads.nearest_endpoint(sid, t),
        building: None,
        landlocked,
        footfall_today: 0,
        footfall_hour: 0,
        car_segment,
        car_point,
        park: None,
        shore: LotShore::default(),
    })
}

/// [free-placement] The lot under a building the player placed: ground the
/// city took out of the zoning. Its use is [`LandUse::Civic`]; its frontage
/// is a street where it has one, else the footpath it lines (a metro
/// entrance or a battery yard may stand on a path), so ground any zone could
/// use is ground a city building can use.
pub fn make_placed_lot(zone_id: ZoneId, poly: Vec<Vec2>, center: Vec2, roads: &RoadNetwork) -> Option<Lot> {
    let street = make_lot(zone_id, LandUse::Civic, poly.clone(), center, roads);
    let mut lot = match street {
        Some(l) if !l.landlocked => l,
        other => match make_lot(zone_id, LandUse::Residential, poly, center, roads) {
            Some(l) if !l.landlocked => l,
            fallback => other.or(fallback)?,
        },
    };
    lot.land_use = LandUse::Civic;
    Some(lot)
}
