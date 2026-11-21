//! [r12-hunger] Probe: load a save and run it on day by day, with where
//! the town's food goes: people, the hungry, who left and why, the grocers
//! (open, jobs and staff, stocked, input-starved, packed), the groceries
//! made / bought by households / sold to passers-by / imported, the trips
//! that found bare shelves or a shut or packed door, and the places to eat.
//!
//! cargo run --profile fast -p nd_sim --example food_week -- saves/city.ron 6
//! `TICKS=1` also prints the mean and worst tick time of each day.

use nd_sim::market::Resource;
use nd_sim::{BusinessKind, World};

const EAT: [BusinessKind; 11] = [
    BusinessKind::Bakery,
    BusinessKind::Cafe,
    BusinessKind::FishMarket,
    BusinessKind::Diner,
    BusinessKind::NoodleBar,
    BusinessKind::PizzaPlace,
    BusinessKind::FastFood,
    BusinessKind::FoodHall,
    BusinessKind::StreetFood,
    BusinessKind::FineDining,
    BusinessKind::SeafoodRestaurant,
];

#[derive(Default)]
struct Hourly {
    /// Grocer-hours while open: in all, with bare shelves, packed, and
    /// short of some input to work.
    open: u32,
    bare: u32,
    packed: u32,
    starved: u32,
}

fn sample(w: &World, h: &mut Hourly) {
    let hour = w.clock.hour() as u8;
    for b in w.buildings.values() {
        let Some(z) = &b.business else { continue };
        if z.kind != BusinessKind::Grocery || z.closed || !b.operating() || !b.opening_hours().contains(hour) {
            continue;
        }
        h.open += 1;
        if z.stock_of(Resource::Groceries) < 1.0 {
            h.bare += 1;
        }
        if b.visitors >= b.customer_capacity() {
            h.packed += 1;
        }
        if z.kind.recipe().inputs.iter().any(|(r, per)| z.stock_of(*r) < per * b.jobs as f32 * 0.25) {
            h.starved += 1;
        }
    }
}

/// The visitor count each grocer keeps against the people really inside.
fn visitors_check(w: &World) {
    use nd_sim::citizens::Activity;
    let mut inside: std::collections::HashMap<nd_sim::BuildingId, u32> = std::collections::HashMap::new();
    for c in w.citizens.values() {
        if let Activity::Visiting(b) | Activity::Sitting { park: b, .. } = c.activity {
            *inside.entry(b).or_default() += 1;
        }
    }
    let (mut n, mut packed, mut packed_real, mut counted, mut real, mut over_all) = (0, 0, 0, 0u32, 0u32, 0u32);
    for (id, b) in w.buildings.iter() {
        let r = inside.get(&id).copied().unwrap_or(0);
        if b.visitors as u32 > r {
            over_all += b.visitors as u32 - r;
        }
        let Some(z) = &b.business else { continue };
        if z.kind != BusinessKind::Grocery || z.closed || !b.operating() {
            continue;
        }
        n += 1;
        counted += b.visitors as u32;
        real += r;
        if b.visitors >= b.customer_capacity() {
            packed += 1;
            if r < b.customer_capacity() as u32 {
                packed_real += 0;
            } else {
                packed_real += 1;
            }
        }
    }
    println!("  {:02}:30 grocers {n}: visitors counted {counted}, really inside {real}; packed {packed} (really packed {packed_real}); all buildings: {over_all} visitors counted over those inside", w.clock.hour());
}

