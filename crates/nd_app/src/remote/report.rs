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

// ── thresholds ──────────────────────────────────────────────────────────────
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

pub const SECTIONS: &[&str] = &["overview", "people", "economy", "land", "services", "industry", "society", "pressure", "alerts", "history", "ridership", "leisure"];

const KINDS: [BusinessKind; BusinessKind::COUNT] = BusinessKind::ALL;

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

// ── report ──────────────────────────────────────────────────────────────────

pub fn report(w: &World, sections: &[String]) -> Result<Value, String> {
    for s in sections {
        if !SECTIONS.contains(&s.as_str()) {
            return Err(format!("unknown report section {s:?}; one of {}", SECTIONS.join(", ")));
        }
    }
    let want = |s: &str| sections.is_empty() || sections.iter().any(|x| x == s);
    // `history` is long and filled in by the caller (`sim_history`).
    let alerts = alerts(w);
    let mut out = Map::new();
    if want("overview") {
        out.insert("overview".into(), overview(w, &alerts));
    }
    if want("people") {
        out.insert("people".into(), people(w));
    }
    if want("economy") {
        let mut e = economy(w);
        e["venues"] = super::places::venue_ratings(w);
        out.insert("economy".into(), e);
    }
    if want("land") {
        out.insert("land".into(), land(w));
    }
    if want("services") {
        out.insert("services".into(), services(w));
    }
    // [society] crime, the street and the bins.
    if want("society") {
        out.insert("society".into(), super::society::society(w));
    }
    if want("industry") {
        out.insert("industry".into(), industry(w));
    }
    // [economy] what the city's businesses want
    if want("pressure") {
        out.insert("pressure".into(), super::economy::report(w));
    }
    if want("alerts") {
        out.insert("alerts".into(), alerts_json(&alerts, usize::MAX));
    }
    // [pt-transit] Who rides and why not: only when asked for by name
    // (it weighs every sampled commute's options).
    // [r4-leisure-places] Places to eat and go out: only when asked for.
    if sections.iter().any(|x| x == "leisure") {
        out.insert("leisure".into(), leisure(w));
    }
    if sections.iter().any(|x| x == "ridership") {
        out.insert("ridership".into(), serde_json::to_value(w.ridership_report(1500)).unwrap_or(Value::Null));
    }
    Ok(Value::Object(out))
}

