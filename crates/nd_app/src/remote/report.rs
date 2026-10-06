//! Whole-city review for the remote API: `report` (overview, people,
//! economy, land, services, alerts) and the ranked alert list that
//! `status` also quotes.
//!
//! Why a separate view from `status`: `status` is the live dashboard of the
//! app (clock, camera, tool), small enough to poll; `report` answers "how is
//! this city doing, where, and why" in one call, so it aggregates over
//! every citizen and building, with distributions instead of single
//! averages and alerts that carry a place and example ids to follow up with
//! `inspect`. Everything is read-only and computed on demand from `World`:
//! a few linear passes, well under a millisecond per thousand citizens.

use std::collections::BTreeMap;

use nd_sim::citizens::TravelMode;
use nd_sim::economy::{Flow, OVERDRAFT};
use nd_sim::market::JOB_RATE;
use nd_sim::rail::RailFacility;
use nd_sim::transit::TransitLeg;
use nd_sim::{Activity, Building, BuildingId, BusinessKind, LandUse, Resource, Tier, TrainKind, Vec2 as SVec2, World};
use serde_json::{json, Map, Value};

use super::view::{building_name, key_str, plant_name, rail_name, round, tier_key, v2};

// â”€â”€ thresholds â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€
//
// Named so a reader of the docs and of a reply can tell what "hungry" or
// "stuck" means; they mirror the sim's own cut-offs where it has one.

/// Satiety below which a citizen counts as hungry (the sim's market stats
/// use the same cut).
pub const HUNGRY_SATIETY: f32 = 0.3;
/// Days without a proper meal that make an alert urgent: at four the sim
/// sends the citizen away.
pub const STARVING_DAYS: u8 = 2;
/// A food seller below this staffing share is flagged.
pub const FOOD_SELLER_MIN_STAFFING: f32 = 0.5;
/// Units of its retail output under which a food seller counts as empty.
pub const FOOD_STOCK_EMPTY: f32 = 1.0;
/// A building with less than this share of its power counts as unpowered
/// (the sim's own threshold for `unpowered_buildings`).
pub const UNPOWERED_BELOW: f32 = 0.6;
/// Rent above this share of income is a burden; above the second it is
/// stress, the sim's trigger for moving out.
pub const RENT_BURDEN_SHARE: f32 = 0.3;
pub const RENT_STRESS_SHARE: f32 = 0.5;
/// A vehicle slower than this (m/s) counts as stopped.
pub const STUCK_SPEED: f32 = 0.5;
/// A vehicle in traffic that has stood still for this many seconds of sim
/// motion (`Car::stalled`) is stuck. A bus dwelling at a stop is not
/// counted (its clock stays at zero) and neither is the head of a queue at
/// a red light (no light is red this long); a car waiting at the kerb for
/// a gap is reported apart. The sim reroutes such vehicles after 40 s and
/// gives up on trips at 1800 s.
pub const STUCK_STALL_S: f32 = 180.0;
/// Demand (0..1) at which vacant lots of that land use are worth flagging.
pub const HIGH_DEMAND: f32 = 0.6;
/// Unemployment share that makes an alert.
pub const UNEMPLOYMENT_ALERT: f32 = 0.15;
/// A resource whose average price is this many times its base is scarce.
pub const SCARCE_PRICE_RATIO: f32 = 1.3;
/// Memories newer than this many ticks count as "recent".
pub const MEMORY_WINDOW_TICKS: u64 = 2 * 1440;
/// Example ids carried by each alert.
const ALERT_EXAMPLES: usize = 5;

pub const SECTIONS: &[&str] = &["overview", "people", "economy", "land", "services", "industry", "alerts"];

const KINDS: [BusinessKind; 19] = BusinessKind::ALL;

pub fn lu_key(lu: LandUse) -> String {
    lu.label().to_ascii_lowercase()
}

pub fn building_pos(w: &World, b: &Building) -> Option<SVec2> {
    w.lots.get(b.lot).map(|l| l.center)
}

/// Rounded to 3 decimals, for shares.
pub fn share(n: usize, d: usize) -> f64 {
    if d == 0 {
        0.0
    } else {
        ((n as f64 / d as f64) * 1000.0).round() / 1000.0
    }
}

fn r3(x: f32) -> f64 {
    (x as f64 * 1000.0).round() / 1000.0
}

/// Count, mean, median, p10/p90 and a histogram with `bins` equal bins
/// over `range` (default: the data's min..max), edges included.
pub fn distribution(mut v: Vec<f32>, bins: usize, range: Option<(f32, f32)>) -> Value {
    v.retain(|x| x.is_finite());
    if v.is_empty() {
        return json!({ "count": 0 });
    }
    v.sort_by(|a, b| a.total_cmp(b));
    let n = v.len();
    let pct = |p: f32| v[((n - 1) as f32 * p).round() as usize];
    let mean = v.iter().map(|&x| x as f64).sum::<f64>() / n as f64;
    let (lo, hi) = range.unwrap_or((v[0], v[n - 1]));
    let width = ((hi - lo) / bins as f32).max(1e-6);
    let mut counts = vec![0u32; bins];
    for &x in &v {
        let i = (((x - lo) / width).floor().max(0.0) as usize).min(bins - 1);
        counts[i] += 1;
    }
    let edges: Vec<f64> = (0..=bins).map(|i| round(lo + width * i as f32)).collect();
    json!({
        "count": n, "mean": (mean * 100.0).round() / 100.0, "median": round(pct(0.5)), "p10": round(pct(0.1)), "p90": round(pct(0.9)),
        "min": round(v[0]), "max": round(v[n - 1]), "histogram": { "edges": edges, "counts": counts },
    })
}

/// The mean point and the mean distance from it, for "where" fields.
fn centroid(points: &[SVec2]) -> (Value, Value) {
    if points.is_empty() {
        return (Value::Null, Value::Null);
    }
    let c = points.iter().fold(SVec2::ZERO, |a, &p| a + p) / points.len() as f32;
    let spread = points.iter().map(|p| p.distance(c)).sum::<f32>() / points.len() as f32;
    (v2(c), json!(spread.round()))
}

// â”€â”€ report â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€

pub fn report(w: &World, sections: &[String]) -> Result<Value, String> {
    for s in sections {
        if !SECTIONS.contains(&s.as_str()) {
            return Err(format!("unknown report section {s:?}; one of {}", SECTIONS.join(", ")));
        }
    }
    let want = |s: &str| sections.is_empty() || sections.iter().any(|x| x == s);
    let alerts = alerts(w);
    let mut out = Map::new();
    if want("overview") {
        out.insert("overview".into(), overview(w, &alerts));
    }
    if want("people") {
        out.insert("people".into(), people(w));
    }
    if want("economy") {
        out.insert("economy".into(), economy(w));
    }
    if want("land") {
        out.insert("land".into(), land(w));
    }
    if want("services") {
        out.insert("services".into(), services(w));
    }
    if want("industry") {
        out.insert("industry".into(), industry(w));
    }
    if want("alerts") {
        out.insert("alerts".into(), alerts_json(&alerts, usize::MAX));
    }
    Ok(Value::Object(out))
}

fn ledger_map(values: &[f32]) -> Value {
    let mut m = Map::new();
    for f in Flow::ALL {
        let x = values[f.index()];
        if x != 0.0 {
            m.insert(f.label().replace(' ', "_"), json!(round(x)));
        }
    }
    Value::Object(m)
}

fn demand(w: &World) -> Value {
    LandUse::ALL.iter().map(|&lu| (lu_key(lu), json!(round(w.economy.demand_for(lu))))).collect::<Map<_, _>>().into()
}