/// Where the grocers' inputs on order are, by channel: units and the mean
/// age of the order in hours (where the channel keeps it); and the inbound
/// businesses have booked that is in no channel at all (phantom).
fn pipeline(w: &World) {
    use std::collections::{BTreeMap, HashMap};
    let kind = |b: nd_sim::BuildingId| w.buildings.get(b).and_then(|x| x.business.as_ref()).map(|z| z.kind);
    let gro = |b: nd_sim::BuildingId| kind(b) == Some(BusinessKind::Grocery);
    let now = w.clock.tick;
    let mut ch: BTreeMap<&str, (f32, u32, u64)> = BTreeMap::new();
    let mut found: HashMap<(nd_sim::BuildingId, Resource), f32> = HashMap::new();
    let mut add = |k: &'static str, c: &nd_sim::Cargo, since: Option<u64>| {
        *found.entry((c.to, c.resource)).or_default() += c.amount;
        if !gro(c.to) {
            return;
        }
        let e = ch.entry(k).or_default();
        e.0 += c.amount;
        if let Some(t) = since {
            e.1 += 1;
            e.2 += now.saturating_sub(t);
        }
    };
    for q in &w.freight_queue {
        add(if q.from.is_some() { "queued lorry (local)" } else { "queued lorry (edge)" }, &q.cargo, None);
    }
    for o in &w.rail.orders {
        add("rail order", &o.cargo, Some(o.since));
    }
    for t in w.rail.trains.values() {
        for c in &t.cargo {
            add("on a train", c, None);
        }
    }
    for b in w.buildings.values() {
        for c in &b.rail_inbound {
            add("yard apron", c, None);
        }
    }
    for (_, c) in &w.ports.inbound {
        add("ship ordered", c, None);
    }
    for sh in w.ports.ships.values() {
        for c in &sh.cargo {
            add("aboard a ship", c, None);
        }
    }
    for l in &w.ports.ferry_freight {
        add("ferry", &l.cargo, Some(l.since));
    }
    for st in w.cargo_hubs.hubs.values() {
        for (k, v) in [("hub waiting", &st.waiting), ("hub trunk", &st.trunk), ("hub inbound", &st.inbound), ("hub shed", &st.ready)] {
            for l in v.iter() {
                let c = nd_sim::Cargo { from: l.buyer, to: l.buyer, resource: l.resource, amount: l.amount, price: 0.0 };
                add(k, &c, Some(l.since));
            }
        }
    }
    for a in w.airports.airports.values() {
        for l in a.cargo_waiting.iter().chain(&a.cargo_landed) {
            add("airport", &l.cargo, Some(l.since));
        }
    }
    for (_, c, _) in &w.shore.inbound {
        add("jetty", c, None);
    }
    for j in w.containers.lorries.values() {
        if let Some(c) = &j.cargo {
            add("box lorry job", c, None);
        }
    }
    for c in w.traffic.cars.values() {
        if let Some(cg) = &c.cargo {
            add("lorry on the road", cg, None);
        }
    }
    let (mut booked_g, mut phantom_g, mut booked, mut phantom) = (0.0f32, 0.0f32, 0.0f32, 0.0f32);
    let mut by_kind: BTreeMap<String, f32> = BTreeMap::new();
    let mut phantom_grocers = 0;
    for (id, b) in w.buildings.iter() {
        let Some(z) = &b.business else { continue };
        let mut ph_here = 0.0;
        for (r, amt) in &z.inbound {
            let f = found.get(&(id, *r)).copied().unwrap_or(0.0);
            let ph = (amt - f).max(0.0);
            booked += amt;
            phantom += ph;
            ph_here += ph;
            if ph > 0.5 {
                *by_kind.entry(format!("{}:{:?}", z.kind.label(), r)).or_default() += ph;
            }
            if z.kind == BusinessKind::Grocery {
                booked_g += amt;
                phantom_g += ph;
            }
        }
        if z.kind == BusinessKind::Grocery && ph_here > 1.0 {
            phantom_grocers += 1;
        }
    }
    let s: Vec<String> = ch.iter().map(|(k, (a, n, age))| if *n > 0 { format!("{k} {a:.0} (age {:.1} h)", *age as f32 / *n as f32 / 60.0) } else { format!("{k} {a:.0}") }).collect();
    println!("  grocers' orders: booked {booked_g:.0}, in no channel {phantom_g:.0} ({phantom_grocers} grocers); {}", s.join(", "));
    let mut k: Vec<_> = by_kind.into_iter().collect();
    k.sort_by(|a, b| b.1.total_cmp(&a.1));
    let k: Vec<String> = k.iter().take(8).map(|(n, v)| format!("{n} {v:.0}")).collect();
    println!("  all businesses: booked {booked:.0}, in no channel {phantom:.0}; most: {}", k.join(", "));
}

