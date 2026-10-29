use crate::buildings::{BuildingId, Tier};
use crate::traffic::CarId;
use crate::zoning::{LandUse, LotId};
use glam::Vec2;
use serde::{Deserialize, Serialize};
use slotmap::new_key_type;

new_key_type! {
    pub struct CitizenId;
    pub struct HouseholdId;
}

/// Age at which a child becomes an adult who works.
pub const ADULT_AGE: u8 = 18;
/// Age at which an adult retires and lives on a pension.
pub const RETIRE_AGE: u8 = 67;
/// School years. Nothing uses them yet: when schools exist, children of this
/// age go to one during the day (see `Citizen::goes_to_school`).
pub const SCHOOL_AGES: std::ops::Range<u8> = 6..ADULT_AGE;
/// Daily pension of a retiree.
pub const PENSION: f32 = 45.0;
/// Groceries one person eats in a day (a unit restores 0.45 satiety, and
/// satiety drains about 0.8 a day).
pub const GROCERIES_PER_PERSON_DAY: f32 = 2.0;
/// Days of groceries a household tries to keep at home.
pub const PANTRY_DAYS: f32 = 2.5;
/// Pocket money each adult carries for fares, lunches and treats; the rest
/// of the household's money is shared.
pub const POCKET_MONEY: f32 = 40.0;

/// Where a citizen is in life: children neither work nor pay, adults work,
/// retirees draw a pension.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum LifeStage {
    Child,
    Adult,
    Retiree,
}

impl LifeStage {
    pub fn of_age(age: u8) -> Self {
        if age < ADULT_AGE {
            LifeStage::Child
        } else if age < RETIRE_AGE {
            LifeStage::Adult
        } else {
            LifeStage::Retiree
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            LifeStage::Child => "child",
            LifeStage::Adult => "adult",
            LifeStage::Retiree => "retiree",
        }
    }
}

/// The people who share a home, a purse and a pantry: one to five citizens
/// of any age. A home holds one household; rent and groceries are paid from
/// its money. Each member keeps their own mood, memories and personality.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Household {
    pub members: Vec<CitizenId>,
    pub home: Option<BuildingId>,
    /// Shared money: every member's earnings come in, rent and groceries go
    /// out. Adults also carry `Citizen::savings` as pocket money.
    pub money: f32,
    /// Meals stocked at home, for everyone.
    pub groceries: f32,
    /// Consecutive days the rent ate more than half the household income.
    #[serde(default)]
    pub rent_stress_days: u8,
    /// Day the household moved in (or arrived), so nobody moves twice a day.
    #[serde(default)]
    pub moved_day: u64,
}

impl Household {
    pub fn size(&self) -> usize {
        self.members.len()
    }

