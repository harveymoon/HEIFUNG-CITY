//! Simulate a small city in the terminal. Useful for tuning the economy and
//! for exercising minds without the renderer.
//!
//! cargo run --example headless -p nd_sim -- [days]

use nd_sim::mind::IdleMind;
use nd_sim::{LandUse, Vec2, World, WorldConfig};

fn main() {
    let days: u64 = std::env::args().nth(1).and_then(|s| s.parse().ok()).unwrap_or(3);
    let mut world = World::new(WorldConfig::default());

    for i in -2..=2 {
        let c = i as f32 * 120.0;
        world.add_road(&[Vec2::new(-700.0, c), Vec2::new(700.0, c)], 1, 1);
        world.add_road(&[Vec2::new(c, -700.0), Vec2::new(c, 700.0)], 1, 1);
    }
    let block = |x0: f32, y0: f32| vec![Vec2::new(x0, y0), Vec2::new(x0 + 240.0, y0), Vec2::new(x0 + 240.0, y0 + 240.0), Vec2::new(x0, y0 + 240.0)];
    world.add_zone(block(-240.0, -240.0), LandUse::Residential);
    world.add_zone(block(0.0, -240.0), LandUse::Commercial);
    world.add_zone(block(-240.0, 0.0), LandUse::Industrial);
    world.add_zone(block(0.0, 0.0), LandUse::Entertainment);

    println!("lots: {}", world.lots.len());
    let mut minds = IdleMind;
    for day in 0..days {
        let mut peak_cars = 0usize;
        for _ in 0..1440 {
            world.tick(&mut minds);
            peak_cars = peak_cars.max(world.traffic.cars.len());
        }
        let e = &world.economy;
        println!(
            "day {:>2}: pop {:>4}  buildings {:>3}  peak cars {:>3}  homes {}/{}  jobs {}/{}  demand R{:.2} C{:.2} I{:.2} E{:.2}",
            day + 1,
            world.citizens.len(),
            world.buildings.len(),
            peak_cars,
            e.homes_total - e.homes_free,
            e.homes_total,
            e.jobs_total - e.jobs_free,
            e.jobs_total,
            e.demand_for(LandUse::Residential),
            e.demand_for(LandUse::Commercial),
            e.demand_for(LandUse::Industrial),
            e.demand_for(LandUse::Entertainment),
        );
    }
    if let Some((_, c)) = world.citizens.iter().next() {
        println!("\n{} ({}):", c.name, c.activity.label());
        for m in &c.memories {
            println!("  - {}", m.text);
        }
    }
}
