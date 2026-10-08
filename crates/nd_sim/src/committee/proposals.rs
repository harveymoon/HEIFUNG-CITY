//! [committee] Proposals: the actions the game can actually do, their
//! validation (a proposal is only tabled once it validates), the topics the
//! rules suggest to each member, how each member leans on a vote, the
//! candidate sites the code finds for a passed proposal (the model never
//! picks coordinates), and the mayor's implementation.

use glam::Vec2;
use serde::{Deserialize, Serialize};

use super::report::{areas, find_area, in_area, thousands, Area, CityReport};
use super::{mix, Committee, CommitteeProject, MemoryEntry, Politician, ProjectStatus, Proposal, Scope, Temper, Vote};
use crate::buildings::Tier;
use crate::power::PowerPlantKind;
use crate::services::{Placeable, ServiceKind};
use crate::tax::TaxCategory;
use crate::zoning::{LandUse, ParcelKey};
use crate::World;

/// What `World::remediate` charges per cell of full contamination
/// (`world/dirt.rs` `CLEANUP_COST`), for the estimate only: implementing
/// charges the real price.
const CLEANUP_COST_ESTIMATE: f32 = 450.0;
/// Biggest tax change one proposal may make, in percentage points.
pub const MAX_TAX_STEP_PCT: f32 = 5.0;

/// A structured proposal, as the members table it and models write it:
/// `{"action": "build_service", "service": "health", "near": "Northside"}`.
/// Free-text fields so a model's slightly-off spelling still parses; the
/// validation decides, and normalises what it accepts.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "action", rename_all = "snake_case")]
pub enum Action {
    /// Police, fire, health, school, trash depot or 911 dispatch.
    BuildService {
        service: String,
        #[serde(default)]
        tier: String,
        /// An area name; empty: wherever it is needed most.
        #[serde(default)]
        near: String,
    },
    /// Solar, wind or coal.
    BuildPower {
        plant: String,
        #[serde(default)]
        near: String,
    },
    BuildDepot {
        #[serde(default)]
        near: String,
    },
    /// Zone free ground in an area: residential, commercial, industrial,
    /// entertainment or park.
    Rezone { area: String, to: String },
    AddBusLine { from: String, to: String },
    /// A tax category key (`residential_mid`) or a land use group
    /// (`residential`, `commercial`, `industrial`, `entertainment`), raised or
    /// cut by `delta_pct` percentage points.
    ChangeTax { category: String, delta_pct: f32 },
    /// A toll booth's price (`toll`: its id; none: every booth).
    SetToll {
        #[serde(default)]
        toll: Option<u32>,
        price: f32,
    },
    /// Road maintenance funding (1.0 is what the network needs).
    RoadRepair { funding: f32 },
    /// Dig out the worst contaminated ground in an area.
    Remediate { area: String },
}

/// The action kinds and their fields, for prompts, the API's help and the
/// docs.
pub const ACTION_HELP: &str = r#"build_service {service: police|fire|health|school|trash_depot|dispatch, tier: low|mid|high, near: <area>}
build_power {plant: solar|wind|coal, near: <area>}
build_depot {near: <area>}
rezone {area: <area>, to: residential|commercial|industrial|entertainment|park}
add_bus_line {from: <area>, to: <area>}
change_tax {category: residential|commercial|industrial|entertainment|<category key>, delta_pct: -5..5}
set_toll {toll: <id or omit for all>, price: 0..50}
road_repair {funding: 0..1.5}
remediate {area: <area>}"#;

const SERVICES: [ServiceKind; 6] = [ServiceKind::Police, ServiceKind::Fire, ServiceKind::Health, ServiceKind::School, ServiceKind::TrashDepot, ServiceKind::Dispatch];

fn parse_tier(s: &str) -> Option<Tier> {
    match s.trim().to_ascii_lowercase().as_str() {
        "" | "low" | "small" | "1" => Some(Tier::Low),
        "mid" | "middle" | "medium" | "2" => Some(Tier::Mid),
        "high" | "large" | "big" | "3" => Some(Tier::High),
        _ => None,
    }
}

fn tier_key(t: Tier) -> &'static str {
    match t {
        Tier::Low => "low",
        Tier::Mid => "mid",
        Tier::High => "high",
    }
}

fn parse_plant(s: &str) -> Option<PowerPlantKind> {
    match s.trim().to_ascii_lowercase().as_str() {
        "solar" | "solar farm" => Some(PowerPlantKind::Solar),
        "wind" | "wind farm" => Some(PowerPlantKind::Wind),
        "coal" | "coal plant" => Some(PowerPlantKind::Coal),
        _ => None,
    }
}

pub fn parse_land_use(s: &str) -> Option<LandUse> {
    match s.trim().to_ascii_lowercase().as_str() {
        "residential" | "housing" | "homes" => Some(LandUse::Residential),
        "commercial" | "shops" => Some(LandUse::Commercial),
        "industrial" | "industry" => Some(LandUse::Industrial),
        "entertainment" | "nightlife" => Some(LandUse::Entertainment),
        "park" | "parks" | "green" => Some(LandUse::Park),
        _ => None,
    }
}

/// Tax categories a `change_tax` touches.
pub fn tax_categories(key: &str) -> Vec<TaxCategory> {
    let k = key.trim().to_ascii_lowercase();
    if let Some(c) = TaxCategory::from_key(&k) {
        return vec![c];
    }
    match parse_land_use(&k) {
        Some(lu) if lu != LandUse::Park => TaxCategory::group(lu),
        _ => Vec::new(),
    }
}