    /// Groceries the household likes to have at home.
    pub fn pantry_target(&self) -> f32 {
        self.members.len().max(1) as f32 * GROCERIES_PER_PERSON_DAY * PANTRY_DAYS
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Purpose {
    Home,
    Work,
    Visit(LandUse),
    Shop(crate::market::Resource),
    Wander,
    /// Walking to a park bench.
    Sit,
    /// Walking to a bus stop.
    ToStop,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum Activity {
    /// Outside the city, not yet rendered.
    Arriving,
    AtHome,
    Working,
    Visiting(BuildingId),
    /// Standing around outside (no home, or wandering).
    Idle,
    Traveling {
        to: Option<BuildingId>,
        purpose: Purpose,
        path: Vec<Vec2>,
        idx: usize,
    },
    /// In a car on the lane network.
    Driving {
        car: CarId,
        to: Option<BuildingId>,
        purpose: Purpose,
    },
    /// On a park bench, in the open.
    Sitting {
        at: Vec2,
        park: BuildingId,
    },
    /// At a bus stop, waiting for the line.
    Waiting {
        at: Vec2,
    },
    /// On a bus.
    Riding {
        bus: CarId,
    },
    /// On a train.
    OnTrain {
        train: crate::rail::TrainId,
    },
    /// In a drone taxi or an air ambulance (`crate::air`).
    Flying {
        drone: crate::air::DroneId,
    },
    /// [subway] On a metro train, underground.
    OnMetro {
        train: crate::subway::MetroTrainId,
    },
    /// [passengers] Riding along in someone else's car (`world/rides.rs`):
    /// walking beside `driver` to the car (`seated` false, drawn), then in
    /// it (`seated`, out of sight, carried with it); out at the far end,
    /// they walk on to `to` themselves.
    Passenger {
        driver: CitizenId,
        to: Option<BuildingId>,
        purpose: Purpose,
        seated: bool,
    },
    /// [ferries] Away for the day in a town off the map (`town`: index
    /// into the terminal's remote towns), back on a ferry into `terminal`
    /// once `until` has passed.
    Abroad {
        terminal: BuildingId,
        town: u8,
        until: u64,
    },
}

impl Activity {
    pub fn label(&self) -> &'static str {
        match self {
            Activity::Arriving => "arriving",
            Activity::AtHome => "at home",
            Activity::Working => "working",
            Activity::Visiting(_) => "out",
            Activity::Idle => "loitering",
            Activity::Sitting { .. } => "sitting in the park",
            Activity::Waiting { .. } => "waiting for the bus",
            Activity::Riding { .. } => "on the bus",
            Activity::OnTrain { .. } => "on the train",
            Activity::Flying { .. } => "in the air",
            Activity::OnMetro { .. } => "on the subway",
            Activity::Passenger { seated: true, .. } => "riding in a car",
            Activity::Passenger { .. } => "walking to the car",
            Activity::Abroad { .. } => "away on a day trip",
            Activity::Traveling { purpose, .. } | Activity::Driving { purpose, .. } => match purpose {
                Purpose::Home => "heading home",
                Purpose::Work => "commuting",
                Purpose::Visit(_) => "going out",
                Purpose::Shop(_) => "shopping",
                Purpose::Wander => "wandering",
                Purpose::Sit => "heading for a bench",
                Purpose::ToStop => "walking to the bus stop",
            },
        }
    }

    pub fn is_traveling(&self) -> bool {
        matches!(self, Activity::Traveling { .. } | Activity::Driving { .. } | Activity::Riding { .. } | Activity::OnTrain { .. } | Activity::Flying { .. } | Activity::OnMetro { .. } | Activity::Passenger { .. } | Activity::Abroad { .. })
    }
}

/// How a citizen gets around on the current trip.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum TravelMode {
    #[default]
    Walk,
    Bike,
}

/// How a whole trip is made, as the citizen chose it (`World::choose_mode`).
/// The index is stable: it addresses `Commute::pain` and the daily counts
/// (`vehicle::CarStats::trips_today`), which have room for more modes.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum TripMode {
    #[default]
    Walk,
    Bike,
    Bus,
    Train,
    Car,
    /// [subway] The metro.
    Subway,
    /// [taxis] A cab hailed at the kerb (`crate::taxi`).
    Taxi,
}

impl TripMode {
    pub const ALL: [TripMode; 7] = [TripMode::Walk, TripMode::Bike, TripMode::Bus, TripMode::Train, TripMode::Car, TripMode::Subway, TripMode::Taxi];

    pub fn index(self) -> usize {
        match self {
            TripMode::Walk => 0,
            TripMode::Bike => 1,
            TripMode::Bus => 2,
            TripMode::Train => 3,
            TripMode::Car => 4,
            TripMode::Subway => 5,
            TripMode::Taxi => 6,
        }
    }

    pub fn key(self) -> &'static str {
        match self {
            TripMode::Walk => "walk",
            TripMode::Bike => "bike",
            TripMode::Bus => "bus",
            TripMode::Train => "train",
            TripMode::Car => "car",
            TripMode::Subway => "subway",
            TripMode::Taxi => "taxi",
        }
    }
}

/// Slots in `Commute::pain` (more than there are modes, so new ones fit).
pub const PAIN_SLOTS: usize = crate::vehicle::MODE_SLOTS;

