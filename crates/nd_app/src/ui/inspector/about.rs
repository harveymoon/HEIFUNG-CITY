//! [feat/pt-ux] The inspector's first box on a building: what it is, in a
//! sentence, and what it does for the city (its benefits and its costs to
//! the neighbours), for the player who clicked it without knowing.

use nd_sim::{BuildingId, BusinessKind, LandUse, World};

use crate::ui::fmt::{capitalise, s};

/// What building `id` is, and what it gives. None for a building that is
/// gone.
pub fn about(w: &World, id: BuildingId) -> Option<(String, String)> {
    let b = w.buildings.get(id)?;
    let area = w.footprint(id).map_or(0.0, |l| nd_sim::geom::polygon_area(&l.polygon).abs());
    if b.land_use == LandUse::Park {
        let beach = w.lots.get(b.lot).is_some_and(|l| l.shore.beach);
        // A park over several plots is one park: its whole area.
        let area = w.lots.get(b.lot).and_then(|l| l.park.as_ref()).map_or(area, |p| p.cluster_area.max(area));
        let what = if beach { format!("A public beach of {:.0} m².", area) } else { format!("A public park of {:.0} m².", area) };
        return Some((what, "People within walking reach come to walk, sit and play; it lifts land value and moods round it, and costs the city its upkeep.".into()));
    }
    if let Some(sv) = b.service {
        // [attractions] A landmark: what it is, and what it does for the city.
        if sv.kind.is_landmark() {
            return Some((format!("The city's {}, one of a kind: a landmark attraction run by the city.", sv.kind.label()), format!("{} {}", nd_sim::attractions::about(sv.kind), crate::ui::help::landmark_rules(sv.kind))));
        }
        return Some((format!("{} run by the city.", capitalise(&a(sv.kind.label()))), crate::ui::help::service_note(sv.kind).to_string()));
    }
    if let Some(k) = b.plant {
        let what = format!("{} power plant: {:.0} of {:.0} MW now.", capitalise(&a(k.label())), w.plant_output_mw(b), w.plant_capacity_mw(b));
        let note = crate::ui::help::power_note(k).trim_start_matches(['.', ' ']).to_string();
        return Some((what, format!("Feeds the grid every building draws on. {note}")));
    }
    if let Some(bat) = &b.battery {
        return Some((format!("A grid battery holding {:.0} MWh.", bat.charge_mwh), "Soaks up the plants' surplus and covers shortfalls before power is bought in.".into()));
    }
    if b.depot {
        return Some(("A bus depot.".into(), "Buses on the lines park here overnight and leave at 05:30; without a depot a line sends no buses.".into()));
    }
    if let Some(p) = w.placed_as(id) {
        if matches!(p, nd_sim::Placeable::MetroStation) {
            return Some(("A metro station.".into(), "Riders walk in from round about and ride the lines underground, above the traffic; it lifts land value nearby.".into()));
        }
        if b.rail.is_some() || b.port.is_some() {
            return Some((format!("{}.", capitalise(&a(&p.label()))), "Links the city to elsewhere: travellers and goods come and go through it.".into()));
        }
    }
    if let Some(z) = &b.business {
        let jobs = format!("{} job{}", b.jobs, s(b.jobs as usize));
        let noun = match z.kind {
            BusinessKind::Parking => "car park".to_string(),
            k => k.label().to_string(),
        };
        let what = format!("{}, {jobs}.", capitalise(&a(&noun)));
        let gives = match z.kind {
            BusinessKind::Parking => format!("{} parking spaces ({} taken now): drivers coming to this street pay to park here instead of circling the block.", b.parking_capacity, b.parking_used),
            BusinessKind::Office => "Office work: steady, well-paid jobs. Its staff buy lunch and coffee nearby.".into(),
            BusinessKind::Hotel => "Rooms for visitors: tourists sleep here and spend in the shops, bars and sights round it.".into(),
            BusinessKind::TowYard => "Tows broken-down and illegally parked cars to its pound; owners pay to get them back.".into(),
            BusinessKind::TaxiCompany => "Runs taxis: people without a car get a ride, for a fare.".into(),
            BusinessKind::CarDealer => "Sells cars: more people drive, and more cars need parking.".into(),
            BusinessKind::Mechanic => "Fixes the town's cars.".into(),
            BusinessKind::TrashCompany => "A private hauler: empties the bins of whoever pays.".into(),
            BusinessKind::CargoHub => "Freight terminal: businesses nearby import through it in shared trucks instead of one lorry each.".into(),
            k if k.is_lodging() => "Rooms for visitors: tourists spend in town.".into(),
            k if k.is_food() => "Feeds the neighbourhood: people on foot drop in to eat and shop, and a street of them draws more.".into(),
            k if k.is_outing() => "Somewhere to go out: draws crowds (and some noise) in the evenings.".into(),
            k => match k.industry() {
                Some(ind) => format!("{} industry: makes goods that shops and other firms buy, with freight trucks in and out{}.", capitalise(ind.label()), if dirty(k) { ", and pollution the neighbours breathe" } else { "" }),
                None => "Sells to the people round about; its customers come on foot and by car.".into(),
            },
        };
        return Some((what, gives));
    }
    if b.homes > 0 {
        let what = format!("Homes: {} of {} let ({} household{}).", b.households.len(), b.homes, b.households.len(), s(b.households.len()));
        let gives = format!("Where its people sleep: they work, shop and go out nearby. Rent {:.0} a day; it follows the land value.", b.rent);
        return Some((what, gives));
    }
    Some((format!("{} building.", capitalise(&a(&b.land_use.label().to_lowercase()))), "Built on its zoning; what it does follows its use.".into()))
}

/// Industries that foul the air.
fn dirty(k: BusinessKind) -> bool {
    matches!(k, BusinessKind::Refinery | BusinessKind::ChemicalWorks | BusinessKind::Smelter | BusinessKind::OilWell | BusinessKind::StripMine | BusinessKind::Slaughterhouse | BusinessKind::Feedlot)
}

/// "a thing" / "an office".
fn a(noun: &str) -> String {
    let vowel = noun.chars().next().is_some_and(|c| "aeiouAEIOU".contains(c));
    format!("{} {noun}", if vowel { "an" } else { "a" })
}

#[cfg(test)]
mod tests {
    use super::a;

    #[test]
    fn articles() {
        assert_eq!(a("office"), "an office");
        assert_eq!(a("grocery"), "a grocery");
    }
}