impl Action {
    pub fn kind_key(&self) -> &'static str {
        match self {
            Action::BuildService { .. } => "build_service",
            Action::BuildPower { .. } => "build_power",
            Action::BuildDepot { .. } => "build_depot",
            Action::Rezone { .. } => "rezone",
            Action::AddBusLine { .. } => "add_bus_line",
            Action::ChangeTax { .. } => "change_tax",
            Action::SetToll { .. } => "set_toll",
            Action::RoadRepair { .. } => "road_repair",
            Action::Remediate { .. } => "remediate",
        }
    }

    /// The scope it mostly belongs to.
    pub fn scope(&self) -> Scope {
        match self {
            Action::BuildService { service, .. } => match ServiceKind::from_key(service) {
                Some(ServiceKind::Health | ServiceKind::School) => Scope::Welfare,
                Some(ServiceKind::TrashDepot) => Scope::Environment,
                _ => Scope::Safety,
            },
            Action::BuildPower { .. } | Action::RoadRepair { .. } => Scope::Utilities,
            Action::BuildDepot { .. } | Action::AddBusLine { .. } | Action::SetToll { .. } => Scope::Transit,
            Action::Rezone { to, .. } => match parse_land_use(to) {
                Some(LandUse::Residential) => Scope::Housing,
                Some(LandUse::Park) => Scope::Environment,
                _ => Scope::Business,
            },
            Action::ChangeTax { .. } => Scope::Finance,
            Action::Remediate { .. } => Scope::Environment,
        }
    }

    /// Does it put something on the map (and so need a site)?
    pub fn needs_site(&self) -> bool {
        !matches!(self, Action::ChangeTax { .. } | Action::SetToll { .. } | Action::RoadRepair { .. })
    }

    fn placeable(&self) -> Option<Placeable> {
        match self {
            Action::BuildService { service, tier, .. } => Some(Placeable::Service(ServiceKind::from_key(service)?, parse_tier(tier)?)),
            Action::BuildPower { plant, .. } => Some(Placeable::Plant(parse_plant(plant)?)),
            Action::BuildDepot { .. } => Some(Placeable::BusDepot),
            _ => None,
        }
    }

    fn near(&self) -> &str {
        match self {
            Action::BuildService { near, .. } | Action::BuildPower { near, .. } | Action::BuildDepot { near } => near,
            Action::Rezone { area, .. } | Action::Remediate { area } => area,
            Action::AddBusLine { from, .. } => from,
            _ => "",
        }
    }

    /// Short name: "Northside clinic", "Westside residential zoning".
    pub fn title(&self) -> String {
        let at = |near: &str| if near.is_empty() { String::new() } else { format!("{near} ") };
        match self {
            Action::BuildService { service, tier, near } => match (ServiceKind::from_key(service), parse_tier(tier)) {
                (Some(k), Some(t)) => format!("{}{}", at(near), k.tier_label(t)),
                _ => format!("{}{service}", at(near)),
            },
            Action::BuildPower { plant, near } => format!("{}{}", at(near), parse_plant(plant).map_or(plant.as_str(), |k| k.label())),
            Action::BuildDepot { near } => format!("{}bus depot", at(near)),
            Action::Rezone { area, to } => format!("{area} {to} zoning"),
            Action::AddBusLine { from, to } => format!("{from} to {to} bus line"),
            Action::ChangeTax { category, delta_pct } => {
                let what = match TaxCategory::from_key(category) {
                    Some(c) => c.label().to_string(),
                    None => format!("{category} taxes"),
                };
                format!("{} {what} by {:.1} points", if *delta_pct >= 0.0 { "Raise" } else { "Cut" }, delta_pct.abs())
            }
            Action::SetToll { toll, price } => match toll {
                Some(id) => format!("Toll {id} at {price:.0} credits"),
                None => format!("Every toll at {price:.0} credits"),
            },
            Action::RoadRepair { funding } => format!("Road maintenance at {:.0}%", funding * 100.0),
            Action::Remediate { area } => format!("{area} ground clean-up"),
        }
    }

    /// The traits voters weigh: (growth, order, relief for residents),
    /// each roughly -1..1; the money is weighed apart (`Proposal::cost`).
    fn traits(&self) -> (f32, f32, f32) {
        match self {
            Action::BuildService { service, .. } => match ServiceKind::from_key(service) {
                Some(ServiceKind::Police) => (0.0, 0.8, 0.2),
                Some(ServiceKind::Fire) | Some(ServiceKind::Dispatch) => (0.0, 0.3, 0.5),
                Some(ServiceKind::TrashDepot) => (-0.1, 0.0, 0.3),
                _ => (0.0, 0.0, 0.6),
            },
            Action::BuildPower { plant, .. } => match parse_plant(plant) {
                Some(PowerPlantKind::Coal) => (0.8, 0.0, 0.1),
                _ => (-0.3, 0.0, 0.3),
            },
            Action::BuildDepot { .. } | Action::AddBusLine { .. } => (-0.2, 0.0, 0.4),
            Action::Rezone { to, .. } => match parse_land_use(to) {
                Some(LandUse::Residential) => (0.3, 0.0, 0.5),
                Some(LandUse::Park) => (-0.6, 0.0, 0.3),
                Some(LandUse::Industrial) => (0.8, 0.0, -0.1),
                _ => (0.6, 0.0, 0.1),
            },
            Action::ChangeTax { delta_pct, .. } => {
                let up = delta_pct.signum();
                (-0.5 * up, 0.0, -0.4 * up)
            }
            Action::SetToll { .. } => (-0.3, 0.1, -0.2),
            Action::RoadRepair { .. } => (0.3, 0.0, 0.2),
            Action::Remediate { .. } => (-0.6, 0.0, 0.3),
        }
    }

    /// Check the action against the city: is it something the game can do
    /// here and now? Returns it normalised (keys spelled the game's way,
    /// area names as the game has them).
    pub fn validate(&self, w: &World) -> Result<Action, String> {
        let areas = areas(w);
        let area = |name: &str, field: &str| -> Result<String, String> {
            find_area(&areas, name).map(|a| a.name.clone()).ok_or_else(|| format!("unknown {field} {name:?}; areas are {}", areas.iter().map(|a| a.name.as_str()).collect::<Vec<_>>().join(", ")))
        };
        let near = |name: &str| -> Result<String, String> {
            if name.trim().is_empty() {
                Ok(String::new())
            } else {
                area(name, "area")
            }
        };
        let afford = |cost: f32| -> Result<(), String> {
            if w.economy.treasury < cost {
                Err(format!("it costs {} and the treasury has {}", thousands(cost), thousands(w.economy.treasury)))
            } else {
                Ok(())
            }
        };
        match self {
            Action::BuildService { service, tier, near: n } => {
                let k = ServiceKind::from_key(&service.trim().to_ascii_lowercase()).filter(|k| SERVICES.contains(k)).ok_or_else(|| format!("unknown service {service:?}; one of police, fire, health, school, trash_depot, dispatch"))?;
                let t = parse_tier(tier).ok_or_else(|| format!("unknown tier {tier:?}; low, mid or high"))?;
                afford(Placeable::Service(k, t).cost())?;
                Ok(Action::BuildService { service: k.key().into(), tier: tier_key(t).into(), near: near(n)? })
            }
            Action::BuildPower { plant, near: n } => {
                let k = parse_plant(plant).ok_or_else(|| format!("unknown plant {plant:?}; solar, wind or coal"))?;
                afford(Placeable::Plant(k).cost())?;
                Ok(Action::BuildPower { plant: k.short().into(), near: near(n)? })
            }
            Action::BuildDepot { near: n } => {
                afford(Placeable::BusDepot.cost())?;
                Ok(Action::BuildDepot { near: near(n)? })
            }
            Action::Rezone { area: a, to } => {
                let lu = parse_land_use(to).ok_or_else(|| format!("cannot zone as {to:?}; residential, commercial, industrial, entertainment or park"))?;
                Ok(Action::Rezone { area: area(a, "area")?, to: lu.label().to_ascii_lowercase() })
            }
            Action::AddBusLine { from, to } => {
                let (f, t) = (area(from, "from area")?, area(to, "to area")?);
                if f == t {
                    return Err("a bus line needs two different areas".into());
                }
                if !w.buildings.values().any(|b| b.depot && b.construction.is_none()) {
                    return Err("there is no bus depot, so no buses could run: build a depot first".into());
                }
                Ok(Action::AddBusLine { from: f, to: t })
            }
            Action::ChangeTax { category, delta_pct } => {
                let cats = tax_categories(category);
                if cats.is_empty() {
                    return Err(format!("unknown tax category {category:?}; residential, commercial, industrial, entertainment or a category key"));
                }
                if !delta_pct.is_finite() || delta_pct.abs() < 0.1 || delta_pct.abs() > MAX_TAX_STEP_PCT {
                    return Err(format!("a tax change of {delta_pct} points: it must be between 0.1 and {MAX_TAX_STEP_PCT} points either way"));
                }
                for c in &cats {
                    let now = w.economy.taxes.rate(*c);
                    let next = now + delta_pct / 100.0;
                    if next < 0.0 || next > c.max_rate() + 1e-4 {
                        return Err(format!("{} is at {:.1}%; {:+.1} points would leave 0..{:.0}%", c.label(), now * 100.0, delta_pct, c.max_rate() * 100.0));
                    }
                }
                let key = if cats.len() == 1 && TaxCategory::from_key(category.trim()).is_some() { cats[0].key().to_string() } else { category.trim().to_ascii_lowercase() };
                Ok(Action::ChangeTax { category: key, delta_pct: (delta_pct * 10.0).round() / 10.0 })
            }
            Action::SetToll { toll, price } => {
                if w.tolls.booths.is_empty() {
                    return Err("the city has no toll booths".into());
                }
                if let Some(id) = toll {
                    if w.tolls.get(*id).is_none() {
                        return Err(format!("no toll booth {id}; booths are {}", w.tolls.booths.iter().map(|b| b.id.to_string()).collect::<Vec<_>>().join(", ")));
                    }
                }
                if !price.is_finite() || *price < 0.0 || *price > crate::tolls::MAX_PRICE {
                    return Err(format!("a toll of {price}: it must be 0..{:.0}", crate::tolls::MAX_PRICE));
                }
                Ok(Action::SetToll { toll: *toll, price: price.round() })
            }
            Action::RoadRepair { funding } => {
                if !funding.is_finite() || *funding < 0.0 || *funding > crate::road_wear::MAX_FUNDING {
                    return Err(format!("road funding {funding}: it must be 0..{}", crate::road_wear::MAX_FUNDING));
                }
                if (funding - w.road_wear.funding).abs() < 0.05 {
                    return Err(format!("road funding is already {:.0}%", w.road_wear.funding * 100.0));
                }
                Ok(Action::RoadRepair { funding: (funding * 20.0).round() / 20.0 })
            }
            Action::Remediate { area: a } => {
                let name = area(a, "area")?;
                let ar = find_area(&areas, &name).unwrap();
                let dirt = area_contamination(w, &areas, ar);
                if dirt < 0.5 {
                    return Err(format!("the ground in {name} is clean enough (contamination {dirt:.1})"));
                }
                Ok(Action::Remediate { area: name })
            }
        }
    }

    /// What it would cost to carry out, roughly (0 for a policy).
    pub fn cost_estimate(&self, w: &World) -> f32 {
        if let Some(p) = self.placeable() {
            return p.cost();
        }
        match self {
            Action::RoadRepair { funding } => (funding - w.road_wear.funding).max(0.0) * w.road_wear.need_per_day(&w.roads) * 30.0,
            Action::Remediate { area } => {
                let ars = areas(w);
                find_area(&ars, area).map_or(0.0, |a| area_contamination(w, &ars, a).min(30.0) * CLEANUP_COST_ESTIMATE * 0.5)
            }
            _ => 0.0,
        }
    }

}