/// What travelling has been like lately: the pain each mode has caused
/// (an average over recent trips, in "bad half-hours", fading day by day),
/// the misery of finding a space, and the trip under way. The mode choice
/// weighs it; the daily habits (buy a bike, sell the car) act on it.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Commute {
    pub pain: [f32; PAIN_SLOTS],
    /// Recent trips per mode (fading), so a mode never tried has no memory.
    pub trips: [f32; PAIN_SLOTS],
    /// Searching for a space and paying for it, on the same scale.
    pub parking_pain: f32,
    /// The trip under way: its mode and the tick it began.
    pub mode: Option<TripMode>,
    pub started: u64,
    /// Straight-line metres of the trip under way.
    pub distance: f32,
    /// Traffic seconds spent looking for a space on this trip.
    pub searched: f32,
    /// Tick the driver began looking for a space (0: not looking).
    pub search_since: u64,
    /// Tick the car pulled out (0: not driving yet), and the minutes the
    /// drive took, once parked.
    pub drive_since: u64,
    pub drive_minutes: f32,
    /// Metres walked from where the car was left to the door.
    pub walk_after: f32,
    /// Parking paid on this trip.
    pub fee: f32,
    /// The last pain felt and on which mode, for the inspector.
    pub last_pain: f32,
    pub last_mode: Option<TripMode>,
    /// Ticks of the last trip.
    pub last_minutes: u32,
    /// [tolls] Tolls paid on the trip under way, and what a car trip has
    /// cost in tolls lately (a running average the mode choice weighs).
    pub toll: f32,
    pub toll_avg: f32,
    /// [rage] The worst road rage on this trip's drive (`0..1`) and the
    /// minutes the drive should have taken.
    pub rage: f32,
    pub expected_drive: f32,
    /// [carlife] The day (plus one; 0: never) this person last bought or
    /// sold a car. Nobody trades their car again for `WHEELS_HOLD_DAYS`:
    /// in a jammed city drivers sold for the jams and bought again for
    /// the buses within days, and every trade was two more vehicles
    /// crawling through the jam to and from the dealer (the vehicle
    /// build-up past ~10k people).
    pub wheels_day: u64,
}

impl Commute {
    /// Fold one trip's pain into the memory of its mode.
    pub fn feel(&mut self, mode: TripMode, pain: f32) {
        let i = mode.index();
        let seen = self.trips[i];
        // The first trips teach the most; after that, a running average.
        let k = if seen < 1.0 { 0.6 } else { 0.3 };
        self.pain[i] += (pain - self.pain[i]) * k;
        self.trips[i] += 1.0;
        self.last_pain = pain;
        self.last_mode = Some(mode);
    }

    /// A day passes: memories fade a little.
    pub fn fade(&mut self) {
        for p in &mut self.pain {
            *p *= 0.85;
        }
        for t in &mut self.trips {
            *t *= 0.7;
        }
        self.parking_pain *= 0.8;
        self.toll_avg *= 0.9;
    }

    pub fn pain_of(&self, mode: TripMode) -> f32 {
        self.pain[mode.index()]
    }
}