/// [r4-leisure-places] Each place to eat or go out: how many are open,
/// their staff, visits yesterday by hour, how they pay; the bookings paid
/// out of town and the nightlife tax.
fn leisure(w: &World) -> Value {
    use nd_sim::tax::TaxCategory as T;
    let st = &w.economy.market.leisure;
    let mut kinds = Map::new();
    for k in nd_sim::leisure::ALL_NIGHTS_OUT {
        let (mut open, mut staff, mut jobs, mut takings) = (0u32, 0usize, 0u32, 0.0f32);
        for b in w.buildings.values() {
            let Some(z) = b.business.as_ref().filter(|z| z.kind == k && !z.closed) else { continue };
            if !b.operating() {
                continue;
            }
            open += 1;
            staff += b.workers.len();
            jobs += b.jobs as u32;
            takings += z.books.last().map_or(0.0, |d| d.revenue);
        }
        kinds.insert(
            k.key(),
            json!({
                "open": open, "staff": staff, "jobs": jobs, "takings_yesterday": r3(takings),
                "visits_yesterday": st.visits(k, true), "visits_today": st.visits(k, false),
                "by_hour_yesterday": st.by_hour(k, true).to_vec(),
                "cover": w.economy.cover_of(k).map(r3),
            }),
        );
    }
    let t = &w.economy.taxes;
    json!({
        "kinds": kinds,
        "bookings_yesterday": r3(st.bookings_yesterday), "bookings_today": r3(st.bookings_today),
        "meals_in_yesterday": st.meals_in_yesterday,
        "family_outings_yesterday": w.places.family_outings_yesterday,
        "nightlife_tax_yesterday": r3(t.yesterday_of(T::NightlifeLow) + t.yesterday_of(T::NightlifeMid) + t.yesterday_of(T::NightlifeHigh)),
        "eatery_tax_yesterday": r3(t.yesterday_of(T::Bakery)),
    })
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

/// [r4-leisure] The commercial bar of the Demand window and its sub-zones.
fn commercial_demand(w: &World) -> Value {
    let d = w.commercial_demand();
    let mut m: Map<String, Value> = nd_sim::commercial::CommercialSub::ALL.iter().map(|s| (s.key().to_string(), json!(round(d[s.index()])))).collect();
    m.insert("overall".into(), json!(round(w.commercial_demand_overall())));
    m.into()
}

/// [w1] The demand the sites going up already meet, per land use.
fn demand_met(w: &World) -> Value {
    let u = w.demand_under_way();
    LandUse::ALL.iter().map(|&lu| (lu_key(lu), json!(round(u[lu.index()])))).collect::<Map<_, _>>().into()
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
        // [w1] The demand the sites going up already meet (the Demand panel's light extension).
        "demand_met_by_sites": demand_met(w),
        // [r4-leisure] Commercial land by sub-zone (leisure is the old
        // entertainment, `demand_by_land_use.leisure`).
        "commercial_demand": commercial_demand(w),
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
        "Rolled into " /* old saves: "Rolled into Neon District" */, "Stepped off the train", "Moved into", "Got hired at", "Sat on a bench in", "Spent the evening at",
        "Moving up to", "Paid ", "was demolished", "was torn down and rebuilt",
    ];
    for k in KNOWN {
        if text.starts_with(k) || (k.starts_with(char::is_lowercase) && text.contains(k)) {
            return k.trim().to_string();
        }
    }
    let words: Vec<&str> = text.split_whitespace().take(4).collect();
    format!("{}…", words.join(" "))
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
            Activity::Riding { .. } if matches!(c.transit.map(|t| t.leg), Some(TransitLeg::Taxi)) => Some("taxi"),
            Activity::Riding { .. } => Some("bus"),
            Activity::OnTrain { .. } => Some("train"),
            Activity::OnMetro { .. } => Some("subway"),
            Activity::OnFerry { .. } => Some("ferry"),
            Activity::Waiting { .. } => Some(match c.transit.map(|t| t.leg) {
                Some(TransitLeg::Rail { .. }) => "train",
                Some(TransitLeg::Subway { .. }) => "subway",
                Some(TransitLeg::Air { .. }) => "air",
                Some(TransitLeg::Taxi) => "taxi",
                Some(TransitLeg::Ferry { .. }) => "ferry",
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
        "moves": { "displaced_by_rent_total": e.displaced_total, "emigrated_total": e.emigrated_total, "emigrated_by_reason": nd_sim::economy::LeaveReason::ALL.iter().map(|r| (r.key().to_string(), serde_json::json!(e.emigrated_by[r.index()]))).collect::<serde_json::Map<_, _>>(), "priced_out_immigrants_total": e.priced_out_total },
        "concerns": super::concerns::summary(w),
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
        "ledger_items": {
            "yesterday": ledger_items(w, &l.items_yesterday, &l.yesterday),
            "today_so_far": ledger_items(w, &l.items_today, &l.today),
            "note": "each flow broken into what made it (building, bus line, income class, land use or a named item); not_itemised is what the row holds beyond its items",
        },
        "tax_today": round(e.tax_today),
        // [tax] [tolls] The schedule by line and the toll booths.
        "taxes": super::taxes::schedule(w),
        "tolls": super::taxes::tolls_summary(w),
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

/// [power] The grid in one object, for `report.services.power` and
/// (without the per-plant hours) `status.power`: supply, demand, imports
/// and exports, the batteries, and a row per plant and battery with its
/// output, cost per MWh, CO2, smog and noise (`World::plant_rows`).
pub fn power_json(w: &World, with_hours: bool) -> Value {
    let p = &w.economy.power;
    let mut plants = Vec::new();
    let mut batteries = Vec::new();
    for r in w.plant_rows() {
        let Some(b) = w.buildings.get(r.id) else { continue };
        let log = w.economy.history.plant(r.id);
        // [plant-groups] A park's hours are its members' summed.
        let summed = w.plant_row_hours(&r);
        let series = |v: Option<&std::collections::VecDeque<f32>>| v.map_or(Value::Null, |l| json!(l.iter().map(|x| round(*x)).collect::<Vec<_>>()));
        match r.asset {
            nd_sim::GridAsset::Plant(k) => {
                let mut v = json!({
                    "id": key_str(r.id), "name": r.name, "kind": plant_name(k), "capacity_mw": round(r.capacity_mw), "available_mw": round(w.plant_output_mw(b)),
                    "output_mw": round(r.now_mw), "output_mw_est": round(r.now_mw), "mean_mw_24h": round(r.mean_mw), "upkeep_per_day": round(r.upkeep_per_day),
                    "cost_per_mwh": r.cost_per_mwh.map(round), "co2_t_per_day": round(r.co2_t_per_day), "co2_t_per_mwh": round(k.co2_t_per_mwh()),
                    "air_index": round(r.air), "noise_db": r.noise_db.map(round), "noise_db_running": round(k.noise_db()),
                    "pollution_radius_m": r.nuisance.0, "pollution_land_value_effect": round(r.nuisance.1),
                    "workers": b.workers.len(), "jobs": b.jobs, "derelict": r.derelict, "pending": r.pending, "pos": building_pos(w, b).map_or(Value::Null, v2),
                    "availability": round(w.plant_availability(b)), "area_m2": round(r.area_m2), "size_scale": round(w.plant_scale(b)),
                    "group": r.members.len() > 1, "members": r.members.iter().map(|m| key_str(*m)).collect::<Vec<_>>(), "lots": r.lots.iter().map(|l| key_str(*l)).collect::<Vec<_>>(),
                    "member_names": r.members.iter().filter_map(|m| w.buildings.get(*m).map(|x| x.name.clone())).collect::<Vec<_>>(),
                });
                if r.members.len() > 1 {
                    v["available_mw"] = json!(round(r.members.iter().filter_map(|m| w.buildings.get(*m)).map(|m| w.plant_output_mw(m)).sum::<f32>()));
                }
                if matches!(k, nd_sim::PowerPlantKind::Wind | nd_sim::PowerPlantKind::Solar) {
                    let s = w.plant_resource(b);
                    v["site"] = json!({ "wind_factor": round(s.wind), "sun_factor": round(s.sun) });
                }
                if with_hours {
                    v["output_mw_hours"] = series(summed.as_ref());
                    v["energy_mwh_days"] = series(log.map(|l| &l.days));
                }
                plants.push(v);
            }
            nd_sim::GridAsset::Battery(k) => {
                let mut v = json!({
                    "id": key_str(r.id), "name": r.name, "kind": k.key(), "capacity_mwh": round(r.capacity_mwh), "max_mw": round(r.capacity_mw),
                    "charge_mwh": round(r.charge_mwh), "level": round(if r.capacity_mwh > 0.0 { r.charge_mwh / r.capacity_mwh } else { 0.0 }),
                    "flow_mw": round(r.now_mw), "state": if r.now_mw > 0.05 { "discharging" } else if r.now_mw < -0.05 { "charging" } else { "idle" },
                    "delivered_mwh_24h": round(r.mean_mw * 24.0), "upkeep_per_day": round(r.upkeep_per_day), "cost_per_mwh": r.cost_per_mwh.map(round),
                    "noise_db": r.noise_db.map(round), "derelict": r.derelict, "pending": r.pending, "pos": building_pos(w, b).map_or(Value::Null, v2),
                });
                if with_hours {
                    v["flow_mw_hours"] = series(log.map(|l| &l.hours));
                    v["charge_mwh_hours"] = series(log.map(|l| &l.level));
                }
                batteries.push(v);
            }
        }
    }
    let level = if p.battery_capacity_mwh > 0.0 { p.battery_charge_mwh / p.battery_capacity_mwh } else { 0.0 };
    json!({
        "supply_mw": round(p.supply_mw), "demand_mw": round(p.demand_mw), "imported_mw": round(p.imported_mw), "served_share": round(p.served),
        "exported_mw": round(p.exported_mw), "curtailed_mw": round(p.curtailed_mw), "plants_count": p.plants,
        "unpowered_buildings": p.unpowered_buildings, "brownout_hours_today": p.brownout_hours_today, "upkeep_per_day": round(p.upkeep_today),
        "import_cost_today": round(p.import_cost_today), "export_income_today": round(p.export_income_today),
        "import": { "max_mw": nd_sim::power::GRID_IMPORT_MW, "price_per_mwh": round(p.import_price), "mean_price_per_mwh": nd_sim::power::GRID_IMPORT_PRICE },
        "export": { "mw": round(p.exported_mw), "max_mw": nd_sim::power::GRID_EXPORT_MW, "price_per_mwh": round(p.export_price), "mean_price_per_mwh": nd_sim::power::GRID_EXPORT_PRICE,
                    "income_today": round(p.export_income_today), "income_yesterday": round(w.economy.ledger.yesterday_of(Flow::GridExport)) },
        "battery_totals": { "count": p.batteries, "charge_mwh": round(p.battery_charge_mwh), "capacity_mwh": round(p.battery_capacity_mwh), "level": round(level),
                            "charging_mw": round(p.battery_in_mw), "discharging_mw": round(p.battery_out_mw),
                            "round_trip_efficiency": round(nd_sim::power::BATTERY_CHARGE_EFF * nd_sim::power::BATTERY_DISCHARGE_EFF),
                            "rule": format!("{:?}", p.battery_mode), "rule_text": p.battery_mode.label(), "cheap_price": round(p.price_bands.cheap), "dear_price": round(p.price_bands.dear),
                            "grid_charge_mw": round(p.battery_grid_charge_mw), "savings_today": round(p.battery_savings_today),
                            "savings_yesterday": round(w.economy.history.days.back().map_or(0.0, |d| d.battery_savings)) },
        "weather": super::land::weather_json(w),
        "grid": super::land::grid_json(w),
        "plants": plants,
        "batteries": batteries,
    })
}

fn services(w: &World) -> Value {
    let e = &w.economy;
    let mut route_waiting: BTreeMap<String, u32> = BTreeMap::new();
    let mut station_waiting: BTreeMap<BuildingId, u32> = BTreeMap::new();
    for c in w.citizens.values() {
        if matches!(c.activity, Activity::Waiting { .. }) {
            match c.transit.map(|t| t.leg) {
                Some(TransitLeg::Bus { route, .. }) => *route_waiting.entry(key_str(route)).or_default() += 1,
                Some(TransitLeg::Rail { board, .. }) => *station_waiting.entry(board).or_default() += 1,
                Some(TransitLeg::Air { .. }) | Some(TransitLeg::Subway { .. }) | Some(TransitLeg::Taxi) | Some(TransitLeg::Ferry { .. }) | None => {}
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
                "inbound_units": round(w.rail.orders_for(id).1), "aboard_units": round(w.rail.aboard_for(id)), "outbound_units": round(b.rail_outbound), "pos": building_pos(w, b).map_or(Value::Null, v2),
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
    let mut out = json!({
        "emergency": emergency(w),
        "power": power_json(w, true),
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
        "subway": crate::remote::subway::report(w),
        "air": crate::remote::view::air_report(w),
        "taxis": super::taxis::taxi_report(w),
        "cargo_hubs": super::taxis::hub_report(w),
        "parking": {
            "capacity": e.parking_capacity, "used": e.parking_used, "occupancy": share(e.parking_used as usize, e.parking_capacity as usize), "parking_lots": e.parking_lots,
            "parked_on_street_today": e.parking_street_today, "fees_today": round(e.parking_fees_today), "last_shortage_at": e.parking_shortage_at.map(v2),
            "enforcement": parking_enforcement(w),
        },
        "traffic": {
            "vehicles": w.traffic.cars.len(), "cars": cars, "trucks": trucks, "buses": buses, "mean_speed_kmh_moving": if moving > 0 { round(speed_sum / moving as f32 * 3.6) } else { 0.0 },
            "stopped": stopped, "stuck": stuck, "stuck_rule": format!("in traffic and standing still for {STUCK_STALL_S} s or more (not a bus at a stop, not at a red light)"),
            "waiting_at_kerb": kerb, "gridlocks": w.traffic.gridlocks().len(),
            "lanes": w.traffic.net.lanes.len(), "intersections": w.traffic.net.intersections.len(), "signals": w.traffic.net.intersections.iter().filter(|i| i.signalized).count(),
            "walk_trips_today": e.walk_trips_today,
            "pedestrians": {
                "waiting_at_crossings": w.peds.waiting_now, "jaywalkers_today": w.peds.jaywalkers_today, "near_misses_today": w.peds.near_misses_today, "ped_collisions_today": w.peds.collisions_today,
                "walking_now": w.citizens.values().filter(|c| c.mode == nd_sim::citizens::TravelMode::Walk && matches!(c.activity, nd_sim::Activity::Traveling { .. })).count(),
                "walk_lights_on_now": w.traffic.peds.walk.iter().flatten().filter(|&&x| x).count(),
                "jaywalks_total": w.peds.jaywalks_total, "near_misses_total": w.peds.near_misses_total, "ped_collisions_total": w.peds.collisions_total,
                // [cyclists] Riders held at a red or for a walker, and lights run.
                "cyclists_waiting_now": w.peds.riders_waiting_now, "cyclist_red_lights_run_today": w.peds.red_lights_run_today, "cyclist_red_lights_run_total": w.peds.red_lights_run_total,
                // [benches] Bench neighbours who got talking; visitors who found a park full.
                "bench_meetings_today": w.seats.met_today, "bench_meetings_total": w.seats.met_total, "parks_full_today": w.seats.full_today, "parks_full_total": w.seats.full_total,
            },
            "left_turn_wait_mean_s": round(w.traffic.left_wait_stats().0), "left_turn_wait_max_s": round(w.traffic.left_wait_stats().1),
            "left_turns_measured": w.traffic.left_wait_stats().2, "protected_left_arrows_now": w.traffic.net.intersections.iter().filter(|i| i.timer < i.arrow).count(),
            "passing_left_turners_now": w.traffic.cars.values().filter(|c| c.passing).count(), "lane_use_set_by_player": w.traffic.lane_use.len(),
            "models": nd_sim::bots::fleet_counts(w),
            "private_cars": car_life(w),
        },
        "bots": {
            "active": w.bots.bots.len(), "delivering": w.bots.bots.values().filter(|b| !b.returning).count(), "delivered_today": w.bots.delivered_today,
            "vans_on_road": nd_sim::Bots::vans(w), "van_orders_today": w.bots.van_orders_today,
            "rule": format!("shops deliver to homes within {} m; by pavement robot where shop and home both have land value >= {}, else by van", nd_sim::bots::DELIVERY_RANGE, nd_sim::bots::BOT_LAND_VALUE),
        },
    });
    // [driving] Crashes, drink and takeovers, in the traffic section.
    if let (Some(t), Value::Object(d)) = (out.get_mut("traffic").and_then(|t| t.as_object_mut()), driving_json(w)) {
        t.extend(d);
    }
    // [walk-crossings] Walkers who planned again on finding their crossing
    // gone, jaywalkers who gave up on a gap, the hurt waiting in the street.
    if let Some(p) = out.pointer_mut("/traffic/pedestrians").and_then(|p| p.as_object_mut()) {
        p.insert("replans_total".into(), json!(w.peds.replans_total));
        p.insert("jay_give_ups_total".into(), json!(w.peds.jay_give_ups_total));
        p.insert("hurt_in_the_street_now".into(), json!(w.down_on_street().len()));
    }
    // [bike-racks] Bikes ridden on buses' front racks, cyclists a full rack
    // left at the stop, and bikes on racks now.
    if let Some(t) = out.get_mut("transit").and_then(|t| t.as_object_mut()) {
        let on_racks: usize = w.traffic.cars.values().filter_map(|c| c.bus.as_ref()).map(|b| b.passengers.iter().filter(|p| w.citizens.get(**p).and_then(|c| c.transit).is_some_and(|t| t.bike)).count()).sum();
        t.insert("bike_rack_rides_total".into(), json!(w.transit.bike_rack_rides_total));
        t.insert("bike_rack_full_total".into(), json!(w.transit.bike_rack_full_total));
        t.insert("bikes_on_racks_now".into(), json!(on_racks));
    }
    out
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
                "units_on_patrol": units.iter().filter(|u| u.state == nd_sim::services::UnitState::Patrol).count(),
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
    // Call-to-arrival per kind: today's answered calls, else yesterday's.
    let response = |k: usize| {
        let (v, when) = if st.response_min_today[k].is_empty() { (&st.response_min_yesterday[k], "yesterday") } else { (&st.response_min_today[k], "today") };
        match nd_sim::services::response_summary(v, 30) {
            Some((median, p90, within)) => json!({ "answered": v.len(), "median_min": median, "p90_min": p90, "within_30_min_pct": round(within * 100.0), "of": when }),
            None => json!({ "answered": 0 }),
        }
    };
    let per_kind = |f: &dyn Fn(usize) -> Value| json!({ "fire": f(0), "crime": f(1), "medical": f(2) });
    let tick = w.clock.tick;
    json!({
        "coverage_pct_of_homes": { "police": cov(0), "fire": cov(1), "health": cov(2), "school": cov(3) },
        "incidents_today": { "fire": st.incidents_today[0], "crime": st.incidents_today[1], "medical": st.incidents_today[2] },
        "resolved_today": st.resolved_today.iter().sum::<u32>(), "unanswered_today": st.unanswered_today, "arrests_today": st.arrests_today,
        "avg_response_min": round(st.avg_response()), "avg_response_min_yesterday": round(st.avg_response_yesterday),
        "units": st.units, "units_busy": st.units_busy, "fires_burning": st.fires_burning, "burned_down_total": st.burned_down_total,
        "waiting_for_ambulance": st.waiting_for_ambulance, "in_hospital": st.in_hospital,
        "dispatch": { "dispatcher_on_duty": dispatcher.map(key_str), "minutes_to_dispatch": delay },
        "response": per_kind(&response),
        "queued": per_kind(&|k| json!(st.queued[k])), "queue_max_today": per_kind(&|k| json!(st.queue_max_today[k])),
        "all_units_busy_min_today": per_kind(&|k| json!(st.saturated_min_today[k])),
        "all_units_busy_now_min": per_kind(&|k| st.saturated_since[k].map_or(Value::Null, |t| json!(tick.saturating_sub(t)))),
        "unanswered_why_today": per_kind(&|k| { let u = &st.unanswered_why_today[k]; json!({ "all_units_busy": u.all_units_busy, "no_route": u.no_route, "no_station": u.no_station }) }),
        "mutual_aid_today": st.mutual_aid_today, "unreachable_today": st.unreachable_today, "diverted_today": st.diverted_today, "units_on_patrol": st.on_patrol,
        "car_chases": { "now": s.chases.len(), "today": st.chases_today, "tagged_today": st.chases_tagged_today, "lost_today": st.chases_lost_today, "total": st.chases_total, "tagged_total": st.chases_tagged_total },
        "stadium_crowd_today": st.stadium_crowd_today, "tickets_today": round(st.tickets_today), "stadium_events_tonight": s.events_today.len(),
        "upkeep_per_day": round(st.upkeep_today), "education_wage_bonus_today": round(st.education_bonus_today), "pupils_today": st.pupils_today,
        "jailed": st.jailed, "held_at_stations": st.held_at_stations, "released_early_total": st.released_early_total,
        "out_of_town_calls_today": st.out_of_town_calls_today, "out_of_town_fees_today": round(st.out_of_town_fees_today),
        "hospital_bills_paid_today": round(st.bills_paid_today), "hospital_bills_covered_by_city_today": round(st.bills_covered_today),
        "stations": stations, "active_incidents": active, "recent": recent,
    })
}

/// One alert per emergency service whose every unit was busy for
/// `SATURATED_ALERT_MIN` or more today (or yesterday, just after midnight),
/// naming the bottleneck: too few crews for the vehicles, or too few
/// vehicles for the calls.
fn saturated_alerts(w: &World) -> Vec<Alert> {
    use nd_sim::services::{IncidentKind, SATURATED_ALERT_MIN};
    let st = &w.services.stats;
    let mut out = Vec::new();
    for kind in IncidentKind::ALL {
        let k = kind.index();
        let busy_min = st.saturated_min_today[k].max(if w.clock.minute_of_day() < 180 { st.saturated_min_yesterday[k] } else { 0 });
        if busy_min < SATURATED_ALERT_MIN {
            continue;
        }
        let service = kind.service();
        let stations: Vec<_> = w.buildings.iter().filter(|(_, b)| b.service.is_some_and(|s| s.kind == service)).collect();
        let units: usize = stations.iter().map(|(id, _)| w.services.units.values().filter(|u| u.station == *id).count()).sum();
        let crewed: usize = stations.iter().map(|(id, _)| w.crewed_units(*id)).sum();
        let label = kind.unit().label();
        let span = if busy_min >= 90 { format!("{:.1} h", busy_min as f32 / 60.0) } else { format!("{busy_min} min") };
        let mut a = Alert::new(
            "emergency_units_saturated",
            if busy_min >= 180 { 3 } else { 2 },
            "every unit of the kind was out on calls while more came in: calls wait in the queue (most urgent first) and go to the next town as mutual aid after a while; another station or a higher tier adds units, and a fully staffed station crews all of them",
        );
        for (id, b) in &stations {
            a.add(key_str(*id), building_pos(w, b));
        }
        let fix = if crewed < units {
            format!("only {crewed} of {units} have a crew: let the {} hire (or raise the budget for staff), or build another", service.label())
        } else {
            format!("build another {} or upgrade the tier", service.label())
        };
        let waited = st.queue_max_today[k];
        let gave_up = st.unanswered_why_today[k].all_units_busy;
        a.message = format!("all {crewed} {label}s busy for {span} today (up to {waited} calls waiting at once, {gave_up} unanswered calls today because every unit was out): {fix}");
        out.push(a);
    }
    let no_route: u32 = st.unanswered_why_today.iter().map(|u| u.no_route).sum::<u32>() + st.unreachable_today;
    if no_route > 0 {
        let mut a = Alert::new("emergency_calls_unreachable", 2, "a free unit had no road to the call (a highway with no ramp to the city's streets, a cut-off block): calls there go to the next town, slowly and for a fee; connect the roads");
        a.count = no_route as usize;
        a.message = format!("{} calls today had no road from any free unit of ours: handed to the next town", a.count);
        out.push(a);
    }
    out
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

// ── alerts ──────────────────────────────────────────────────────────────────

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
    pub(super) fn new(kind: &'static str, severity: u8, hint: &'static str) -> Self {
        Self { kind, severity, count: 0, message: String::new(), hint, points: Vec::new(), examples: Vec::new() }
    }

    pub(super) fn add(&mut self, id: String, at: Option<SVec2>) {
        self.count += 1;
        if let Some(p) = at {
            self.points.push(p);
        }
        if self.examples.len() < ALERT_EXAMPLES {
            self.examples.push(id);
        }
    }

    /// As `add`, for a slotmap id: the id is spelled out only while
    /// examples are still wanted (the scan sees every citizen and building
    /// and keeps five ids per alert).
    fn add_id<K: slotmap::Key>(&mut self, id: K, at: Option<SVec2>) {
        self.count += 1;
        if let Some(p) = at {
            self.points.push(p);
        }
        if self.examples.len() < ALERT_EXAMPLES {
            self.examples.push(key_str(id));
        }
    }

    /// [pt-ux] The centroid of the cases (for the Alerts window's "go").
    pub fn at(&self) -> Option<SVec2> {
        (!self.points.is_empty()).then(|| self.points.iter().fold(SVec2::ZERO, |a, &p| a + p) / self.points.len() as f32)
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
    let mut rail_broken = Alert::new("rail_facility_not_connected", 3, "a station or yard needs its rail stub joined to the track and its street stub joined to a road (draw one to the marked end; a station on the track alone sees trains call but serves nobody)");
    // [dock-access] Docks off the roads (ferries still sail; nothing lands).
    let mut dock_off = Alert::new("dock_not_connected", 2, "not connected to the road network: draw a road to the marked end of the dock's access road (inspect road_access.stub_ends)");
    let mut route_broken = Alert::new("bus_route_broken", 2, "a leg of the line has no road between its stops, or it has fewer than two stops; move or remove stops");
    let mut sites = Alert::new("construction_waiting_for_materials", 1, "sites wait for parts deliveries each quarter; many waiting means parts are short (economy.resources parts)");
    // [r2-traffic] Sites no truck can get to (K6).
    let mut sites_cut = Alert::new("construction_unreachable", 2, "building sites no delivery truck or builder can drive to: join their street to the roads that lead out of town (inspect building under_construction.blocked)");
    let mut stuck = Alert::new("stuck_vehicles", 2, "vehicles standing still for minutes: a gridlock ring (listed with its junctions) or a street over capacity; add lanes, another route or transit there");
    let mut derelict = Alert::new("derelict_buildings", 1, "condition below 0.3; they are redeveloped or demolished in time");
    let mut vacant_demand = Alert::new("vacant_lots_with_high_demand", 1, "demand is high but lots stand empty: they may lack road access or the area may be unattractive");
    for (id, c) in w.citizens.iter() {
        if c.satiety < HUNGRY_SATIETY {
            hungry.add_id(id, Some(c.pos));
        }
        if c.hungry_days >= STARVING_DAYS {
            starving.add_id(id, Some(c.pos));
        }
        if c.home.is_none() {
            unhoused.add_id(id, Some(c.pos));
        }
        if let Some(home) = c.home.and_then(|h| w.buildings.get(h)) {
            if c.rent_stress_days > 0 || w.rent_share_of(id).unwrap_or(home.rent / c.income.max(0.01)) > RENT_STRESS_SHARE {
                rent.add_id(id, building_pos(w, home));
            }
        }
    }
    for (id, b) in w.buildings.iter() {
        let at = building_pos(w, b);
        if b.construction.is_none() {
            if let Some(z) = &b.business {
                let food = matches!(z.kind, BusinessKind::Grocery | BusinessKind::Bakery | BusinessKind::Cafe);
                if z.closed {
                    closed.add_id(id, at);
                } else {
                    if food && b.staffing() < FOOD_SELLER_MIN_STAFFING {
                        food_staff.add_id(id, at);
                    }
                    if food && z.kind.recipe().outputs.iter().all(|(r, _)| z.stock_of(*r) < FOOD_STOCK_EMPTY) {
                        food_empty.add_id(id, at);
                    }
                    if z.broke_days > 0 {
                        broke.add_id(id, at);
                    }
                }
            }
            if b.households.len() > b.homes as usize && b.homes > 0 {
                crowded.add_id(id, at);
            }
            if b.is_derelict() {
                derelict.add_id(id, at);
            }
        } else if b.demolition.is_none() && b.construction.as_ref().is_some_and(|c| c.blocked.is_some()) {
            sites_cut.add_id(id, at);
        } else if b.demolition.is_none() && b.construction.as_ref().is_some_and(|c| c.waiting_for_materials()) {
            sites.add_id(id, at);
        }
        if b.powered < UNPOWERED_BELOW && nd_sim::power::demand_mw(b) > 0.0 {
            unpowered.add_id(id, at);
        }
        if let Some(l) = &b.rail {
            if !l.works() {
                rail_broken.add_id(id, at);
            }
        }
    }
    // [sweep-sim] A ferry terminal people can walk to is not off the network.
    for p in w.ports.ports.values().filter(|p| !p.road_connected && !p.walk_only()) {
        if let Some(b) = p.building {
            dock_off.add_id(b, Some(p.center));
        }
    }
    let mut rail_missing = (0, 0);
    for b in w.buildings.values() {
        if let Some(l) = &b.rail {
            rail_missing.0 += !l.rail_connected as u32;
            rail_missing.1 += !l.road_connected as u32;
        }
    }
    // [pt-transit] Lines that run but get no buses (no depot, none that
    // reaches them), and more buses wanted than the depots hold.
    let mut no_buses = Alert::new("bus_line_gets_no_buses", 2, "the line can run but no bus can be sent to it: build a depot, or one with a road to its first stop (bus problems: the line's own list)");
    for (id, r) in w.transit.routes.iter() {
        if !r.runnable() {
            route_broken.add_id(id, r.stops.first().map(|s| s.pos));
        } else if w.line_problems(id).iter().any(|p| matches!(p, nd_sim::LineProblem::NoDepot | nd_sim::LineProblem::DepotUnreachable)) {
            no_buses.add_id(id, r.stops.first().map(|s| s.pos));
        }
    }
    no_buses.message = format!("{} bus lines get no buses", no_buses.count);
    let (fleet_wanted, fleet_room) = w.fleet_balance();
    let mut fleet_short = Alert::new("bus_fleet_short", 2, "the lines want more buses than the depots hold: build another depot or run fewer buses");
    if fleet_wanted > fleet_room && fleet_room > 0 {
        fleet_short.count = 1;
        fleet_short.message = format!("the lines want {fleet_wanted} buses; the depots hold {fleet_room}");
    }
    let gridlocks = w.traffic.gridlocks();
    let mut kerb = 0;
    for (id, c) in w.traffic.cars.iter() {
        if is_stuck(c) {
            stuck.add_id(id, Some(c.pos));
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
            vacant_demand.add_id(id, Some(l.center));
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
    dock_off.message = format!("{} docks are not connected to the road network", dock_off.count);
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
                    a.add_id(*id, building_pos(w, b));
                }
            }
            a.message = format!("{} of {} residential buildings are outside {} coverage", a.count, homes.len(), ["police", "fire", "health", "school"][k]);
            uncovered.push(a);
        }
    }
    let mut fires = Alert::new("fires_burning", 3, "fire engines on the way show in services.emergency.active_incidents; with no fire station in reach they burn until the building is gone");
    for i in w.services.active().filter(|i| i.kind == nd_sim::IncidentKind::Fire) {
        fires.add_id(i.building, Some(i.pos));
    }
    fires.message = format!("{} buildings are on fire", fires.count);
    let st = &w.services.stats;
    if st.avg_response() > 45.0 {
        let mut a = Alert::new("slow_emergency_response", 2, "a dispatcher on shift (911 dispatch centre with staff) and stations closer to the calls cut response times");
        a.count = 1;
        a.message = format!("emergency units take {:.0} minutes on average from the 911 call to arrival", st.avg_response());
        uncovered.push(a);
    }
    uncovered.extend(saturated_alerts(w));
    if w.services.incidents.iter().any(|i| i.reported.is_some()) && w.dispatcher().0.is_none() {
        let mut a = Alert::new("no_dispatcher", 2, "911 calls go to the slow regional line: place a 911 dispatch centre and let it hire");
        a.count = 1;
        a.message = format!("no dispatcher on duty: jobs go out {} minutes after the call", w.dispatcher().1);
        uncovered.push(a);
    }
    let mut out = vec![fires, hungry, starving, unhoused, rent, food_staff, food_empty, closed, broke, unpowered, crowded, rail_broken, dock_off, route_broken, no_buses, fleet_short, sites_cut, sites, stuck, derelict, vacant_demand];
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
    // [society] hot spots, no jail, camps, overflowing bins.
    out.extend(super::society::alerts(w));
    // [concerns] What people complain about most this week.
    out.extend(super::concerns::alerts(w));
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
                            "note": "0..1 per cell; oil wells, refineries, chemical works, mines, smelters, factory farms and coal ash raise it; it spreads a little and fades about 1% a day (faster under parks; `remediate` cleans it for money); it costs land value, spoils crops and wears residents down" },
        // [dirty-industry] The dirty plants and the neighbours they hurt.
        "dirty": w.dirty.iter().map(|s| json!({ "building": super::view::key_str(s.building), "name": w.buildings.get(s.building).map(|b| b.name.clone()), "reads_as": s.as_kind.label(), "level": r3(s.level), "reach_m": round(s.reach) })).collect::<Vec<_>>(),
        "suffering_neighbours": w.buildings.keys().filter(|id| w.building_exposure(*id) > 0.05).count(),
        "fleet": { "truck_and_bus_lots": fleet_lots, "bays": fleet_bays, "trucks_parked": fleet_parked, "trucks_on_road": trucks, "returning_empty": returning },
        "biases": { "count": w.industry_bias.len(), "newest_first": biases },
        "plant_land_value": plants,
        "land_value_note": "plants and heavy industry are nuisances: homes feel 125% of them, shops 85%, industry 30%; working industry lifts land below 0.35 by up to 0.1",
    })
}

/// [driving] Crashes, drunk drivers and street takeovers for `report.traffic`.
fn driving_json(w: &World) -> Value {
    json!({
            "crashes_today": w.driving.crashes_today, "injuries_today": w.driving.injuries_today, "drunk_drivers_now": w.drunk_drivers_now(),
            "drunk_trips_today": w.driving.drunk_trips_today, "angry_drivers_now": w.angry_drivers_now(), "road_rage_today": w.driving.road_rage_today, "red_lights_run_today": w.driving.red_runs_today, "crashes_total": w.driving.crashes_total,
            "wrecks_on_road": w.driving.wrecks.len(), "tow_trucks_now": w.driving.tows.len(), "towed_total": w.driving.towed_total,
            "drivers_by_style": ({
                let mut m = serde_json::Map::new();
                for c in w.traffic.cars.values() {
                    let k = c.drive.profile.label();
                    let n = m.get(k).and_then(|v| v.as_u64()).unwrap_or(0);
                    m.insert(k.to_string(), json!(n + 1));
                }
                Value::Object(m)
            }),
            "recent_crashes": w.driving.recent.iter().rev().take(5).map(|r| json!({ "id": r.id, "tick": r.tick, "pos": [r.pos.x, r.pos.y], "place": r.place, "cause": r.cause.label(), "severity": round(r.severity), "injured": r.injured.len(), "drunk": r.drunk, "incident": r.incident })).collect::<Vec<_>>(),
            "takeovers": {
                "active": w.takeover().is_some(),
                "location": w.takeover().map(|t| json!([t.centre.x, t.centre.y])),
                "place": w.takeover().map(|t| t.place.clone()),
                "stage": w.takeover().map(|t| t.stage.label()),
                "participants": w.takeover().map_or(0, |t| t.participants()),
                "residents": w.takeover().map_or(0, |t| t.residents.len()),
                "outsiders": w.takeover().map_or(0, |t| t.outsiders),
                "in_show": w.takeover().map_or(0, |t| t.cars.iter().filter(|&&c| w.traffic.cars.get(c).is_some_and(|c| c.in_stunt())).count()),
                "crowd": w.takeover_crowd_now(),
                "incident": w.takeover().and_then(|t| t.incident),
                "planned_tick": w.driving.takeovers.planned,
                "total": w.driving.takeovers.total,
                "last": w.driving.takeovers.history.last().map(|r| json!({ "id": r.id, "place": r.place, "participants": r.participants, "crowd": r.crowd, "arrests": r.arrests, "police": r.police, "started": r.started, "ended": r.ended })),
            },
            // [events] Street races and raves tonight, and the state of the roads.
            "street_events": {
                "race": w.race().map(|r| json!({ "id": r.id, "stage": r.stage.label(), "meet": r.meet.name, "finish": r.finish.name, "racers": r.cars.len(), "incident": r.incident })),
                "rave": w.rave().map(|r| json!({ "id": r.id, "place": r.place, "inside": r.inside.len(), "queue": r.queue.len(), "complaints": r.complaints, "incident": r.incident })),
                "races_total": w.street_events.races_total, "raves_total": w.street_events.raves_total, "busts_total": w.street_events.busts_total,
                "last_news": w.street_events.news.last().map(|n| n.headline.clone()),
            },
            "road_condition": super::nights::report_json(w),
    })
}

/// [carlife] Private cars and how people travel: where the cars are,
/// purchases and sales, the parking search, trip shares by mode and the
/// pain each mode has caused lately.
pub fn car_life(w: &World) -> Value {
    use nd_sim::citizens::TripMode;
    use nd_sim::ParkKind;
    let v = &w.vehicles;
    let st = &v.stats;
    let (parked, driving, searching) = v.counts(&w.traffic);
    let mut by_kind: BTreeMap<&str, u32> = BTreeMap::new();
    for car in v.cars.values() {
        if let Some(s) = car.spot() {
            *by_kind.entry(s.kind.label()).or_default() += 1;
        }
    }
    let (kerb_total, kerb_used) = w.kerb_occupancy();
    let shares = |t: &[u32]| -> Value {
        let n: u32 = TripMode::ALL.iter().map(|m| t[m.index()]).sum();
        let m: Map<String, Value> = TripMode::ALL.iter().map(|m| (m.key().to_string(), json!({ "trips": t[m.index()], "share": share(t[m.index()] as usize, n as usize) }))).collect();
        json!({ "trips": n, "modes": m })
    };
    // Pain among those who used the mode lately (bad half-hours per trip).
    let mut pain: Map<String, Value> = Map::new();
    for m in TripMode::ALL {
        let users: Vec<f32> = w.citizens.values().filter(|c| c.commute.trips[m.index()] > 0.3).map(|c| c.commute.pain_of(m)).collect();
        let mean = if users.is_empty() { 0.0 } else { users.iter().sum::<f32>() / users.len() as f32 };
        pain.insert(m.key().to_string(), json!({ "users": users.len(), "mean": r3(mean) }));
    }
    let drivers: Vec<f32> = w.citizens.values().filter(|c| c.car.is_some()).map(|c| c.commute.parking_pain).collect();
    let parking_pain = if drivers.is_empty() { 0.0 } else { drivers.iter().sum::<f32>() / drivers.len() as f32 };
    let _ = ParkKind::Kerb;
    json!({
        "cars": v.cars.len(), "cars_parked": parked, "cars_driving": driving, "cars_searching": searching,
        "parked_by_place": by_kind, "kerb_slots": { "total": kerb_total, "used": kerb_used, "occupancy": share(kerb_used as usize, kerb_total as usize) },
        "created_total": st.created_total, "destroyed_total": st.destroyed_total, "events_total": st.events_total,
        "bought_today": st.bought_today, "sold_today": st.sold_today, "bikes_bought_today": st.bikes_bought_today, "bikes_sold_today": st.bikes_sold_today,
        "circled_today": st.circled_today, "search_minutes_today": round(st.search_seconds_today / w.config.traffic_seconds_per_tick),
        "parked_today": { "home_or_building": st.parked_today[0], "lot": st.parked_today[1], "kerb": st.parked_today[2], "no_space": st.parked_today[3] },
        "mode_shares_today": shares(&st.trips_today), "mode_shares_yesterday": shares(&st.trips_yesterday),
        "commute_pain": pain, "parking_pain_mean_of_owners": r3(parking_pain),
        "fleet_parked_at_businesses": v.fleet_parked.values().map(|n| *n as u32).sum::<u32>(), "fleet_bought_total": st.fleet_bought_total, "fleet_returned_today": st.fleet_returned_today,
        "rule": "cars come only with arriving households or purchases and leave only when sold, scrapped or driven out of town; trips are never timed out",
        // [passengers] People riding along in someone else's car.
        "passengers": {
            "riding_now": w.citizens.values().filter(|c| matches!(c.activity, nd_sim::Activity::Passenger { seated: true, .. })).count(),
            "walking_to_a_car": w.citizens.values().filter(|c| matches!(c.activity, nd_sim::Activity::Passenger { seated: false, .. })).count(),
            "cars_with_passengers": w.traffic.cars.values().filter(|c| !c.occupants.is_empty()).count(),
            "today": w.rides.stats.passengers_today, "outings_today": w.rides.stats.outings_today, "carpools_today": w.rides.stats.carpools_today,
            "school_runs_today": w.rides.stats.school_runs_today, "lifts_today": w.rides.stats.lifts_today,
            "yesterday": w.rides.stats.passengers_yesterday, "total": w.rides.stats.passengers_total, "fullest_car": w.rides.stats.fullest_car,
            "drop_offs_pending": w.rides.onward.len(),
        },
    })
}

/// [parking] Kerb rules, the officers, fines, tows, the pound and the lots
/// the city runs.
pub fn parking_enforcement(w: &World) -> Value {
    use nd_sim::enforcement::*;
    let s = &w.enforcement.stats;
    let offices: Vec<Value> = w
        .enforcement_offices()
        .into_iter()
        .filter_map(|id| w.buildings.get(id).map(|b| (id, b)))
        .map(|(id, b)| {
            let tier = b.service.map(|s| s.tier).unwrap_or(Tier::Mid);
            json!({
                "id": key_str(id), "name": b.name, "tier": tier_key(tier), "officers": b.workers.len(), "jobs": b.jobs,
                "patrol_cars_now": w.enforcement.patrols.iter().filter(|p| p.office == id).count(), "patrol_cars_wanted": w.enforcement_cars(id),
                "radius_m": nd_sim::ServiceKind::ParkingEnforcement.radius(tier), "pos": building_pos(w, b).map_or(Value::Null, v2),
            })
        })
        .collect();
    // Share of legal kerb slots within an office's reach.
    let reach: Vec<(SVec2, f32)> = w.enforcement_offices().into_iter().filter_map(|id| w.buildings.get(id).and_then(|b| Some((building_pos(w, b)?, b.service?.kind.radius(b.service?.tier))))).collect();
    let (mut slots, mut covered) = (0u32, 0u32);
    for (sid, seg) in w.roads.alive_segments() {
        let n = w.kerb_slot_count(sid) as u32 * ((seg.lanes_fwd > 0) as u32 + (seg.lanes_bwd > 0) as u32);
        if n == 0 {
            continue;
        }
        let (a, b) = w.roads.segment_points(sid);
        let mid = a.lerp(b, 0.5);
        slots += n;
        if reach.iter().any(|(p, r)| p.distance(mid) <= *r) {
            covered += n;
        }
    }
    let mut kinds: BTreeMap<&str, u32> = BTreeMap::new();
    let (mut impounded, mut in_town, mut over_limit, mut ticketed) = (0, 0, 0, 0);
    let tick = w.clock.tick;
    for (vid, car) in w.vehicles.cars.iter() {
        let Some(sp) = car.spot() else { continue };
        *kinds.entry(sp.kind.label()).or_default() += 1;
        if sp.kind == nd_sim::ParkKind::Pound {
            impounded += 1;
            in_town += sp.building.is_some() as u32;
        }
        if sp.kind.on_street() {
            over_limit += w.kerb_limit(vid).is_some_and(|l| tick >= sp.since + l) as u32;
            ticketed += (sp.ticketed > 0) as u32;
        }
    }
    let companies: Vec<Value> = w
        .buildings
        .iter()
        .filter(|(_, b)| b.business.as_ref().is_some_and(|z| z.kind == BusinessKind::TowYard))
        .map(|(id, b)| json!({ "id": key_str(id), "name": b.name, "staff": b.workers.len(), "trucks": w.tow_trucks(id), "in_pound": w.vehicles.cars.values().filter(|c| c.spot().is_some_and(|s| s.kind == nd_sim::ParkKind::Pound && s.building == Some(id))).count(), "money": round(b.business.as_ref().map_or(0.0, |z| z.money)) }))
        .collect();
    let city_lots: Vec<Value> = w
        .vehicles
        .city_lots
        .iter()
        .filter_map(|(id, l)| w.buildings.get(*id).map(|b| json!({ "id": key_str(*id), "name": b.name, "size": l.size.key(), "capacity": b.parking_capacity, "used": b.parking_used, "occupancy": share(b.parking_used as usize, b.parking_capacity as usize), "price": round(l.price) })))
        .collect();
    json!({
        "rule": format!("kerb parking {KERB_LIMIT} min on every street (per-street overrides: {}); residents within {PERMIT_RADIUS:.0} m of home exempt; a ticket ({FINE:.0}) over the limit, a clamp ({CLAMP_FINE:.0}) {GRACE} min later, towed if unpaid after {} h, the tow truck at once for illegal parking; release fee {TOW_FEE:.0} in town, {OUT_OF_TOWN_TOW_FEE:.0} out of town, plus {STORAGE_PER_DAY:.0} a day; auction after {} days", CLAMP_LIMIT / 60, w.vehicles.kerb_rules.len(), AUCTION_AFTER / 1440),
        "offices": offices, "patrol_cars_now": w.enforcement.patrols.len(),
        "coverage": { "kerb_slots": slots, "within_patrol_reach": covered, "share": share(covered as usize, slots as usize) },
        "tickets_today": s.tickets_today, "fines_today": round(s.fines_today), "tickets_total": s.tickets_total, "fines_total": round(s.fines_total),
        "on_street_now": { "over_limit": over_limit, "ticketed": ticketed },
        "tows_today": s.tows_today, "tows_out_of_town_today": s.tows_out_of_town_today, "tows_total": s.tows_total, "tows_out_of_town_total": s.tows_out_of_town_total,
        "boots_today": s.boots_today, "boots_total": s.boots_total,
        "clamps": { "today": s.clamps_today, "total": s.clamps_total, "paid_today": s.clamps_paid_today, "paid_total": s.clamps_paid_total, "fines_today": round(s.clamp_fines_today), "towed_unpaid_total": s.clamps_towed_total, "clamped_now": w.enforcement.jobs.iter().filter(|j| j.reason == nd_sim::enforcement::TowReason::Unpaid && matches!(j.state, nd_sim::enforcement::TowState::Waiting { .. })).count() },
        "tow_jobs_open": w.enforcement.jobs.len(), "tow_trucks_on_road": w.enforcement.jobs.iter().filter(|j| j.truck.is_some()).count(), "wrecks_towed_total": s.wrecks_towed_total,
        "pound": { "cars": impounded, "in_town": in_town, "out_of_town": impounded - in_town, "released_today": s.released_today, "release_fees_today": round(s.release_fees_today), "released_total": s.released_total, "auctioned_today": s.auctioned_today, "auctioned_total": s.auctioned_total },
        "tow_companies": companies,
        "city_lots": city_lots, "city_lot_fees_today": round(s.city_lot_fees_today),
        "parked_by_place": kinds,
    })
}

// ── itemised ledger and the sim's history ──────────────────────────────────

/// One flow's items, biggest first: `{flow: {items: [...], not_itemised}}`.
fn ledger_items(w: &World, items: &[nd_sim::economy::ItemEntry], row: &[f32; nd_sim::economy::FLOWS]) -> Value {
    use nd_sim::economy::{Item, Ledger};
    let mut m = Map::new();
    for f in Flow::ALL {
        let (lines, rest) = Ledger::items_of(items, row, f);
        if lines.is_empty() && rest.abs() < 0.5 {
            continue;
        }
        let rows: Vec<Value> = lines
            .iter()
            .map(|e| {
                let mut v = match &e.item {
                    Item::Building(b) => json!({ "building": key_str(*b), "name": w.buildings.get(*b).map_or("(gone)".to_string(), |x| x.name.clone()) }),
                    Item::Class(t) => json!({ "class": tier_key(*t) }),
                    Item::LandUse(lu) => json!({ "land_use": lu_key(*lu) }),
                    Item::Route(r) => json!({ "route": key_str(*r), "name": w.transit.routes.get(*r).map_or("(removed)".to_string(), |x| x.name.clone()) }),
                    Item::Named(n) => json!({ "name": n }),
                };
                v["amount"] = json!(round(e.amount));
                v["postings"] = json!(e.count);
                v
            })
            .collect();
        m.insert(f.label().replace(' ', "_"), json!({ "total": round(row[f.index()]), "items": rows, "not_itemised": round(rest) }));
    }
    Value::Object(m)
}

/// The sim's daily and hourly history rings (`nd_sim::history`) as
/// columns: `days.population` is a list over the days, oldest first, and
/// so on. Arrays in a record become named sub-columns (ledger by flow,
/// energy by plant kind, tax by class and land use, calls by kind,
/// riders by bus line).
pub fn sim_history(w: &World, days: usize) -> Value {
    use nd_sim::PowerPlantKind;
    let h = &w.economy.history;
    let recs: Vec<&nd_sim::history::DayRecord> = h.last_days(days).collect();
    let mut cols: BTreeMap<String, Vec<Value>> = BTreeMap::new();
    let n = recs.len();
    let put = |cols: &mut BTreeMap<String, Vec<Value>>, i: usize, k: String, v: Value| {
        cols.entry(k).or_insert_with(|| vec![Value::Null; n])[i] = v;
    };
    let kinds = ["fire", "crime", "medical"];
    for (i, d) in recs.iter().enumerate() {
        let mut flat = serde_json::to_value(d).unwrap_or(Value::Null);
        if let Value::Object(o) = &mut flat {
            for k in ["ledger", "income_tax_by_class", "business_tax_by_use", "demand", "supply_mwh", "incidents", "resolved", "avg_rent", "classes", "route_riders"] {
                o.remove(k);
            }
            for (k, v) in o.iter() {
                let v = v.as_f64().map_or(v.clone(), |x| json!(round(x as f32)));
                put(&mut cols, i, k.clone(), v);
            }
        }
        put(&mut cols, i, "net".into(), json!(round(d.net())));
        put(&mut cols, i, "income".into(), json!(round(d.income())));
        put(&mut cols, i, "spending".into(), json!(round(d.spending())));
        for f in Flow::ALL {
            put(&mut cols, i, format!("ledger.{}", f.label().replace(' ', "_")), json!(round(d.flow(f))));
        }
        for t in Tier::ALL {
            put(&mut cols, i, format!("income_tax.{}", tier_key(t)), json!(round(d.income_tax_by_class[t as usize])));
            put(&mut cols, i, format!("classes.{}", tier_key(t)), json!(d.classes[t as usize]));
            put(&mut cols, i, format!("avg_rent.{}", tier_key(t)), json!(round(d.avg_rent[t as usize])));
        }
        for lu in LandUse::ALL {
            put(&mut cols, i, format!("business_tax.{}", lu_key(lu)), json!(round(d.business_tax_by_use[lu.index()])));
            put(&mut cols, i, format!("demand.{}", lu_key(lu)), json!(round(d.demand[lu.index()])));
        }
        for k in PowerPlantKind::ALL {
            put(&mut cols, i, format!("supply_mwh.{}", k.short()), json!(round(d.supply_mwh[k.index()])));
        }
        for (j, k) in kinds.iter().enumerate() {
            put(&mut cols, i, format!("incidents.{k}"), json!(d.incidents[j]));
            put(&mut cols, i, format!("resolved.{k}"), json!(d.resolved[j]));
        }
        for (r, riders) in &d.route_riders {
            let name = w.transit.routes.get(*r).map_or_else(|| key_str(*r), |x| x.name.clone());
            put(&mut cols, i, format!("route_riders.{name}"), json!(riders));
        }
    }
    let hours: Vec<&nd_sim::history::HourRecord> = h.last_hours(nd_sim::history::HOURS_KEPT).collect();
    let mut hcols: BTreeMap<String, Vec<Value>> = BTreeMap::new();
    let hn = hours.len();
    for (i, r) in hours.iter().enumerate() {
        let mut flat = serde_json::to_value(r).unwrap_or(Value::Null);
        if let Value::Object(o) = &mut flat {
            o.remove("supply_by_kind");
            for (k, v) in o.iter() {
                let v = v.as_f64().map_or(v.clone(), |x| json!(round(x as f32)));
                hcols.entry(k.clone()).or_insert_with(|| vec![Value::Null; hn])[i] = v;
            }
        }
        for k in PowerPlantKind::ALL {
            hcols.entry(format!("supply_mw.{}", k.short())).or_insert_with(|| vec![Value::Null; hn])[i] = json!(round(r.supply_by_kind[k.index()]));
        }
    }
    json!({
        "days_kept": h.days.len(), "days": cols, "hours": hcols,
        "note": "kept in the save (nd_sim::history): up to 365 days and 72 hours; a day's record closes at midnight, so counters are that day's totals and levels are as it closed",
    })
}

#[cfg(test)]
mod tests {
    use super::memory_reason;

    /// [rebrand] Arrivals group together whether the memory names the city
    /// (new) or the game's old name (saves from before the rename).
    #[test]
    fn arrivals_group_under_one_reason_old_and_new() {
        assert_eq!(memory_reason("Rolled into Neon District on day 2 at 19:45."), "Rolled into");
        assert_eq!(memory_reason("Rolled into Kowloon Bay on day 9 at 07:15."), "Rolled into");
        assert_eq!(memory_reason("Rolled into town on day 1 at 00:15."), "Rolled into");
    }
}