fn overview(w: &World, alerts: &[Alert]) -> Value {
    let e = &w.economy;
    let (mut built, mut sites, mut derelict, mut demolishing) = (0, 0, 0, 0);
    for b in w.buildings.values() {
        if b.demolition.is_some() {
            demolishing += 1;
        }
        if b.being_demolished() {
            // Counted above only.
        } else if b.construction.is_some() {
            sites += 1;
        } else {
            built += 1;
        }
        if b.is_derelict() {
            derelict += 1;
        }
    }
    let vacant = w.lots.values().filter(|l| l.building.is_none() && l.usable()).count();
    let unusable = w.lots.values().filter(|l| !l.usable()).count();
    let housed = w.households.values().filter(|h| h.home.is_some()).count();
    json!({
        "day": w.clock.day() + 1, "time": w.clock.format_hhmm(),
        "population": w.citizens.len(), "households": w.households.len(), "households_housed": housed,
        "homes": { "total": e.homes_total, "free": e.homes_free, "being_built": e.homes_building },
        "jobs": { "total": e.jobs_total, "filled": e.jobs_total - e.jobs_free.min(e.jobs_total), "free": e.jobs_free, "being_built": e.jobs_building },
        "buildings": { "built": built, "under_construction": sites, "being_demolished": demolishing, "derelict": derelict, "vacant_lots": vacant, "unusable_lots": unusable, "lots_total": w.lots.len() },
        "treasury": round(e.treasury),
        "net_yesterday": round(e.ledger.yesterday.iter().sum()), "net_today_so_far": round(e.ledger.today.iter().sum()),
        "land_value_avg": round(e.avg_land_value), "gini": round(e.gini),
        "demand_by_land_use": demand(w),
        "headline_alerts": alerts.iter().take(5).map(|a| a.message.clone()).collect::<Vec<_>>(),
    })
}

/// Group a memory under a stable reason: the sim's own templates are
/// matched by their fixed part (names cut out); anything else (a mind's
/// free text) by its first few words.
fn memory_reason(text: &str) -> String {
    const KNOWN: &[&str] = &[
        "Rent got too steep", "Couldn't keep myself fed", "Couldn't afford anything at", "Shelves were empty at", "Grabbed a bite at",
        "Treated myself to something new at", "Circled for ages", "Sat in the dark at", "Bought a bike", "Bought a car", "Sold the car",
        "Rolled into Neon District", "Stepped off the train", "Moved into", "Got hired at", "Sat on a bench in", "Spent the evening at",
        "Moving up to", "Paid ", "was demolished", "was torn down and rebuilt",
    ];
    for k in KNOWN {
        if text.starts_with(k) || (k.starts_with(char::is_lowercase) && text.contains(k)) {
            return k.trim().to_string();
        }
    }
    let words: Vec<&str> = text.split_whitespace().take(4).collect();
    format!("{}â€¦", words.join(" "))
}

/// Households: how many, their sizes, life stages, shared money and rent.
fn households(w: &World) -> Value {
    let e = &w.economy;
    let mut stages = [0u32; 3];
    for c in w.citizens.values() {
        stages[c.stage() as usize] += 1;
    }
    let money: Vec<f32> = w.households.values().map(|h| h.money).collect();
    let s = e.household_sizes;
    json!({
        "count": w.households.len(),
        "sizes": { "1": s[1], "2": s[2], "3": s[3], "4": s[4], "5+": s[5] },
        "mean_size": round(w.citizens.len() as f32 / w.households.len().max(1) as f32),
        "life_stages": { "children": stages[0], "adults": stages[1], "retirees": stages[2] },
        "rent_stressed": e.rent_stressed_households, "rent_stressed_share": share(e.rent_stressed_households as usize, w.households.len()),
        "housing_demand_by_tier": { "low": round(e.housing_demand_by_tier[0]), "middle": round(e.housing_demand_by_tier[1]), "high": round(e.housing_demand_by_tier[2]) },
        "money": distribution(money, 16, None),
    })
}

fn people(w: &World) -> Value {
    let tick = w.clock.tick;
    let n = w.citizens.len();
    let mut by_class = [0u32; 3];
    let mut by_activity: BTreeMap<&str, u32> = BTreeMap::new();
    let (mut employed, mut on_sites, mut in_business) = (0, 0, 0);
    let mut housed = 0;
    let mut incomes = Vec::with_capacity(n);
    let mut savings = Vec::with_capacity(n);
    let mut moods = Vec::with_capacity(n);
    let mut rent_shares = Vec::new();
    let (mut overdraft, mut at_floor) = (0, 0);
    let (mut burden30, mut burden50, mut stressed) = (0, 0, 0);
    let (mut hungry, mut no_groceries) = (0, 0);
    let mut hungry_days = [0u32; 5];
    let mut satiety_sum = 0.0f64;
    let mut modes: BTreeMap<&str, u32> = BTreeMap::new();
    let (mut owns_car, mut owns_bike) = (0, 0);
    let mut reasons: BTreeMap<String, u32> = BTreeMap::new();
    for (cid, c) in w.citizens.iter() {
        by_class[c.class().index()] += 1;
        *by_activity.entry(c.activity.label()).or_default() += 1;
        if let Some(job) = c.work.and_then(|b| w.buildings.get(b)) {
            employed += 1;
            if job.construction.is_some() {
                on_sites += 1;
            } else if job.business.is_some() {
                in_business += 1;
            }
        }
        incomes.push(c.income);
        savings.push(c.savings);
        moods.push(c.mood);
        if c.savings < 0.0 {
            overdraft += 1;
        }
        if c.savings <= OVERDRAFT + 1.0 {
            at_floor += 1;
        }
        if let Some(home) = c.home.and_then(|h| w.buildings.get(h)) {
            housed += 1;
            // Rent is paid by the household, from everyone's income.
            let s = w.rent_share_of(cid).unwrap_or(if c.income > 0.0 { home.rent / c.income } else { f32::INFINITY });
            if s > RENT_BURDEN_SHARE {
                burden30 += 1;
            }
            if s > RENT_STRESS_SHARE {
                burden50 += 1;
            }
            rent_shares.push(s.min(5.0));
        }
        if c.rent_stress_days > 0 {
            stressed += 1;
        }
        if c.satiety < HUNGRY_SATIETY {
            hungry += 1;
        }
        if c.groceries <= 0.01 {
            no_groceries += 1;
        }
        satiety_sum += c.satiety as f64;
        hungry_days[(c.hungry_days as usize).min(4)] += 1;
        let mode = match &c.activity {
            Activity::Driving { .. } => Some("car"),
            Activity::Riding { .. } => Some("bus"),
            Activity::OnTrain { .. } => Some("train"),
            Activity::Waiting { .. } => Some(match c.transit.map(|t| t.leg) {
                Some(TransitLeg::Rail { .. }) => "train",
                Some(TransitLeg::Air { .. }) => "air",
                _ => "bus",
            }),
            Activity::Flying { .. } => Some("air"),
            Activity::Traveling { .. } => Some(if c.mode == TravelMode::Bike { "bike" } else { "walk" }),
            _ => None,
        };
        if let Some(m) = mode {
            *modes.entry(m).or_default() += 1;
        }
        owns_car += c.owns_car as u32;
        owns_bike += c.owns_bike as u32;
        for m in c.memories.iter().filter(|m| m.tick + MEMORY_WINDOW_TICKS >= tick) {
            *reasons.entry(memory_reason(&m.text)).or_default() += 1;
        }
    }
    let travelling: u32 = modes.values().sum();
    let mode_json: Map<String, Value> = ["walk", "bike", "car", "bus", "train"]
        .iter()
        .map(|m| {
            let k = modes.get(m).copied().unwrap_or(0);
            (m.to_string(), json!({ "count": k, "share": share(k as usize, travelling as usize) }))
        })
        .collect();
    let mut reasons: Vec<(String, u32)> = reasons.into_iter().collect();
    reasons.sort_by(|a, b| b.1.cmp(&a.1));
    let e = &w.economy;
    json!({
        "population": n,
        "by_class": { "low": by_class[0], "middle": by_class[1], "high": by_class[2] },
        "by_activity": by_activity,
        "employment": {
            "employed": employed, "employed_in_businesses": in_business, "employed_on_building_sites": on_sites,
            "unemployed_on_gig_income": n - employed, "unemployment_share": share(n - employed, n), "jobs_free": e.jobs_free,
            // Children and retirees do not look for work; these count adults of working age.
            "working_age_adults": e.working_age, "unemployed_adults": e.unemployed,
            "adult_unemployment_share": share(e.unemployed as usize, e.working_age as usize),
        },
        "housing": { "housed": housed, "unhoused": n - housed, "waiting_immigrants": e.waiting_immigrants, "homes_free": e.homes_free },
        "households": households(w),
        "income_per_day": distribution(incomes, 16, None),
        "savings": { "in_overdraft": overdraft, "at_overdraft_floor": at_floor, "overdraft_floor": round(OVERDRAFT), "distribution": distribution(savings, 16, None) },
        "rent_burden": {
            "housed": housed, "over_30pct_of_income": burden30, "over_50pct_of_income": burden50,
            "share_over_30pct": share(burden30, housed), "share_over_50pct": share(burden50, housed), "rent_stressed_now": stressed,
            "rent_to_income": distribution(rent_shares, 10, Some((0.0, 1.0))),
        },
        "mood": distribution(moods, 10, Some((0.0, 1.0))),
        "hunger": {
            "hungry_now": hungry, "hungry_threshold_satiety": round(HUNGRY_SATIETY), "mean_satiety": r3((satiety_sum / n.max(1) as f64) as f32),
            "no_groceries_at_home": no_groceries,
            "hungry_days": { "0": hungry_days[0], "1": hungry_days[1], "2": hungry_days[2], "3": hungry_days[3], "4+": hungry_days[4] },
            "note": "a citizen leaves after 4 hungry days",
        },
        "commute_modes_now": { "travelling": travelling, "modes": mode_json, "owns_car_share": share(owns_car as usize, n), "owns_bike_share": share(owns_bike as usize, n) },
        "moves": { "displaced_by_rent_total": e.displaced_total, "emigrated_total": e.emigrated_total, "priced_out_immigrants_total": e.priced_out_total },
        "recent_memory_reasons": {
            "window_days": MEMORY_WINDOW_TICKS / 1440,
            "note": "counted over residents still in the city; people who left took their memories with them",
            "top": reasons.iter().take(15).map(|(r, k)| json!({ "reason": r, "count": k })).collect::<Vec<_>>(),
        },
    })
}

