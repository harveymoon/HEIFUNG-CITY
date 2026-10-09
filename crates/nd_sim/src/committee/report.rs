//! [committee] The compact city report the committee reads: the same data
//! as the remote API's `report`, `history` and `map` (the sim's daily
//! history ring, the economy, coverage, the grid, the ground), boiled down
//! to a few dozen figures and a handful of named areas, so a model gets the
//! city in a few hundred tokens and the stand-in can quote real numbers.

use glam::Vec2;
use serde::{Deserialize, Serialize};

use crate::services::ServiceKind;
use crate::zoning::LandUse;
use crate::World;

/// A figure with its unit and, when there is one, the value it is compared
/// against ("a week ago", "at the last meeting").
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Fact {
    pub label: String,
    pub value: f32,
    pub before: Option<f32>,
    /// "%", "/day", "credits", "" ...
    pub unit: String,
}

impl Fact {
    pub fn new(label: &str, value: f32, unit: &str) -> Self {
        Self { label: label.into(), value, before: None, unit: unit.into() }
    }

    pub fn vs(mut self, before: f32) -> Self {
        self.before = Some(before);
        self
    }

    /// The value as people say it: shares as percentages, money rounded.
    pub fn show(&self, v: f32) -> String {
        match self.unit.as_str() {
            "%" => format!("{:.0}%", v * 100.0),
            "credits" => format!("{} credits", thousands(v)),
            "/day" if v.abs() >= 10.0 || v.fract().abs() < 0.05 => format!("{:.0} a day", v),
            "/day" => format!("{v:.1} a day"),
            "" => thousands(v),
            u => format!("{} {u}", thousands(v)),
        }
    }

    /// Change against `before`, as a signed percentage of it.
    pub fn change(&self) -> Option<f32> {
        let b = self.before?;
        if b.abs() < 1e-3 {
            return None;
        }
        Some((self.value - b) / b.abs())
    }

    /// "rent burden 41% (was 33%)".
    pub fn line(&self) -> String {
        match self.before.filter(|b| self.show(*b) != self.show(self.value)) {
            Some(b) => format!("{} {} (was {})", self.label, self.show(self.value), self.show(b)),
            None => format!("{} {}", self.label, self.show(self.value)),
        }
    }
}

/// 12,345 / -1,200 / 3.5 for small numbers.
pub fn thousands(v: f32) -> String {
    if v.abs() < 10.0 && v.fract().abs() > 0.05 {
        return format!("{v:.1}");
    }
    let n = v.round() as i64;
    let s = n.unsigned_abs().to_string();
    let mut out = String::new();
    for (i, c) in s.chars().enumerate() {
        if i > 0 && (s.len() - i) % 3 == 0 {
            out.push(',');
        }
        out.push(c);
    }
    if n < 0 {
        format!("-{out}")
    } else {
        out
    }
}

/// A named part of town the committee can point at: the districts the
/// player painted, and Downtown plus four compass sides of the built city.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Area {
    pub name: String,
    pub center: Vec2,
    /// For a compass area, roughly how far it reaches.
    pub radius: f32,
    /// The district's id, for a painted district.
    #[serde(default)]
    pub district: Option<u16>,
}

/// What an area holds, for the report.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct AreaSummary {
    pub name: String,
    pub homes: u32,
    pub jobs: u32,
    pub buildings: u32,
    /// Mean coverage of its homes: police, fire, health, school.
    pub coverage: [f32; 4],
    /// Contamination over its ground (cells' worth).
    pub contamination: f32,
    /// Unzoned parcels by a street, free to build on.
    pub free_parcels: u32,
}

