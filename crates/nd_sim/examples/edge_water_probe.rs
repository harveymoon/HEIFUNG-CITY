//! [r4-edge-bridges] Is the water continuous across the map's edge? For
//! each preset (and an Arctic coast), walk the edge of the map and of the
//! land tiles and compare the map's own water (inside) with the far land's
//! (just outside): where one says wet and the other dry, and how far the
//! natural ground steps.
use nd_sim::landscape::{far::FarLand, Climate, Landscape, LandscapeParams, LandscapePreset, LAND_HALF};
use nd_sim::Vec2;

fn main() {
    let presets = [
        ("river_valley", LandscapePreset::RiverValley, Climate::Temperate),
        ("islands", LandscapePreset::Islands, Climate::Tropical),
        ("coast", LandscapePreset::Coast, Climate::Temperate),
        ("delta", LandscapePreset::Delta, Climate::Temperate),
        ("plains", LandscapePreset::Plains, Climate::Temperate),
        ("arctic_coast", LandscapePreset::Coast, Climate::Arctic),
    ];
    let seeds: Vec<u64> = std::env::args().nth(1).map(|s| s.split(',').map(|x| x.parse().unwrap()).collect()).unwrap_or(vec![1, 7, 42]);
    for (name, preset, clim) in presets {
        for &seed in &seeds {
            let mut p = LandscapeParams::preset(preset, seed);
            p.climate = clim;
            let land = Landscape::new(&p);
            let far = FarLand::new(&land);
            let (mut wet_in, mut mism, mut worst_h, mut at) = (0, 0, 0.0f32, Vec2::ZERO);
            let e = LAND_HALF;
            let mut t = -e;
            while t <= e {
                for (inside, outside) in [
                    (Vec2::new(-e + 0.5, t), Vec2::new(-e - 0.5, t)),
                    (Vec2::new(e - 0.5, t), Vec2::new(e + 0.5, t)),
                    (Vec2::new(t, -e + 0.5), Vec2::new(t, -e - 0.5)),
                    (Vec2::new(t, e - 0.5), Vec2::new(t, e + 0.5)),
                ] {
                    let a = land.water_distance(inside);
                    let b = far.water_distance(outside);
                    if a < 0.0 {
                        wet_in += 1;
                    }
                    if (a < -2.0 && b > 2.0) || (a > 2.0 && b < -2.0) {
                        mism += 1;
                        at = inside;
                    }
                    let dh = (land.height(inside) - far.natural(outside)).abs();
                    if dh > worst_h {
                        worst_h = dh;
                    }
                }
                t += 5.0;
            }
            println!("{name:13} seed {seed:3}: wet edge samples {wet_in:4}, wet/dry mismatches {mism:4} (last at {at}), worst ground step {worst_h:.1} m");
        }
    }
}