/// On the way to the parked car, then driving from it to `to`.
#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
pub struct ToCar {
    pub car: crate::vehicle::VehicleId,
    pub to: BuildingId,
    pub purpose: Purpose,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Memory {
    pub tick: u64,
    pub text: String,
}

pub const MAX_MEMORIES: usize = 24;
/// Points kept in the breadcrumb trail drawn behind a selected citizen.
pub const MAX_TRAIL: usize = 300;
/// A citizen moved at least this far since the last point to earn another.
pub const TRAIL_STEP: f32 = 0.5;
/// A jump longer than this (arriving by train, a respawn) is not a walk:
/// the trail starts over rather than drawing a line across the map.
const TRAIL_BREAK: f32 = 150.0;

/// [pt-walk] The stops along a citizen's trail: each as the trail point it
/// was made at, counted from the first point ever laid (`base` points have
/// since dropped off the tail), so a stop leaves with its stretch of trail.
#[derive(Clone, Debug, Default)]
pub struct TrailStops {
    pub base: u32,
    pub stops: Vec<(u32, Vec2)>,
}

impl TrailStops {
    /// The stops still on the trail, oldest first, with their index into it.
    pub fn on_trail(&self, len: usize) -> impl Iterator<Item = (usize, Vec2)> + '_ {
        self.stops.iter().filter(move |(i, _)| *i >= self.base && ((*i - self.base) as usize) < len).map(|(i, p)| ((*i - self.base) as usize, *p))
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Citizen {
    pub name: String,
    pub seed: u64,
    pub home: Option<BuildingId>,
    pub work: Option<BuildingId>,
    pub pos: Vec2,
    pub heading: f32,
    /// Position and heading at the previous tick, for smooth rendering.
    #[serde(default)]
    pub prev_pos: Vec2,
    #[serde(default)]
    pub prev_heading: f32,
    #[serde(default)]
    pub mode: TravelMode,
    #[serde(default)]
    pub owns_car: bool,
    #[serde(default)]
    pub owns_bike: bool,
    /// The car (or motorcycle, scooter) owned, chosen at purchase; `None`
    /// on old saves, where it is derived from the seed on the first drive.
    #[serde(default)]
    pub vehicle: Option<crate::vehicle::VehicleModel>,
    /// Where the car is parked while the citizen is inside somewhere.
    #[serde(default)]
    pub parked_at: Option<BuildingId>,
    /// The bus trip under way, if any.
    #[serde(default)]
    pub transit: Option<crate::transit::TransitTrip>,
    pub activity: Activity,
    pub memories: Vec<Memory>,
    /// [places] The venues, shops and parks this citizen knows, how each
    /// went, and the people met at their regular haunts.
    #[serde(default)]
    pub places: crate::places::PlaceBook,
    pub thought: String,
    /// Spotlight citizens may be driven by an expensive mind (an LLM).
    pub spotlight: bool,
    pub next_decision_tick: u64,
    pub arrived_tick: u64,
    /// Where the citizen is right now, if inside a building.
    pub inside: Option<BuildingId>,
    /// Satisfaction in `0..1`; feeds the economy and future happiness systems.
    pub mood: f32,
    /// Daily income from work (or gig money when unemployed).
    pub income: f32,
    pub savings: f32,
    /// Consecutive days where rent ate too much of the income.
    pub rent_stress_days: u8,
    /// Hunger satisfaction in `0..1`; drops every hour, restored by eating.
    pub satiety: f32,
    /// Meals stocked at home.
    pub groceries: f32,
    /// Days without a proper meal.
    pub hungry_days: u8,
    /// Where the citizen has been lately, oldest first, for drawing the
    /// route so far. Rebuilt as they move; never saved.
    #[serde(skip)]
    pub trail: Vec<Vec2>,
    /// [pt-walk] Where along the trail they stopped (went in somewhere,
    /// sat down, waited for a ride): the "o"s drawn on it.
    #[serde(skip)]
    pub trail_stops: TrailStops,
    /// Lots the current journey walks past, counted as footfall once the
    /// journey ends (not when it is planned, so abandoned trips count for
    /// nothing, and not per leg of a bus or train trip).
    #[serde(default)]
    pub pending_footfall: Vec<LotId>,
    /// The household this citizen lives with. Old saves get one household
    /// per citizen on load.
    #[serde(default)]
    pub household: Option<HouseholdId>,
    /// Age in years; saves from before ages count everyone as an adult.
    #[serde(default = "default_age")]
    pub age: u8,
    /// Alertness in `0..1`: drains while awake, restored by sleeping at home.
    #[serde(default = "one")]
    pub wakefulness: f32,
    /// Asleep at home (until rested and it is day).
    #[serde(default)]
    pub asleep: bool,
    /// A meal taken from the household pantry on the way out, eaten when
    /// hungry away from home.
    #[serde(default)]
    pub packed_meal: f32,
    /// [driving] Drunkenness `0..1`: rises with drinks at a venue (faster
    /// for hedonists), wears off over a few hours (`World::step_driving`).
    #[serde(default)]
    pub drunk: f32,
    /// [carlife] The car this citizen owns, in `World::vehicles` (where it
    /// is parked or that it is on the road). `owns_car` mirrors it.
    #[serde(default)]
    pub car: Option<crate::vehicle::VehicleId>,
    /// [carlife] Walking to the parked car to drive somewhere.
    #[serde(default)]
    pub to_car: Option<ToCar>,
    /// [carlife] How travelling has felt lately, per mode.
    #[serde(default)]
    pub commute: Commute,
    /// [looks] What they look like, chosen on arrival with their family
    /// (`looks::family_looks`). `None` only on saves from before looks,
    /// filled on load.
    #[serde(default)]
    pub look: Option<crate::looks::Appearance>,
}

/// [driving] Drunk enough that driving is a bad idea.
pub const DRUNK: f32 = 0.35;
/// Rebelliousness at which a drunk drives anyway.
pub const REBEL: f32 = 0.55;

/// The five personality traits, `[sociability, ambition, hedonism,
/// caution, curiosity]`, drawn from the seed exactly as
/// `nd_minds::Personality::from_seed` does. [review] The one copy: places,
/// vehicles, looks and driving all read it.
pub fn traits_of(seed: u64) -> [f32; 5] {
    use rand::{Rng, SeedableRng};
    let mut rng = crate::SimRng::seed_from_u64(seed);
    [rng.random(), rng.random(), rng.random(), rng.random(), rng.random()]
}

/// The personality traits that matter behind the wheel, `(caution,
/// hedonism)`, as `nd_minds::Personality` derives them from the seed (the
/// fourth and third draws).
pub fn driver_traits(seed: u64) -> (f32, f32) {
    let t = traits_of(seed);
    (t[3], t[2])
}

/// [rage] Patience behind the wheel, `0..1`, from the same personality
/// draws as `driver_traits`: the cautious and the unambitious sit a jam
/// out; the driven and the pleasure-seeking do not.
pub fn patience(seed: u64) -> f32 {
    let t = traits_of(seed);
    let (ambition, hedonism, caution) = (t[1], t[2], t[3]);
    (0.5 * caution + 0.3 * (1.0 - ambition) + 0.2 * (1.0 - hedonism)).clamp(0.0, 1.0)
}

/// How little someone cares for rules, `0..1`: careless and pleasure-seeking.
pub fn rebelliousness(caution: f32, hedonism: f32) -> f32 {
    (0.55 * (1.0 - caution) + 0.45 * hedonism).clamp(0.0, 1.0)
}

/// [driving] Hook for mode choice: a drunk who owns a car and is
/// rebellious enough drives home anyway. A drunk who is not leaves the car.
pub fn wants_to_drive_drunk(c: &Citizen) -> bool {
    let (caution, hedonism) = driver_traits(c.seed);
    c.owns_car && c.drunk >= DRUNK && rebelliousness(caution, hedonism) >= REBEL
}

fn default_age() -> u8 {
    35
}

fn one() -> f32 {
    1.0
}

/// Wakefulness below this and a citizen heads home to sleep.
pub const TIRED: f32 = 0.2;

/// [lights] Whether `hour` is night for a citizen with this seed: everyone
/// keeps their own hours, to bed between 21:00 and midnight and up between
/// 05:00 and 07:00. With one bedtime for the whole city every home went
/// dark on the same stroke of 22:00, which read as a switch, not a city.
pub fn night_for(seed: u64, hour: u64) -> bool {
    let mut h = seed ^ 0x5851_F42D_4C95_7F2D;
    h ^= h >> 33;
    h = h.wrapping_mul(0xFF51_AFD7_ED55_8CCD);
    h ^= h >> 29;
    let bed = [21, 22, 22, 23, 23, 24][(h % 6) as usize];
    let rise = [5, 6, 6, 7][((h >> 8) % 4) as usize];
    hour >= bed || hour < rise
}
/// Wakefulness below this at work and the shift goes badly.
pub const EXHAUSTED: f32 = 0.12;

impl Citizen {
    /// [review] Someone new in town, fresh: at `pos` by `home`, idle, awake,
    /// sober, with a packed meal, no car, no memories and a blank commute.
    /// Callers set what differs (income, savings, mood, wheels, looks).
    /// Newcomers used to be built from `..template`, a clone of a random
    /// adult, and inherited their drunkenness, sleep and commute.
    pub fn newcomer(name: String, seed: u64, age: u8, home: Option<BuildingId>, household: Option<HouseholdId>, pos: Vec2, tick: u64) -> Self {
        Citizen {
            name,
            seed,
            home,
            work: None,
            pos,
            heading: 0.0,
            prev_pos: pos,
            prev_heading: 0.0,
            mode: TravelMode::Walk,
            owns_car: false,
            owns_bike: false,
            vehicle: None,
            parked_at: None,
            transit: None,
            activity: Activity::Idle,
            memories: Vec::new(),
            places: Default::default(),
            thought: String::new(),
            spotlight: false,
            next_decision_tick: tick + 5,
            arrived_tick: tick,
            inside: None,
            mood: 0.6,
            income: 0.0,
            savings: 0.0,
            rent_stress_days: 0,
            satiety: 0.8,
            groceries: 0.0,
            hungry_days: 0,
            trail: Vec::new(),
            trail_stops: TrailStops::default(),
            pending_footfall: Vec::new(),
            household,
            age,
            wakefulness: 0.9,
            asleep: false,
            packed_meal: 1.0,
            drunk: 0.0,
            car: None,
            to_car: None,
            commute: Default::default(),
            look: None,
        }
    }