/// Per open business: hourly recipe throughput at its current staffing.
fn throughput(b: &Building) -> f32 {
    b.jobs as f32 * b.staffing() * JOB_RATE
}

fn economy(w: &World) -> Value {
    let e = &w.economy;
    #[derive(Default)]
    struct Agg {
        count: u32,
        open: u32,
        staffing: f32,
        money: f32,
        revenue: f32,
        spend: f32,
        in_stock: BTreeMap<Resource, (f32, f32)>,
        out_stock: BTreeMap<Resource, (f32, f32)>,
    }
    let mut aggs: Vec<Agg> = KINDS.iter().map(|_| Agg::default()).collect();
    let mut supply: BTreeMap<Resource, (f32, u32)> = BTreeMap::new();
    let mut use_: BTreeMap<Resource, f32> = BTreeMap::new();
    let mut profits: Vec<(f32, Value)> = Vec::new();
    for (id, b) in w.buildings.iter() {
        let Some(z) = &b.business else { continue };
        let Some(k) = KINDS.iter().position(|k| *k == z.kind) else { continue };
        let a = &mut aggs[k];
        a.count += 1;
        a.staffing += b.staffing();
        a.money += z.money;
        a.revenue += z.revenue_today;
        a.spend += z.spend_today;
        let recipe = z.kind.recipe();
        for (r, _) in recipe.inputs {
            let s = a.in_stock.entry(*r).or_default();
            s.0 += z.stock_of(*r);
            s.1 += z.target_input(*r, b.jobs);
        }
        for (r, _) in recipe.outputs {
            let s = a.out_stock.entry(*r).or_default();
            s.0 += z.stock_of(*r);
            s.1 += z.sell_price(*r, b.jobs);
        }
        if !z.closed && b.construction.is_none() {
            a.open += 1;
            let t = throughput(b) * 24.0;
            for (r, per) in recipe.outputs {
                let s = supply.entry(*r).or_default();
                s.0 += per * t;
                s.1 += 1;
            }
            for (r, per) in recipe.inputs {
                *use_.entry(*r).or_default() += per * t;
            }
        }
        if z.kind != BusinessKind::Parking {
            profits.push((
                z.revenue_today - z.spend_today,
                json!({
                    "id": key_str(id), "name": b.name, "kind": z.kind.label(), "profit_today": round(z.revenue_today - z.spend_today),
                    "revenue_today": round(z.revenue_today), "spend_today": round(z.spend_today), "money": round(z.money),
                    "staffing": r3(b.staffing()), "closed": z.closed, "broke_days": z.broke_days,
                }),
            ));
        }
    }
    let kinds: Vec<Value> = KINDS
        .iter()
        .zip(&aggs)
        .filter(|(_, a)| a.count > 0)
        .map(|(k, a)| {
            let n = a.count as f32;
            json!({
                "kind": k.label(), "count": a.count, "open": a.open, "closed": a.count - a.open,
                "mean_staffing": r3(a.staffing / n), "mean_money": round(a.money / n),
                "revenue_today_total": round(a.revenue), "spend_today_total": round(a.spend),
                "inputs": a.in_stock.iter().map(|(r, (s, t))| json!({ "resource": r.label(), "stock": round(*s), "target": round(*t), "fill": if *t > 0.0 { r3(s / t) } else { 0.0 } })).collect::<Vec<_>>(),
                "outputs": a.out_stock.iter().map(|(r, (s, p))| json!({ "resource": r.label(), "stock": round(*s), "mean_price": round(p / n) })).collect::<Vec<_>>(),
            })
        })
        .collect();
    let resources: Vec<Value> = Resource::ALL
        .iter()
        .map(|&r| {
            let stock = e.market.stock.get(&r).copied().unwrap_or(0.0);
            let price = e.market.avg_price.get(&r).copied().unwrap_or(r.base_price());
            let (made, sellers) = supply.get(&r).copied().unwrap_or((0.0, 0));
            let used = use_.get(&r).copied().unwrap_or(0.0);
            let ratio = price / r.base_price();
            let scarce = ratio >= SCARCE_PRICE_RATIO || (used > 0.0 && stock < used * 0.5) || (used > 0.0 && made < used * 0.5);
            json!({
                "resource": r.label(), "retail": r.is_retail(), "town_stock": round(stock), "avg_price": round(price), "base_price": round(r.base_price()),
                "price_vs_base": r3(ratio), "local_sellers_open": sellers,
                "local_supply_per_day": round(made), "business_demand_per_day": round(used), "scarce": scarce,
            })
        })
        .collect();
    profits.sort_by(|a, b| b.0.total_cmp(&a.0));
    let most: Vec<Value> = profits.iter().take(10).map(|p| p.1.clone()).collect();
    let least: Vec<Value> = profits.iter().rev().take(10).map(|p| p.1.clone()).collect();
    let l = &e.ledger;
    json!({
        "treasury": round(e.treasury),
        "ledger": {
            "today_so_far": ledger_map(&l.today), "yesterday": ledger_map(&l.yesterday),
            "net_today_so_far": round(l.today.iter().sum()), "net_yesterday": round(l.yesterday.iter().sum()),
            "note": "positive is income to the treasury; upkeep is booked at midnight",
        },
        "tax_today": round(e.tax_today),
        "upkeep_today": { "power_plants": round(e.power.upkeep_today), "transit": round(e.transit.upkeep_today), "rail": round(w.rail.stats.upkeep_today), "grid_import_cost": round(e.power.import_cost_today) },
        "trade_today": {
            "imports_value": round(e.market.imports_today), "exports_value": round(e.market.exports_today), "building_materials_imported": round(e.materials_today),
            "deals": e.market.deals_today, "meals_sold": e.market.meals_sold_today, "walk_in_sales": round(e.walk_in_sales_today), "parking_fees": round(e.parking_fees_today),
        },
        "businesses": { "total": aggs.iter().map(|a| a.count).sum::<u32>(), "open": aggs.iter().map(|a| a.open).sum::<u32>(), "closed_ever": e.market.closed_total, "by_kind": kinds },
        "resources": resources,
        "resources_note": "local_supply_per_day = open producers' output at current staffing if inputs allow; business_demand_per_day = open buyers' inputs at current staffing; citizens' own grocery and goods purchases are not included",
        "most_profitable_today": most,
        "least_profitable_today": least,
    })
}

