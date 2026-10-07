//! [camera] The player's hand in a resident's life: a gift of money from
//! the treasury to their household, and the places that are theirs (home,
//! work, businesses they own) for the inspector's map pins.

use super::*;
use crate::economy::{Flow, Item};

/// What the inspector's gift button gives.
pub const GIFT_AMOUNT: f32 = 1000.0;

/// A gift that was given: who got the money and where it went.
#[derive(Clone, Debug, PartialEq)]
pub struct Gift {
    pub citizen: CitizenId,
    /// The household's purse, or `None` when it went to the citizen's own
    /// savings (someone without a household).
    pub household: Option<HouseholdId>,
    pub amount: f32,
    /// The treasury after the gift.
    pub treasury: f32,
}

/// How a building belongs in someone's life.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Holding {
    Home,
    Work,
    /// A business they own (a firm they founded).
    Owns,
}

impl Holding {
    pub fn label(self) -> &'static str {
        match self {
            Holding::Home => "home",
            Holding::Work => "work",
            Holding::Owns => "owns",
        }
    }
}

impl World {
    /// Move `amount` from the city treasury to `citizen`'s household (their
    /// own savings if they have none). Refused when the treasury cannot pay
    /// or the amount is not positive. Booked as `Flow::Gifts`, itemised by
    /// the receiver's name; they remember it and cheer up.
    pub fn gift(&mut self, citizen: CitizenId, amount: f32) -> Result<Gift, String> {
        if !(amount > 0.0 && amount.is_finite()) {
            return Err("the gift must be a positive amount".into());
        }
        let Some(c) = self.citizens.get(citizen) else { return Err("no such citizen".into()) };
        if self.economy.treasury < amount {
            return Err(format!("not enough money: the treasury has {:.0}", self.economy.treasury.max(0.0)));
        }
        let name = c.name.clone();
        let household = c.household.filter(|h| self.households.contains_key(*h));
        self.economy.debit_item(Flow::Gifts, amount, Item::Named(name));
        let members = match household {
            Some(h) => {
                let hh = &mut self.households[h];
                hh.money += amount;
                hh.members.clone()
            }
            None => {
                self.citizens[citizen].savings += amount;
                vec![citizen]
            }
        };
        let tick = self.clock.tick;
        for m in members {
            if let Some(x) = self.citizens.get_mut(m) {
                x.mood = (x.mood + if m == citizen { 0.15 } else { 0.05 }).min(1.0);
            }
        }
        self.citizens[citizen].remember(tick, format!("The city sent us a gift of ${amount:.0}. Somebody up there likes me."));
        Ok(Gift { citizen, household, amount, treasury: self.economy.treasury })
    }