    /// Record the current position if the citizen has moved on since the
    /// last point. Called once per tick after every system has moved people.
    pub fn push_trail(&mut self) {
        if let Some(&last) = self.trail.last() {
            let d = last.distance(self.pos);
            if d < TRAIL_STEP {
                return;
            }
            if d > TRAIL_BREAK {
                let ts = &mut self.trail_stops;
                ts.base = ts.base.wrapping_add(self.trail.len() as u32);
                ts.stops.clear();
                self.trail.clear();
            }
        }
        self.trail.push(self.pos);
        if self.trail.len() > MAX_TRAIL {
            self.trail.remove(0);
            let ts = &mut self.trail_stops;
            ts.base = ts.base.wrapping_add(1);
            let base = ts.base;
            ts.stops.retain(|(i, _)| *i >= base);
        }
    }

    /// [pt-walk] Mark the trail's newest point a stop, once: called each
    /// tick the citizen is indoors, on a bench or waiting for a ride.
    pub fn mark_stop(&mut self) {
        let Some(&p) = self.trail.last() else { return };
        let ts = &mut self.trail_stops;
        let i = ts.base.wrapping_add(self.trail.len() as u32 - 1);
        if ts.stops.last().is_some_and(|(j, _)| *j == i) {
            return;
        }
        ts.stops.push((i, p));
    }