/// Downtown and the compass sides around the city's centre of mass, then
/// the painted districts. North is -y (as the junction editor's compass).
pub fn areas(w: &World) -> Vec<Area> {
    let pts: Vec<Vec2> = w.buildings.values().filter_map(|b| w.lots.get(b.lot)).map(|l| l.center).collect();
    let pts: Vec<Vec2> = if pts.is_empty() { w.lots.values().map(|l| l.center).collect() } else { pts };
    let (c, r) = if pts.is_empty() {
        (Vec2::ZERO, 300.0)
    } else {
        let c = pts.iter().fold(Vec2::ZERO, |a, &p| a + p) / pts.len() as f32;
        let r = (pts.iter().map(|p| p.distance(c)).sum::<f32>() / pts.len() as f32).max(120.0);
        (c, r)
    };
    let mut out = vec![Area { name: "Downtown".into(), center: c, radius: r * 0.55, district: None }];
    for (name, d) in [("Northside", Vec2::new(0.0, -1.0)), ("Eastside", Vec2::new(1.0, 0.0)), ("Southside", Vec2::new(0.0, 1.0)), ("Westside", Vec2::new(-1.0, 0.0))] {
        out.push(Area { name: name.into(), center: c + d * r * 1.1, radius: r, district: None });
    }
    let extents = w.districts.grid.extents();
    for d in &w.districts.list {
        if let Some(e) = extents.get(&d.id) {
            if !out.iter().any(|a| a.name.eq_ignore_ascii_case(&d.name)) {
                out.push(Area { name: d.name.clone(), center: e.centroid(), radius: r * 0.5, district: Some(d.id) });
            }
        }
    }
    out
}

/// The compass area a point belongs to: Downtown inside its radius, else
/// the nearest side.
pub fn compass_area_of(areas: &[Area], p: Vec2) -> Option<&Area> {
    let down = areas.first()?;
    if p.distance(down.center) <= down.radius {
        return Some(down);
    }
    areas.iter().take(5).skip(1).min_by(|a, b| a.center.distance(p).total_cmp(&b.center.distance(p)))
}

/// Is `p` in the named area? A district by its painted cells, a compass
/// area by `compass_area_of`.
pub fn in_area(w: &World, areas: &[Area], area: &Area, p: Vec2) -> bool {
    match area.district {
        Some(id) => w.districts.grid.at(p) == id,
        None => compass_area_of(areas, p).is_some_and(|a| a.name == area.name),
    }
}

/// Find an area by name, forgiving case and a missing "side".
pub fn find_area<'a>(areas: &'a [Area], name: &str) -> Option<&'a Area> {
    let n = name.trim().to_ascii_lowercase();
    if n.is_empty() {
        return None;
    }
    areas.iter().find(|a| a.name.to_ascii_lowercase() == n).or_else(|| {
        let short = n.trim_end_matches("side").trim_end_matches(" end").trim();
        let alias = match short {
            "north" => "northside",
            "east" => "eastside",
            "south" => "southside",
            "west" => "westside",
            "centre" | "center" | "city centre" | "city center" | "downtown" => "downtown",
            other => other,
        };
        areas.iter().find(|a| a.name.to_ascii_lowercase() == alias)
    })
}

/// The city in figures, as the committee reads it.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct CityReport {
    pub day: u32,
    pub city: String,
    pub population: u32,
    /// Population a week ago (from the history ring).
    pub population_week_ago: u32,
    pub treasury: f32,
    /// Means over the last seven days.
    pub income_per_day: f32,
    pub spending_per_day: f32,
    pub net_per_day: f32,
    pub unemployment: f32,
    pub unhoused: u32,
    pub waiting: u32,
    pub homes: u32,
    pub homes_free: u32,
    pub jobs: u32,
    pub jobs_free: u32,
    /// Share of housed households paying over 30% of income in rent.
    pub rent_burden: f32,
    pub avg_rent: f32,
    pub riders_per_day: f32,
    /// The seven days before those.
    pub riders_prev: f32,
    pub bus_lines: u32,
    pub depots: u32,
    pub crime_per_day: f32,
    pub crime_prev: f32,
    pub fires_per_day: f32,
    pub unanswered_per_day: f32,
    /// Share of homes covered: police, fire, health, school.
    pub coverage: [f32; 4],
    pub unpowered: u32,
    pub brownout_hours_week: u32,
    pub power_served: f32,
    pub road_wear: f32,
    pub road_funding: f32,
    pub contaminated_cells: u32,
    pub contamination_peak: f32,
    /// Demand per land use (`LandUse::ALL` order), 0..1.
    pub demand: [f32; 6],
    pub land_value: f32,
    pub gini: f32,
    pub tolls: u32,
    /// A few tax rates by category key: homes by tier, shops, industry.
    pub tax_rates: Vec<(String, f32)>,
    pub areas: Vec<AreaSummary>,
}