/// Every open grocer, a line each: where, jobs and staff, groceries on the
/// shelf, the day's takings, inputs, the hungry round it and the homes
/// within 600 m.
fn dump(w: &World, path: &str) {
    use std::io::Write;
    let homes: Vec<(glam::Vec2, usize)> = w.buildings.values().filter(|b| !b.residents.is_empty()).filter_map(|b| w.lots.get(b.lot).map(|l| (l.center, b.residents.len()))).collect();
    let mut f = std::fs::File::create(path).expect("dump file");
    writeln!(f, "id,name,x,y,jobs,staff,groceries,revenue,price,produce,flour,meat,dairy,inbound,hungry_near,residents_600m,visitors,cap,district").unwrap();
    for (id, b) in w.buildings.iter() {
        let Some(z) = &b.business else { continue };
        if z.kind != BusinessKind::Grocery || z.closed || !b.operating() {
            continue;
        }
        let Some(l) = w.lots.get(b.lot) else { continue };
        let p = l.center;
        let near: usize = homes.iter().filter(|(h, _)| h.distance(p) < 600.0).map(|(_, n)| n).sum();
        let dist = w.districts.at(p).map(|d| d.name.clone()).unwrap_or_default();
        writeln!(
            f,
            "{:?},{},{:.0},{:.0},{},{},{:.0},{:.0},{:.1},{:.0},{:.0},{:.0},{:.0},{:.0},{:.0},{near},{},{},{}",
            id,
            b.name.replace(',', " "),
            p.x,
            p.y,
            b.jobs,
            b.workers.len(),
            z.stock_of(Resource::Groceries),
            z.revenue_today,
            z.sell_price(Resource::Groceries, b.jobs),
            z.stock_of(Resource::Produce),
            z.stock_of(Resource::Flour),
            z.stock_of(Resource::Meat),
            z.stock_of(Resource::Dairy),
            z.inbound.values().sum::<f32>(),
            w.food_need.hungry_near(p),
            b.visitors,
            b.customer_capacity(),
            dist.replace(',', " ")
        )
        .unwrap();
    }
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let src = args.get(1).cloned().expect("save path");
    let days: u32 = args.get(2).and_then(|s| s.parse().ok()).unwrap_or(6);
    let ticks = std::env::var("TICKS").is_ok();
    let mut w = World::read_save(&src).expect("load");
    let mut minds = nd_minds::RuleBasedMind::default();
    let pop0 = w.citizens.len();
    println!("start: day {} pop {pop0} hungry {}", w.clock.day(), w.citizens.values().filter(|c| c.satiety < 0.25).count());
    pipeline(&w);
    let mut left0 = w.economy.emigrated_by;
    let mut total_left = vec![0u32; left0.len()];
    for _ in 0..days {
        let t0 = std::time::Instant::now();
        let (mut hungry_sum, mut hungry_n) = (0usize, 0usize);
        let mut hr = Hourly::default();
        let (mut worst, mut n) = (0f64, 0u32);
        loop {
            let tt = std::time::Instant::now();
            w.tick(&mut minds);
            w.drain_events();
            let dt = tt.elapsed().as_secs_f64() * 1000.0;
            worst = worst.max(dt);
            n += 1;
            let m = w.clock.minute_of_day();
            if m % 60 == 30 {
                sample(&w, &mut hr);
            }
            if m == 12 * 60 + 30 || m == 18 * 60 + 30 {
                visitors_check(&w);
                pipeline(&w);
            }
            if m % 60 == 0 {
                hungry_sum += w.citizens.values().filter(|c| c.satiety < 0.25).count();
                hungry_n += 1;
            }
            if m == 1439 {
                break;
            }
        }
        let e = &w.economy;
        let left: Vec<u32> = (0..left0.len()).map(|i| e.emigrated_by[i] - left0[i]).collect();
        for (t, l) in total_left.iter_mut().zip(&left) {
            *t += l;
        }
        left0 = e.emigrated_by;
        let (mut gro, mut jobs, mut staff, mut stocked, mut stock) = (0, 0u32, 0usize, 0, 0.0f32);
        let mut inputs = [0.0f32; 4];
        let mut targets = [0.0f32; 4];
        let mut inbound = [0.0f32; 4];
        // The food chain behind the grocers: count and stock of output.
        let chain = [BusinessKind::VertFarm, BusinessKind::GrainFarm, BusinessKind::Mill, BusinessKind::Dairy, BusinessKind::ProteinVat, BusinessKind::FishPlant, BusinessKind::Slaughterhouse];
        let mut chain_n = [0u32; 7];
        let mut chain_stock = [0.0f32; 7];
        let mut stocks: Vec<f32> = Vec::new();
        let mut eat = [0u32; EAT.len()];
        let mut building = 0;
        let mut derelict = 0;
        for (id, b) in w.buildings.iter() {
            let Some(z) = &b.business else { continue };
            if z.closed {
                continue;
            }
            if b.construction.is_some() {
                if z.kind == BusinessKind::Grocery {
                    building += 1;
                }
                continue;
            }
            if !b.operating() {
                continue;
            }
            if let Some(i) = EAT.iter().position(|k| *k == z.kind) {
                eat[i] += 1;
            }
            if let Some(i) = chain.iter().position(|k| *k == z.kind) {
                chain_n[i] += 1;
                chain_stock[i] += z.kind.recipe().outputs.first().map_or(0.0, |(r, _)| z.stock_of(*r));
            }
            if z.kind != BusinessKind::Grocery {
                continue;
            }
            gro += 1;
            if b.is_derelict() {
                derelict += 1;
                if std::env::var("WHY").is_ok() {
                    let shelves: f32 = z.stock_of(Resource::Groceries) * 3.0;
                    println!("    derelict grocer {}: tenant {} workers {} money {:.0} shelves {:.0} condition {:.2} host {:?}", b.name, b.is_tenant(), b.workers.len(), z.money, shelves, b.condition, w.host_of(id));
                }
            }
            jobs += b.jobs as u32;
            staff += b.workers.len();
            let s = z.stock_of(Resource::Groceries);
            stocks.push(s);
            stock += s;
            if s >= 1.0 {
                stocked += 1;
            }
            for (i, (r, _)) in z.kind.recipe().inputs.iter().enumerate().take(4) {
                inputs[i] += z.stock_of(*r);
                targets[i] += z.target_input(*r, b.jobs);
                inbound[i] += z.inbound_of(*r);
            }
        }
        let f = e.market.food.today;
        let eat_s: Vec<String> = EAT.iter().zip(eat).filter(|(_, n)| *n > 0).map(|(k, n)| format!("{} {n}", k.label())).collect();
        println!(
            "day {}: pop {} | left {:?} | hungry avg {} now {} | grocers {gro} (+{building} building) jobs {jobs} staff {staff} stocked {stocked} stock {stock:.0} | inputs % of target {:.0}/{:.0}/{:.0}/{:.0} | grocer-hours open {} bare {} packed {} starved {} | groceries made {:.0} bought {:.0} walk-in {:.0} imported {:.0} | trips bare {} shut {} packed {} | meals sold {} | demand com {:.2} | {:.0}s{}",
            w.clock.day(),
            w.citizens.len(),
            left,
            hungry_sum / hungry_n.max(1),
            w.citizens.values().filter(|c| c.satiety < 0.25).count(),
            inputs[0] / targets[0].max(1.0) * 100.0,
            inputs[1] / targets[1].max(1.0) * 100.0,
            inputs[2] / targets[2].max(1.0) * 100.0,
            inputs[3] / targets[3].max(1.0) * 100.0,
            hr.open,
            hr.bare,
            hr.packed,
            hr.starved,
            f.made,
            f.bought,
            f.walk_in,
            f.imported,
            f.empty_shelves,
            f.shut,
            f.packed,
            e.market.meals_sold_today,
            e.demand_for(nd_sim::LandUse::Commercial),
            t0.elapsed().as_secs_f32(),
            if ticks { format!(" | tick mean {:.1} ms worst {worst:.0} ms", t0.elapsed().as_secs_f64() * 1000.0 / n as f64) } else { String::new() }
        );
        println!("  eat: {}", eat_s.join(", "));
        let n = &w.food_need;
        let hub_in: usize = w.cargo_hubs.hubs.values().map(|h| h.inbound.len()).sum();
        println!("  food need: hungry counted {} ({:.1}%), cells {}, food businesses opened for hunger {} | written off today {:.0} | hub loads expected {hub_in} | grocers in derelict buildings {derelict}, shops repaired at midnight {}", n.hungry, n.share * 100.0, n.cells.len(), n.opened_total, f.written_off, e.market.food.yesterday.shops_repaired);
        stocks.sort_by(|a, b| b.total_cmp(a));
        let top: f32 = stocks.iter().take(stocks.len() / 10).sum();
        println!("  grocer stock: top 10% hold {:.0} of {stock:.0}; inbound % of target {:.0}/{:.0}/{:.0}/{:.0}", top, inbound[0] / targets[0].max(1.0) * 100.0, inbound[1] / targets[1].max(1.0) * 100.0, inbound[2] / targets[2].max(1.0) * 100.0, inbound[3] / targets[3].max(1.0) * 100.0);
        let ch: Vec<String> = chain.iter().enumerate().map(|(i, k)| format!("{} {} ({:.0})", k.label(), chain_n[i], chain_stock[i])).collect();
        println!("  chain: {}", ch.join(", "));
        if let Ok(path) = std::env::var("DUMP") {
            dump(&w, &format!("{path}_day{}.csv", w.clock.day()));
        }
        // Over midnight into the next day.
        w.tick(&mut minds);
        w.drain_events();
    }
    println!("end: pop {pop0} -> {} ({:+}), left by reason {:?}", w.citizens.len(), w.citizens.len() as i64 - pop0 as i64, total_left);
}