/// Contamination over an area's ground, in cells' worth.
fn area_contamination(w: &World, areas: &[Area], a: &Area) -> f32 {
    let c = &w.contamination;
    let n = c.n.max(1);
    c.cells.iter().filter(|(_, &v)| v >= 0.05).map(|(&i, &v)| (c.cell_center(i as usize % n, i as usize / n), v)).filter(|(p, _)| in_area(w, areas, a, *p)).map(|(_, v)| v).sum()
}

// ── topics: what the rules would table ───────────────────────────────────────

/// The area of the report where `f` is smallest among areas with homes
/// (ties: more homes first).
fn worst_area(r: &CityReport, f: impl Fn(&super::report::AreaSummary) -> f32) -> Option<String> {
    r.areas.iter().filter(|a| a.homes > 0).min_by(|a, b| f(a).total_cmp(&f(b)).then(b.homes.cmp(&a.homes))).map(|a| a.name.clone())
}

fn most(r: &CityReport, f: impl Fn(&super::report::AreaSummary) -> f32) -> Option<String> {
    r.areas.iter().max_by(|a, b| f(a).total_cmp(&f(b))).filter(|a| f(a) > 0.0).map(|a| a.name.clone())
}

/// What the rules would have a member table, with the reason in figures;
/// None when their scope has nothing pressing. Not yet validated.
pub fn suggest(member: &Politician, r: &CityReport) -> Option<(Action, String)> {
    let l = &member.leanings;
    let pct = |x: f32| format!("{:.0}%", x * 100.0);
    let free = |a: &super::report::AreaSummary| a.free_parcels as f32;
    match member.scope {
        Scope::Transit => {
            if r.depots == 0 && r.population > 150 {
                let near = most(r, |a| a.homes as f32).unwrap_or_default();
                return Some((Action::BuildDepot { near }, format!("{} people and not one bus depot", r.population)));
            }
            let wanted = 1 + r.homes / 1500;
            let falling = r.riders_prev > 30.0 && r.riders_per_day < r.riders_prev * 0.85;
            if r.depots > 0 && (r.bus_lines < wanted || falling) {
                let from = most(r, |a| a.homes as f32)?;
                let to = r.areas.iter().filter(|a| a.name != from).max_by(|a, b| a.jobs.cmp(&b.jobs)).map(|a| a.name.clone())?;
                let why = if falling { format!("riders fell from {:.0} to {:.0} a day", r.riders_prev, r.riders_per_day) } else { format!("{} bus lines for {} homes", r.bus_lines, r.homes) };
                return Some((Action::AddBusLine { from, to }, why));
            }
            if r.tolls > 0 && l.growth < -0.2 {
                return Some((Action::SetToll { toll: None, price: 6.0 }, "make drivers pay for the road and the bus will look better".into()));
            }
            None
        }
        Scope::Housing => {
            let tight = r.homes > 0 && (r.homes_free as f32) < r.homes as f32 * 0.03;
            if r.rent_burden > 0.3 || tight || r.unhoused >= 5 {
                let area = most(r, free)?;
                let why = if r.rent_burden > 0.3 { format!("{} of households pay over 30% of income in rent", pct(r.rent_burden)) } else if r.unhoused >= 5 { format!("{} people unhoused", r.unhoused) } else { format!("only {} free homes", r.homes_free) };
                return Some((Action::Rezone { area, to: "residential".into() }, why));
            }
            None
        }
        Scope::Finance => {
            if r.net_per_day < -50.0 || r.treasury < 10_000.0 {
                if r.road_funding > 1.05 && l.spending < 0.0 {
                    return Some((Action::RoadRepair { funding: 1.0 }, format!("we lose {} a day; the roads can live on what they need", thousands(-r.net_per_day))));
                }
                let category = if member.temper == Temper::Populist { "commercial" } else { "residential" };
                return Some((Action::ChangeTax { category: category.into(), delta_pct: 1.0 }, format!("net {} a day, treasury {}", thousands(r.net_per_day), thousands(r.treasury))));
            }
            if r.net_per_day > 400.0 && r.treasury > 120_000.0 && l.spending > -0.2 {
                return Some((Action::ChangeTax { category: "residential".into(), delta_pct: -1.0 }, format!("a surplus of {} a day: give some back", thousands(r.net_per_day))));
            }
            None
        }
        Scope::Safety => {
            if r.population >= 150 && r.coverage[0] < 0.6 {
                let near = worst_area(r, |a| a.coverage[0]).unwrap_or_default();
                return Some((Action::BuildService { service: "police".into(), tier: "low".into(), near }, format!("only {} of homes have police cover", pct(r.coverage[0]))));
            }
            if r.population >= 150 && r.coverage[1] < 0.6 {
                let near = worst_area(r, |a| a.coverage[1]).unwrap_or_default();
                return Some((Action::BuildService { service: "fire".into(), tier: "low".into(), near }, format!("only {} of homes have fire cover", pct(r.coverage[1]))));
            }
            if r.crime_per_day >= 1.0 && r.crime_per_day > r.crime_prev * 1.2 {
                let near = most(r, |a| a.homes as f32).unwrap_or_default();
                return Some((Action::BuildService { service: "police".into(), tier: "mid".into(), near }, format!("crime up from {:.1} to {:.1} a day", r.crime_prev, r.crime_per_day)));
            }
            None
        }
        Scope::Environment => {
            if let Some(a) = r.areas.iter().filter(|a| a.contamination >= 1.0).max_by(|a, b| a.contamination.total_cmp(&b.contamination)) {
                return Some((Action::Remediate { area: a.name.clone() }, format!("{:.1} cells' worth of poisoned ground in {}", a.contamination, a.name)));
            }
            if r.population >= 150 && (r.demand[5] > 0.4 || l.growth < -0.3) {
                let area = most(r, free)?;
                return Some((Action::Rezone { area, to: "park".into() }, format!("park demand at {}", pct(r.demand[5]))));
            }
            None
        }
        Scope::Business => {
            if r.population >= 150 && r.unemployment > 0.08 {
                let area = most(r, free)?;
                let to = if l.growth > 0.2 { "industrial" } else { "commercial" };
                return Some((Action::Rezone { area, to: to.into() }, format!("unemployment at {}", pct(r.unemployment))));
            }
            if r.demand[1] > 0.55 {
                let area = most(r, free)?;
                return Some((Action::Rezone { area, to: "commercial".into() }, format!("commercial demand at {}", pct(r.demand[1]))));
            }
            if r.net_per_day > 0.0 && l.growth > 0.4 {
                return Some((Action::ChangeTax { category: "commercial".into(), delta_pct: -1.0 }, "lighter taxes bring the shops in".into()));
            }
            None
        }
        Scope::Utilities => {
            if r.unpowered > 0 || r.brownout_hours_week > 0 {
                let plant = if l.growth > 0.3 { "coal" } else { "solar" };
                let near = most(r, |a| a.homes as f32).unwrap_or_default();
                return Some((Action::BuildPower { plant: plant.into(), near }, format!("{} buildings without power, {} brownout hours this week", r.unpowered, r.brownout_hours_week)));
            }
            if r.road_wear > 0.4 && r.road_funding < 1.3 {
                return Some((Action::RoadRepair { funding: (r.road_funding + 0.3).min(crate::road_wear::MAX_FUNDING) }, format!("road wear at {}", pct(r.road_wear))));
            }
            None
        }
        Scope::Welfare => {
            if r.population >= 150 && r.coverage[2] < 0.6 {
                let near = worst_area(r, |a| a.coverage[2]).unwrap_or_default();
                return Some((Action::BuildService { service: "health".into(), tier: "low".into(), near }, format!("only {} of homes are near a clinic", pct(r.coverage[2]))));
            }
            if r.population >= 150 && r.coverage[3] < 0.5 {
                let near = worst_area(r, |a| a.coverage[3]).unwrap_or_default();
                return Some((Action::BuildService { service: "school".into(), tier: "low".into(), near }, format!("only {} of homes are near a school", pct(r.coverage[3]))));
            }
            None
        }
    }
}

