//! [r4-leisure-places] Places to eat and go out: diners, noodle bars,
//! pizza places, drive-throughs, food halls, street food, fine dining and
//! bubble tea; bars, karaoke boxes, cinemas, theatres, live-music halls,
//! bowling alleys, arcades, gyms and comedy clubs (the nightclub is the old
//! `Venue`). What each is like to the simulation lives here: who likes it
//! at what hour (`appeal`), what an hour there costs, what share of the
//! takings books the films and the acts, how loud it is and how much trash
//! it leaves. The kinds themselves are rows of `market::BusinessKind`;
//! where they grow is `crate::commercial`, who goes `world/places.rs`.
//!
//! Everything here is a pure function of its arguments: no random draws.

use crate::buildings::Tier;
use crate::citizens::LifeStage;
use crate::market::{BusinessKind, Resource};

use BusinessKind as K;

/// The new places to eat, in `BusinessKind` order.
pub const EATERIES: [BusinessKind; 8] = [K::Diner, K::NoodleBar, K::PizzaPlace, K::FastFood, K::FoodHall, K::StreetFood, K::FineDining, K::BubbleTea];
/// The new places to go out, in `BusinessKind` order (the nightclub is the
/// `Venue`, listed in [`ALL_NIGHTS_OUT`]).
pub const NIGHTS_OUT: [BusinessKind; 9] = [K::Bar, K::KaraokeBox, K::Cinema, K::Theatre, K::ConcertHall, K::BowlingAlley, K::Arcade, K::Gym, K::ComedyClub];
/// Every kind this module is about: the eateries, the nights out and the
/// nightclub.
pub const ALL_NIGHTS_OUT: [BusinessKind; 18] = [
    K::Diner,
    K::NoodleBar,
    K::PizzaPlace,
    K::FastFood,
    K::FoodHall,
    K::StreetFood,
    K::FineDining,
    K::BubbleTea,
    K::Bar,
    K::KaraokeBox,
    K::Cinema,
    K::Theatre,
    K::ConcertHall,
    K::BowlingAlley,
    K::Arcade,
    K::Gym,
    K::ComedyClub,
    K::Venue,
];

impl BusinessKind {
    /// One of the new places to eat or go out (not the nightclub, which
    /// is older and keeps its own rules: the cover at the door, its name).
    pub fn is_eat_or_go_out(self) -> bool {
        EATERIES.contains(&self) || NIGHTS_OUT.contains(&self)
    }

    /// A new place to eat.
    pub fn is_eatery(self) -> bool {
        EATERIES.contains(&self)
    }

    /// Cooks meals it sells over the counter (as baked goods): every new
    /// eatery but the fine-dining room, which seats its diners only.
    pub fn cooks_meals(self) -> bool {
        self.is_eatery() && self.recipe().outputs.first().is_some_and(|(r, _)| *r == Resource::BakedGoods)
    }

    /// Share of an hour's takings paid out of town for what is on: the
    /// film's distributor, the touring company, the band, the comic.
    pub fn booking_share(self) -> f32 {
        match self {
            K::Cinema => 0.45,
            K::ConcertHall => 0.4,
            K::Theatre => 0.3,
            K::ComedyClub => 0.25,
            _ => 0.0,
        }
    }

    /// Somewhere a parent takes the children before nine.
    pub fn family_friendly(self) -> bool {
        matches!(self, K::Diner | K::PizzaPlace | K::FastFood | K::FoodHall | K::BowlingAlley | K::Cinema | K::Arcade | K::BubbleTea)
    }

    /// Somewhere for a date.
    pub fn date_spot(self) -> bool {
        matches!(self, K::FineDining | K::Cinema | K::Theatre | K::ComedyClub | K::ConcertHall | K::Bar | K::NoodleBar)
    }

    /// How loud it is out on the street, `0..1`: the nuisance it puts on
    /// the land round it (homes mind it most).
    pub fn loudness(self) -> f32 {
        match self {
            K::Venue => 0.9,
            K::ConcertHall => 0.8,
            K::KaraokeBox => 0.7,
            K::Bar => 0.6,
            K::FastFood | K::StreetFood => 0.5,
            K::Arcade | K::ComedyClub | K::BowlingAlley => 0.4,
            K::Cinema | K::FoodHall | K::Gym => 0.2,
            _ => 0.1,
        }
    }

