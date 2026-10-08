//! [plant-scale] Probe: how big the free-placement site for a plant gets on
//! the unzoned ground of the example cities.
//!
//! cargo run --profile fast -p nd_procgen --example plant_site_probe -- [coal|nuclear|solar]

use nd_procgen::citygen::{self, CityPreset};
use nd_sim::{Placeable, PowerPlantKind};

fn main() {
    let kind = match std::env::args().nth(1).as_deref() {
        Some("nuclear") => PowerPlantKind::Nuclear,
        Some("solar") => PowerPlantKind::Solar,
        _ => PowerPlantKind::Coal,
    };
    let what = Placeable::Plant(kind);
    for preset in CityPreset::ALL {
        let (w, _, _) = citygen::generate_world(preset, 1);
        let mut parcels: Vec<f32> = Vec::new();
        let mut sites: Vec<(f32, usize)> = Vec::new();
        for (i, p) in w.parcels.parcels.iter().enumerate() {
            if p.lot.is_some() || p.land_use.is_some() || p.rough {
                continue;
            }
            parcels.push(p.area);
            if let Ok(s) = w.placement_site(what, p.center) {
                if s.lot.is_none() && s.parcels.contains(&i) {
                    sites.push((nd_sim::zoning::polygon_area(&s.outline).abs(), s.parcels.len()));
                }
            }
        }
        parcels.sort_by(|a, b| a.total_cmp(b));
        sites.sort_by(|a, b| a.0.total_cmp(&b.0));
        let pct = |v: &[f32], q: f32| v.get(((v.len() as f32 - 1.0) * q) as usize).copied().unwrap_or(0.0);
        let sa: Vec<f32> = sites.iter().map(|s| s.0).collect();
        let lots: Vec<f32> = { let mut v: Vec<f32> = w.lots.values().map(|l| l.area()).collect(); v.sort_by(|a, b| a.total_cmp(b)); v };
        println!("{preset:?}: {} unzoned parcels, area p50 {:.0} p90 {:.0} max {:.0}; {} sites p50 {:.0} p90 {:.0} max {:.0} (max parcels {}); lots p50 {:.0} p99 {:.0} max {:.0}",
            parcels.len(), pct(&parcels, 0.5), pct(&parcels, 0.9), parcels.last().copied().unwrap_or(0.0),
            sites.len(), pct(&sa, 0.5), pct(&sa, 0.9), sa.last().copied().unwrap_or(0.0), sites.iter().map(|s| s.1).max().unwrap_or(0),
            pct(&lots, 0.5), pct(&lots, 0.99), lots.last().copied().unwrap_or(0.0));
    }
}