/// How pressing a member's topic is, for choosing who tables (higher
/// first): the worst firing rule in their scope.
pub fn urgency(member: &Politician, r: &CityReport, c: &Committee, today: u32) -> u8 {
    super::rules::firing(r, c, today).iter().filter(|(rule, _, _)| rule.scope == member.scope).map(|(_, s, _)| *s).max().unwrap_or(0)
}

// ── votes by the rules ──────────────────────────────────────────────────────

/// How a member would vote by their leanings, temper and memory, and the
/// main reason. The stand-in votes so; a model is shown it as a hint and
/// may differ.
pub fn lean(member: &Politician, p: &Proposal, proposer: Option<&Politician>, r: &CityReport, seed: u64) -> (Vote, String) {
    if p.by == member.id {
        return (Vote::Yes, "I tabled it".into());
    }
    let l = &member.leanings;
    let (growth, order, relief) = p.action.traits();
    let mut reasons: Vec<(f32, String)> = Vec::new();
    let mut add = |v: f32, why: String| reasons.push((v, why));
    add(0.1, "it does no harm".into());
    if p.action.scope() == member.scope {
        add(0.6, "it is my portfolio and it is overdue".into());
    }
    if growth.abs() > 0.05 {
        let v = l.growth * growth * 0.9;
        add(v, if v >= 0.0 { if growth > 0.0 { "growth and jobs come first".into() } else { "clean ground matters more than another tower".into() } } else if growth > 0.0 { "it dirties the city for a few jobs".into() } else { "it holds the city back".into() });
    }
    if order.abs() > 0.05 {
        let v = l.order * order * 0.8;
        add(v, if v >= 0.0 { "people want to feel safe".into() } else { "we do not need more uniforms on the street".into() });
    }
    if relief.abs() > 0.05 {
        let mut v = relief * 0.4;
        if member.temper == Temper::Populist {
            v *= 2.0;
        }
        add(v, if v >= 0.0 { "it helps ordinary people".into() } else { "it hits ordinary people".into() });
    }
    if p.cost > 0.0 {
        let burden = (p.cost / r.treasury.max(2_000.0)).min(1.5);
        let v = if l.spending < 0.0 { l.spending * burden * 1.3 } else { l.spending * 0.25 };
        add(v, if v < 0.0 { format!("{} credits we cannot spare", thousands(p.cost)) } else { "worth every credit".into() });
    }
    if let Action::ChangeTax { delta_pct, .. } = &p.action {
        let v = if *delta_pct > 0.0 { l.spending * 0.4 - 0.15 } else { -l.spending * 0.4 + 0.1 };
        add(v, if *delta_pct > 0.0 { if v >= 0.0 { "the city needs the revenue".into() } else { "no new taxes".into() } } else if v >= 0.0 { "people keep more of what they earn".into() } else { "we cannot afford a tax cut".into() });
    }
    // Memory: consistent with the last vote on the same kind, unless it
    // went badly.
    if let Some(m) = member.past_vote_on(p.action.kind_key()) {
        let sign = match m.vote {
            Some(Vote::Yes) => 1.0,
            Some(Vote::No) => -1.0,
            _ => 0.0,
        };
        let k = if m.verdict < 0 { -0.25 } else { 0.4 };
        if sign != 0.0 {
            add(sign * k, if sign * k > 0.0 { format!("I voted {} on \"{}\" on day {} and I stand by it", m.vote.map_or("", |v| v.key()), m.proposal, m.day + 1) } else { format!("\"{}\" did not work out, and I have learned from it", m.proposal) });
        }
    }
    match member.temper {
        Temper::Contrarian if proposer.is_some_and(|x| x.scope != member.scope) => add(-0.45, "somebody has to say no".into()),
        Temper::OldGuard if matches!(p.action, Action::AddBusLine { .. } | Action::Rezone { .. } | Action::BuildPower { .. }) => add(-0.3, "we have tried new things before".into()),
        Temper::Dealmaker if proposer.is_some_and(|x| x.party == member.party) => add(0.35, "a friend's proposal; they will remember".into()),
        Temper::Idealist => {
            for r in reasons.iter_mut() {
                r.0 *= 1.3;
            }
        }
        _ => {}
    }
    let noise = ((mix(seed, (member.id as u64) << 16 | p.id as u64) % 1000) as f32 / 1000.0 - 0.5) * 0.3;
    let score: f32 = reasons.iter().map(|r| r.0).sum::<f32>() + noise;
    let firm = matches!(member.temper, Temper::Firebrand | Temper::Idealist);
    let vote = if score > 0.2 || (firm && score > 0.0) {
        Vote::Yes
    } else if score < -0.2 || (firm && score <= 0.0) {
        Vote::No
    } else {
        Vote::Abstain
    };
    let reason = match vote {
        Vote::Yes => reasons.iter().filter(|r| r.0 > 0.0).max_by(|a, b| a.0.total_cmp(&b.0)),
        Vote::No => reasons.iter().filter(|r| r.0 < 0.0).min_by(|a, b| a.0.total_cmp(&b.0)),
        Vote::Abstain => None,
    }
    .map_or_else(|| "the case is not made either way".to_string(), |r| r.1.clone());
    (vote, reason)
}