    /// Trash a worker's day leaves here against its land use's rate:
    /// takeaway boxes and cups most of all.
    pub fn trash_factor(self) -> f32 {
        match self {
            K::FastFood | K::StreetFood => 2.0,
            K::FoodHall | K::BubbleTea | K::PizzaPlace => 1.6,
            K::Diner | K::NoodleBar | K::Bar | K::KaraokeBox => 1.3,
            K::Cinema | K::BowlingAlley | K::Arcade | K::ConcertHall => 1.2,
            K::FineDining | K::Theatre | K::ComedyClub => 1.0,
            K::Gym => 0.6,
            _ => 1.0,
        }
    }

    /// What it does to the land value round it beyond its land use's
    /// splat: `(radius, weight)`. A theatre or a fine-dining room lifts the
    /// street; the loud ones' nuisance is [`BusinessKind::loudness`].
    pub fn land_value_lift(self) -> Option<(f32, f32)> {
        Some(match self {
            K::Theatre | K::ConcertHall => (160.0, 0.06),
            K::FineDining => (110.0, 0.05),
            K::Cinema | K::FoodHall => (120.0, 0.035),
            K::BubbleTea | K::Gym | K::Diner | K::NoodleBar => (80.0, 0.02),
            _ => return None,
        })
    }
}

/// What an hour here costs against a nightclub round (`VENUE_TAB`).
pub fn tab_factor(kind: BusinessKind) -> f32 {
    match kind {
        K::FineDining => 3.0,
        K::Theatre => 2.2,
        K::ConcertHall => 2.0,
        K::ComedyClub => 1.5,
        K::Cinema => 1.3,
        K::KaraokeBox | K::BowlingAlley => 1.2,
        K::Bar => 1.0,
        K::Diner | K::PizzaPlace | K::FoodHall => 0.9,
        K::NoodleBar | K::Arcade => 0.8,
        K::Gym => 0.7,
        K::FastFood | K::StreetFood => 0.6,
        K::BubbleTea => 0.5,
        _ => 1.0,
    }
}

/// People a town needs before the market opens its places to eat and go
/// out (`World::kinds_viable`); a pressure or the player may sooner.
pub const MIN_TOWN: u32 = 300;
/// People per kind of place to eat or go out a town carries: a new kind
/// is tried only while the town has fewer kinds open than this goes into
/// its people (and none is being tried already).
pub const PEOPLE_PER_KIND: u32 = 400;

/// Meals (baked goods) a hungry diner eats on arrival.
pub const MEAL: f32 = 1.0;

/// Who is choosing where to go, as `appeal` reads them.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Outgoer {
    pub age: u8,
    pub stage: LifeStage,
    /// Wants a meal.
    pub hungry: bool,
    /// Children at home.
    pub kids: bool,
    /// Has a partner.
    pub partner: bool,
    pub class: Tier,
    /// A regular at the gym ([`gym_goer`]).
    pub gym_goer: bool,
    /// `0..1` personality.
    pub hedonism: f32,
    pub sociability: f32,
}

