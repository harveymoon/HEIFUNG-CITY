//! The water surface: the fine translucent sheet and the far water.

use super::*;

/// The water surface: one flat sheet at the waterline over every cell near
/// water (the banks hide its edges), shaded deeper away from the shore.
/// Drawn with its own translucent material.
pub fn build_water(world: &World, palette: &Palette) -> MeshData {
    let land = &world.landscape;
    let mut mesh = MeshData::default();
    if land.is_flat() {
        return mesh;
    }
    let cell = 10.0;
    // [r4-tiles] Over the land bought too.
    let edge = land.land_half();
    let half = edge + 400.0;
    let n = ((half * 2.0) / cell) as i32;
    let look = climate::look(land.params.climate);
    let (shallow, deep) = look.water.unwrap_or((mix(mul(palette.water, 0.45), [0.2, 0.3, 0.28], 0.4), mul(palette.water, 0.22)));
    // [climate] Still water frozen over: lakes, and the sea out to
    // `sea_ice` from the shore. Rivers and streams keep running.
    let crack = Fbm::<Perlin>::new(land.params.seed as u32 ^ 0x1CE).set_octaves(3);
    // [horizon] Past the map the far land says where the water is (the sea
    // running on past where the map's sea polygon ended), so the sheet
    // meets the far water's rings with no gap and no straight edge.
    let far = crate::horizon::FarLand::new(land);
    let inside = |p: Vec2| p.x.abs() <= edge && p.y.abs() <= edge;
    let frozen = |p: Vec2, d: f32| -> bool {
        // [r4-edge-bridges] Past the map only the sea freezes, as on it: a
        // river running off the map stayed open to the edge and froze
        // over in a straight line past it.
        let sea = if inside(p) { land.sea_now().is_some_and(|s| s.contains(p)) } else { far.is_sea(p) };
        look.ice.is_some() && (land.lakes.iter().any(|l| l.center.distance(p) < 400.0 && nd_sim::geom::point_in_polygon(p, &l.outline)) || (d > -look.sea_ice * (0.5 + 0.9 * (crack.get([p.x as f64 * 0.004, p.y as f64 * 0.004]) as f32 * 0.5 + 0.5)) && sea))
    };
    let wet = |p: Vec2| -> Option<f32> {
        if !inside(p) {
            let d = far.water_distance(p);
            return (d < 12.0).then_some(d);
        }
        let d = land.water_distance(p);
        (d < 12.0).then_some(d)
    };
    for j in 0..n {
        for i in 0..n {
            let x0 = -half + i as f32 * cell;
            let z0 = -half + j as f32 * cell;
            let corners = [Vec2::new(x0, z0), Vec2::new(x0 + cell, z0), Vec2::new(x0 + cell, z0 + cell), Vec2::new(x0, z0 + cell)];
            let ds: Vec<Option<f32>> = corners.iter().map(|c| wet(*c)).collect();
            if ds.iter().all(|d| d.is_none()) {
                continue;
            }
            let base = mesh.positions.len() as u32;
            // Ice per corner, so its edge fades over a cell into open water
            // instead of stepping; corners on the bank follow the cell's water.
            let mut fr = [false; 4];
            if look.ice.is_some() {
                for (k, c) in corners.iter().enumerate() {
                    fr[k] = ds[k].is_some_and(|d| d < 0.0 && frozen(*c, d));
                }
                let any = fr.iter().any(|f| *f);
                for k in 0..4 {
                    if ds[k].is_none_or(|d| d >= 0.0) {
                        fr[k] = any;
                    }
                }
            }
            for (k, c) in corners.iter().enumerate() {
                let d = ds[k].unwrap_or(12.0);
                let t = ((-d) / 25.0).clamp(0.0, 1.0);
                let (rgb, a) = match look.ice.filter(|_| fr[k]) {
                    Some(ice) => {
                        let v = crack.get([c.x as f64 * 0.03, c.y as f64 * 0.03]) as f32;
                        (mul(ice, 0.85 + 0.3 * v.abs() + 0.15 * (1.0 - t)), 0.97)
                    }
                    None => (mix(shallow, deep, t), 0.86),
                };
                mesh.positions.push([c.x, WATER_LEVEL, c.y]);
                mesh.normals.push([0.0, 1.0, 0.0]);
                mesh.colors.push([rgb[0], rgb[1], rgb[2], a]);
                mesh.uvs.push([c.x, -c.y]);
            }
            mesh.indices.extend_from_slice(&[base, base + 3, base + 1, base + 1, base + 3, base + 2]);
        }
    }
    mesh
}

/// The water beyond the fine sheet of [`build_water`]: the sea and the
/// rivers running on out of sight, in coarse cells. It depends on the
/// landscape alone, so it is built once per map.
pub fn build_far_water(world: &World, palette: &Palette) -> MeshData {
    let land = &world.landscape;
    let mut mesh = MeshData::default();
    if land.is_flat() {
        return mesh;
    }
    let half = LAND_HALF + 400.0;
    let deep = climate::look(land.params.climate).water.map_or(mul(palette.water, 0.22), |w| w.1);
    let far_cell = 40.0;
    // On the same grid as the fine sheet's edge, so the two meet exactly.
    let outer = half + ((nd_sim::landscape::FAR_GROUND - half) / far_cell).ceil() * far_cell;
    let m = ((outer * 2.0) / far_cell) as i32;
    for j in 0..m {
        for i in 0..m {
            let x0 = -outer + i as f32 * far_cell;
            let z0 = -outer + j as f32 * far_cell;
            // Only outside the fine sheet.
            if x0 >= -half && x0 + far_cell <= half && z0 >= -half && z0 + far_cell <= half {
                continue;
            }
            let corners = [Vec2::new(x0, z0), Vec2::new(x0 + far_cell, z0), Vec2::new(x0 + far_cell, z0 + far_cell), Vec2::new(x0, z0 + far_cell)];
            if !corners.iter().any(|c| land.is_water(*c)) && !land.is_water(Vec2::new(x0 + far_cell * 0.5, z0 + far_cell * 0.5)) {
                continue;
            }
            let base = mesh.positions.len() as u32;
            for c in corners {
                mesh.positions.push([c.x, WATER_LEVEL, c.y]);
                mesh.normals.push([0.0, 1.0, 0.0]);
                mesh.colors.push([deep[0], deep[1], deep[2], 0.86]);
                mesh.uvs.push([c.x, -c.y]);
            }
            mesh.indices.extend_from_slice(&[base, base + 3, base + 1, base + 1, base + 3, base + 2]);
        }
    }
    mesh
}