// ── sites ───────────────────────────────────────────────────────────────────

/// One place (or setting) a passed proposal could be carried out with, as
/// the code found it valid.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct SiteOption {
    pub label: String,
    /// Where a building would go (the click point for the placement).
    pub point: Option<Vec2>,
    /// Parcels to zone (stable keys).
    pub parcels: Vec<ParcelKey>,
    /// Bus stops in order.
    pub stops: Vec<Vec2>,
    /// Clean-up radius.
    pub radius: f32,
    pub area: String,
    pub area_m2: f32,
    pub cost: f32,
    /// Higher is better; options come sorted by it.
    pub score: f32,
    pub notes: Vec<String>,
}

/// The street a point is on, as people would say it.
fn street_near(w: &World, p: Vec2) -> Option<String> {
    let lot = w.lots.values().min_by(|a, b| a.center.distance_squared(p).total_cmp(&b.center.distance_squared(p)))?;
    if lot.center.distance(p) > 150.0 {
        return None;
    }
    w.streets.of(lot.front_segment).map(|s| s.name.clone())
}

/// Homes within `reach` of `p` without good cover by `kind`.
fn uncovered_homes(w: &World, p: Vec2, kind: ServiceKind, reach: f32) -> f32 {
    w.buildings
        .values()
        .filter(|b| b.homes > 0 && b.construction.is_none())
        .filter_map(|b| w.lots.get(b.lot).map(|l| (b, l)))
        .filter(|(_, l)| l.center.distance(p) <= reach)
        .map(|(b, _)| b.homes as f32 * (1.0 - w.services.coverage_of(b.lot, kind)).max(0.0))
        .sum()
}

fn homes_near(w: &World, p: Vec2, reach: f32) -> f32 {
    w.buildings.values().filter(|b| b.homes > 0).filter_map(|b| w.lots.get(b.lot).map(|l| (b, l))).filter(|(_, l)| l.center.distance(p) <= reach).map(|(b, _)| b.homes as f32).sum()
}