    pub fn remember(&mut self, tick: u64, text: impl Into<String>) {
        self.memories.push(Memory { tick, text: text.into() });
        if self.memories.len() > MAX_MEMORIES {
            self.memories.remove(0);
        }
    }

    /// [looks] Their look; one from the seed alone if none was given yet.
    pub fn appearance(&self) -> crate::looks::Appearance {
        self.look.unwrap_or_else(|| crate::looks::Appearance::of_seed(self.seed, self.age, self.class()))
    }

    pub fn class(&self) -> Tier {
        Tier::for_income(self.income)
    }

    pub fn stage(&self) -> LifeStage {
        LifeStage::of_age(self.age)
    }

    /// Only adults take jobs.
    pub fn can_work(&self) -> bool {
        self.stage() == LifeStage::Adult
    }

    /// Hook for schools: children of school age would spend the day there.
    /// Nothing reads it yet beyond the minds keeping children near home.
    pub fn goes_to_school(&self) -> bool {
        SCHOOL_AGES.contains(&self.age)
    }

    pub fn is_tired(&self) -> bool {
        self.wakefulness < TIRED
    }

    /// Output of a shift worked at this wakefulness, `0.5..1`.
    pub fn productivity(&self) -> f32 {
        if self.wakefulness < EXHAUSTED {
            0.5
        } else {
            1.0
        }
    }

