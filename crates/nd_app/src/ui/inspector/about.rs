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
    // [fix/buildings-ui] A building coming down with nothing to follow, or
    // a grown one still going up: what it is now, not what it did or will
    // do (a site has no homes, jobs or lights yet).
    // (A city structure taking the lot over describes what is coming.)
    if let Some(d) = b.demolition.as_ref().filter(|_| w.placed_as(id).is_none()) {
        let what = format!("{} coming down.", capitalise(&a(&format!("{} building", d.land_use.label().to_lowercase()))));
        let then = if d.clear_lot { "The lot is left vacant once the rubble is hauled away." } else { "A new building goes up once the rubble is hauled away." };
        return Some((what, format!("Stripped out and being pulled down: the power is off and nobody lives or works here any more. {then}")));
    }
    if b.construction.is_some() && w.placed_as(id).is_none() {
        let noun = b.business.as_ref().map_or_else(|| format!("{} building", b.land_use.label().to_lowercase()), |z| z.kind.label().to_string());
        return Some((format!("{} going up.", capitalise(&a(&noun))), "A building site: only the crew works here. Its homes, jobs and lights come when it is finished.".into()));
    }
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
        if let Some(note) = placed_note((b.parking_capacity, b.parking_used), p) {
            return Some(note);
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
            k => business_note(k),
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

/// [fix/buildings-ui] What one of the city's placed buildings (a station,
/// dock, depot, car park or hub) does, by its kind: a fishing port is not a
/// gateway to elsewhere. None leaves it to the other notes.
fn placed_note(parking: (u16, u16), p: nd_sim::Placeable) -> Option<(String, String)> {
    use nd_sim::{Placeable as P, PortKind as K, RailFacility as R};
    let what = format!("{}.", capitalise(&a(&p.label())));
    let gives: String = match p {
        P::MetroStation => return Some(("A metro station.".into(), "Riders walk in from round about and ride the lines underground, above the traffic; it lifts land value nearby.".into())),
        P::Rail(R::Station) => "Passenger trains call here: people ride the line across town and out to elsewhere, and visitors arrive; it lifts land value nearby.".into(),
        P::Rail(R::Central) => return Some(("Central Station.".into(), "The city's main station: intercity trains, a metro station and a bus terminal under one roof. Travellers come and go to elsewhere through it.".into())),
        P::Rail(R::FreightYard) => "Goods trains load and unload here: businesses import and export by rail instead of by lorry from the edge of the map.".into(),
        P::Dock(K::ShippingPort) => "Container ships bring imports in cheaper than by road and carry exports out for more; freight lorries come and go from its yard.".into(),
        P::Dock(K::FerryTerminal) => "Ferries carry passengers to the other terminals on connected water, or out to elsewhere, for a fare.".into(),
        P::Dock(K::FerryTerminalLarge) => "Car ferries carry passengers and their cars to the other terminals on connected water, or out to elsewhere: a fare a head and a dearer one a car.".into(),
        P::Dock(K::MarinaSmall) => "Berths let to the wealthy households round about for their boats; it lifts land value nearby and draws seafood restaurants.".into(),
        P::Dock(K::MarinaLarge) => "Berths for boats and yachts from all over town, and a clubhouse to go out to; it lifts land value nearby and draws seafood restaurants.".into(),
        P::Dock(K::FishingPortSmall | K::FishingPortLarge) => "Adds to the local fishing economy and provides a dock for the fishing fleet: its trawlers put out before dawn and land fish for the gutting sheds, the fish-stick factory and the fish markets nearby.".into(),
        P::CargoHub => "Freight terminal: businesses nearby import through it in shared trucks instead of one lorry each.".into(),
        P::ParkingLot(_) => format!("A car park the city runs: {} spaces ({} taken now). Drivers pay to park here instead of circling the block.", parking.0, parking.1),
        P::BusDepot => return Some(("A bus depot.".into(), "Buses on the lines park here overnight and leave at 05:30; without a depot a line sends no buses.".into())),
        // [airport]
        P::Airport(_) => return Some((what, "Planes to and from towns off the map: visitors fly in to the hotels, residents and business travellers fly out, and dear goods come and go by air cargo. Landing and passenger fees pay for it; the flight paths are loud and cheapen the land under them.".into())),
        P::Plant(_) | P::Battery(_) | P::Service(..) | P::Toll(_) => return None,
    };
    Some((what, gives))
}

/// [fix/buildings-ui] What a business does for the town, kind by kind (the
/// farms and sheds of the food chain are not corner shops). [r13-catalogue]
/// The lines are the catalogue's (`nd_sim::catalogue`, `about`).
pub(crate) fn business_note(k: BusinessKind) -> String {
    let row = k.row();
    let dirt = if row.fouls_air { ", and pollution the neighbours breathe" } else { "" };
    match row.about {
        Some(text) => text.replace("{dirt}", dirt),
        None => match k.industry() {
            Some(ind) => format!("{} industry: makes goods that shops and other firms buy, with freight trucks in and out{dirt}.", capitalise(ind.label())),
            None => "Sells to the people round about; its customers come on foot and by car.".into(),
        },
    }
}

/// "a thing" / "an office" (by the sound: `nd_sim::names::a`).
fn a(noun: &str) -> String {
    nd_sim::names::a(noun)
}

#[cfg(test)]
mod tests {
    use super::{a, business_note, placed_note};

    #[test]
    fn fishing_port_is_not_a_gateway() {
        for k in [nd_sim::PortKind::FishingPortSmall, nd_sim::PortKind::FishingPortLarge] {
            let (what, gives) = placed_note((0, 0), nd_sim::Placeable::Dock(k)).unwrap();
            assert!(what.contains("fishing port"), "{what}");
            assert!(gives.contains("fishing economy") && gives.contains("fishing fleet"), "{gives}");
            assert!(!gives.contains("elsewhere"), "{gives}");
        }
        // Only the gateways link to elsewhere.
        for k in nd_sim::PortKind::ALL {
            let (_, gives) = placed_note((0, 0), nd_sim::Placeable::Dock(k)).unwrap();
            assert_eq!(gives.contains("elsewhere"), k.is_ferry(), "{k:?}: {gives}");
        }
    }

    #[test]
    fn farms_are_not_shops() {
        for k in [nd_sim::BusinessKind::GrainFarm, nd_sim::BusinessKind::Slaughterhouse, nd_sim::BusinessKind::FishGutting] {
            assert!(!business_note(k).contains("drop in"), "{k:?}");
        }
    }

    #[test]
    fn articles() {
        assert_eq!(a("office"), "an office");
        assert_eq!(a("grocery"), "a grocery");
        assert_eq!(a("observatory"), "an observatory");
        assert_eq!(a("university"), "a university");
    }
}
