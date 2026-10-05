//! Simulate a small city in the terminal. Useful for tuning the economy and
//! for exercising minds without the renderer.
//!
//! cargo run --example headless -p nd_sim -- [days]

use nd_minds::RuleBasedMind;
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
    world.add_zone(block(0.0, 0.0), LandUse::Residential);
    world.add_zone(vec![Vec2::new(-120.0, -60.0), Vec2::new(-10.0, -60.0), Vec2::new(-10.0, -10.0), Vec2::new(-120.0, -10.0)], LandUse::Park);
    world.add_zone(block(240.0, -240.0), LandUse::Industrial);

    println!("lots: {}", world.lots.len());
    let mut minds = RuleBasedMind::default();
    for day in 0..days {
        let mut peak_cars = 0usize;
        for _ in 0..1440 {
            world.tick(&mut minds);
            peak_cars = peak_cars.max(world.traffic.cars.len());
        }
        let e = &world.economy;
        println!(
            "day {:>2}: pop {:>4} bld {:>3} cars {:>3} | homes {}/{} jobs {}/{} | tiers L{} M{} H{} | class L{} M{} H{} | rent {:.0}/{:.0}/{:.0} | gini {:.2} lv {:.2} | moved {} left {} priced-out {}",
            day + 1,
            world.citizens.len(),
            world.buildings.len(),
            peak_cars,
            e.homes_total - e.homes_free,
            e.homes_total,
            e.jobs_total - e.jobs_free,
            e.jobs_total,
            e.tier_counts[0],
            e.tier_counts[1],
            e.tier_counts[2],
            e.class_counts[0],
            e.class_counts[1],
            e.class_counts[2],
            e.avg_rent[0],
            e.avg_rent[1],
            e.avg_rent[2],
            e.gini,
            e.avg_land_value,
            e.displaced_total,
            e.emigrated_total,
            e.priced_out_total,
        );
        let m = &e.market;
        println!(
            "         market: businesses {} deals {} imports {:.0} exports {:.0} meals {} hungry {} closed {} | groceries {:.1} ({:.0}) produce {:.1} flour {:.1} electronics {:.1}",
            m.businesses,
            m.deals_today,
            m.imports_today,
            m.exports_today,
            m.meals_sold_today,
            m.hungry,
            m.closed_total,
            m.avg_price.get(&nd_sim::Resource::Groceries).copied().unwrap_or(0.0),
            m.stock.get(&nd_sim::Resource::Groceries).copied().unwrap_or(0.0),
            m.avg_price.get(&nd_sim::Resource::Produce).copied().unwrap_or(0.0),
            m.avg_price.get(&nd_sim::Resource::Flour).copied().unwrap_or(0.0),
            m.avg_price.get(&nd_sim::Resource::Electronics).copied().unwrap_or(0.0),
        );
    }
    if let Some((_, c)) = world.citizens.iter().next() {
        println!("\n{} ({}, income {:.0}/day, savings {:.0}):", c.name, c.activity.label(), c.income, c.savings);
        for m in &c.memories {
            println!("  - {}", m.text);
        }
    }
}
