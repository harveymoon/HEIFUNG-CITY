//! [r8-parks] The demo district's park as it goes up: its sites, their
//! progress and blocks, every few hours. Not part of the game.
use nd_minds::RuleBasedMind;
use nd_sim::{LandUse, World, WorldConfig};

fn main() {
    let mut w = World::new(WorldConfig::default());
    w.drain_events();
    nd_sim::scenarios::demo_district(&mut w);
    let mut m = RuleBasedMind::default();
    for t in 0..1440 * 4 {
        w.tick(&mut m);
        if t % 240 == 0 {
            let parks: Vec<String> = w
                .buildings
                .values()
                .filter(|b| b.land_use == LandUse::Park)
                .map(|b| match &b.construction {
                    Some(c) => format!("{}{:.2}{}d{}", if c.follows_park { "f" } else { "L" }, c.progress, c.blocked.map_or("", |x| x.key()), c.deliveries),
                    None => "open".to_string(),
                })
                .collect();
            let vacant = w.lots.values().filter(|l| l.land_use == LandUse::Park && l.building.is_none()).count();
            println!("day {} {:02}:{:02} vacant {vacant} parks {:?}", w.clock.day(), w.clock.hour(), w.clock.minute(), parks);
        }
    }
}
