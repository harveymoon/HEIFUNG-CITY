//! Incidents: fires, crimes, people taken ill and crashes, who is involved,
//! the damage, their stages and outcomes.

use super::*;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum IncidentKind {
    Fire,
    Crime,
    Medical,
    /// [driving] A bad road crash: the police clear the scene (the hurt
    /// get a `Medical` incident of their own, on the road).
    Crash,
}

impl IncidentKind {
    pub const ALL: [IncidentKind; 3] = [IncidentKind::Fire, IncidentKind::Crime, IncidentKind::Medical];
    /// Slots of the per-service figures (`slot`): one per `UnitKind`.
    pub const SLOTS: usize = 3;

    pub fn index(self) -> usize {
        self as usize
    }

    /// Slot in the per-service response figures (`ServiceStats::queued`,
    /// `response_min_today`, ...): fire, police, ambulance. Crashes count
    /// with the police, who clear the scene.
    pub fn slot(self) -> usize {
        self.unit() as usize
    }

    pub fn label(self) -> &'static str {
        match self {
            IncidentKind::Fire => "fire",
            IncidentKind::Crime => "crime",
            IncidentKind::Medical => "medical",
            IncidentKind::Crash => "crash",
        }
    }

    pub fn unit(self) -> UnitKind {
        match self {
            IncidentKind::Fire => UnitKind::FireEngine,
            IncidentKind::Crime | IncidentKind::Crash => UnitKind::PoliceCar,
            IncidentKind::Medical => UnitKind::Ambulance,
        }
    }

    pub fn service(self) -> ServiceKind {
        match self {
            IncidentKind::Fire => ServiceKind::Fire,
            IncidentKind::Crime => ServiceKind::Police,
            IncidentKind::Medical => ServiceKind::Health,
            IncidentKind::Crash => ServiceKind::Police,
        }
    }
}

/// What kind of crime (`World::report_crime`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum CrimeKind {
    #[default]
    Robbery,
    Theft,
    Assault,
    Vandalism,
    /// [driving] A street takeover: an intersection held for donuts.
    StreetTakeover,
    /// [events] An illegal street race.
    StreetRace,
    /// [events] A noise complaint about a warehouse rave.
    Rave,
    /// [murder] Someone was killed: a long investigation on scene
    /// (`HOMICIDE_WORK`), a likelier arrest when the killer is known.
    Homicide,
    /// [moto] A motorbike pack tearing through town.
    RecklessRiding,
}

impl CrimeKind {
    pub const ALL: [CrimeKind; 4] = [CrimeKind::Robbery, CrimeKind::Theft, CrimeKind::Assault, CrimeKind::Vandalism];

    /// Robbery and assault put people in danger: they jump the queue.
    pub fn violent(self) -> bool {
        matches!(self, CrimeKind::Robbery | CrimeKind::Assault | CrimeKind::Homicide)
    }

    pub fn label(self) -> &'static str {
        match self {
            CrimeKind::Robbery => "robbery",
            CrimeKind::Theft => "theft",
            CrimeKind::Assault => "assault",
            CrimeKind::Vandalism => "vandalism",
            CrimeKind::StreetTakeover => "street takeover",
            CrimeKind::StreetRace => "street race",
            CrimeKind::Rave => "illegal rave",
            CrimeKind::Homicide => "homicide",
            CrimeKind::RecklessRiding => "reckless riding",
        }
    }
}

/// Someone involved in an incident, named as they were at the time.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Involved {
    pub citizen: CitizenId,
    pub name: String,
    /// caller, dispatcher, victim, suspect, patient, resident
    pub role: String,
    /// Where they ended up: a hospital, a jail, the station, home.
    #[serde(default)]
    pub taken_to: Option<String>,
}

/// Fire damage to one building.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Damage {
    pub building: BuildingId,
    pub name: String,
    /// Condition lost, 0..1.
    pub lost: f32,
    pub destroyed: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Stage {
    /// Happening; nobody has called yet.
    Unreported,
    /// A 911 call is in; waiting for a dispatcher and a free unit.
    Reported,
    /// A unit is on its way.
    Dispatched,
    /// The unit is there and working.
    OnScene,
    /// An ambulance carrying the patient to hospital.
    Transporting,
    Resolved,
    /// [emergency] The suspect fled by car and the police are after them.
    Pursuit,
}

