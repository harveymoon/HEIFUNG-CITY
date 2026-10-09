//! What a proposal may do: parsing an action's words, checking it against
//! the city, its scope, title and cost.

use super::*;

pub(super) fn parse_tier(s: &str) -> Option<Tier> {
    match s.trim().to_ascii_lowercase().as_str() {
        "" | "low" | "small" | "1" => Some(Tier::Low),
        "mid" | "middle" | "medium" | "2" => Some(Tier::Mid),
        "high" | "large" | "big" | "3" => Some(Tier::High),
        _ => None,
    }
}

pub(super) fn tier_key(t: Tier) -> &'static str {
    match t {
        Tier::Low => "low",
        Tier::Mid => "mid",
        Tier::High => "high",
    }
}

pub(super) fn parse_plant(s: &str) -> Option<PowerPlantKind> {
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

    pub(super) fn placeable(&self) -> Option<Placeable> {
        match self {
            Action::BuildService { service, tier, .. } => Some(Placeable::Service(ServiceKind::from_key(service)?, parse_tier(tier)?)),
            Action::BuildPower { plant, .. } => Some(Placeable::Plant(parse_plant(plant)?)),
            Action::BuildDepot { .. } => Some(Placeable::BusDepot),
            _ => None,
        }
    }

    pub(super) fn near(&self) -> &str {
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
                let d = delta_pct.abs();
                let pts = if (d - d.round()).abs() < 0.05 { format!("{d:.0} point{}", if (d - 1.0).abs() < 0.05 { "" } else { "s" }) } else { format!("{d:.1} points") };
                format!("{} {what} by {pts}", if *delta_pct >= 0.0 { "Raise" } else { "Cut" })
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
    pub(super) fn traits(&self) -> (f32, f32, f32) {
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
                if !w.buildings.values().any(|b| b.depot && b.operating()) {
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
pub(super) fn area_contamination(w: &World, areas: &[Area], a: &Area) -> f32 {
    let c = &w.contamination;
    let n = c.n.max(1);
    c.cells.iter().filter(|(_, &v)| v >= 0.05).map(|(&i, &v)| (c.cell_center(i as usize % n, i as usize / n), v)).filter(|(p, _)| in_area(w, areas, a, *p)).map(|(_, v)| v).sum()
}