fn land(w: &World) -> Value {
    let mut lots: Vec<[u32; 5]> = vec![[0; 5]; 6]; // vacant, built, under construction, unusable, total
    let mut lot_area = [0.0f32; 6];
    let mut lvs = Vec::with_capacity(w.lots.len());
    let (mut parks, mut park_area) = (0, 0.0f32);
    for l in w.lots.values() {
        let i = l.land_use.index();
        let slot = if !l.usable() {
            3
        } else {
            match l.building.and_then(|b| w.buildings.get(b)) {
                None => 0,
                Some(b) if b.construction.is_some() => 2,
                Some(_) => 1,
            }
        };
        lots[i][slot] += 1;
        lots[i][4] += 1;
        lot_area[i] += l.area();
        lvs.push(w.land_value.at(l.center));
        if l.land_use == LandUse::Park {
            parks += 1;
            park_area += l.area();
        }
    }
    let lots_json: Map<String, Value> = LandUse::ALL
        .iter()
        .map(|&lu| {
            let c = lots[lu.index()];
            (lu_key(lu), json!({ "vacant": c[0], "built": c[1], "under_construction": c[2], "unusable": c[3], "total": c[4], "area_m2": lot_area[lu.index()].round() }))
        })
        .collect();
    let mut grid = vec![[0u32; 3]; 6];
    let (mut plants, mut depots, mut stations, mut yards, mut parking) = (0, 0, 0, 0, 0);
    let mut floors: BTreeMap<&str, u32> = BTreeMap::new();
    let mut rents: Vec<Vec<f32>> = vec![Vec::new(); 3];
    let mut homes = [(0u32, 0u32); 3];
    for b in w.buildings.values().filter(|b| b.construction.is_none()) {
        if b.plant.is_some() {
            plants += 1;
        } else if b.depot {
            depots += 1;
        } else if let Some(r) = &b.rail {
            if r.kind == RailFacility::Station {
                stations += 1;
            } else {
                yards += 1;
            }
        } else if b.is_parking() {
            parking += 1;
        } else {
            grid[b.land_use.index()][b.tier.index()] += 1;
            let bucket = match b.floors {
                0..=1 => "1",
                2 => "2",
                3 => "3",
                4..=5 => "4-5",
                6..=8 => "6-8",
                9..=12 => "9-12",
                _ => "13+",
            };
            *floors.entry(bucket).or_default() += 1;
            if b.land_use == LandUse::Residential {
                rents[b.tier.index()].push(b.rent);
                homes[b.tier.index()].0 += b.homes as u32;
                homes[b.tier.index()].1 += b.residents.len() as u32;
            }
        }
    }
    let grid_json: Map<String, Value> = LandUse::ALL
        .iter()
        .map(|&lu| {
            let g = grid[lu.index()];
            (lu_key(lu), json!({ "low": g[0], "middle": g[1], "high": g[2] }))
        })
        .collect();
    let rents_json: Map<String, Value> = Tier::ALL
        .iter()
        .map(|&t| {
            let r = &rents[t.index()];
            let n = r.len().max(1) as f32;
            let (h, occ) = homes[t.index()];
            (
                t.label().to_string(),
                json!({
                    "buildings": r.len(), "homes": h, "residents": occ, "occupancy": share(occ as usize, h as usize),
                    "rent_per_home_day_avg": round(r.iter().sum::<f32>() / n), "min": if r.is_empty() { 0.0 } else { round(r.iter().copied().fold(f32::INFINITY, f32::min)) },
                    "max": round(r.iter().copied().fold(0.0, f32::max)),
                }),
            )
        })
        .collect();
    let floor_order = ["1", "2", "3", "4-5", "6-8", "9-12", "13+"];
    let floors_json: Map<String, Value> = floor_order.iter().map(|k| (k.to_string(), json!(floors.get(k).copied().unwrap_or(0)))).collect();
    json!({
        "lots_by_land_use": lots_json,
        "buildings_by_land_use_and_tier": grid_json,
        "special_buildings": { "power_plants": plants, "bus_depots": depots, "stations": stations, "freight_yards": yards, "parking_lots": parking },
        "floors_histogram": floors_json,
        "rents_by_tier": rents_json,
        "land_value": { "avg_grid": round(w.economy.avg_land_value), "lots": distribution(lvs, 10, Some((0.0, 1.0))), "tier_cuts": { "middle_from": 0.38, "high_from": 0.66 } },
        "parks": { "lots": parks, "area_m2": park_area.round() },
    })
}