impl Stage {
    pub fn label(self) -> &'static str {
        match self {
            Stage::Unreported => "unreported",
            Stage::Reported => "awaiting dispatch",
            Stage::Dispatched => "unit en route",
            Stage::OnScene => "unit on scene",
            Stage::Transporting => "to hospital",
            Stage::Resolved => "resolved",
            Stage::Pursuit => "car chase",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Outcome {
    Extinguished,
    /// Burned until the building was gone.
    BurnedDown,
    Arrest,
    /// Police came, the suspect got away.
    Escaped,
    /// Nobody came.
    Unsolved,
    Admitted,
    /// No hospital bed: treated where they were.
    TreatedOnSite,
    /// Nobody came; they got better on their own.
    Recovered,
    /// The building or person is gone.
    Gone,
    /// [driving] Crash scene cleared, wrecks towed.
    Cleared,
    /// [services] A civic structure burned out: it stands, out of service,
    /// until the city has repaired it (civic buildings never burn down).
    Gutted,
}

impl Outcome {
    pub fn label(self) -> &'static str {
        match self {
            Outcome::Extinguished => "fire out",
            Outcome::BurnedDown => "burned down",
            Outcome::Arrest => "arrest made",
            Outcome::Escaped => "suspect escaped",
            Outcome::Unsolved => "unsolved",
            Outcome::Admitted => "admitted to hospital",
            Outcome::TreatedOnSite => "treated on site",
            Outcome::Recovered => "recovered without help",
            Outcome::Gone => "gone",
            Outcome::Cleared => "scene cleared",
            Outcome::Gutted => "gutted, under repair",
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Incident {
    pub id: u32,
    pub kind: IncidentKind,
    pub building: BuildingId,
    pub pos: Vec2,
    /// The patient, or the victim of a crime.
    pub citizen: Option<CitizenId>,
    /// Whoever did it (crimes), if anyone in town.
    pub suspect: Option<CitizenId>,
    pub caller: Option<CitizenId>,
    pub dispatcher: Option<CitizenId>,
    pub stage: Stage,
    pub started: u64,
    /// When the 911 call goes in.
    pub report_at: u64,
    pub reported: Option<u64>,
    /// When the dispatcher has the job out.
    pub dispatch_at: Option<u64>,
    pub dispatched: Option<u64>,
    pub arrived: Option<u64>,
    pub resolved: Option<u64>,
    pub unit: Option<UnitId>,
    /// Fires: 0..1.
    pub intensity: f32,
    /// Ticks of work left on scene (crimes, patients).
    pub work_left: f32,
    pub outcome: Option<Outcome>,
    // ── the record, for the Emergencies window and the API ──
    /// Name of the building at the time.
    #[serde(default)]
    pub place: String,
    /// Crimes: what kind.
    #[serde(default)]
    pub crime: Option<CrimeKind>,
    /// Answered by a unit from out of town (slow, billed).
    #[serde(default)]
    pub outside: bool,
    /// When the out-of-town unit gets there.
    #[serde(default)]
    pub arrive_at: Option<u64>,
    /// Fee paid for out-of-town help.
    #[serde(default)]
    pub fee: f32,
    /// The unit's call sign and station ("Engine 2, Eastgate Fire HQ").
    #[serde(default)]
    pub unit_name: Option<String>,
    #[serde(default)]
    pub people: Vec<Involved>,
    /// Fires: every building it burned (spread fires have their own incident too).
    #[serde(default)]
    pub damage: Vec<Damage>,
    /// [driving] Out on the street at `pos` (a crash, someone hurt in
    /// one, a takeover), not inside `building`, which only names the place.
    #[serde(default)]
    pub on_road: bool,
    /// [emergency] The car chase, if the suspect fled by car.
    #[serde(default)]
    pub chase: Option<ChaseRecord>,
    /// [fire-scene] A big fire's second engine (the second alarm), and
    /// when it was sounded (only once a fire).
    #[serde(default)]
    pub second: Option<UnitId>,
    #[serde(default)]
    pub second_alarm: Option<u64>,
    /// [murder] The police know who did it (a feud everyone knew about):
    /// a homicide arrest is likelier.
    #[serde(default)]
    pub known_suspect: bool,
}

impl Incident {
    pub fn active(&self) -> bool {
        self.stage != Stage::Resolved
    }

    /// Minutes from the call to the unit arriving.
    pub fn response_ticks(&self) -> Option<u64> {
        Some(self.arrived?.saturating_sub(self.reported?))
    }

    /// Minutes from dispatch to arrival, and from arrival to the end.
    pub fn travel_ticks(&self) -> Option<u64> {
        Some(self.arrived?.saturating_sub(self.dispatched?))
    }

    pub fn work_ticks(&self) -> Option<u64> {
        Some(self.resolved?.saturating_sub(self.arrived?))
    }

    /// Dispatch priority, 0 first: fires and violent crime (lives and
    /// property at stake), then the sick and hurt (crash victims included)
    /// and crash scenes blocking a street, then theft, vandalism and street
    /// takeovers.
    /// Equal priorities go in call order.
    pub fn priority(&self) -> u8 {
        match (self.kind, self.crime) {
            (IncidentKind::Fire, _) => 0,
            (IncidentKind::Crime, Some(c)) if c.violent() => 0,
            (IncidentKind::Medical, _) | (IncidentKind::Crash, _) => 1,
            (IncidentKind::Crime, _) => 2,
        }
    }

    /// What it was, in a word or two ("fire", "robbery", "medical").
    pub fn title(&self) -> &'static str {
        match (self.kind, self.crime) {
            (IncidentKind::Crime, Some(c)) => c.label(),
            (k, _) => k.label(),
        }
    }

    /// Note someone involved (once per role).
    pub fn involve(&mut self, citizen: CitizenId, name: &str, role: &str) {
        if !self.people.iter().any(|p| p.citizen == citizen && p.role == role) {
            self.people.push(Involved { citizen, name: name.to_string(), role: role.to_string(), taken_to: None });
        }
    }

    /// Where someone involved was taken.
    pub fn taken(&mut self, citizen: CitizenId, to: &str) {
        for p in self.people.iter_mut().filter(|p| p.citizen == citizen) {
            p.taken_to = Some(to.to_string());
        }
    }
}