use crate::services::COVERED;

impl CityReport {
    pub fn build(w: &World) -> CityReport {
        let e = &w.economy;
        let days: Vec<&crate::history::DayRecord> = e.history.days.iter().collect();
        let n = days.len();
        let window = |from: usize, to: usize| -> Vec<&crate::history::DayRecord> {
            let lo = n.saturating_sub(to);
            let hi = n.saturating_sub(from);
            days[lo..hi].to_vec()
        };
        let last7 = window(0, 7);
        let prev7 = window(7, 14);
        let mean = |v: &[&crate::history::DayRecord], f: &dyn Fn(&crate::history::DayRecord) -> f32| -> f32 {
            if v.is_empty() {
                0.0
            } else {
                v.iter().map(|d| f(d)).sum::<f32>() / v.len() as f32
            }
        };
        let riders = |d: &crate::history::DayRecord| (d.bus_riders + d.rail_riders + d.ferry_riders + d.air_riders) as f32;
        let population = w.citizens.len() as u32;
        let population_week_ago = days.get(n.saturating_sub(8)).map_or(population, |d| d.population);
        let latest = days.last();
        let working_age = latest.map_or(0, |d| d.working_age);
        let unemployed = latest.map_or(0, |d| d.unemployed);
        // Rent burden over housed households.
        let (mut housed, mut burdened) = (0u32, 0u32);
        for (hid, hh) in w.households.iter() {
            if hh.home.is_none() {
                continue;
            }
            if let Some(s) = w.household_rent_share(hid) {
                housed += 1;
                if s > 0.3 {
                    burdened += 1;
                }
            }
        }
        let rent_burden = if housed == 0 { 0.0 } else { burdened as f32 / housed as f32 };
        let rents: Vec<f32> = w.buildings.values().filter(|b| b.homes > 0 && b.operating()).map(|b| b.rent).collect();
        let avg_rent = if rents.is_empty() { 0.0 } else { rents.iter().sum::<f32>() / rents.len() as f32 };
        let depots = w.buildings.values().filter(|b| b.depot && b.operating()).count() as u32;
        let cont = &w.contamination;
        let taxes = &e.taxes;
        let tax_rates = ["residential_low", "residential_mid", "residential_high", "grocery", "gadget_shop", "manufacturing", "logistics"]
            .iter()
            .filter_map(|k| crate::tax::TaxCategory::from_key(k).map(|c| (k.to_string(), taxes.rate(c))))
            .collect();
        let mut r = CityReport {
            day: w.clock.day() as u32,
            city: if w.identity.name.is_empty() { "the city".into() } else { w.identity.name.clone() },
            population,
            population_week_ago,
            treasury: e.treasury,
            income_per_day: mean(&last7, &|d| d.income()),
            spending_per_day: mean(&last7, &|d| d.spending()),
            net_per_day: mean(&last7, &|d| d.net()),
            unemployment: if working_age == 0 { 0.0 } else { unemployed as f32 / working_age as f32 },
            unhoused: latest.map_or(0, |d| d.unhoused),
            waiting: e.waiting_immigrants,
            homes: e.homes_total,
            homes_free: e.homes_free,
            jobs: e.jobs_total,
            jobs_free: e.jobs_free,
            rent_burden,
            avg_rent,
            riders_per_day: mean(&last7, &riders),
            riders_prev: mean(&prev7, &riders),
            bus_lines: w.transit.routes.values().filter(|r| r.stops.len() >= 2).count() as u32,
            depots,
            crime_per_day: mean(&last7, &|d| d.incidents[1] as f32),
            crime_prev: mean(&prev7, &|d| d.incidents[1] as f32),
            fires_per_day: mean(&last7, &|d| d.incidents[0] as f32),
            unanswered_per_day: mean(&last7, &|d| d.unanswered as f32),
            coverage: w.services.stats.coverage,
            unpowered: e.power.unpowered_buildings,
            brownout_hours_week: last7.iter().map(|d| d.brownout_hours).sum(),
            power_served: e.power.served,
            road_wear: w.road_wear.mean_wear(&w.roads),
            road_funding: w.road_wear.funding,
            contaminated_cells: cont.cells_over(0.2) as u32,
            contamination_peak: cont.max(),
            demand: std::array::from_fn(|i| e.demand_for(LandUse::ALL[i])),
            land_value: latest.map_or(0.0, |d| d.avg_land_value),
            gini: latest.map_or(0.0, |d| d.gini),
            tolls: w.tolls.booths.len() as u32,
            tax_rates,
            areas: Vec::new(),
        };
        r.areas = area_summaries(w);
        r
    }

