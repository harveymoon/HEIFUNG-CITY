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
                // [r15-water] For the water shader (`render::water_look`):
                // the current (m/s, x and z) in the first UV set, the
                // distance to the shore and whether it is frozen in the
                // second.
                let f = if inside(*c) { flow_at(land, *c) } else { Vec2::ZERO };
                mesh.uvs.push([f.x, f.y]);
                mesh.aux.push([d.clamp(-60.0, 12.0), if fr[k] { 1.0 } else { 0.0 }]);
            }
            mesh.indices.extend_from_slice(&[base, base + 3, base + 1, base + 1, base + 3, base + 2]);
        }
    }
    mesh
}

/// [r15-water] The current at `p`, in metres per second: a river's or
/// stream's along its course (out to its banks and a little past, so the
/// sheet under the bank carries it too), still water elsewhere.
pub fn flow_at(land: &nd_sim::landscape::Landscape, p: Vec2) -> Vec2 {
    let mut best: Option<(f32, Vec2)> = None;
    for r in &land.rivers {
        let (seg, _, d) = r.nearest(p);
        let reach = r.width * 0.5 + 12.0;
        if d <= reach && best.is_none_or(|(bd, _)| d < bd) {
            best = Some((d, r.flow(seg) * r.current));
        }
    }
    best.map_or(Vec2::ZERO, |b| b.1)
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

#[cfg(test)]
mod water_tests {
    use super::*;
    use nd_sim::landscape::{LandscapeParams, LandscapePreset};
    use nd_sim::WorldConfig;

    fn river_world() -> World {
        World::new(WorldConfig { landscape: LandscapeParams::preset(LandscapePreset::RiverValley, 8), ..WorldConfig::default() })
    }

    /// [r15-water] No grid on the water: everything the sheet's shading
    /// reads (colour, current, distance to shore, ice) is a function of the
    /// place alone, so the cells that share a corner agree on it and
    /// nothing steps from one 10 m cell to the next; and the current runs
    /// along the rivers.
    #[test]
    fn the_water_sheet_has_no_grid_and_carries_the_current() {
        let w = river_world();
        let m = build_water(&w, &Palette::default());
        assert!(!m.indices.is_empty());
        assert_eq!(m.aux.len(), m.positions.len(), "every vertex has its shore distance");
        let mut seen: std::collections::HashMap<(i32, i32), ([f32; 4], [f32; 2], [f32; 2])> = Default::default();
        let mut moving = 0;
        for k in 0..m.positions.len() {
            let key = ((m.positions[k][0] * 10.0).round() as i32, (m.positions[k][2] * 10.0).round() as i32);
            let here = (m.colors[k], m.uvs[k], m.aux[k]);
            if let Some(prev) = seen.insert(key, here) {
                assert_eq!(prev, here, "two cells disagree at the corner {key:?}");
            }
            let f = Vec2::new(m.uvs[k][0], m.uvs[k][1]);
            assert!(f.length() < 10.0, "a current of {} m/s", f.length());
            moving += (f.length() > 0.05) as usize;
        }
        assert!(moving > 100, "the rivers carry their current to the sheet ({moving})");
        // The shore distance runs from the deep water out under the banks.
        assert!(m.aux.iter().any(|a| a[0] < -5.0) && m.aux.iter().any(|a| a[0] > 2.0));
    }

    /// [r15-water] The wet band: the low ground right by the water is drawn
    /// darker than the dry shore a little way up.
    #[test]
    fn the_ground_by_the_water_is_darker() {
        let w = river_world();
        let c = super::super::heightfield::LandColours::for_climate(w.landscape.params.climate, &Palette::default());
        let spot = |d: f32| super::super::heightfield::GroundSpot { v: 0.5, d, hot: None };
        let lum = |c: [f32; 3]| c[0] + c[1] + c[2];
        assert!(lum(c.rgb(spot(1.0), 0.3)) < 0.8 * lum(c.rgb(spot(13.0), 1.6)));
    }
}