/// Valid sites for a proposal, best first (empty: nothing valid; the
/// secretary asks the mayor). Policies get one option: as voted.
pub fn site_options(w: &World, action: &Action) -> Vec<SiteOption> {
    let areas = areas(w);
    let area = find_area(&areas, action.near()).cloned();
    let in_it = |p: Vec2| area.as_ref().is_none_or(|a| in_area(w, &areas, a, p));
    let area_name = area.as_ref().map_or_else(|| "the city".to_string(), |a| a.name.clone());
    match action {
        Action::BuildService { .. } | Action::BuildPower { .. } | Action::BuildDepot { .. } => {
            let Some(what) = action.placeable() else { return Vec::new() };
            let mut cands: Vec<(Vec2, bool)> = Vec::new();
            for p in &w.parcels.parcels {
                if p.lot.is_none() && p.land_use.is_none() && !p.rough && p.street_frontage && in_it(p.center) {
                    cands.push((p.center, true));
                }
            }
            for l in w.lots.values() {
                if l.building.is_none() && l.usable() && l.car_access() && l.land_use != LandUse::Park && in_it(l.center) {
                    cands.push((l.center, false));
                }
            }
            let target = area.as_ref().map_or(Vec2::ZERO, |a| a.center);
            cands.sort_by(|a, b| a.0.distance_squared(target).total_cmp(&b.0.distance_squared(target)).then(b.1.cmp(&a.1)));
            cands.truncate(90);
            let kind = match what {
                Placeable::Service(k, _) => Some(k),
                _ => None,
            };
            let mut out: Vec<SiteOption> = Vec::new();
            for (p, unzoned) in cands {
                let Ok(site) = w.placement_problem_at(what, p) else { continue };
                let m2 = crate::geom::polygon_area(&site.outline).abs();
                let mut notes = vec![site.note()];
                let mut score = if unzoned { 0.3 } else { 0.0 };
                if let Some(k) = kind.filter(|k| k.coverage_index().is_some()) {
                    let gap = uncovered_homes(w, p, k, 450.0);
                    score += gap / 50.0;
                    notes.push(format!("about {:.0} homes nearby lack {} cover", gap, k.key()));
                } else if matches!(what, Placeable::BusDepot) {
                    score += homes_near(w, p, 500.0) / 200.0;
                } else if let Placeable::Plant(PowerPlantKind::Coal) = what {
                    let h = homes_near(w, p, 300.0);
                    score -= h / 50.0;
                    notes.push(format!("{h:.0} homes within 300 m of the smoke"));
                }
                score -= p.distance(target) / 2_000.0;
                let street = street_near(w, p).unwrap_or_else(|| "a side street".into());
                let ground = if unzoned { "unzoned ground" } else { "a vacant lot" };
                out.push(SiteOption { label: format!("{ground} on {street}, {area_name} ({:.0} m2)", m2), point: Some(p), area: area_name.clone(), area_m2: m2, cost: what.cost(), score, notes, ..Default::default() });
            }
            out.sort_by(|a, b| b.score.total_cmp(&a.score));
            let mut kept: Vec<SiteOption> = Vec::new();
            for o in out {
                if kept.iter().all(|k| k.point.unwrap().distance(o.point.unwrap()) > 60.0) {
                    kept.push(o);
                }
                if kept.len() == 4 {
                    break;
                }
            }
            kept
        }
        Action::Rezone { to, .. } => {
            let Some(lu) = parse_land_use(to) else { return Vec::new() };
            let Some(a) = area.as_ref() else { return Vec::new() };
            let mut free: Vec<usize> = (0..w.parcels.parcels.len())
                .filter(|&i| {
                    let p = &w.parcels.parcels[i];
                    p.lot.is_none() && p.land_use.is_none() && p.street_frontage && p.refusal(lu).is_none() && in_area(w, &areas, a, p.center)
                })
                .collect();
            free.sort_by(|&x, &y| w.parcels.parcels[x].center.distance_squared(a.center).total_cmp(&w.parcels.parcels[y].center.distance_squared(a.center)));
            let mut used = vec![false; free.len()];
            let mut out = Vec::new();
            for s in 0..free.len() {
                if used[s] || out.len() == 3 {
                    continue;
                }
                let seed = w.parcels.parcels[free[s]].center;
                let mut group = Vec::new();
                for (j, &i) in free.iter().enumerate() {
                    if !used[j] && w.parcels.parcels[i].center.distance(seed) <= 120.0 && group.len() < 14 {
                        used[j] = true;
                        group.push(i);
                    }
                }
                let m2: f32 = group.iter().map(|&i| w.parcels.parcels[i].area).sum();
                let street = street_near(w, seed).unwrap_or_else(|| "a new street".into());
                out.push(SiteOption {
                    label: format!("{} parcels ({:.0} m2) along {street}, {}", group.len(), m2, a.name),
                    point: Some(seed),
                    parcels: group.iter().map(|&i| w.parcels.parcels[i].key).collect(),
                    area: a.name.clone(),
                    area_m2: m2,
                    score: m2 / 1000.0 - seed.distance(a.center) / 1_000.0,
                    notes: vec![format!("free unzoned ground by a street, all accepting {}", lu.label().to_ascii_lowercase())],
                    ..Default::default()
                });
            }
            out.sort_by(|a, b| b.score.total_cmp(&a.score));
            out
        }
        Action::AddBusLine { from, to } => {
            let (Some(fa), Some(ta)) = (find_area(&areas, from), find_area(&areas, to)) else { return Vec::new() };
            let stops_in = |a: &Area, homes: bool| -> Vec<Vec2> {
                let mut v: Vec<(Vec2, u16)> = w
                    .buildings
                    .values()
                    .filter(|b| b.construction.is_none())
                    .filter_map(|b| w.lots.get(b.lot).map(|l| (l.entrance(), if homes { b.homes } else { b.jobs })))
                    .filter(|(p, n)| *n > 0 && in_area(w, &areas, a, *p))
                    .collect();
                v.sort_by(|x, y| y.1.cmp(&x.1).then(x.0.x.total_cmp(&y.0.x)));
                let mut out: Vec<Vec2> = Vec::new();
                for (p, _) in v {
                    if crate::transit::BusStop::snap(&w.roads, p).is_some() && out.iter().all(|q| q.distance(p) > 150.0) {
                        out.push(p);
                    }
                    if out.len() == 2 {
                        break;
                    }
                }
                out
            };
            let (home_stops, job_stops) = (stops_in(fa, true), stops_in(ta, false));
            if home_stops.is_empty() || job_stops.is_empty() {
                return Vec::new();
            }
            let name = |p: &Vec2| street_near(w, *p).unwrap_or_else(|| "a corner".into());
            let mut out = Vec::new();
            let full: Vec<Vec2> = home_stops.iter().chain(job_stops.iter()).copied().collect();
            out.push(SiteOption { label: format!("{} stops: {}", full.len(), full.iter().map(name).collect::<Vec<_>>().join(", ")), stops: full.clone(), area: format!("{} - {}", fa.name, ta.name), score: full.len() as f32, notes: vec!["the busiest homes and workplaces at each end".into()], ..Default::default() });
            let mid = (home_stops[0] + job_stops[0]) * 0.5;
            if crate::transit::BusStop::snap(&w.roads, mid).is_some() && home_stops[0].distance(job_stops[0]) > 500.0 {
                let short = vec![home_stops[0], mid, job_stops[0]];
                out.push(SiteOption { label: format!("3 stops: {}", short.iter().map(name).collect::<Vec<_>>().join(", ")), stops: short, area: format!("{} - {}", fa.name, ta.name), score: 2.5, notes: vec!["a quicker line with a stop halfway".into()], ..Default::default() });
            }
            out
        }
        Action::Remediate { .. } => {
            let Some(a) = area.as_ref() else { return Vec::new() };
            let c = &w.contamination;
            let n = c.n.max(1);
            let mut cells: Vec<(Vec2, f32)> = c.cells.iter().filter(|(_, &v)| v >= 0.1).map(|(&i, &v)| (c.cell_center(i as usize % n, i as usize / n), v)).filter(|(p, _)| in_area(w, &areas, a, *p)).collect();
            cells.sort_by(|x, y| y.1.total_cmp(&x.1).then(x.0.x.total_cmp(&y.0.x)));
            let mut out: Vec<SiteOption> = Vec::new();
            for (p, _) in cells {
                if out.iter().any(|o| o.point.unwrap().distance(p) < 160.0) {
                    continue;
                }
                let radius = 90.0;
                let amount = c.within(p, radius);
                if amount < 0.05 {
                    continue;
                }
                let cost = amount * CLEANUP_COST_ESTIMATE;
                let street = street_near(w, p).unwrap_or_else(|| "open ground".into());
                out.push(SiteOption { label: format!("hotspot near {street}, {}: {amount:.1} cells, about {} credits", a.name, thousands(cost)), point: Some(p), radius, area: a.name.clone(), cost, score: amount - cost / 50_000.0, notes: vec![format!("{} homes within 300 m", homes_near(w, p, 300.0))], ..Default::default() });
                if out.len() == 3 {
                    break;
                }
            }
            out.sort_by(|a, b| b.score.total_cmp(&a.score));
            out
        }
        _ => vec![SiteOption { label: format!("as voted: {}", action.title()), area: "citywide".into(), cost: action.cost_estimate(w), score: 1.0, ..Default::default() }],
    }
}