fn services(w: &World) -> Value {
    let e = &w.economy;
    let p = &e.power;
    let day = w.clock.day_fraction();
    // Plants share the load pro rata: output = available Ã— min(1, demand / supply).
    let dispatch = if p.supply_mw > 0.0 { (p.demand_mw / p.supply_mw).min(1.0) } else { 0.0 };
    let plants: Vec<Value> = w
        .buildings
        .iter()
        .filter_map(|(id, b)| b.plant.map(|k| (id, b, k)))
        .map(|(id, b, k)| {
            let avail = if b.is_derelict() { 0.0 } else { k.capacity_mw() * k.availability(&w.weather, day) };
            let (radius, weight) = k.pollution();
            json!({
                "id": key_str(id), "name": b.name, "kind": plant_name(k), "capacity_mw": round(k.capacity_mw()), "available_mw": round(avail),
                "output_mw_est": round(avail * dispatch), "upkeep_per_day": round(k.upkeep_per_day()), "pollution_radius_m": radius, "pollution_land_value_effect": round(weight),
                "workers": b.workers.len(), "jobs": b.jobs, "derelict": b.is_derelict(), "pos": building_pos(w, b).map_or(Value::Null, v2),
            })
        })
        .collect();
    let mut route_waiting: BTreeMap<String, u32> = BTreeMap::new();
    let mut station_waiting: BTreeMap<BuildingId, u32> = BTreeMap::new();
    for c in w.citizens.values() {
        if matches!(c.activity, Activity::Waiting { .. }) {
            match c.transit.map(|t| t.leg) {
                Some(TransitLeg::Bus { route, .. }) => *route_waiting.entry(key_str(route)).or_default() += 1,
                Some(TransitLeg::Rail { board, .. }) => *station_waiting.entry(board).or_default() += 1,
                Some(TransitLeg::Air { .. }) | None => {}
            }
        }
    }
    let mut buses_on_route: BTreeMap<String, u32> = BTreeMap::new();
    let (mut cars, mut trucks, mut buses, mut stopped, mut stuck, mut kerb) = (0, 0, 0, 0, 0, 0);
    let mut speed_sum = 0.0f32;
    let mut moving = 0;
    for c in w.traffic.cars.values() {
        if let Some(bus) = &c.bus {
            buses += 1;
            *buses_on_route.entry(key_str(bus.route)).or_default() += 1;
        } else if c.cargo.is_some() {
            trucks += 1;
        } else {
            cars += 1;
        }
        if c.v < STUCK_SPEED {
            stopped += 1;
            if is_stuck(c) {
                stuck += 1;
            } else if c.entering && c.stalled > STUCK_STALL_S {
                kerb += 1;
            }
        } else {
            moving += 1;
            speed_sum += c.v;
        }
    }
    let routes: Vec<Value> = w
        .transit
        .routes
        .iter()
        .map(|(id, r)| {
            let k = key_str(id);
            json!({
                "id": k, "name": r.name, "stops": r.stops.len(), "buses_wanted": r.buses, "buses_running": buses_on_route.get(&k).copied().unwrap_or(0),
                "riders_today": r.riders_today, "waiting": route_waiting.get(&k).copied().unwrap_or(0), "length_m": route_length(r).round(),
                "fare": round(r.fare), "broken_legs": r.broken.len(), "runnable": r.runnable(),
            })
        })
        .collect();
    let facilities: Vec<Value> = w
        .buildings
        .iter()
        .filter_map(|(id, b)| b.rail.as_ref().map(|l| (id, b, l)))
        .map(|(id, b, l)| {
            json!({
                "id": key_str(id), "name": b.name, "kind": rail_name(l.kind), "rail_connected": l.rail_connected, "road_connected": l.road_connected,
                "works": l.works() && !b.is_derelict(), "waiting_passengers": station_waiting.get(&id).copied().unwrap_or(0),
                "inbound_units": round(b.rail_inbound.iter().map(|c| c.amount).sum()), "outbound_units": round(b.rail_outbound), "pos": building_pos(w, b).map_or(Value::Null, v2),
            })
        })
        .collect();
    let (mut ptrains, mut ftrains, mut riding, mut cargo) = (0, 0, 0, 0.0f32);
    for t in w.rail.trains.values() {
        match t.kind {
            TrainKind::Passenger => ptrains += 1,
            TrainKind::Freight => ftrains += 1,
        }
        riding += t.passengers.len();
        cargo += t.cargo.iter().map(|c| c.amount).sum::<f32>();
    }
    let t = &e.transit;
    let r = &w.rail.stats;
    json!({
        "emergency": emergency(w),
        "power": {
            "supply_mw": round(p.supply_mw), "demand_mw": round(p.demand_mw), "imported_mw": round(p.imported_mw), "served_share": round(p.served),
            "unpowered_buildings": p.unpowered_buildings, "brownout_hours_today": p.brownout_hours_today, "upkeep_per_day": round(p.upkeep_today),
            "import_cost_today": round(p.import_cost_today), "weather": { "label": w.weather.label(), "cloud": round(w.weather.cloud), "wind": round(w.weather.wind) },
            "plants": plants,
        },
        "transit": {
            "routes": routes, "buses": t.buses, "depots": t.depots, "depot_capacity": t.depot_capacity, "waiting": t.waiting, "riding": t.riding,
            "riders_today": t.riders_today, "fares_today": round(t.fares_today), "upkeep_today": round(t.upkeep_today),
        },
        "rail": {
            "stations": r.stations, "yards": r.yards, "facilities": facilities,
            "trains": { "passenger": ptrains, "freight": ftrains, "passengers_aboard": riding, "cargo_units_aboard": round(cargo) },
            "waiting": r.waiting, "riders_today": r.riders_today, "fares_today": round(r.fares_today), "passenger_trains_today": r.passenger_trains_today,
            "freight_trains_today": r.freight_trains_today, "freight_in_today": round(r.freight_in_today), "freight_waiting": round(r.freight_waiting),
            "next_passenger_in_ticks": r.next_passenger_in,
        },
        "air": crate::remote::view::air_report(w),
        "parking": {
            "capacity": e.parking_capacity, "used": e.parking_used, "occupancy": share(e.parking_used as usize, e.parking_capacity as usize), "parking_lots": e.parking_lots,
            "parked_on_street_today": e.parking_street_today, "fees_today": round(e.parking_fees_today), "last_shortage_at": e.parking_shortage_at.map(v2),
        },
        "traffic": {
            "vehicles": w.traffic.cars.len(), "cars": cars, "trucks": trucks, "buses": buses, "mean_speed_kmh_moving": if moving > 0 { round(speed_sum / moving as f32 * 3.6) } else { 0.0 },
            "stopped": stopped, "stuck": stuck, "stuck_rule": format!("in traffic and standing still for {STUCK_STALL_S} s or more (not a bus at a stop, not at a red light)"),
            "waiting_at_kerb": kerb, "gridlocks": w.traffic.gridlocks().len(),
            "lanes": w.traffic.net.lanes.len(), "intersections": w.traffic.net.intersections.len(), "signals": w.traffic.net.intersections.iter().filter(|i| i.signalized).count(),
            "walk_trips_today": e.walk_trips_today,
            "left_turn_wait_mean_s": round(w.traffic.left_wait_stats().0), "left_turn_wait_max_s": round(w.traffic.left_wait_stats().1),
            "left_turns_measured": w.traffic.left_wait_stats().2, "protected_left_arrows_now": w.traffic.net.intersections.iter().filter(|i| i.timer < i.arrow).count(),
            "passing_left_turners_now": w.traffic.cars.values().filter(|c| c.passing).count(), "lane_use_set_by_player": w.traffic.lane_use.len(),
            "models": nd_sim::bots::fleet_counts(w),
        },
        "bots": {
            "active": w.bots.bots.len(), "delivering": w.bots.bots.values().filter(|b| !b.returning).count(), "delivered_today": w.bots.delivered_today,
            "vans_on_road": nd_sim::Bots::vans(w), "van_orders_today": w.bots.van_orders_today,
            "rule": format!("shops deliver to homes within {} m; by pavement robot where shop and home both have land value >= {}, else by van", nd_sim::bots::DELIVERY_RANGE, nd_sim::bots::BOT_LAND_VALUE),
        },
    })
}