    pub fn is_visible(&self) -> bool {
        matches!(self.activity, Activity::Traveling { .. } | Activity::Idle | Activity::Sitting { .. } | Activity::Waiting { .. } | Activity::Passenger { seated: false, .. }) && !self.on_platform()
    }

    /// [r4-metroloop] At a stop, platform, pad or kerb waiting for the
    /// bus, metro, train, drone or cab of a trip under way: as good as on
    /// it. The ordinary decisions leave them be (they used to decide again
    /// as soon as one was due and set off afresh from where they stood);
    /// the wait ends when the ride comes or they give up on it.
    pub fn waiting_for_ride(&self) -> bool {
        matches!(self.activity, Activity::Waiting { .. }) && self.transit.is_some()
    }

    /// [pt-transit] Waiting on a metro platform, underground: not drawn at
    /// street level (the underground view may show them there).
    pub fn on_platform(&self) -> bool {
        matches!(self.activity, Activity::Waiting { .. }) && matches!(self.transit.map(|t| t.leg), Some(crate::transit::TransitLeg::Subway { .. }))
    }
}

#[cfg(test)]
mod tests {

    #[test]
    fn bedtimes_differ_from_person_to_person() {
        let share = |h: u64| (0..1000u64).filter(|s| night_for(s.wrapping_mul(0x9E37_79B9_7F4A_7C15), h)).count();
        assert_eq!(share(3), 1000, "everyone sleeps at three in the morning");
        assert_eq!(share(14), 0, "nobody's night at two in the afternoon");
        let (at21, at22, at23) = (share(21), share(22), share(23));
        assert!(at21 > 50 && at21 < at22 && at22 < at23 && at23 < 1000, "the city goes to bed over hours: {at21} {at22} {at23}");
        assert!(share(5) > share(6) && share(6) > share(7) && share(5) < 1000, "and gets up over hours");
    }

    use super::*;

    fn citizen_at(pos: Vec2) -> Citizen {
        Citizen {
            name: "Test".into(),
            seed: 1,
            home: None,
            work: None,
            pos,
            heading: 0.0,
            prev_pos: pos,
            prev_heading: 0.0,
            mode: TravelMode::Walk,
            owns_car: false,
            owns_bike: false,
            vehicle: None,
            parked_at: None,
            transit: None,
            activity: Activity::Idle,
            memories: Vec::new(),
            places: Default::default(),
            thought: String::new(),
            spotlight: false,
            next_decision_tick: 0,
            arrived_tick: 0,
            inside: None,
            mood: 0.5,
            income: 0.0,
            savings: 0.0,
            rent_stress_days: 0,
            satiety: 1.0,
            groceries: 0.0,
            hungry_days: 0,
            trail: Vec::new(),
            trail_stops: TrailStops::default(),
            pending_footfall: Vec::new(),
            household: None,
            age: 35,
            wakefulness: 1.0,
            asleep: false,
            packed_meal: 0.0,
            drunk: 0.0,
            car: None,
            to_car: None,
            commute: Default::default(),
            look: None,
        }
    }

    #[test]
    fn driver_traits_match_the_personality() {
        for seed in [3u64, 77, 123_456] {
            let p = nd_minds::Personality::from_seed(seed);
            assert_eq!(driver_traits(seed), (p.caution, p.hedonism));
        }
    }

