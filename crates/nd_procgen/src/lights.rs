//! How much of a building's windows are lit, from who is inside and what
//! they are doing. The renderer marks every window light with a rank (see
//! `buildings::parts::window_light`) and lights the windows whose rank is
//! below the share computed here, so a building fills up window by window
//! as people come home and goes dark as they fall asleep.

/// Who is in a building right now, and what it is.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Occupancy {
    /// Homes in the building.
    pub homes: u16,
    /// Jobs in the building.
    pub jobs: u16,
    /// Residents at home and awake.
    pub awake_home: u16,
    /// Residents at home asleep.
    pub asleep_home: u16,
    /// Workers inside.
    pub staff_in: u16,
    /// Customers and guests inside.
    pub visitors: u16,
    /// Inside its opening hours (a business), or always (no business).
    pub open: bool,
    /// Hour of day, `0..24`.
    pub hour: f32,
    /// Share of the building's power demand met, `0..1`.
    pub powered: f32,
}

/// Average household size used to turn people at home into lit rooms.
const PEOPLE_PER_HOME: f32 = 1.6;

/// Share of a building's windows lit, `0..1`.
///
/// - Homes: a lit room per awake household at home, fewer in broad
///   daylight; sleepers leave only a night light; nobody home, dark.
/// - Workplaces: lit in proportion to the staff in, plus customers while
///   open; a closed, empty one keeps a security light or two.
/// - Buildings with neither (plants, depots, parks): lit.
/// - Everything dims with the power share and is dark without power.
pub fn lit_share(o: &Occupancy) -> f32 {
    let power = o.powered.clamp(0.0, 1.0);
    if power < 0.05 {
        return 0.0;
    }
    let day = (6.5..18.0).contains(&o.hour);
    let home = if o.homes > 0 {
        let cap = o.homes as f32 * PEOPLE_PER_HOME;
        let awake = (o.awake_home as f32 / cap).min(1.0);
        let asleep = (o.asleep_home as f32 / cap).min(1.0);
        // A lamp or two stays on in any occupied home (hall lights, night
        // owls), so a sleeping block is dim rather than black.
        let anyone = if o.awake_home + o.asleep_home > 0 { 0.06 } else { 0.0 };
        (anyone + awake * if day { 0.45 } else { 0.95 } + asleep * 0.06).min(1.0)
    } else {
        0.0
    };
    let work = if o.jobs > 0 {
        let staff = (o.staff_in as f32 / o.jobs as f32).min(1.0);
        if o.staff_in == 0 && !(o.open && o.visitors > 0) {
            0.05
        } else {
            let custom = if o.open { (o.visitors as f32 / (o.jobs as f32 * 2.0)).min(1.0) * 0.25 } else { 0.0 };
            (0.3 + 0.55 * staff + custom).min(1.0)
        }
    } else {
        0.0
    };
    let share = match (o.homes > 0, o.jobs > 0) {
        (false, false) => 1.0,
        (true, false) => home,
        (false, true) => work,
        (true, true) => {
            let wh = o.homes as f32 * PEOPLE_PER_HOME;
            let wj = o.jobs as f32;
            (home * wh + work * wj) / (wh + wj)
        }
    };
    share * (0.25 + 0.75 * power)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn home(awake: u16, asleep: u16, hour: f32) -> Occupancy {
        Occupancy { homes: 10, awake_home: awake, asleep_home: asleep, open: true, hour, powered: 1.0, ..Default::default() }
    }

    #[test]
    fn homes_light_up_when_people_are_in_and_awake() {
        let evening = lit_share(&home(14, 0, 20.5));
        let asleep = lit_share(&home(0, 16, 2.0));
        let empty = lit_share(&home(0, 0, 20.5));
        let noon = lit_share(&home(14, 0, 12.0));
        assert!(evening > 0.7, "awake households in the evening: {evening}");
        assert!(asleep < 0.15 && asleep > 0.0, "everyone asleep: a night light or two, {asleep}");
        assert_eq!(empty, 0.0, "nobody home");
        assert!(noon < evening, "fewer lamps on at noon");
        assert!(lit_share(&home(4, 0, 21.0)) < evening, "more people home, more windows");
    }

    #[test]
    fn workplaces_follow_staff_and_opening_hours() {
        let shop = |staff: u16, visitors: u16, open: bool| lit_share(&Occupancy { jobs: 8, staff_in: staff, visitors, open, hour: 15.0, powered: 1.0, ..Default::default() });
        assert!(shop(8, 10, true) > 0.85, "open and staffed");
        assert!(shop(2, 0, false) > 0.3, "staff in after hours keep some lights on");
        assert!(shop(0, 0, false) < 0.1, "closed and empty: security lights only");
        assert!(shop(8, 10, true) > shop(2, 0, true));
    }

    #[test]
    fn power_and_infrastructure() {
        let plant = Occupancy { open: true, hour: 23.0, powered: 1.0, ..Default::default() };
        assert_eq!(lit_share(&plant), 1.0, "no homes or jobs: lit");
        let dark = Occupancy { powered: 0.0, ..home(14, 0, 21.0) };
        assert_eq!(lit_share(&dark), 0.0, "no power, no light");
        let brown = Occupancy { powered: 0.4, ..home(14, 0, 21.0) };
        assert!(lit_share(&brown) < lit_share(&home(14, 0, 21.0)));
        let mixed = Occupancy { homes: 10, jobs: 10, awake_home: 16, staff_in: 0, open: false, hour: 22.0, powered: 1.0, ..Default::default() };
        let s = lit_share(&mixed);
        assert!(s > 0.3 && s < 1.0, "flats over a closed shop: {s}");
    }
}