/// City services and emergencies: coverage, stations and their units,
/// incidents today and under way, response times, hospital beds, stadiums.
fn emergency(w: &World) -> Value {
    use nd_sim::services::{IncidentKind, ServiceKind};
    let s = &w.services;
    let st = &s.stats;
    let stations: Vec<Value> = w
        .buildings
        .iter()
        .filter_map(|(id, b)| b.service.map(|sv| (id, b, sv)))
        .map(|(id, b, sv)| {
            let units: Vec<&nd_sim::Unit> = s.units.values().filter(|u| u.station == id).collect();
            json!({
                "id": key_str(id), "name": b.name, "kind": sv.kind.key(), "tier": tier_key(sv.tier), "label": sv.kind.tier_label(sv.tier),
                "workers": b.workers.len(), "jobs": b.jobs, "radius_m": sv.kind.radius(sv.tier), "upkeep_per_day": round(sv.kind.upkeep_per_day(sv.tier)),
                "units": units.len(), "units_crewed": w.crewed_units(id), "units_busy": units.iter().filter(|u| u.busy()).count(),
                "beds_used": if sv.kind == ServiceKind::Health { json!(s.admissions.iter().filter(|a| a.hospital == id).count()) } else { Value::Null },
                "capacity": sv.kind.capacity(sv.tier), "pos": building_pos(w, b).map_or(Value::Null, v2),
            })
        })
        .collect();
    let active: Vec<Value> = s
        .active()
        .map(|i| {
            json!({
                "id": i.id, "kind": i.kind.label(), "stage": i.stage.label(), "building": key_str(i.building), "name": building_name(w, Some(i.building)),
                "pos": v2(i.pos), "minutes_since_start": w.clock.tick.saturating_sub(i.started), "intensity": if i.kind == IncidentKind::Fire { json!(round(i.intensity)) } else { Value::Null },
                "unit": i.unit.map(key_str), "dispatcher": i.dispatcher.map(key_str),
            })
        })
        .collect();
    let recent: Vec<Value> = s
        .incidents
        .iter()
        .rev()
        .filter(|i| !i.active())
        .take(10)
        .map(|i| json!({ "id": i.id, "kind": i.kind.label(), "outcome": i.outcome.map(|o| o.label()), "name": building_name(w, Some(i.building)), "response_min": i.response_ticks() }))
        .collect();
    let (dispatcher, delay) = w.dispatcher();
    let cov = |k: usize| round(st.coverage[k] * 100.0);
    json!({
        "coverage_pct_of_homes": { "police": cov(0), "fire": cov(1), "health": cov(2), "school": cov(3) },
        "incidents_today": { "fire": st.incidents_today[0], "crime": st.incidents_today[1], "medical": st.incidents_today[2] },
        "resolved_today": st.resolved_today.iter().sum::<u32>(), "unanswered_today": st.unanswered_today, "arrests_today": st.arrests_today,
        "avg_response_min": round(st.avg_response()), "avg_response_min_yesterday": round(st.avg_response_yesterday),
        "units": st.units, "units_busy": st.units_busy, "fires_burning": st.fires_burning, "burned_down_total": st.burned_down_total,
        "waiting_for_ambulance": st.waiting_for_ambulance, "in_hospital": st.in_hospital,
        "dispatch": { "dispatcher_on_duty": dispatcher.map(key_str), "minutes_to_dispatch": delay },
        "stadium_crowd_today": st.stadium_crowd_today, "tickets_today": round(st.tickets_today), "stadium_events_tonight": s.events_today.len(),
        "upkeep_per_day": round(st.upkeep_today), "education_wage_bonus_today": round(st.education_bonus_today), "pupils_today": st.pupils_today,
        "jailed": st.jailed, "held_at_stations": st.held_at_stations, "released_early_total": st.released_early_total,
        "out_of_town_calls_today": st.out_of_town_calls_today, "out_of_town_fees_today": round(st.out_of_town_fees_today),
        "hospital_bills_paid_today": round(st.bills_paid_today), "hospital_bills_covered_by_city_today": round(st.bills_covered_today),
        "stations": stations, "active_incidents": active, "recent": recent,
    })
}

pub fn route_length(r: &nd_sim::BusRoute) -> f32 {
    let n = r.stops.len();
    if n < 2 {
        return 0.0;
    }
    (0..n)
        .map(|i| match r.legs.get(i) {
            Some(leg) if r.legs.len() == n => nd_sim::geom::path_length(leg),
            _ => r.stops[i].pos.distance(r.stops[(i + 1) % n].pos),
        })
        .sum()
}

// â”€â”€ alerts â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€

/// One concrete problem: how many, where (the centroid of the cases and
/// their mean distance from it), and ids to `inspect`.
pub struct Alert {
    pub kind: &'static str,
    /// 3 high, 2 medium, 1 low.
    pub severity: u8,
    pub count: usize,
    pub message: String,
    pub hint: &'static str,
    points: Vec<SVec2>,
    examples: Vec<String>,
}

impl Alert {
    fn new(kind: &'static str, severity: u8, hint: &'static str) -> Self {
        Self { kind, severity, count: 0, message: String::new(), hint, points: Vec::new(), examples: Vec::new() }
    }

    fn add(&mut self, id: String, at: Option<SVec2>) {
        self.count += 1;
        if let Some(p) = at {
            self.points.push(p);
        }
        if self.examples.len() < ALERT_EXAMPLES {
            self.examples.push(id);
        }
    }

    /// Ranking: severity first, then (logarithmically) how many.
    fn score(&self) -> f32 {
        self.severity as f32 * 10.0 + (self.count as f32).ln_1p()
    }

    pub fn to_json(&self, rank: usize) -> Value {
        let (at, spread) = centroid(&self.points);
        json!({
            "rank": rank, "kind": self.kind, "severity": match self.severity { 3 => "high", 2 => "medium", _ => "low" },
            "count": self.count, "message": self.message, "where": at, "spread_m": spread, "example_ids": self.examples, "hint": self.hint,
        })
    }
}

/// In traffic, standing still for minutes, not dwelling at a stop, not at
/// a red light and not still waiting at the kerb to set off.
fn is_stuck(c: &nd_sim::Car) -> bool {
    c.stalled > STUCK_STALL_S && c.hold <= 0.0 && !c.entering && c.wait != nd_sim::traffic::Wait::Signal
}

pub fn alerts_json(alerts: &[Alert], limit: usize) -> Value {
    Value::Array(alerts.iter().take(limit).enumerate().map(|(i, a)| a.to_json(i + 1)).collect())
}