    /// A headline figure by key, for memory ("what was crime when I
    /// voted?") and for the remote API.
    pub fn metric(&self, key: &str) -> Option<f32> {
        Some(match key {
            "population" => self.population as f32,
            "treasury" => self.treasury,
            "net_per_day" => self.net_per_day,
            "unemployment" => self.unemployment,
            "unhoused" => self.unhoused as f32,
            "rent_burden" => self.rent_burden,
            "riders_per_day" => self.riders_per_day,
            "crime_per_day" => self.crime_per_day,
            "police_coverage" => self.coverage[0],
            "fire_coverage" => self.coverage[1],
            "health_coverage" => self.coverage[2],
            "school_coverage" => self.coverage[3],
            "unpowered" => self.unpowered as f32,
            "road_wear" => self.road_wear,
            "contaminated_cells" => self.contaminated_cells as f32,
            "commercial_demand" => self.demand[1],
            "residential_demand" => self.demand[0],
            _ => return None,
        })
    }

    /// Whether a higher value of the metric is good news.
    pub fn higher_is_better(key: &str) -> bool {
        matches!(key, "population" | "treasury" | "net_per_day" | "riders_per_day" | "police_coverage" | "fire_coverage" | "health_coverage" | "school_coverage" | "commercial_demand" | "residential_demand")
    }

