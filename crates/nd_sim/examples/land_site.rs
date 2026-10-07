//! [land] Probe: load a save and say where a billionaire would build, or
//! why nowhere will do.
//!
//! cargo run --profile fast -p nd_sim --example land_site -- saves/city.ron

fn main() {
    let path = std::env::args().nth(1).expect("a save path");
    let text = std::fs::read_to_string(&path).expect("read the save");
    let w = nd_sim::World::from_ron(&text).expect("load");
    let (pop, gini, ratio, open) = w.elite_gate();
    println!("pop {pop} gini {gini:.2} top/median {ratio:.1} gate {open}");
    match w.elite_site_why() {
        Ok(lots) => println!("site: {:?}", lots.iter().map(|l| w.lots[*l].center).collect::<Vec<_>>()),
        Err(e) => println!("{e}"),
    }
}