/// Every alert with a non-zero count, most urgent first.
pub fn alerts(w: &World) -> Vec<Alert> {
    let e = &w.economy;
    let mut hungry = Alert::new("hungry_citizens", 2, "food sellers need stock and staff near homes: check economy.resources groceries and grocery staffing; zone commercial near housing");
    let mut starving = Alert::new("starving_citizens", 3, "these leave the city at 4 hungry days; see food_sellers_* alerts and map layer=hunger");
    let mut unhoused = Alert::new("unhoused_citizens", 3, "zone residential (cheap land builds low-tier homes) or wait for sites to finish");
    let mut rent = Alert::new("rent_stressed_residents", 2, "rent above half of income; more low-tier housing or jobs with better wages");
    let mut food_staff = Alert::new("food_sellers_understaffed", 3, "groceries and bakeries short of workers cannot produce; more housing nearby or better transit");
    let mut food_empty = Alert::new("food_sellers_empty", 3, "retail food out of stock: check the supply chain inputs (produce, flour, meat, dairy) in economy.businesses.by_kind");
    let mut closed = Alert::new("closed_businesses", 1, "closed businesses go derelict; usually short of staff or customers");
    let mut broke = Alert::new("broke_businesses", 2, "businesses losing money; they close after several broke days");
    let mut unpowered = Alert::new("unpowered_buildings", 3, "build power plants (status.power, report services.power)");
    let mut crowded = Alert::new("overcrowded_homes", 1, "more residents than homes");
    let mut rail_broken = Alert::new("rail_facility_not_connected", 3, "a station or yard needs both its rail stub joined to the track and its street stub joined to a road");
    let mut route_broken = Alert::new("bus_route_broken", 2, "a leg of the line has no road between its stops, or it has fewer than two stops; move or remove stops");
    let mut sites = Alert::new("construction_waiting_for_materials", 1, "sites wait for parts deliveries each quarter; many waiting means parts are short (economy.resources parts)");
    let mut stuck = Alert::new("stuck_vehicles", 2, "vehicles standing still for minutes: a gridlock ring (listed with its junctions) or a street over capacity; add lanes, another route or transit there");
    let mut derelict = Alert::new("derelict_buildings", 1, "condition below 0.3; they are redeveloped or demolished in time");
    let mut vacant_demand = Alert::new("vacant_lots_with_high_demand", 1, "demand is high but lots stand empty: they may lack road access or the area may be unattractive");
    for (id, c) in w.citizens.iter() {
        let k = key_str(id);
        if c.satiety < HUNGRY_SATIETY {
            hungry.add(k.clone(), Some(c.pos));
        }
        if c.hungry_days >= STARVING_DAYS {
            starving.add(k.clone(), Some(c.pos));
        }
        if c.home.is_none() {
            unhoused.add(k.clone(), Some(c.pos));
        }
        if let Some(home) = c.home.and_then(|h| w.buildings.get(h)) {
            if c.rent_stress_days > 0 || w.rent_share_of(id).unwrap_or(home.rent / c.income.max(0.01)) > RENT_STRESS_SHARE {
                rent.add(k, building_pos(w, home));
            }
        }
    }
    for (id, b) in w.buildings.iter() {
        let k = key_str(id);
        let at = building_pos(w, b);
        if b.construction.is_none() {
            if let Some(z) = &b.business {
                let food = matches!(z.kind, BusinessKind::Grocery | BusinessKind::Bakery);
                if z.closed {
                    closed.add(k.clone(), at);
                } else {
                    if food && b.staffing() < FOOD_SELLER_MIN_STAFFING {
                        food_staff.add(k.clone(), at);
                    }
                    if food && z.kind.recipe().outputs.iter().all(|(r, _)| z.stock_of(*r) < FOOD_STOCK_EMPTY) {
                        food_empty.add(k.clone(), at);
                    }
                    if z.broke_days > 0 {
                        broke.add(k.clone(), at);
                    }
                }
            }
            if b.households.len() > b.homes as usize && b.homes > 0 {
                crowded.add(k.clone(), at);
            }
            if b.is_derelict() {
                derelict.add(k.clone(), at);
            }
        } else if b.demolition.is_none() && b.construction.as_ref().is_some_and(|c| c.waiting_for_materials()) {
            sites.add(k.clone(), at);
        }
        if b.powered < UNPOWERED_BELOW && nd_sim::power::demand_mw(b) > 0.0 {
            unpowered.add(k.clone(), at);
        }
        if let Some(l) = &b.rail {
            if !l.works() {
                rail_broken.add(k, at);
            }
        }
    }
    let mut rail_missing = (0, 0);
    for b in w.buildings.values() {
        if let Some(l) = &b.rail {
            rail_missing.0 += !l.rail_connected as u32;
            rail_missing.1 += !l.road_connected as u32;
        }
    }
    for (id, r) in w.transit.routes.iter() {
        if !r.runnable() {
            route_broken.add(key_str(id), r.stops.first().map(|s| s.pos));
        }
    }
    let gridlocks = w.traffic.gridlocks();
    let mut kerb = 0;
    for (id, c) in w.traffic.cars.iter() {
        if is_stuck(c) {
            stuck.add(key_str(id), Some(c.pos));
        } else if c.entering && c.stalled > STUCK_STALL_S {
            kerb += 1;
        }
    }
    if !gridlocks.is_empty() {
        stuck.severity = 3;
        // Point the alert at the gridlocks rather than at the queues.
        stuck.points = gridlocks.iter().flat_map(|g| g.points.iter().copied()).collect();
    }
    let high: Vec<LandUse> = LandUse::ALL.iter().copied().filter(|&lu| lu != LandUse::Park && e.demand_for(lu) >= HIGH_DEMAND).collect();
    for (id, l) in w.lots.iter() {
        if l.building.is_none() && l.usable() && high.contains(&l.land_use) && !w.under_facility(l.center) {
            vacant_demand.add(key_str(id), Some(l.center));
        }
    }

    let n = w.citizens.len();
    hungry.message = format!("{} of {} citizens are hungry (satiety < {HUNGRY_SATIETY})", hungry.count, n);
    starving.message = format!("{} citizens have gone {STARVING_DAYS}+ days without a proper meal", starving.count);
    unhoused.message = format!("{} citizens have no home ({} more immigrants waiting, {} homes free)", unhoused.count, e.waiting_immigrants, e.homes_free);
    rent.message = format!("{} residents pay more than {:.0}% of their income in rent", rent.count, RENT_STRESS_SHARE * 100.0);
    food_staff.message = format!("{} open groceries/bakeries are below {:.0}% staffing", food_staff.count, FOOD_SELLER_MIN_STAFFING * 100.0);
    food_empty.message = format!("{} open groceries/bakeries have nothing to sell", food_empty.count);
    closed.message = format!("{} businesses are closed ({} closures in total)", closed.count, e.market.closed_total);
    let open_businesses = w.buildings.values().filter(|b| b.construction.is_none() && b.business.as_ref().is_some_and(|z| !z.closed)).count();
    broke.message = format!("{} of {open_businesses} open businesses are in the red past their overdraft (they close after 5 such days)", broke.count);
    unpowered.message = format!("{} buildings get less than {:.0}% of their power (served {:.0}% of demand)", unpowered.count, UNPOWERED_BELOW * 100.0, e.power.served * 100.0);
    crowded.message = format!("{} buildings house more people than they have homes", crowded.count);
    rail_broken.message = format!("{} stations/yards do not work: {} lack a track connection, {} a street connection", rail_broken.count, rail_missing.0, rail_missing.1);
    route_broken.message = format!("{} bus lines cannot run", route_broken.count);
    sites.message = format!("{} building sites are waiting for materials", sites.count);
    stuck.message = format!("{} vehicles have stood still for {STUCK_STALL_S:.0}+ s in traffic; {kerb} more wait at the kerb to pull out", stuck.count);
    if !gridlocks.is_empty() {
        let rings: Vec<String> = gridlocks
            .iter()
            .take(4)
            .map(|g| {
                let at: Vec<String> = g.points.iter().map(|p| format!("({:.0}, {:.0})", p.x, p.y)).collect();
                format!("{} vehicles through {}", g.vehicles, at.join(" "))
            })
            .collect();
        stuck.message += &format!("; {} gridlock(s): {}", gridlocks.len(), rings.join("; "));
    }
    derelict.message = format!("{} buildings are derelict", derelict.count);
    vacant_demand.message = format!("{} vacant lots of high-demand land use ({}) stand empty", vacant_demand.count, high.iter().map(|lu| lu_key(*lu)).collect::<Vec<_>>().join(", "));

    // Services: homes outside each service's road reach, fires, slow responses.
    let mut uncovered: Vec<Alert> = Vec::new();
    let homes: Vec<(BuildingId, &Building)> = w.buildings.iter().filter(|(_, b)| b.homes > 0 && b.construction.is_none()).collect();
    if homes.len() >= 10 {
        for (k, (kind, sev, hint)) in [
            ("no_police_coverage", 2u8, "place a police station (Buildings > Services) near these homes; crime falls and arrests come quicker"),
            ("no_fire_coverage", 2, "place a fire station (Buildings > Services); fires nobody fights spread and burn buildings down"),
            ("no_health_coverage", 1, "place a clinic or hospital (Buildings > Services); the sick wait for an ambulance that may not come"),
            ("no_school_coverage", 1, "place a school (Buildings > Services); workers near one earn more"),
        ]
        .into_iter()
        .enumerate()
        {
            let mut a = Alert::new(kind, sev, hint);
            for (id, b) in &homes {
                if w.services.coverage.get(&b.lot).is_none_or(|c| c[k] < nd_sim::services::COVERED) {
                    a.add(key_str(*id), building_pos(w, b));
                }
            }
            a.message = format!("{} of {} residential buildings are outside {} coverage", a.count, homes.len(), ["police", "fire", "health", "school"][k]);
            uncovered.push(a);
        }
    }
    let mut fires = Alert::new("fires_burning", 3, "fire engines on the way show in services.emergency.active_incidents; with no fire station in reach they burn until the building is gone");
    for i in w.services.active().filter(|i| i.kind == nd_sim::IncidentKind::Fire) {
        fires.add(key_str(i.building), Some(i.pos));
    }
    fires.message = format!("{} buildings are on fire", fires.count);
    let st = &w.services.stats;
    if st.avg_response() > 45.0 {
        let mut a = Alert::new("slow_emergency_response", 2, "a dispatcher on shift (911 dispatch centre with staff) and stations closer to the calls cut response times");
        a.count = 1;
        a.message = format!("emergency units take {:.0} minutes on average from the 911 call to arrival", st.avg_response());
        uncovered.push(a);
    }
    if w.services.incidents.iter().any(|i| i.reported.is_some()) && w.dispatcher().0.is_none() {
        let mut a = Alert::new("no_dispatcher", 2, "911 calls go to the slow regional line: place a 911 dispatch centre and let it hire");
        a.count = 1;
        a.message = format!("no dispatcher on duty: jobs go out {} minutes after the call", w.dispatcher().1);
        uncovered.push(a);
    }
    let mut out = vec![fires, hungry, starving, unhoused, rent, food_staff, food_empty, closed, broke, unpowered, crowded, rail_broken, route_broken, sites, stuck, derelict, vacant_demand];
    // City-wide conditions with no single place.
    // Only working-age adults look for work: children and retirees are not
    // unemployed.
    let adults = w.citizens.values().filter(|c| c.can_work()).count();
    let unemployed = w.citizens.values().filter(|c| c.can_work() && c.work.is_none()).count();
    if adults > 0 && unemployed as f32 / adults as f32 >= UNEMPLOYMENT_ALERT {
        let mut a = Alert::new("high_unemployment", 2, "zone commercial or industrial; jobs_free shows whether jobs exist but are out of reach");
        a.count = unemployed;
        a.message = format!("{:.0}% of working-age adults have no job ({unemployed} of {adults}; {} jobs free)", unemployed as f32 / adults as f32 * 100.0, e.jobs_free);
        out.push(a);
    }
    let net = e.ledger.yesterday.iter().sum::<f32>();
    if e.treasury < 0.0 || net < 0.0 {
        let mut a = Alert::new("treasury_losing_money", if e.treasury < 0.0 { 3 } else { 2 }, "see economy.ledger: plant and transit upkeep against taxes");
        a.count = 1;
        a.message = format!("treasury {:.0}, net yesterday {:.0}", e.treasury, net);
        out.push(a);
    }
    if e.parking_street_today > 0 {
        let mut a = Alert::new("parking_shortage", 1, "drivers found no space; place parking near the last shortage location");
        a.count = e.parking_street_today as usize;
        a.message = format!("{} drivers parked on the street today for lack of spaces", e.parking_street_today);
        if let Some(p) = e.parking_shortage_at {
            a.points.push(p);
        }
        out.push(a);
    }
    let scarce: Vec<String> = Resource::ALL
        .iter()
        .filter_map(|&r| {
            let price = e.market.avg_price.get(&r).copied()?;
            (price / r.base_price() >= SCARCE_PRICE_RATIO).then(|| format!("{} ({:.2}x base)", r.label(), price / r.base_price()))
        })
        .collect();
    if !scarce.is_empty() {
        let retail = Resource::ALL.iter().any(|r| r.is_retail() && e.market.avg_price.get(r).is_some_and(|p| p / r.base_price() >= SCARCE_PRICE_RATIO));
        let mut a = Alert::new("scarce_resources", if retail { 2 } else { 1 }, "prices well above base: few local producers for the chain (see economy.resources local_supply_per_day vs business_demand_per_day); zone industry");
        a.count = scarce.len();
        a.message = format!("{} resources are scarce: {}", scarce.len(), scarce.join(", "));
        out.push(a);
    }
    out.extend(uncovered);
    out.retain(|a| a.count > 0);
    out.sort_by(|a, b| b.score().total_cmp(&a.score()));
    out
}

