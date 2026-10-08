//! [social] What people's lives together look like in a saved city: run it
//! for some days and print the day's conversations, friendships, feuds,
//! groups and what they made.
//!
//! cargo run --profile fast -p nd_sim --example social_probe -- city.ron 5

use nd_minds::RuleBasedMind;
use nd_sim::social::Bond;
use nd_sim::World;
use std::time::Instant;

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let path = args.get(1).expect("save path");
    let days: u64 = args.get(2).and_then(|s| s.parse().ok()).unwrap_or(5);
    let mut w = World::from_ron(&std::fs::read_to_string(path).expect("read")).expect("load");
    let mut minds = RuleBasedMind::default();
    println!("pop {} day {}", w.citizens.len(), w.clock.day());
    let t0 = Instant::now();
    for _ in 0..days {
        for _ in 0..1440 {
            w.tick(&mut minds);
        }
        let s = &w.social;
        let d = s.yesterday().cloned().unwrap_or_default();
        let bonds = s.bond_counts();
        let n = |b: Bond| bonds.get(&b).copied().unwrap_or(0) / 2;
        println!(
            "day {:>3}: talks {:>5} (good {} bad {}) | new friends {} close {} couples {} feuds {} fights {} | meetups {} visits {} favours {} loans {} jobs {} flats {} moved in {} avoided {} | groups +{} -{} split {} works {} | standing: friends {} close {} partners {} rivals {} enemies {} groups {} ({:.0} s)",
            d.day, d.talks, d.good_talks, d.bad_talks, d.friendships, d.close_friendships, d.couples, d.feuds, d.fights, d.meetups, d.home_visits, d.favours, d.loans, d.jobs_found, d.homes_found, d.moved_in, d.avoided,
            d.groups_formed, d.groups_folded, d.splits, d.works,
            n(Bond::Friend), n(Bond::CloseFriend), n(Bond::Partner), n(Bond::Rival), n(Bond::Enemy), s.active_groups().count(), t0.elapsed().as_secs_f64()
        );
    }
    println!("-- groups");
    for g in w.social.active_groups() {
        println!("{:>3} {:<34} {:<26} members {:>2} momentum {:.2} rep {:+.2} works {:?}", g.id, g.name, g.kind.label(), g.members.len(), g.momentum, g.reputation, g.works.iter().map(|x| x.key()).collect::<Vec<_>>());
    }
    println!("-- news");
    for n in w.social.news.iter().rev().take(25) {
        println!("d{} {}", n.tick / 1440 + 1, n.text);
    }
    println!("murals {} gardens {} petitions {}", w.social.murals.len(), w.social.gardens.len(), w.social.petitions.len());
    let out = format!("{path}.social.ron");
    std::fs::write(&out, w.to_ron().unwrap()).unwrap();
    println!("saved {out}");
}