    /// The headline metric of a scope.
    pub fn scope_metric(scope: super::Scope) -> &'static str {
        use super::Scope::*;
        match scope {
            Transit => "riders_per_day",
            Housing => "rent_burden",
            Finance => "net_per_day",
            Safety => "crime_per_day",
            Environment => "contaminated_cells",
            Business => "unemployment",
            Utilities => "unpowered",
            Welfare => "health_coverage",
        }
    }

    /// The figures a member of `scope` reports on, compared with `before`
    /// (the last meeting's report) where there was one.
    pub fn scope_facts(&self, scope: super::Scope, before: Option<&CityReport>) -> Vec<Fact> {
        use super::Scope::*;
        let b = |f: &dyn Fn(&CityReport) -> f32| before.map(f);
        let with = |fact: Fact, prev: Option<f32>| match prev {
            Some(p) => fact.vs(p),
            None => fact,
        };
        match scope {
            Transit => vec![
                with(Fact::new("transit riders", self.riders_per_day, "/day"), Some(b(&|r| r.riders_per_day).unwrap_or(self.riders_prev))),
                Fact::new("bus lines running", self.bus_lines as f32, ""),
                Fact::new("bus depots", self.depots as f32, ""),
                Fact::new("toll booths", self.tolls as f32, ""),
            ],
            Housing => vec![
                with(Fact::new("households over 30% rent burden", self.rent_burden, "%"), b(&|r| r.rent_burden)),
                with(Fact::new("people unhoused", self.unhoused as f32, ""), b(&|r| r.unhoused as f32)),
                Fact::new("free homes", self.homes_free as f32, ""),
                Fact::new("waiting to move in", self.waiting as f32, ""),
                Fact::new("residential demand", self.demand[0], "%"),
            ],
            Finance => vec![
                Fact::new("treasury", self.treasury, "credits"),
                with(Fact::new("net per day", self.net_per_day, "credits"), b(&|r| r.net_per_day)),
                Fact::new("income per day", self.income_per_day, "credits"),
                Fact::new("spending per day", self.spending_per_day, "credits"),
            ],
            Safety => vec![
                with(Fact::new("crimes", self.crime_per_day, "/day"), Some(b(&|r| r.crime_per_day).unwrap_or(self.crime_prev))),
                with(Fact::new("homes with police cover", self.coverage[0], "%"), b(&|r| r.coverage[0])),
                Fact::new("homes with fire cover", self.coverage[1], "%"),
                Fact::new("unanswered calls", self.unanswered_per_day, "/day"),
            ],
            Environment => vec![
                with(Fact::new("contaminated ground cells", self.contaminated_cells as f32, ""), b(&|r| r.contaminated_cells as f32)),
                Fact::new("worst contamination", self.contamination_peak, "%"),
                Fact::new("park demand", self.demand[5], "%"),
            ],
            Business => vec![
                with(Fact::new("unemployment", self.unemployment, "%"), b(&|r| r.unemployment)),
                Fact::new("open jobs", self.jobs_free as f32, ""),
                Fact::new("commercial demand", self.demand[1], "%"),
                Fact::new("industrial demand", self.demand[2], "%"),
            ],
            Utilities => vec![
                with(Fact::new("unpowered buildings", self.unpowered as f32, ""), b(&|r| r.unpowered as f32)),
                Fact::new("brownout hours this week", self.brownout_hours_week as f32, ""),
                with(Fact::new("road wear", self.road_wear, "%"), b(&|r| r.road_wear)),
                Fact::new("road maintenance funding", self.road_funding, "%"),
            ],
            Welfare => vec![
                with(Fact::new("homes near a clinic", self.coverage[2], "%"), b(&|r| r.coverage[2])),
                with(Fact::new("homes near a school", self.coverage[3], "%"), b(&|r| r.coverage[3])),
                Fact::new("population", self.population as f32, ""),
            ],
        }
    }

    /// The report as compact text for a prompt: a few hundred tokens.
    pub fn to_text(&self) -> String {
        let pct = |x: f32| format!("{:.0}%", x * 100.0);
        let mut s = String::new();
        s.push_str(&format!("CITY REPORT: {}, day {}\n", self.city, self.day + 1));
        s.push_str(&format!("People: population {} (a week ago {}), unemployment {}, unhoused {}, waiting to move in {}\n", self.population, self.population_week_ago, pct(self.unemployment), self.unhoused, self.waiting));
        s.push_str(&format!("Housing: homes {} ({} free), rent burden over 30% of income: {} of households, mean rent {:.0}\n", self.homes, self.homes_free, pct(self.rent_burden), self.avg_rent));
        s.push_str(&format!("Money: treasury {:.0}, per day (7-day mean) income {:.0}, spending {:.0}, net {:.0}\n", self.treasury, self.income_per_day, self.spending_per_day, self.net_per_day));
        s.push_str(&format!("Taxes: {}\n", self.tax_rates.iter().map(|(k, v)| format!("{k} {:.1}%", v * 100.0)).collect::<Vec<_>>().join(", ")));
        s.push_str(&format!("Jobs: {} ({} open); demand residential {}, commercial {}, industrial {}, park {}\n", self.jobs, self.jobs_free, pct(self.demand[0]), pct(self.demand[1]), pct(self.demand[2]), pct(self.demand[5])));
        s.push_str(&format!("Transit: riders/day {:.0} (previous week {:.0}), bus lines {}, depots {}, toll booths {}\n", self.riders_per_day, self.riders_prev, self.bus_lines, self.depots, self.tolls));
        s.push_str(&format!("Safety: crimes/day {:.1} (previous week {:.1}), fires/day {:.1}, unanswered calls/day {:.1}; homes covered: police {}, fire {}, health {}, school {}\n", self.crime_per_day, self.crime_prev, self.fires_per_day, self.unanswered_per_day, pct(self.coverage[0]), pct(self.coverage[1]), pct(self.coverage[2]), pct(self.coverage[3])));
        s.push_str(&format!("Utilities: unpowered buildings {}, brownout hours this week {}, power served {}; road wear {}, road funding {}\n", self.unpowered, self.brownout_hours_week, pct(self.power_served), pct(self.road_wear), pct(self.road_funding)));
        s.push_str(&format!("Ground: contaminated cells {}, worst {}; land value {:.2}, inequality (Gini) {:.2}\n", self.contaminated_cells, pct(self.contamination_peak), self.land_value, self.gini));
        s.push_str("Areas (homes, jobs, police/health/school cover, contamination, free parcels):\n");
        for a in &self.areas {
            s.push_str(&format!("- {}: {} homes, {} jobs, cover {}/{}/{}, contamination {:.1}, free parcels {}\n", a.name, a.homes, a.jobs, pct(a.coverage[0]), pct(a.coverage[2]), pct(a.coverage[3]), a.contamination, a.free_parcels));
        }
        s
    }
}