    /// The buildings that are `citizen`'s: their home, their work, then any
    /// business they own, each once (a founder working at their own firm
    /// has it as work), in that order. Empty for someone unknown.
    pub fn holdings_of(&self, citizen: CitizenId) -> Vec<(BuildingId, Holding)> {
        let Some(c) = self.citizens.get(citizen) else { return Vec::new() };
        let mut out: Vec<(BuildingId, Holding)> = Vec::new();
        let add = |b: BuildingId, h: Holding, out: &mut Vec<(BuildingId, Holding)>| {
            if self.buildings.contains_key(b) && !out.iter().any(|(x, _)| *x == b) {
                out.push((b, h));
            }
        };
        if let Some(h) = c.home {
            add(h, Holding::Home, &mut out);
        }
        if let Some(wk) = c.work {
            add(wk, Holding::Work, &mut out);
        }
        // [land] Everything they own, from the owner records (a
        // billionaire's other homes and firms too), in a stable order.
        for b in self.properties_of(citizen) {
            add(b, Holding::Owns, &mut out);
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::citizens::Household;
    use crate::landscape::LandscapeParams;

    fn town() -> (World, CitizenId, HouseholdId, BuildingId, BuildingId) {
        let mut w = World::new(WorldConfig { landscape: LandscapeParams::flat(), ..WorldConfig::test_town() });
        w.add_road(&[Vec2::new(-300.0, 0.0), Vec2::new(300.0, 0.0)], 1, 1);
        let sq = |x0: f32, y0: f32, s: f32| vec![Vec2::new(x0, y0), Vec2::new(x0 + s, y0), Vec2::new(x0 + s, y0 + s), Vec2::new(x0, y0 + s)];
        w.add_zone(sq(-150.0, 8.0, 300.0), LandUse::Residential);
        w.add_zone(sq(-150.0, -308.0, 300.0), LandUse::Commercial);
        let lots: Vec<LotId> = w.lots.iter().filter(|(_, l)| l.building.is_none() && l.usable()).map(|(id, _)| id).collect();
        for l in lots {
            w.build_on(l);
        }
        let sites: Vec<BuildingId> = w.buildings.keys().collect();
        for b in sites {
            w.finish_construction(b);
        }
        let home = w.buildings.iter().find(|(_, b)| b.land_use == LandUse::Residential).map(|(id, _)| id).unwrap();
        let shop = w.buildings.iter().find(|(_, b)| b.land_use == LandUse::Commercial).map(|(id, _)| id).unwrap();
        let hid = w.households.insert(Household { members: Vec::new(), home: Some(home), money: 100.0, groceries: 5.0, ..Default::default() });
        w.buildings[home].households.push(hid);
        let e = w.building_entrance(home).unwrap();
        let id = w.citizens.insert(Citizen {
            name: "Mara Voss".into(),
            seed: 7,
            home: Some(home),
            work: Some(shop),
            pos: e,
            heading: 0.0,
            prev_pos: e,
            prev_heading: 0.0,
            mode: crate::citizens::TravelMode::Walk,
            car: None,
            commute: Default::default(),
            look: None,
            to_car: None,
            drunk: Default::default(),
            owns_car: false,
            owns_bike: false,
            vehicle: None,
            parked_at: None,
            transit: None,
            activity: Activity::AtHome,
            memories: Vec::new(),
            places: Default::default(),
            thought: String::new(),
            spotlight: false,
            next_decision_tick: u64::MAX / 2,
            arrived_tick: 0,
            inside: Some(home),
            mood: 0.5,
            income: 80.0,
            savings: 40.0,
            rent_stress_days: 0,
            satiety: 0.9,
            groceries: 0.0,
            hungry_days: 0,
            trail: Vec::new(),
            pending_footfall: Vec::new(),
            household: Some(hid),
            age: 35,
            wakefulness: 1.0,
            asleep: false,
            packed_meal: 0.0,
        });
        w.households[hid].members.push(id);
        (w, id, hid, home, shop)
    }

    #[test]
    fn a_gift_moves_money_from_the_treasury_to_the_household() {
        let (mut w, id, hid, _, _) = town();
        w.economy.treasury = 5000.0;
        let g = w.gift(id, GIFT_AMOUNT).unwrap();
        assert_eq!(g.household, Some(hid));
        assert!((w.economy.treasury - 4000.0).abs() < 1e-3, "the treasury paid");
        assert!((w.households[hid].money - 1100.0).abs() < 1e-3, "the household got it");
        assert!((w.economy.ledger.today[Flow::Gifts.index()] + 1000.0).abs() < 1e-3, "booked as a gift");
        assert!(w.economy.ledger.items_today.iter().any(|e| e.flow == Flow::Gifts && e.item == Item::Named("Mara Voss".into())), "itemised by who got it");
        assert!(w.citizens[id].mood > 0.5, "they are pleased");
        assert!(w.citizens[id].memories.iter().any(|m| m.text.contains("gift")), "and remember it");
    }

    #[test]
    fn a_gift_the_treasury_cannot_pay_is_refused_and_moves_nothing() {
        let (mut w, id, hid, _, _) = town();
        w.economy.treasury = 600.0;
        assert!(w.gift(id, GIFT_AMOUNT).is_err());
        assert_eq!(w.economy.treasury, 600.0);
        assert_eq!(w.households[hid].money, 100.0);
        assert!(w.gift(id, -5.0).is_err(), "no negative gifts");
        assert!(w.gift(CitizenId::default(), 10.0).is_err(), "no gifts to nobody");
    }

    #[test]
    fn without_a_household_the_gift_goes_to_their_savings() {
        let (mut w, id, _, _, _) = town();
        w.citizens[id].household = None;
        let before = w.citizens[id].savings;
        let g = w.gift(id, 250.0).unwrap();
        assert_eq!(g.household, None);
        assert!((w.citizens[id].savings - before - 250.0).abs() < 1e-3);
    }

    #[test]
    fn holdings_are_home_work_and_the_businesses_they_own() {
        let (mut w, id, _, home, shop) = town();
        assert_eq!(w.holdings_of(id), vec![(home, Holding::Home), (shop, Holding::Work)]);
        // A founder owns firms besides: each is listed once, after home and work.
        let others: Vec<BuildingId> = w.buildings.iter().filter(|(b, x)| *b != shop && x.land_use == LandUse::Commercial).map(|(b, _)| b).take(2).collect();
        assert_eq!(others.len(), 2, "two more shops to own");
        for &b in others.iter().chain(std::iter::once(&shop)) {
            let mut z = crate::market::Business::new(crate::market::BusinessKind::Grocery, 4);
            z.owner = Some(id);
            w.buildings[b].business = Some(z);
        }
        let h = w.holdings_of(id);
        assert_eq!(h.len(), 4, "home, work (also owned, listed once), two more: {h:?}");
        assert_eq!(h[0], (home, Holding::Home));
        assert_eq!(h[1], (shop, Holding::Work));
        assert!(h[2..].iter().all(|(b, k)| *k == Holding::Owns && others.contains(b)));
        // Someone unknown has nothing.
        assert!(w.holdings_of(CitizenId::default()).is_empty());
    }
}