/// How a kind suits `who` at `hour`, added to the place's utility in
/// `World::choose_place` (about `-1..1`). 0 for every kind not in
/// [`ALL_NIGHTS_OUT`].
pub fn appeal(kind: BusinessKind, hour: u8, who: &Outgoer) -> f32 {
    if !ALL_NIGHTS_OUT.contains(&kind) {
        return 0.0;
    }
    let h = hour % 24;
    let in_range = |a: u8, b: u8| if a <= b { (a..b).contains(&h) } else { h >= a || h < b };
    let young = ((35.0 - who.age as f32) / 17.0).clamp(0.0, 1.0);
    let old = ((who.age as f32 - 50.0) / 25.0).clamp(0.0, 1.0);
    let rich = match who.class {
        Tier::High => 1.0,
        Tier::Mid => 0.35,
        Tier::Low => 0.0,
    };
    let poor = if who.class == Tier::Low { 1.0 } else { 0.0 };
    let meal_time = in_range(6, 10) || in_range(11, 15) || in_range(17, 22);
    let mut a = 0.0;
    if kind.is_eatery() {
        a += if who.hungry { 0.6 } else { -0.25 };
        if meal_time {
            a += 0.15;
        }
    }
    a += match kind {
        K::Diner => (if in_range(6, 11) { 0.3 } else { 0.0 }) + 0.15 * old + if who.kids { 0.1 } else { 0.0 },
        K::NoodleBar => (if in_range(21, 3) { 0.25 } else { 0.0 }) + 0.15 * young,
        K::PizzaPlace => (if who.kids { 0.3 } else { 0.0 }) + 0.15 * young,
        K::FastFood => 0.25 * poor + 0.15 * young - 0.3 * rich + if in_range(23, 4) { 0.2 } else { 0.0 },
        K::FoodHall => (if in_range(11, 15) { 0.3 } else { 0.0 }) + 0.1 * who.sociability,
        K::StreetFood => (if in_range(18, 2) { 0.3 } else { 0.0 }) + 0.2 * poor + 0.15 * who.sociability,
        K::FineDining => (if in_range(19, 23) { 0.25 } else { -0.4 }) + 0.5 * rich - 0.5 * poor + if who.partner { 0.3 } else { 0.0 },
        K::BubbleTea => (if in_range(13, 19) { 0.3 } else { 0.0 }) + 0.35 * young - 0.2 * old,
        K::Bar => (if in_range(18, 2) { 0.25 } else { -0.3 }) + 0.2 * who.hedonism + 0.1 * who.sociability - 0.1 * old,
        K::Venue => (if in_range(22, 4) { 0.35 } else { -0.35 }) + 0.3 * young * who.hedonism - 0.3 * old,
        K::KaraokeBox => (if in_range(20, 4) { 0.3 } else { -0.3 }) + 0.3 * who.sociability + 0.1 * young,
        K::Cinema => (if in_range(18, 23) { 0.4 } else if in_range(13, 18) { 0.15 } else { -0.3 }) + (if who.kids && in_range(13, 21) { 0.3 } else { 0.0 }) + if who.partner { 0.15 } else { 0.0 },
        K::Theatre => (if in_range(19, 23) { 0.35 } else if in_range(14, 17) { 0.1 } else { -0.4 }) + 0.35 * old + 0.3 * rich - 0.2 * young,
        K::ConcertHall => (if in_range(20, 1) { 0.4 } else { -0.4 }) + 0.2 * young + 0.1 * who.hedonism,
        K::BowlingAlley => (if in_range(15, 23) { 0.25 } else { 0.0 }) + (if who.kids { 0.45 } else { 0.0 }) + 0.1 * who.sociability,
        K::Arcade => (if in_range(14, 24) { 0.15 } else { 0.0 }) + 0.4 * young - 0.3 * old + if who.kids { 0.15 } else { 0.0 },
        K::Gym => {
            let slot = in_range(6, 9) || in_range(17, 21);
            match (who.gym_goer, slot) {
                (true, true) => 0.9,
                (true, false) => 0.2,
                (false, _) => -0.6,
            }
        }
        K::ComedyClub => (if in_range(20, 1) { 0.3 } else { -0.4 }) + 0.15 * who.sociability + if who.partner { 0.1 } else { 0.0 },
        _ => 0.0,
    };
    if who.stage == LifeStage::Child && !kind.family_friendly() {
        a -= 1.0;
    }
    a
}

/// A regular at the gym: about one adult in five, the driven and careful
/// more often. By the seed alone.
pub fn gym_goer(seed: u64, ambition: f32, caution: f32) -> bool {
    crate::hash::unit(crate::hash::mix(seed, 0x6E_5917)) < 0.08 + 0.18 * ambition + 0.08 * caution
}

/// Whether a gym-goer goes on `day` (four days in seven or so).
pub fn gym_day(seed: u64, day: u64) -> bool {
    crate::hash::unit(crate::hash::mix2_salted(seed, day, 0x6E_5918)) < 0.6
}

/// Hours a gym-goer heads out before work.
pub fn workout_hour(hour: u64) -> bool {
    (6..8).contains(&hour)
}