    #[test]
    fn life_stages_follow_age() {
        assert_eq!(LifeStage::of_age(4), LifeStage::Child);
        assert_eq!(LifeStage::of_age(18), LifeStage::Adult);
        assert_eq!(LifeStage::of_age(70), LifeStage::Retiree);
        let mut c = citizen_at(Vec2::ZERO);
        c.age = 9;
        assert!(!c.can_work() && c.goes_to_school());
        c.age = 40;
        assert!(c.can_work() && !c.goes_to_school());
        c.wakefulness = 0.05;
        assert!(c.is_tired() && c.productivity() < 1.0);
    }

    #[test]
    fn an_old_citizen_without_the_new_fields_loads_as_a_rested_adult() {
        let mut c = citizen_at(Vec2::ZERO);
        c.age = 12;
        let text = ron::to_string(&c).unwrap();
        // Strip the fields added with households, as an old save would lack them.
        let old = text.replace(",household:None", "").replace(",age:12", "").replace(",wakefulness:1.0", "").replace(",asleep:false", "").replace(",packed_meal:0.0", "");
        assert!(!old.contains("wakefulness"), "{old}");
        let back: Citizen = ron::from_str(&old).unwrap();
        assert_eq!(back.age, 35);
        assert_eq!(back.wakefulness, 1.0);
        assert!(back.household.is_none());
    }

    /// [pt-walk] Each stop along the way is marked once on the trail, and
    /// drops off the tail with the stretch of trail it was on.
    #[test]
    fn trail_stops_are_marked_once_and_fade_with_the_tail() {
        let mut c = citizen_at(Vec2::ZERO);
        c.push_trail();
        c.mark_stop();
        c.mark_stop();
        assert_eq!(c.trail_stops.on_trail(c.trail.len()).collect::<Vec<_>>(), vec![(0, Vec2::ZERO)], "one stop, however long they stay");
        for i in 1..=10 {
            c.pos = Vec2::new(i as f32, 0.0);
            c.push_trail();
        }
        c.mark_stop();
        let stops: Vec<_> = c.trail_stops.on_trail(c.trail.len()).collect();
        assert_eq!(stops, vec![(0, Vec2::ZERO), (10, Vec2::new(10.0, 0.0))]);
        for i in 11..(11 + MAX_TRAIL) {
            c.pos = Vec2::new(i as f32, 0.0);
            c.push_trail();
        }
        assert!(c.trail_stops.on_trail(c.trail.len()).next().is_none(), "both stops have left the trail");
        assert!(c.trail_stops.stops.is_empty());
        c.mark_stop();
        let (i, p) = c.trail_stops.on_trail(c.trail.len()).next().unwrap();
        assert_eq!((i, p), (c.trail.len() - 1, c.pos), "a new stop at the head");
    }

    #[test]
    fn trail_records_movement_only() {
        let mut c = citizen_at(Vec2::ZERO);
        c.push_trail();
        c.push_trail();
        assert_eq!(c.trail.len(), 1, "standing still adds nothing after the first point");
        c.pos = Vec2::new(0.2, 0.0);
        c.push_trail();
        assert_eq!(c.trail.len(), 1, "a shuffle under half a metre is not a step");
        c.pos = Vec2::new(1.0, 0.0);
        c.push_trail();
        assert_eq!(c.trail.len(), 2);
    }

    #[test]
    fn trail_is_capped_and_breaks_on_a_jump() {
        let mut c = citizen_at(Vec2::ZERO);
        for i in 0..(MAX_TRAIL + 50) {
            c.pos = Vec2::new(i as f32, 0.0);
            c.push_trail();
        }
        assert_eq!(c.trail.len(), MAX_TRAIL);
        assert_eq!(c.trail[0], Vec2::new(50.0, 0.0), "the oldest points are dropped first");
        c.pos = Vec2::new(5000.0, 0.0);
        c.push_trail();
        assert_eq!(c.trail, vec![Vec2::new(5000.0, 0.0)], "a teleport starts a fresh trail");
    }
}
