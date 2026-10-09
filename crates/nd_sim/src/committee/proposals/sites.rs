//! Where a proposal could be carried out: the site options offered.

use super::*;

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
pub(super) fn street_near(w: &World, p: Vec2) -> Option<String> {
    let lot = w.lots.values().min_by(|a, b| a.center.distance_squared(p).total_cmp(&b.center.distance_squared(p)))?;
    if lot.center.distance(p) > 150.0 {
        return None;
    }
    w.streets.of(lot.front_segment).map(|s| s.name.clone())
}

/// Homes within `reach` of `p` without good cover by `kind`.
pub(super) fn uncovered_homes(w: &World, p: Vec2, kind: ServiceKind, reach: f32) -> f32 {
    w.buildings
        .values()
        .filter(|b| b.homes > 0 && b.operating())
        .filter_map(|b| w.lots.get(b.lot).map(|l| (b, l)))
        .filter(|(_, l)| l.center.distance(p) <= reach)
        .map(|(b, _)| b.homes as f32 * (1.0 - w.services.coverage_of(b.lot, kind)).max(0.0))
        .sum()
}

pub(super) fn homes_near(w: &World, p: Vec2, reach: f32) -> f32 {
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
                    .filter(|b| b.operating())
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