/// How much more a visitor from out of town wants a place of `kind` at
/// `hour` than a shop (1): the theatre and the live-music hall at night,
/// the food hall and street food, fine dining for dinner.
pub fn tourist_pull(kind: BusinessKind, hour: u8) -> f32 {
    let evening = (18..24).contains(&(hour % 24));
    match kind {
        K::Theatre | K::ConcertHall if evening => 3.0,
        K::FineDining if evening => 2.0,
        K::StreetFood | K::FoodHall => 1.8,
        K::ComedyClub | K::KaraokeBox | K::Cinema if evening => 1.3,
        K::Theatre | K::ConcertHall | K::FineDining => 0.3,
        k if k.is_eat_or_go_out() => 0.8,
        _ => 1.0,
    }
}

/// What a citizen remembers of a visit to a place of `kind` named `name`.
pub fn visit_memory(kind: Option<BusinessKind>, name: &str) -> String {
    match kind {
        Some(K::Diner) => format!("Eggs and coffee at {name}."),
        Some(K::NoodleBar) => format!("A bowl of noodles at {name}."),
        Some(K::PizzaPlace) => format!("Pizza at {name}."),
        Some(K::FastFood) => format!("Burger and fries at {name}."),
        Some(K::FoodHall) => format!("Lunch at the counters in {name}."),
        Some(K::StreetFood) => format!("Ate off the stalls at {name}."),
        Some(K::FineDining) => format!("A long dinner at {name}."),
        Some(K::BubbleTea) => format!("Bubble tea at {name}."),
        Some(K::Bar) => format!("Drinks at {name}."),
        Some(K::KaraokeBox) => format!("Sang my heart out at {name}."),
        Some(K::Cinema) => format!("Saw a film at {name}."),
        Some(K::Theatre) => format!("A show at {name}."),
        Some(K::ConcertHall) => format!("A gig at {name}."),
        Some(K::BowlingAlley) => format!("Bowled a few frames at {name}."),
        Some(K::Arcade) => format!("Played the machines at {name}."),
        Some(K::Gym) => format!("Worked out at {name}."),
        Some(K::ComedyClub) => format!("Laughed all night at {name}."),
        _ => format!("Spent the evening at {name}."),
    }
}

/// Visits by kind and hour, today and the whole of yesterday, and the
/// bookings paid out of town (films, touring shows, bands, comics).
#[derive(Clone, Debug, Default, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(default)]
pub struct LeisureStats {
    /// Arrivals per `BusinessKind::index`, per hour of the day.
    pub today: Vec<[u32; 24]>,
    pub yesterday: Vec<[u32; 24]>,
    pub bookings_today: f32,
    pub bookings_yesterday: f32,
    /// Diners fed at a table today (meals eaten in, not to go).
    pub meals_in_today: u32,
    pub meals_in_yesterday: u32,
}

impl LeisureStats {
    /// Someone arrived at a place of `kind` at `hour`.
    pub fn note(&mut self, kind: BusinessKind, hour: u8) {
        if !ALL_NIGHTS_OUT.contains(&kind) {
            return;
        }
        if self.today.len() < BusinessKind::COUNT {
            self.today.resize(BusinessKind::COUNT, [0; 24]);
        }
        self.today[kind.index()][(hour % 24) as usize] += 1;
    }

    /// Midnight: today becomes yesterday.
    pub fn roll(&mut self) {
        self.yesterday = std::mem::take(&mut self.today);
        self.bookings_yesterday = std::mem::take(&mut self.bookings_today);
        self.meals_in_yesterday = std::mem::take(&mut self.meals_in_today);
    }

    /// Visits of a kind over a day (`yesterday` or today so far).
    pub fn visits(&self, kind: BusinessKind, yesterday: bool) -> u32 {
        let v = if yesterday { &self.yesterday } else { &self.today };
        v.get(kind.index()).map_or(0, |h| h.iter().sum())
    }