fn area_summaries(w: &World) -> Vec<AreaSummary> {
    let areas = areas(w);
    let mut out: Vec<AreaSummary> = areas.iter().map(|a| AreaSummary { name: a.name.clone(), ..Default::default() }).collect();
    let mut covered_homes = vec![[0.0f32; 4]; areas.len()];
    for b in w.buildings.values().filter(|b| b.operating()) {
        let Some(l) = w.lots.get(b.lot) else { continue };
        for (i, a) in areas.iter().enumerate() {
            if !in_area(w, &areas, a, l.center) {
                continue;
            }
            let s = &mut out[i];
            s.buildings += 1;
            s.homes += b.homes as u32;
            s.jobs += b.jobs as u32;
            if b.homes > 0 {
                for (k, kind) in ServiceKind::COVERING.iter().enumerate() {
                    if w.services.coverage_of(b.lot, *kind) >= COVERED {
                        covered_homes[i][k] += b.homes as f32;
                    }
                }
            }
        }
    }
    for (i, s) in out.iter_mut().enumerate() {
        if s.homes > 0 {
            for k in 0..4 {
                s.coverage[k] = covered_homes[i][k] / s.homes as f32;
            }
        }
    }
    for p in &w.parcels.parcels {
        if p.lot.is_some() || p.land_use.is_some() || p.rough || !p.street_frontage {
            continue;
        }
        for (i, a) in areas.iter().enumerate() {
            if in_area(w, &areas, a, p.center) {
                out[i].free_parcels += 1;
            }
        }
    }
    let cont = &w.contamination;
    for (&idx, &v) in &cont.cells {
        if v < 0.05 {
            continue;
        }
        let (ix, iy) = (idx as usize % cont.n.max(1), idx as usize / cont.n.max(1));
        let p = cont.cell_center(ix, iy);
        for (i, a) in areas.iter().enumerate() {
            if in_area(w, &areas, a, p) {
                out[i].contamination += v;
            }
        }
    }
    out
}
