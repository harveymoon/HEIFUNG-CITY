use crate::mesh::MeshData;
use crate::palette::Palette;
use noise::{Fbm, MultiFractal, NoiseFn, Perlin};

#[derive(Clone, Debug)]
pub struct TerrainConfig {
    pub seed: u32,
    /// Half-size of the terrain mesh in metres.
    pub half_size: f32,
    pub cell: f32,
    /// Radius of the flat basin the city sits in.
    pub basin_radius: f32,
    pub hill_height: f32,
}

impl Default for TerrainConfig {
    fn default() -> Self {
        Self { seed: 7, half_size: 1800.0, cell: 12.0, basin_radius: 1000.0, hill_height: 160.0 }
    }
}

pub fn terrain_height(cfg: &TerrainConfig, noise: &Fbm<Perlin>, x: f32, z: f32) -> f32 {
    let r = (x * x + z * z).sqrt();
    if r <= cfg.basin_radius {
        return 0.0;
    }
    let t = ((r - cfg.basin_radius) / (cfg.half_size - cfg.basin_radius)).clamp(0.0, 1.0);
    let ramp = t * t * (3.0 - 2.0 * t);
    let n = noise.get([x as f64 * 0.0025, z as f64 * 0.0025]) as f32 * 0.5 + 0.5;
    ramp * cfg.hill_height * (0.4 + 0.6 * n)
}

/// Height-field mesh with colours from the palette.
pub fn build_terrain(cfg: &TerrainConfig, palette: &Palette) -> MeshData {
    let noise = Fbm::<Perlin>::new(cfg.seed).set_octaves(4);
    let detail = Fbm::<Perlin>::new(cfg.seed.wrapping_add(99)).set_octaves(3);
    let n = ((cfg.half_size * 2.0) / cfg.cell).ceil() as usize + 1;
    let mut mesh = MeshData::default();
    let mut heights = vec![0.0f32; n * n];

    for j in 0..n {
        for i in 0..n {
            let x = -cfg.half_size + i as f32 * cfg.cell;
            let z = -cfg.half_size + j as f32 * cfg.cell;
            let h = terrain_height(cfg, &noise, x, z);
            heights[j * n + i] = h;
            let v = detail.get([x as f64 * 0.02, z as f64 * 0.02]) as f32 * 0.5 + 0.5;
            let hill_t = (h / cfg.hill_height).clamp(0.0, 1.0);
            let mut c = [0.0f32; 4];
            for k in 0..3 {
                let flat = palette.ground[k] + palette.ground_variation[k] * v;
                c[k] = flat * (1.0 - hill_t) + palette.hills[k] * hill_t;
            }
            c[3] = 1.0;
            mesh.positions.push([x, h, z]);
            mesh.normals.push([0.0, 1.0, 0.0]);
            mesh.colors.push(c);
            mesh.uvs.push([x, -z]);
        }
    }

    // Normals from finite differences.
    for j in 0..n {
        for i in 0..n {
            let h = |ii: isize, jj: isize| {
                let ii = ii.clamp(0, n as isize - 1) as usize;
                let jj = jj.clamp(0, n as isize - 1) as usize;
                heights[jj * n + ii]
            };
            let dx = h(i as isize + 1, j as isize) - h(i as isize - 1, j as isize);
            let dz = h(i as isize, j as isize + 1) - h(i as isize, j as isize - 1);
            let nrm = glam::Vec3::new(-dx, 2.0 * cfg.cell, -dz).normalize();
            mesh.normals[j * n + i] = nrm.to_array();
        }
    }

    for j in 0..n - 1 {
        for i in 0..n - 1 {
            let a = (j * n + i) as u32;
            let b = a + 1;
            let c = a + n as u32;
            let d = c + 1;
            mesh.indices.extend_from_slice(&[a, c, b, b, c, d]);
        }
    }
    mesh
}