/// Industry by sub-zone, the ground it fouls, the player's biases, truck
/// and bus lots, and what the plants cost the land around them.
fn industry(w: &World) -> Value {
    use nd_sim::IndustryKind;
    let mut rows: Vec<(u32, u32, u32, f32, f32)> = vec![(0, 0, 0, 0.0, 0.0); IndustryKind::ALL.len()];
    let (mut fleet_lots, mut fleet_bays, mut fleet_parked) = (0u32, 0u32, 0u32);
    for (id, b) in w.buildings.iter() {
        let Some(z) = &b.business else { continue };
        let Some(k) = z.kind.industry() else { continue };
        let r = &mut rows[k.index()];
        r.0 += 1;
        if !z.closed && b.construction.is_none() {
            r.1 += 1;
        }
        r.2 += b.jobs as u32;
        r.3 += b.staffing();
        r.4 += z.revenue_today - z.spend_today;
        if z.kind == BusinessKind::FleetLot {
            fleet_lots += 1;
            fleet_bays += w.fleet_capacity(id) as u32;
            fleet_parked += z.fleet_trucks as u32;
        }
    }
    let kinds: Map<String, Value> = IndustryKind::ALL
        .iter()
        .map(|k| {
            let r = rows[k.index()];
            let (radius, nuisance) = k.nuisance();
            (
                k.key().to_string(),
                json!({ "label": k.label(), "buildings": r.0, "open": r.1, "jobs": r.2, "mean_staffing": if r.0 > 0 { r3(r.3 / r.0 as f32) } else { 0.0 },
                        "profit_today": round(r.4), "nuisance": { "radius_m": radius, "weight": nuisance }, "fouls_ground": k.spill().is_some() }),
            )
        })
        .collect();
    let returning = w.traffic.cars.values().filter(|c| c.cargo.as_ref().is_some_and(nd_sim::world::is_return_trip)).count();
    let trucks = w.traffic.cars.values().filter(|c| c.cargo.is_some()).count();
    let biases: Vec<Value> = w
        .industry_bias
        .iter()
        .rev()
        .take(20)
        .map(|b| json!({ "kind": b.kind.map(|k| k.key()), "corners": b.polygon.len(), "first": b.polygon.first().map(|p| [round(p.x), round(p.y)]) }))
        .collect();
    let plants: Vec<Value> = nd_sim::PowerPlantKind::ALL
        .iter()
        .map(|k| {
            let (r, wgt) = k.pollution();
            json!({ "kind": k.label(), "radius_m": r, "land_value": wgt, "load": round(k.pollution_load()), "ash": k.ground_spill().is_some() })
        })
        .collect();
    let c = &w.contamination;
    json!({
        "by_kind": kinds,
        "contamination": { "cells": c.cells.len(), "cells_over_0_3": c.cells_over(0.3), "max": r3(c.max()), "total": round(c.total()), "cell_m": c.cell,
                            "note": "0..1 per cell; oil wells, refineries and coal ash raise it, it spreads and fades daily; it costs land value and spoils crops" },
        "fleet": { "truck_and_bus_lots": fleet_lots, "bays": fleet_bays, "trucks_parked": fleet_parked, "trucks_on_road": trucks, "returning_empty": returning },
        "biases": { "count": w.industry_bias.len(), "newest_first": biases },
        "plant_land_value": plants,
        "land_value_note": "plants and heavy industry are nuisances: homes feel 125% of them, shops 85%, industry 30%; working industry lifts land below 0.35 by up to 0.1",
    })
}