// ── the world's side: projects ──────────────────────────────────────────────

impl World {
    /// File a passed proposal as a committee project: its options, the
    /// secretary's pick and reason. Zoning goes into a world project of the
    /// same name too, parked, so the map previews it dimmed like the
    /// mayor's own plans. With no options the project waits for the mayor.
    pub fn committee_file_project(&mut self, meeting: u32, proposal: &Proposal, options: Vec<SiteOption>, pick: Option<usize>, reason: String) -> u32 {
        let id = self.committee.fresh_id();
        let name = format!("Committee: {}", proposal.title);
        let pick = pick.filter(|&i| i < options.len());
        let status = if pick.is_some() { ProjectStatus::Pending } else { ProjectStatus::NeedsMayor };
        let mut p = CommitteeProject { id, name, meeting, proposal: proposal.id, by: proposal.by, action: proposal.action.clone(), options, pick, reason, status, world_project: None, log: Vec::new() };
        self.stage_zoning(&mut p);
        self.committee.projects.push(p);
        id
    }

    /// For a zoning project with a site: the parked world project holding
    /// the painted parcels (replacing an older one).
    fn stage_zoning(&mut self, p: &mut CommitteeProject) {
        let Action::Rezone { to, .. } = &p.action else { return };
        let Some(lu) = parse_land_use(to) else { return };
        if let Some(old) = p.world_project.take() {
            self.delete_project(old);
        }
        let Some(site) = p.site().cloned() else { return };
        let ids: Vec<usize> = site.parcels.iter().filter_map(|k| self.parcels.parcels.iter().position(|q| q.key == *k)).collect();
        if ids.is_empty() {
            return;
        }
        let before = self.project.as_ref().map(|q| q.id);
        let wid = self.new_project(Some(&p.name));
        let (_, painted) = self.paint_parcels(&ids, Some(lu), None);
        match before {
            Some(b) => {
                self.switch_project(b);
            }
            None => {
                self.park_project();
            }
        }
        if painted == 0 {
            self.delete_project(wid);
            p.log.push("none of the parcels could be zoned any more".into());
            return;
        }
        p.world_project = Some(wid);
    }

    /// The mayor picks another of the options.
    pub fn committee_pick_site(&mut self, id: u32, pick: usize) -> Result<(), String> {
        let mut p = self.committee.project(id).cloned().ok_or("no such committee project")?;
        if !p.status.is_open() {
            return Err(format!("{} is {}", p.name, p.status.key()));
        }
        if pick >= p.options.len() {
            return Err(format!("{} has {} options", p.name, p.options.len()));
        }
        p.pick = Some(pick);
        p.status = ProjectStatus::Pending;
        p.log.push(format!("day {}: the mayor picked option {}", self.committee_today() + 1, pick + 1));
        self.stage_zoning(&mut p);
        *self.committee.project_mut(id).unwrap() = p;
        Ok(())
    }

    /// Look for sites again (the city changed, or there were none).
    pub fn committee_refresh_sites(&mut self, id: u32) -> Result<usize, String> {
        let mut p = self.committee.project(id).cloned().ok_or("no such committee project")?;
        if !p.status.is_open() {
            return Err(format!("{} is {}", p.name, p.status.key()));
        }
        p.options = site_options(self, &p.action);
        p.pick = if p.options.is_empty() { None } else { Some(0) };
        p.status = if p.pick.is_some() { ProjectStatus::Pending } else { ProjectStatus::NeedsMayor };
        p.log.push(format!("day {}: sites looked for again, {} found", self.committee_today() + 1, p.options.len()));
        self.stage_zoning(&mut p);
        let n = p.options.len();
        *self.committee.project_mut(id).unwrap() = p;
        Ok(n)
    }

    /// The mayor turns a committee project down.
    pub fn committee_dismiss_project(&mut self, id: u32) -> Result<(), String> {
        let today = self.committee_today();
        let p = self.committee.project(id).cloned().ok_or("no such committee project")?;
        if !p.status.is_open() {
            return Err(format!("{} is {}", p.name, p.status.key()));
        }
        if let Some(w) = p.world_project {
            self.delete_project(w);
        }
        let q = self.committee.project_mut(id).unwrap();
        q.world_project = None;
        q.status = ProjectStatus::Dismissed { day: today };
        q.log.push(format!("day {}: dismissed by the mayor", today + 1));
        self.committee_note_outcome(&p.name, &format!("the mayor dismissed it on day {}", today + 1), -1);
        Ok(())
    }