    /// Visits of a kind in each hour of a day.
    pub fn by_hour(&self, kind: BusinessKind, yesterday: bool) -> [u32; 24] {
        let v = if yesterday { &self.yesterday } else { &self.today };
        v.get(kind.index()).copied().unwrap_or([0; 24])
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn who() -> Outgoer {
        Outgoer { age: 30, stage: LifeStage::Adult, hungry: false, kids: false, partner: false, class: Tier::Mid, gym_goer: false, hedonism: 0.5, sociability: 0.5 }
    }

    #[test]
    fn every_new_kind_is_an_outing_with_its_own_hours_staff_and_tab() {
        for k in EATERIES.into_iter().chain(NIGHTS_OUT) {
            assert!(k.is_outing(), "{k:?}");
            assert!(k.customers_per_job().is_some(), "{k:?}");
            assert!(!k.needs_site(), "{k:?} is opened by the market on its sub-zone");
            assert!(!k.hours().always(), "{k:?}");
            assert!(tab_factor(k) > 0.0);
            assert!(k.max_floors().is_some(), "{k:?}");
            assert_ne!(k.label(), "venue");
        }
        // Kitchens sell meals over the counter; the fine-dining room seats only.
        for k in EATERIES {
            assert_eq!(k.walk_in_sale().is_some(), k != K::FineDining, "{k:?}");
        }
        for k in NIGHTS_OUT {
            assert_eq!(k.walk_in_sale(), None, "{k:?}");
        }
        // Distinct labels and keys round-trip.
        for k in BusinessKind::ALL {
            assert_eq!(BusinessKind::from_key(&k.key()), Some(k), "{k:?}");
        }
    }

    #[test]
    fn the_hours_of_a_day_out() {
        let me = who();
        // The gym in the morning for its regulars, and nobody else.
        let fit = Outgoer { gym_goer: true, ..me };
        assert!(appeal(K::Gym, 7, &fit) > 0.5);
        assert!(appeal(K::Gym, 7, &me) < 0.0);
        assert!(appeal(K::Gym, 7, &fit) > appeal(K::Gym, 13, &fit));
        // Lunch in a food hall when hungry, not after a meal.
        let hungry = Outgoer { hungry: true, ..me };
        assert!(appeal(K::FoodHall, 12, &hungry) > appeal(K::FoodHall, 12, &me) + 0.5);
        // Cinemas in the evening, clubs late.
        assert!(appeal(K::Cinema, 20, &me) > appeal(K::Cinema, 10, &me));
        assert!(appeal(K::Venue, 23, &me) > appeal(K::Venue, 19, &me));
        // Families at the bowling alley, dates over fine dining.
        let family = Outgoer { kids: true, ..me };
        assert!(appeal(K::BowlingAlley, 18, &family) > appeal(K::BowlingAlley, 18, &me));
        let rich_couple = Outgoer { partner: true, class: Tier::High, ..me };
        assert!(appeal(K::FineDining, 20, &rich_couple) > appeal(K::FineDining, 20, &me) + 0.4);
        // The theatre for the old, the arcade for the young.
        let old = Outgoer { age: 68, stage: LifeStage::Retiree, ..me };
        let teen = Outgoer { age: 17, ..me };
        assert!(appeal(K::Theatre, 20, &old) > appeal(K::Theatre, 20, &teen));
        assert!(appeal(K::Arcade, 16, &teen) > appeal(K::Arcade, 16, &old));
        // Older kinds are untouched.
        assert_eq!(appeal(K::Grocery, 12, &me), 0.0);
        assert_eq!(appeal(K::GolfRange, 20, &me), 0.0);
    }

    #[test]
    fn bookings_noise_and_trash() {
        assert!(K::Cinema.booking_share() > 0.0 && K::Theatre.booking_share() > 0.0);
        assert_eq!(K::Bar.booking_share(), 0.0);
        assert!(K::Venue.loudness() > K::Theatre.loudness());
        assert!(K::FastFood.trash_factor() > K::FineDining.trash_factor());
        assert!(K::Theatre.land_value_lift().is_some() && K::FastFood.land_value_lift().is_none());
    }

    #[test]
    fn gym_goers_are_a_minority_and_go_most_days() {
        let n = (0..2000u64).filter(|s| gym_goer(*s, 0.5, 0.5)).count();
        assert!((200..600).contains(&n), "{n}");
        let days = (0..700u64).filter(|d| gym_day(7, *d)).count();
        assert!((300..560).contains(&days), "{days}");
    }
}
