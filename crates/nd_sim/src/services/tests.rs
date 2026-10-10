use super::*;
use crate::ports::PortKind;
use crate::power::PowerPlantKind;
use crate::rail::RailFacility;

#[test]
fn buildings_tool_categories_hold_every_placeable_once() {
    let mut seen: Vec<Placeable> = Vec::new();
    for cat in BuildCategory::ALL {
        let items = cat.items();
        assert!(!items.is_empty(), "{cat:?} offers nothing");
        for p in items {
            assert_eq!(p.category(), cat, "{p:?} listed under {cat:?}");
            assert!(!seen.contains(&p), "{p:?} listed twice");
            // [tolls] A toll booth is no building: its collectors are in the upkeep.
            let staffed = p.jobs() > 0 || matches!(p, Placeable::Toll(_));
            assert!(p.cost() > 0.0 && p.upkeep() > 0.0 && staffed, "{p:?} has no cost, upkeep or jobs");
            seen.push(p);
        }
    }
    // Power is a sub-category with every plant and every battery size; every service kind is somewhere.
    assert_eq!(BuildCategory::Power.items().len(), PowerPlantKind::ALL.len() + crate::power::BatteryKind::ALL.len());
    for k in ServiceKind::ALL {
        assert!(seen.iter().any(|p| matches!(p, Placeable::Service(s, _) if *s == k)), "{k:?} is not on the menu");
    }
    assert!(seen.contains(&Placeable::BusDepot));
    assert!(crate::tolls::TollSize::OFFERED.iter().all(|&t| BuildCategory::Transit.items().contains(&Placeable::Toll(t))), "toll booths under Transit");
    assert!(seen.contains(&Placeable::Rail(RailFacility::Station)) && seen.contains(&Placeable::Rail(RailFacility::FreightYard)));
    assert!(seen.contains(&Placeable::Dock(PortKind::ShippingPort)) && seen.contains(&Placeable::Dock(PortKind::FerryTerminal)), "docks live under Water");
    assert_eq!(BuildCategory::Attractions.items().len(), 2);
}

#[test]
fn tiers_scale_cost_capacity_and_reach() {
    for k in ServiceKind::ALL {
        let [l, m, h] = Tier::ALL.map(|t| Placeable::Service(k, t));
        assert!(l.cost() < m.cost() && m.cost() < h.cost(), "{k:?} cost");
        assert!(l.upkeep() < m.upkeep() && m.upkeep() < h.upkeep(), "{k:?} upkeep");
        assert!(l.jobs() <= m.jobs() && m.jobs() < h.jobs(), "{k:?} jobs");
        assert!(k.min_area(Tier::Low) < k.min_area(Tier::High));
        if k.coverage_index().is_some() {
            assert!(l.radius() < m.radius() && m.radius() < h.radius(), "{k:?} radius");
            assert!(l.radius() > COVERAGE_FADE, "{k:?} low tier must cover something fully");
        } else if k != ServiceKind::TrashDepot && k != ServiceKind::ParkingEnforcement {
            assert_eq!(m.radius(), 0.0);
        }
        if k.unit_kind().is_some() {
            assert!(k.units(Tier::Low) < k.units(Tier::High));
            // A fully staffed station can crew every vehicle it has.
            for t in Tier::ALL {
                assert!(k.jobs(t) as usize >= k.units(t) * k.unit_kind().unwrap().crew(), "{k:?} {t:?} cannot crew its units");
            }
        }
    }
}

#[test]
fn remote_kinds_parse_with_tiers() {
    assert_eq!(Placeable::parse("police", None), Some(Placeable::Service(ServiceKind::Police, Tier::Mid)));
    assert_eq!(Placeable::parse("fire:high", None), Some(Placeable::Service(ServiceKind::Fire, Tier::High)));
    assert_eq!(Placeable::parse("hospital", Some(Tier::Low)), Some(Placeable::Service(ServiceKind::Health, Tier::Low)));
    assert_eq!(Placeable::parse("clinic", None), Some(Placeable::Service(ServiceKind::Health, Tier::Low)));
    assert_eq!(Placeable::parse("Stadium/LOW", None), Some(Placeable::Service(ServiceKind::Stadium, Tier::Low)));
    assert_eq!(Placeable::parse("solar", Some(Tier::High)), Some(Placeable::Plant(PowerPlantKind::Solar)));
    assert_eq!(Placeable::parse("yard", None), Some(Placeable::Rail(RailFacility::FreightYard)));
    assert_eq!(Placeable::parse("geothermal", None), Some(Placeable::Plant(PowerPlantKind::Geothermal)));
    assert_eq!(Placeable::parse("ferry", None), Some(Placeable::Dock(PortKind::FerryTerminal)));
    assert_eq!(Placeable::parse("vertiport", None), Some(Placeable::Service(ServiceKind::Vertiport, Tier::Mid)));
    assert_eq!(Placeable::parse("skyport:high", None), Some(Placeable::Service(ServiceKind::Vertiport, Tier::High)));
    assert_eq!(Placeable::Service(ServiceKind::Vertiport, Tier::Low).category(), BuildCategory::Transit);
    assert_eq!(Placeable::parse("castle", None), None);
}