    /// The mayor implements a committee project: the one way the
    /// committee's work reaches the city. Returns what was done.
    pub fn committee_implement(&mut self, id: u32) -> Result<String, String> {
        let today = self.committee_today();
        let p = self.committee.project(id).cloned().ok_or("no such committee project")?;
        if !p.status.is_open() {
            return Err(format!("{} is {}", p.name, p.status.key()));
        }
        let result = self.carry_out(&p);
        let q = self.committee.project_mut(id).unwrap();
        match &result {
            Ok(done) => {
                q.status = ProjectStatus::Implemented { day: today };
                q.world_project = None;
                q.log.push(format!("day {}: implemented by the mayor: {done}", today + 1));
            }
            Err(why) => {
                q.status = ProjectStatus::Failed { why: why.clone() };
                q.log.push(format!("day {}: could not implement: {why}", today + 1));
            }
        }
        if result.is_ok() {
            self.committee_note_outcome(&p.name, &format!("implemented on day {}", today + 1), 0);
        }
        result
    }

    fn carry_out(&mut self, p: &CommitteeProject) -> Result<String, String> {
        let a = p.action.validate(self)?;
        let site = p.site().cloned();
        let need_site = || -> Result<SiteOption, String> { site.clone().ok_or_else(|| "no site picked: look for sites again or pick one".to_string()) };
        match &a {
            Action::BuildService { .. } | Action::BuildPower { .. } | Action::BuildDepot { .. } => {
                let s = need_site()?;
                let what = a.placeable().ok_or("unknown building")?;
                let pt = s.point.ok_or("the site has no point")?;
                let b = self.place_building(what, pt)?;
                let name = self.buildings.get(b).map_or_else(String::new, |b| b.name.clone());
                Ok(format!("{} \"{name}\" built on {}", a.title(), s.label))
            }
            Action::Rezone { .. } => {
                let wid = p.world_project.ok_or("the zoning plan is gone: look for sites again")?;
                let warnings = self.implement_project(wid).ok_or("the zoning plan is gone: look for sites again")?;
                if self.all_projects().any(|q| q.id == wid) {
                    return Err(warnings.first().cloned().unwrap_or_else(|| "the zoning was refused".into()));
                }
                Ok(format!("zoned {} parcels{}", site.map_or(0, |s| s.parcels.len()), if warnings.is_empty() { String::new() } else { format!(" ({})", warnings.join("; ")) }))
            }
            Action::AddBusLine { .. } => {
                let s = need_site()?;
                let route = self.add_bus_route();
                let mut added = 0;
                for q in &s.stops {
                    if self.add_bus_stop(route, *q).is_ok() {
                        added += 1;
                    }
                }
                if added < 2 {
                    self.remove_bus_route(route);
                    return Err("the stops no longer sit by a street".into());
                }
                let short = p.name.trim_start_matches("Committee: ").to_string();
                let _ = self.rename_bus_route(route, &short);
                self.set_route_buses(route, 2);
                Ok(format!("bus line \"{short}\" with {added} stops and 2 buses"))
            }
            Action::ChangeTax { category, delta_pct } => {
                let mut done = Vec::new();
                for c in tax_categories(category) {
                    let now = self.economy.taxes.rate(c);
                    let set = self.set_tax_rate(c, now + delta_pct / 100.0);
                    done.push(format!("{} {:.1}% -> {:.1}%", c.label(), now * 100.0, set * 100.0));
                }
                Ok(done.join(", "))
            }
            Action::SetToll { toll, price } => {
                let ids: Vec<u32> = match toll {
                    Some(id) => vec![*id],
                    None => self.tolls.booths.iter().map(|b| b.id).collect(),
                };
                for id in &ids {
                    self.set_toll_price(*id, *price)?;
                }
                Ok(format!("{} toll booth(s) at {price:.0} credits", ids.len()))
            }
            Action::RoadRepair { funding } => {
                let set = self.set_road_funding(*funding);
                Ok(format!("road maintenance funding at {:.0}%", set * 100.0))
            }
            Action::Remediate { .. } => {
                let s = need_site()?;
                let (cost, amount) = self.remediate(s.point.ok_or("the site has no point")?, s.radius.max(40.0))?;
                Ok(format!("{amount:.1} cells of ground cleaned for {} credits", thousands(cost)))
            }
        }
    }

    /// Tell the members who voted on `name` what came of it.
    fn committee_note_outcome(&mut self, project_name: &str, outcome: &str, verdict: i8) {
        let title = project_name.trim_start_matches("Committee: ");
        for p in self.committee.pool.iter_mut() {
            if let Some(m) = p.memory.iter_mut().rev().find(|m| m.proposal == title) {
                m.outcome = outcome.to_string();
                if verdict != 0 {
                    m.verdict = verdict;
                }
            }
        }
    }
}

impl Committee {
    /// After the votes: every seated member remembers how they voted and
    /// whether it passed, with their scope's headline figure to judge it by
    /// later; the record counts go up.
    pub fn remember_votes(&mut self, meeting: u32, day: u32, proposals: &[Proposal], report: &CityReport) {
        for prop in proposals {
            let passed = prop.outcome == Some(super::Outcome::Passed);
            if let Some(m) = self.member_mut(prop.by) {
                m.record.proposed += 1;
                if passed {
                    m.record.proposals_passed += 1;
                }
            }
            for b in &prop.ballots {
                let Some(m) = self.member_mut(b.member) else { continue };
                match b.vote {
                    Vote::Yes => m.record.votes_yes += 1,
                    Vote::No => m.record.votes_no += 1,
                    Vote::Abstain => m.record.abstained += 1,
                }
                let key = CityReport::scope_metric(m.scope);
                let entry = MemoryEntry { day, meeting, proposal: prop.title.clone(), kind: prop.action.kind_key().into(), vote: Some(b.vote), passed, proposed: prop.by == b.member, metric: report.metric(key).map(|v| (key.to_string(), v)), outcome: String::new(), verdict: 0 };
                m.remember(entry);
            }
        }
    }

    /// At a meeting's start: judge what members remember by how their
    /// figures moved since ("crime down 18% since").
    pub fn judge_memories(&mut self, report: &CityReport) {
        for p in self.pool.iter_mut() {
            for m in p.memory.iter_mut() {
                let Some((key, then)) = m.metric.clone() else { continue };
                let Some(now) = report.metric(&key) else { continue };
                if m.day >= report.day {
                    continue;
                }
                let d = now - then;
                let rel = if then.abs() > 1e-3 { d / then.abs() } else { d };
                let label = key.replace('_', " ");
                let moved = if rel.abs() < 0.03 { format!("{label} flat since") } else { format!("{label} {} {:.0}% since", if d > 0.0 { "up" } else { "down" }, rel.abs() * 100.0) };
                let good = (d > 0.0) == CityReport::higher_is_better(&key);
                let base = m.outcome.split(';').next().unwrap_or("").trim().to_string();
                let base = if base.contains(" since") { String::new() } else { base };
                m.outcome = if base.is_empty() { moved } else { format!("{base}; {moved}") };
                if m.passed && m.verdict == 0 && rel.abs() >= 0.03 {
                    m.verdict = if good { 1 } else { -1 };
                }
            }
        }
    }
}
