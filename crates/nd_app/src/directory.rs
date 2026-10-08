//! [directory] The city's directory: every business and every person as a
//! row, for City Data's Directory tab (`ui/directory.rs`) and the remote
//! API's `directory` query. No egui here: the rows are read from the world
//! in one pass (a snapshot the page keeps for a few seconds), and the sort
//! and the filter are pure functions over them, tested below.
//!
//! A row holds what its columns show, already worked out (names of the
//! workplace and home, the street, the district, the state), so sorting
//! ten thousand people is a sort of plain values, never a walk of the
//! world. `order` filters and sorts and returns indices into the rows;
//! missing values (no rating yet, no job) sit at the bottom whichever way
//! the column runs, and ties fall back to the name, then the row's place,
//! so the order is stable.

use std::cmp::Ordering;

use bevy::math::Vec2;
use nd_sim::citizens::{LifeStage, TripMode};
use nd_sim::estate::{Class, OwnerKind};
use nd_sim::{Activity, BuildingId, BusinessKind, CitizenId, IncidentKind, World};

// ── what a column sorts by ─────────────────────────────────────────────────

/// One cell's sort key.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Key<'a> {
    Num(f64),
    Text(&'a str),
    /// No value: always sorted last.
    None,
}

/// Case-blind text order without allocating.
fn text_cmp(a: &str, b: &str) -> Ordering {
    a.bytes().map(|c| c.to_ascii_lowercase()).cmp(b.bytes().map(|c| c.to_ascii_lowercase()))
}

/// Order two keys in a direction; `None` last either way.
fn key_cmp(a: Key, b: Key, desc: bool) -> Ordering {
    let o = match (a, b) {
        (Key::None, Key::None) => return Ordering::Equal,
        (Key::None, _) => return Ordering::Greater,
        (_, Key::None) => return Ordering::Less,
        (Key::Num(x), Key::Num(y)) => x.total_cmp(&y),
        (Key::Text(x), Key::Text(y)) => text_cmp(x, y),
        // Mixed kinds should not happen in one column: numbers first.
        (Key::Num(_), Key::Text(_)) => Ordering::Less,
        (Key::Text(_), Key::Num(_)) => Ordering::Greater,
    };
    if desc {
        o.reverse()
    } else {
        o
    }
}

/// Text, or no value when empty.
fn text(s: &str) -> Key<'_> {
    if s.is_empty() {
        Key::None
    } else {
        Key::Text(s)
    }
}

/// A table of rows the directory can sort and filter.
pub trait Row {
    /// The columns, in the order they show.
    type Col: Copy + PartialEq + 'static;
    fn key(&self, c: Self::Col) -> Key<'_>;
    fn name(&self) -> &str;
    fn matches(&self, f: &Filter) -> bool;
}

/// What narrows the rows: a search over names, streets and kinds, and the
/// pickers. `None` is "all".
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Filter {
    /// Words to find, case-blind; every word must appear somewhere in the
    /// row's text (name, kind, owner, street, district, workplace, home).
    pub search: String,
    pub district: Option<String>,
    /// Businesses: one kind.
    pub kind: Option<BusinessKind>,
    /// Businesses: one state.
    pub state: Option<BizState>,
    /// People: one class.
    pub class: Option<Class>,
    /// People: one status.
    pub status: Option<Status>,
}

/// The text a row's search looks in: its parts, lower case, in one string.
fn hay_of(parts: &[&str]) -> String {
    let mut h = String::with_capacity(parts.iter().map(|p| p.len() + 1).sum());
    for p in parts {
        for ch in p.chars() {
            h.extend(ch.to_lowercase());
        }
        h.push(' ');
    }
    h
}

/// Every word of `search` in `hay` (both lower case already: `order`
/// lowers the search once).
fn found(hay: &str, search: &str) -> bool {
    search.split_whitespace().all(|w| hay.contains(w))
}

/// The rows that pass the filter, sorted by `col` (descending when
/// `desc`), as indices into `rows`.
pub fn order<R: Row>(rows: &[R], f: &Filter, col: R::Col, desc: bool) -> Vec<u32> {
    let lowered;
    let f = if f.search.chars().any(char::is_uppercase) {
        lowered = Filter { search: f.search.to_lowercase(), ..f.clone() };
        &lowered
    } else {
        f
    };
    let mut idx: Vec<u32> = (0..rows.len() as u32).filter(|&i| rows[i as usize].matches(f)).collect();
    idx.sort_by(|&a, &b| {
        let (ra, rb) = (&rows[a as usize], &rows[b as usize]);
        key_cmp(ra.key(col), rb.key(col), desc).then_with(|| text_cmp(ra.name(), rb.name())).then(a.cmp(&b))
    });
    idx
}

// ── businesses ─────────────────────────────────────────────────────────────

/// Where a business stands right now, most urgent first (the sort order).
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum BizState {
    /// The building is still going up.
    Building,
    /// Five days broke in a row: out of business.
    Shut,
    /// In debt (or a day broke) and still trading.
    Broke,
    /// Open hours, but nobody on the staff.
    NoStaff,
    Open,
    /// Outside its hours.
    AfterHours,
}

impl BizState {
    pub const ALL: [BizState; 6] = [BizState::Building, BizState::Shut, BizState::Broke, BizState::NoStaff, BizState::Open, BizState::AfterHours];

    pub fn label(self) -> &'static str {
        match self {
            BizState::Building => "under construction",
            BizState::Shut => "out of business",
            BizState::Broke => "broke",
            BizState::NoStaff => "no staff",
            BizState::Open => "open",
            BizState::AfterHours => "closed",
        }
    }

    pub fn key(self) -> &'static str {
        match self {
            BizState::Building => "building",
            BizState::Shut => "shut",
            BizState::Broke => "broke",
            BizState::NoStaff => "no_staff",
            BizState::Open => "open",
            BizState::AfterHours => "closed",
        }
    }

    pub fn parse(s: &str) -> Option<BizState> {
        let s = s.trim().to_ascii_lowercase().replace([' ', '-'], "_");
        BizState::ALL.into_iter().find(|x| x.key() == s || x.label().replace(' ', "_") == s)
    }
}

/// One business, as its row shows it.
#[derive(Clone, Debug)]
pub struct BizRow {
    pub id: BuildingId,
    pub name: String,
    pub kind: BusinessKind,
    /// "Grocery", "Mill · farming".
    pub kind_label: String,
    /// Who owns it: a chain, a developer, a billionaire, a founder; empty
    /// for an independent.
    pub owner: String,
    /// "chain", "developer", "billionaire", "person", "household", ...
    pub owner_kind: &'static str,
    pub district: String,
    pub street: String,
    pub staff: u16,
    pub jobs: u16,
    pub revenue_today: f32,
    pub spend_today: f32,
    /// Yesterday's takings and net, from the closed books.
    pub revenue_yesterday: Option<f32>,
    pub profit_yesterday: Option<f32>,
    pub money: f32,
    /// Stars (1-5) and how many visits rated it; `None` before anyone has.
    pub rating: Option<f32>,
    pub rated: u32,
    /// People who walked past its door today.
    pub footfall: u32,
    pub state: BizState,
    pub pos: Option<Vec2>,
    /// Lower-case text the search looks in.
    hay: String,
}

/// The business table's columns.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum BizCol {
    Name,
    Kind,
    Owner,
    District,
    Street,
    Staff,
    Today,
    Yesterday,
    Profit,
    Money,
    Rating,
    Footfall,
    State,
}

impl BizCol {
    pub const ALL: [BizCol; 13] = [
        BizCol::Name,
        BizCol::Kind,
        BizCol::Owner,
        BizCol::District,
        BizCol::Street,
        BizCol::Staff,
        BizCol::Today,
        BizCol::Yesterday,
        BizCol::Profit,
        BizCol::Money,
        BizCol::Rating,
        BizCol::Footfall,
        BizCol::State,
    ];

    pub fn label(self) -> &'static str {
        match self {
            BizCol::Name => "name",
            BizCol::Kind => "kind",
            BizCol::Owner => "owner",
            BizCol::District => "district",
            BizCol::Street => "street",
            BizCol::Staff => "staff",
            BizCol::Today => "in today",
            BizCol::Yesterday => "in yday",
            BizCol::Profit => "net yday",
            BizCol::Money => "cash",
            BizCol::Rating => "rating",
            BizCol::Footfall => "footfall",
            BizCol::State => "state",
        }
    }

    pub fn key(self) -> &'static str {
        match self {
            BizCol::Name => "name",
            BizCol::Kind => "kind",
            BizCol::Owner => "owner",
            BizCol::District => "district",
            BizCol::Street => "street",
            BizCol::Staff => "staff",
            BizCol::Today => "revenue_today",
            BizCol::Yesterday => "revenue_yesterday",
            BizCol::Profit => "profit_yesterday",
            BizCol::Money => "money",
            BizCol::Rating => "rating",
            BizCol::Footfall => "footfall",
            BizCol::State => "state",
        }
    }

    pub fn tip(self) -> &'static str {
        match self {
            BizCol::Name => "The business's name (its building's)",
            BizCol::Kind => "What it does, and its industry",
            BizCol::Owner => "Who owns the building: a chain, a developer, a billionaire or a founder; blank for an independent",
            BizCol::District => "The district it stands in",
            BizCol::Street => "The street it fronts",
            BizCol::Staff => "Staff on the books of the jobs it has",
            BizCol::Today => "Takings today so far",
            BizCol::Yesterday => "Takings yesterday",
            BizCol::Profit => "Yesterday's takings less what it paid out (wages, inputs, tax)",
            BizCol::Money => "Cash in hand",
            BizCol::Rating => "Stars from the people who visited",
            BizCol::Footfall => "People who walked past its door today",
            BizCol::State => "Open, closed for the night, short of staff, broke, out of business or still being built",
        }
    }

    /// Numbers sort biggest first on the first click.
    pub fn numeric(self) -> bool {
        matches!(self, BizCol::Staff | BizCol::Today | BizCol::Yesterday | BizCol::Profit | BizCol::Money | BizCol::Rating | BizCol::Footfall)
    }

    pub fn parse(s: &str) -> Option<BizCol> {
        let s = s.trim().to_ascii_lowercase().replace([' ', '-'], "_");
        let s = match s.as_str() {
            "revenue" | "today" | "takings" => "revenue_today",
            "yesterday" => "revenue_yesterday",
            "profit" | "net" => "profit_yesterday",
            "cash" => "money",
            "stars" => "rating",
            "jobs" | "workers" => "staff",
            "address" => "street",
            other => other,
        };
        BizCol::ALL.into_iter().find(|c| c.key() == s)
    }
}

impl Row for BizRow {
    type Col = BizCol;

    fn key(&self, c: BizCol) -> Key<'_> {
        let opt = |v: Option<f32>| v.map_or(Key::None, |x| Key::Num(x as f64));
        match c {
            BizCol::Name => Key::Text(&self.name),
            BizCol::Kind => Key::Text(&self.kind_label),
            BizCol::Owner => text(&self.owner),
            BizCol::District => text(&self.district),
            BizCol::Street => text(&self.street),
            // Staff, then the jobs it has.
            BizCol::Staff => Key::Num(self.staff as f64 + self.jobs as f64 / 100_000.0),
            BizCol::Today => Key::Num(self.revenue_today as f64),
            BizCol::Yesterday => opt(self.revenue_yesterday),
            BizCol::Profit => opt(self.profit_yesterday),
            BizCol::Money => Key::Num(self.money as f64),
            BizCol::Rating => opt(self.rating),
            BizCol::Footfall => Key::Num(self.footfall as f64),
            BizCol::State => Key::Num(self.state as u8 as f64),
        }
    }

    fn name(&self) -> &str {
        &self.name
    }

    fn matches(&self, f: &Filter) -> bool {
        f.kind.is_none_or(|k| k == self.kind) && f.state.is_none_or(|s| s == self.state) && f.district.as_ref().is_none_or(|d| *d == self.district) && found(&self.hay, &f.search)
    }
}

fn capitalise(s: &str) -> String {
    let mut c = s.chars();
    c.next().map_or(String::new(), |f| f.to_uppercase().collect::<String>() + c.as_str())
}

/// What kind of owner, in a word.
pub fn owner_kind_label(k: &OwnerKind) -> &'static str {
    #[allow(unreachable_patterns)]
    match k {
        OwnerKind::Firm { .. } => "chain",
        OwnerKind::Developer => "developer",
        OwnerKind::Household { .. } => "household",
        OwnerKind::Elite { .. } => "billionaire",
        OwnerKind::Person { .. } => "person",
        _ => "corporation",
    }
}

/// The street a lot fronts, by name.
fn street_of(w: &World, lot: &nd_sim::zoning::Lot) -> String {
    w.streets.of(lot.front_segment).map_or(String::new(), |s| s.name.clone())
}

/// Every business in town, as rows, in the world's order.
pub fn businesses(w: &World) -> Vec<BizRow> {
    let hour = w.clock.hour() as u8;
    w.buildings
        .iter()
        .filter_map(|(id, b)| b.business.as_ref().map(|z| (id, b, z)))
        .map(|(id, b, z)| {
            let lot = w.lots.get(b.lot);
            let (owner, owner_kind) = match (w.owner_of(id), z.owner.and_then(|c| w.citizens.get(c))) {
                (Some((_, o)), _) => (o.name.clone(), owner_kind_label(&o.kind)),
                (None, Some(c)) => (c.name.clone(), "person"),
                (None, None) => (String::new(), ""),
            };
            let state = if b.construction.is_some() {
                BizState::Building
            } else if z.closed {
                BizState::Shut
            } else if z.broke_days > 0 || z.money < -300.0 {
                BizState::Broke
            } else if !z.kind.hours().contains(hour) {
                BizState::AfterHours
            } else if b.jobs > 0 && b.workers.is_empty() {
                BizState::NoStaff
            } else {
                BizState::Open
            };
            let kind_label = match z.kind.industry() {
                Some(i) => format!("{} · {}", capitalise(z.kind.label()), i.label()),
                None => capitalise(z.kind.label()),
            };
            let district = w.district_of_lot(b.lot).map_or(String::new(), |d| d.name.clone());
            let street = lot.map_or(String::new(), |l| street_of(w, l));
            let rep = w.reputation(id).filter(|r| r.rated > 0);
            let hay = hay_of(&[&b.name, &kind_label, &owner, &street, &district, state.label()]);
            BizRow {
                id,
                name: b.name.clone(),
                kind: z.kind,
                kind_label,
                owner,
                owner_kind,
                district,
                street,
                staff: b.workers.len() as u16,
                jobs: b.jobs,
                revenue_today: z.revenue_today,
                spend_today: z.spend_today,
                revenue_yesterday: z.books.last().map(|d| d.revenue),
                profit_yesterday: z.books.last().map(|d| d.revenue - d.spend),
                money: z.money,
                rating: rep.map(|r| r.stars()),
                rated: rep.map_or(0, |r| r.rated),
                footfall: lot.map_or(0, |l| l.footfall_today),
                state,
                pos: w.building_lot(id).map(|l| l.center),
                hay,
            }
        })
        .collect()
}

// ── people ─────────────────────────────────────────────────────────────────

/// What a person is up to, in a word (the sort order is this one).
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Status {
    Working,
    AtHome,
    Asleep,
    Out,
    Travelling,
    /// Ill and waiting for help, or in a hospital bed.
    Ill,
    /// No home to go to.
    Homeless,
    /// On the way in from out of town.
    Arriving,
    /// Away for the day (a ferry trip).
    Away,
}

impl Status {
    pub const ALL: [Status; 9] = [Status::Working, Status::AtHome, Status::Asleep, Status::Out, Status::Travelling, Status::Ill, Status::Homeless, Status::Arriving, Status::Away];

    pub fn label(self) -> &'static str {
        match self {
            Status::Working => "working",
            Status::AtHome => "at home",
            Status::Asleep => "asleep",
            Status::Out => "out",
            Status::Travelling => "on the move",
            Status::Ill => "ill",
            Status::Homeless => "homeless",
            Status::Arriving => "arriving",
            Status::Away => "away",
        }
    }

    pub fn key(self) -> &'static str {
        match self {
            Status::Working => "working",
            Status::AtHome => "at_home",
            Status::Asleep => "asleep",
            Status::Out => "out",
            Status::Travelling => "travelling",
            Status::Ill => "ill",
            Status::Homeless => "homeless",
            Status::Arriving => "arriving",
            Status::Away => "away",
        }
    }

    pub fn parse(s: &str) -> Option<Status> {
        let s = s.trim().to_ascii_lowercase().replace([' ', '-'], "_");
        let s = match s.as_str() {
            "home" => "at_home",
            "traveling" | "on_the_move" | "moving" => "travelling",
            "sick" | "patient" => "ill",
            other => other,
        };
        Status::ALL.into_iter().find(|x| x.key() == s)
    }

    /// From what they are doing (illness and homelessness weigh first).
    pub fn of(a: &Activity, asleep: bool, ill: bool, housed: bool) -> Status {
        if ill {
            return Status::Ill;
        }
        match a {
            Activity::Arriving => Status::Arriving,
            Activity::Abroad { .. } => Status::Away,
            Activity::AtHome if asleep => Status::Asleep,
            Activity::AtHome => Status::AtHome,
            Activity::Working => Status::Working,
            Activity::Visiting(_) | Activity::Sitting { .. } => Status::Out,
            Activity::Idle if !housed => Status::Homeless,
            Activity::Idle => Status::Out,
            _ if !housed && !a.is_traveling() => Status::Homeless,
            _ => Status::Travelling,
        }
    }
}

pub fn class_key(c: Class) -> &'static str {
    match c {
        Class::Low => "low",
        Class::Middle => "middle",
        Class::High => "high",
        Class::Elite => "elite",
    }
}

pub fn class_parse(s: &str) -> Option<Class> {
    match s.trim().to_ascii_lowercase().as_str() {
        "low" | "low income" | "poor" => Some(Class::Low),
        "middle" | "mid" | "middle class" => Some(Class::Middle),
        "high" | "high income" | "rich" => Some(Class::High),
        "elite" | "super-elite" | "billionaire" => Some(Class::Elite),
        _ => None,
    }
}

/// One person, as their row shows them.
#[derive(Clone, Debug)]
pub struct PersonRow {
    pub id: CitizenId,
    pub name: String,
    pub age: u8,
    pub stage: LifeStage,
    pub class: Class,
    pub income: f32,
    pub work: Option<BuildingId>,
    /// The workplace's name, or what they are instead ("unemployed",
    /// "child", "retired").
    pub work_label: String,
    pub home: Option<BuildingId>,
    pub home_label: String,
    /// The street and district of their home.
    pub street: String,
    pub district: String,
    pub household: u8,
    pub mood: f32,
    pub satiety: f32,
    pub hungry_days: u8,
    pub ill: bool,
    /// The way they have travelled most lately.
    pub commute: Option<TripMode>,
    pub favourite: String,
    pub friends: u16,
    pub status: Status,
    pub pos: Vec2,
    hay: String,
}

impl PersonRow {
    /// One figure for health: ill sorts below everyone, then how well fed.
    pub fn health(&self) -> f32 {
        if self.ill {
            -1.0
        } else {
            self.satiety
        }
    }
}

/// The people table's columns.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum PersonCol {
    Name,
    Age,
    Class,
    Income,
    Job,
    Home,
    Household,
    Mood,
    Health,
    Commute,
    Favourite,
    Friends,
    Status,
}

impl PersonCol {
    pub const ALL: [PersonCol; 13] = [
        PersonCol::Name,
        PersonCol::Age,
        PersonCol::Class,
        PersonCol::Income,
        PersonCol::Job,
        PersonCol::Home,
        PersonCol::Household,
        PersonCol::Mood,
        PersonCol::Health,
        PersonCol::Commute,
        PersonCol::Favourite,
        PersonCol::Friends,
        PersonCol::Status,
    ];

    pub fn label(self) -> &'static str {
        match self {
            PersonCol::Name => "name",
            PersonCol::Age => "age",
            PersonCol::Class => "class",
            PersonCol::Income => "income",
            PersonCol::Job => "works at",
            PersonCol::Home => "home",
            PersonCol::Household => "hh",
            PersonCol::Mood => "mood",
            PersonCol::Health => "health",
            PersonCol::Commute => "travels",
            PersonCol::Favourite => "favourite place",
            PersonCol::Friends => "friends",
            PersonCol::Status => "now",
        }
    }

    pub fn key(self) -> &'static str {
        match self {
            PersonCol::Name => "name",
            PersonCol::Age => "age",
            PersonCol::Class => "class",
            PersonCol::Income => "income",
            PersonCol::Job => "job",
            PersonCol::Home => "home",
            PersonCol::Household => "household",
            PersonCol::Mood => "mood",
            PersonCol::Health => "health",
            PersonCol::Commute => "commute",
            PersonCol::Favourite => "favourite",
            PersonCol::Friends => "friends",
            PersonCol::Status => "status",
        }
    }

    pub fn tip(self) -> &'static str {
        match self {
            PersonCol::Name => "Their name",
            PersonCol::Age => "Age in years",
            PersonCol::Class => "Income class (the super-elite are the billionaires)",
            PersonCol::Income => "Daily income from work (or gig money)",
            PersonCol::Job => "Where they work",
            PersonCol::Home => "Where they live, and on which street",
            PersonCol::Household => "People in their household",
            PersonCol::Mood => "How happy they are",
            PersonCol::Health => "How well fed they are (days without a proper meal); ill means waiting for help or in hospital",
            PersonCol::Commute => "The way they have travelled most lately",
            PersonCol::Favourite => "The place they like best (a regular haunt first)",
            PersonCol::Friends => "Friends they keep (the Social page has more)",
            PersonCol::Status => "What they are doing now",
        }
    }

    pub fn numeric(self) -> bool {
        matches!(self, PersonCol::Age | PersonCol::Class | PersonCol::Income | PersonCol::Household | PersonCol::Mood | PersonCol::Health | PersonCol::Friends)
    }

    pub fn parse(s: &str) -> Option<PersonCol> {
        let s = s.trim().to_ascii_lowercase().replace([' ', '-'], "_");
        let s = match s.as_str() {
            "work" | "works_at" | "workplace" => "job",
            "hh" | "household_size" => "household",
            "hunger" | "satiety" => "health",
            "mode" | "travels" => "commute",
            "favorite" | "favourite_place" => "favourite",
            "social" => "friends",
            "now" | "activity" => "status",
            other => other,
        };
        PersonCol::ALL.into_iter().find(|c| c.key() == s)
    }
}

impl Row for PersonRow {
    type Col = PersonCol;

    fn key(&self, c: PersonCol) -> Key<'_> {
        match c {
            PersonCol::Name => Key::Text(&self.name),
            PersonCol::Age => Key::Num(self.age as f64),
            // Class, then income within it.
            PersonCol::Class => Key::Num(self.class as u8 as f64 * 1e7 + self.income as f64),
            PersonCol::Income => Key::Num(self.income as f64),
            PersonCol::Job => match self.work {
                Some(_) => Key::Text(&self.work_label),
                None => Key::None,
            },
            PersonCol::Home => match self.home {
                Some(_) => Key::Text(&self.home_label),
                None => Key::None,
            },
            PersonCol::Household => Key::Num(self.household as f64),
            PersonCol::Mood => Key::Num(self.mood as f64),
            PersonCol::Health => Key::Num(self.health() as f64),
            PersonCol::Commute => self.commute.map_or(Key::None, |m| Key::Text(m.key())),
            PersonCol::Favourite => text(&self.favourite),
            PersonCol::Friends => Key::Num(self.friends as f64),
            PersonCol::Status => Key::Num(self.status as u8 as f64),
        }
    }

    fn name(&self) -> &str {
        &self.name
    }

    fn matches(&self, f: &Filter) -> bool {
        f.class.is_none_or(|c| c == self.class) && f.status.is_none_or(|s| s == self.status) && f.district.as_ref().is_none_or(|d| *d == self.district) && found(&self.hay, &f.search)
    }
}

/// The way someone has travelled most lately (fading counts per mode).
fn usual_mode(c: &nd_sim::Citizen) -> Option<TripMode> {
    TripMode::ALL.into_iter().map(|m| (m, c.commute.trips.get(m.index()).copied().unwrap_or(0.0))).filter(|(_, n)| *n > 0.05).max_by(|a, b| a.1.total_cmp(&b.1)).map(|(m, _)| m)
}

/// Everyone in town, as rows, in the world's order.
pub fn people(w: &World) -> Vec<PersonRow> {
    // The ill, once: waiting for an ambulance or in a hospital bed.
    let ill: std::collections::HashSet<CitizenId> = w.services.active().filter(|i| i.kind == IncidentKind::Medical).filter_map(|i| i.citizen).chain(w.services.admissions.iter().map(|a| a.citizen)).collect();
    let bname = |b: BuildingId| w.buildings.get(b).map_or("(gone)", |b| b.name.as_str());
    let mut homes: std::collections::HashMap<BuildingId, (String, String, String)> = std::collections::HashMap::new();
    w.citizens
        .iter()
        .map(|(id, c)| {
            let stage = c.stage();
            let work_label = match c.work {
                Some(b) => bname(b).to_string(),
                None => match stage {
                    LifeStage::Child => "child".into(),
                    LifeStage::Retiree => "retired".into(),
                    LifeStage::Adult => "unemployed".into(),
                },
            };
            // Households share a home: its street and district are read once.
            let (home_label, street, district) = match c.home {
                Some(h) => homes
                    .entry(h)
                    .or_insert_with(|| {
                        let lot = w.buildings.get(h).map(|b| b.lot);
                        let street = lot.and_then(|l| w.lots.get(l)).map_or(String::new(), |l| street_of(w, l));
                        let district = lot.and_then(|l| w.district_of_lot(l)).map_or(String::new(), |d| d.name.clone());
                        (bname(h).to_string(), street, district)
                    })
                    .clone(),
                None => ("no home".to_string(), String::new(), String::new()),
            };
            let favourite = c
                .places
                .places
                .iter()
                .filter(|p| !p.never && w.buildings.contains_key(p.place))
                .max_by(|a, b| (a.regular, a.score).partial_cmp(&(b.regular, b.score)).unwrap_or(Ordering::Equal))
                .map_or(String::new(), |p| bname(p.place).to_string());
            let friends = w.social.circles.get(&id).map_or(0, |k| k.friends().count()) as u16;
            let is_ill = ill.contains(&id);
            let status = Status::of(&c.activity, c.asleep, is_ill, c.home.is_some());
            let class = w.class_of(id);
            let hay = hay_of(&[&c.name, &work_label, &home_label, &street, &district, &favourite, status.label()]);
            PersonRow {
                id,
                name: c.name.clone(),
                age: c.age,
                stage,
                class,
                income: c.income,
                work: c.work,
                work_label,
                home: c.home,
                home_label,
                street,
                district,
                household: c.household.and_then(|h| w.households.get(h)).map_or(1, |h| h.size().min(255)) as u8,
                mood: c.mood,
                satiety: c.satiety,
                hungry_days: c.hungry_days,
                ill: is_ill,
                commute: usual_mode(c),
                favourite,
                friends,
                status,
                pos: c.pos,
                hay,
            }
        })
        .collect()
}

/// The districts that appear in some rows, sorted, for the picker.
pub fn districts_in<'a>(names: impl Iterator<Item = &'a str>) -> Vec<String> {
    let mut v: Vec<String> = names.filter(|s| !s.is_empty()).map(str::to_string).collect();
    v.sort();
    v.dedup();
    v
}

#[cfg(test)]
mod tests {
    use super::*;

    fn biz(name: &str, kind: BusinessKind, money: f32, rating: Option<f32>, state: BizState, district: &str, street: &str) -> BizRow {
        let kind_label = capitalise(kind.label());
        BizRow {
            id: BuildingId::default(),
            name: name.into(),
            kind,
            hay: format!("{name} {kind_label} {street} {district}").to_lowercase(),
            kind_label,
            owner: String::new(),
            owner_kind: "",
            district: district.into(),
            street: street.into(),
            staff: 3,
            jobs: 5,
            revenue_today: money / 10.0,
            spend_today: 0.0,
            revenue_yesterday: None,
            profit_yesterday: None,
            money,
            rating,
            rated: rating.map_or(0, |_| 4),
            footfall: 0,
            state,
            pos: None,
        }
    }

    fn rows() -> Vec<BizRow> {
        vec![
            biz("Lucky Bodega", BusinessKind::Grocery, 500.0, Some(4.2), BizState::Open, "Old Town", "Kessler Street"),
            biz("bright Bakes", BusinessKind::Bakery, 120.0, None, BizState::AfterHours, "Docks", "Harbour Road"),
            biz("Apex Office", BusinessKind::Office, -400.0, Some(2.0), BizState::Broke, "Old Town", "Main Avenue"),
            biz("Corner Grocer", BusinessKind::Grocery, 900.0, Some(3.1), BizState::Open, "", "Kessler Street"),
        ]
    }

    fn names(rows: &[BizRow], idx: &[u32]) -> Vec<String> {
        idx.iter().map(|&i| rows[i as usize].name.clone()).collect()
    }

    /// Text sorts case-blind; descending reverses it.
    #[test]
    fn sorts_by_name_either_way() {
        let r = rows();
        let f = Filter::default();
        assert_eq!(names(&r, &order(&r, &f, BizCol::Name, false)), ["Apex Office", "bright Bakes", "Corner Grocer", "Lucky Bodega"]);
        assert_eq!(names(&r, &order(&r, &f, BizCol::Name, true)), ["Lucky Bodega", "Corner Grocer", "bright Bakes", "Apex Office"]);
    }

    /// Numbers sort as numbers, negatives included.
    #[test]
    fn sorts_numbers() {
        let r = rows();
        let f = Filter::default();
        assert_eq!(names(&r, &order(&r, &f, BizCol::Money, true)), ["Corner Grocer", "Lucky Bodega", "bright Bakes", "Apex Office"]);
        assert_eq!(names(&r, &order(&r, &f, BizCol::Money, false))[0], "Apex Office");
    }

    /// Missing values (no rating yet, no district) sit at the bottom both ways.
    #[test]
    fn missing_values_last_both_ways() {
        let r = rows();
        let f = Filter::default();
        assert_eq!(names(&r, &order(&r, &f, BizCol::Rating, true)), ["Lucky Bodega", "Corner Grocer", "Apex Office", "bright Bakes"]);
        assert_eq!(names(&r, &order(&r, &f, BizCol::Rating, false)), ["Apex Office", "Corner Grocer", "Lucky Bodega", "bright Bakes"]);
        assert_eq!(names(&r, &order(&r, &f, BizCol::District, false)).last().unwrap(), "Corner Grocer");
        assert_eq!(names(&r, &order(&r, &f, BizCol::District, true)).last().unwrap(), "Corner Grocer");
    }

    /// Ties fall back to the name, so the order is stable.
    #[test]
    fn ties_break_by_name() {
        let r = rows();
        let f = Filter::default();
        // Two on Kessler Street.
        let by_street = names(&r, &order(&r, &f, BizCol::Street, false));
        assert_eq!(by_street, ["bright Bakes", "Corner Grocer", "Lucky Bodega", "Apex Office"]);
        // State: broke first (more urgent), then the two open ones by name, then closed.
        assert_eq!(names(&r, &order(&r, &f, BizCol::State, false)), ["Apex Office", "Corner Grocer", "Lucky Bodega", "bright Bakes"]);
    }

    /// The search finds any word in name, kind, street or district,
    /// case-blind; every word must match.
    #[test]
    fn search_filters() {
        let r = rows();
        let mut f = Filter { search: "kessler".into(), ..Default::default() };
        assert_eq!(names(&r, &order(&r, &f, BizCol::Name, false)), ["Corner Grocer", "Lucky Bodega"]);
        f.search = "GROCERY old".into();
        assert_eq!(names(&r, &order(&r, &f, BizCol::Name, false)), ["Lucky Bodega"]);
        f.search = "bakery".into();
        assert_eq!(names(&r, &order(&r, &f, BizCol::Name, false)), ["bright Bakes"]);
        f.search = "  ".into();
        assert_eq!(order(&r, &f, BizCol::Name, false).len(), 4);
        f.search = "nowhere".into();
        assert!(order(&r, &f, BizCol::Name, false).is_empty());
    }

    /// The pickers: kind, state and district, together with the search.
    #[test]
    fn pickers_filter() {
        let r = rows();
        let f = Filter { kind: Some(BusinessKind::Grocery), ..Default::default() };
        assert_eq!(names(&r, &order(&r, &f, BizCol::Money, true)), ["Corner Grocer", "Lucky Bodega"]);
        let f = Filter { district: Some("Old Town".into()), ..Default::default() };
        assert_eq!(names(&r, &order(&r, &f, BizCol::Name, false)), ["Apex Office", "Lucky Bodega"]);
        let f = Filter { state: Some(BizState::Open), district: Some("Old Town".into()), ..Default::default() };
        assert_eq!(names(&r, &order(&r, &f, BizCol::Name, false)), ["Lucky Bodega"]);
        let f = Filter { kind: Some(BusinessKind::Grocery), search: "corner".into(), ..Default::default() };
        assert_eq!(names(&r, &order(&r, &f, BizCol::Name, false)), ["Corner Grocer"]);
    }

    fn person(name: &str, age: u8, class: Class, income: f32, work: Option<&str>, status: Status, ill: bool) -> PersonRow {
        let work_label = work.unwrap_or("unemployed").to_string();
        PersonRow {
            id: CitizenId::default(),
            name: name.into(),
            age,
            stage: LifeStage::of_age(age),
            class,
            income,
            work: work.map(|_| BuildingId::default()),
            hay: format!("{name} {work_label}").to_lowercase(),
            work_label,
            home: None,
            home_label: "no home".into(),
            street: String::new(),
            district: String::new(),
            household: 1,
            mood: 0.5,
            satiety: 0.8,
            hungry_days: 0,
            ill,
            commute: None,
            favourite: String::new(),
            friends: 0,
            status,
            pos: Vec2::ZERO,
        }
    }

    /// People: class sorts by class then income; jobless sit last on the
    /// job column; the ill sort below everyone on health; the class and
    /// status pickers filter.
    #[test]
    fn people_sort_and_filter() {
        let r = vec![
            person("Ada Moss", 34, Class::Middle, 120.0, Some("Lucky Bodega"), Status::Working, false),
            person("Bo Lin", 70, Class::Low, 10.0, None, Status::AtHome, false),
            person("Cy Vance", 51, Class::Elite, 9000.0, Some("Apex Office"), Status::Out, false),
            person("Di Park", 29, Class::Middle, 80.0, Some("Apex Office"), Status::Ill, true),
        ];
        let f = Filter::default();
        let n = |idx: Vec<u32>| idx.iter().map(|&i| r[i as usize].name.clone()).collect::<Vec<_>>();
        assert_eq!(n(order(&r, &f, PersonCol::Class, true)), ["Cy Vance", "Ada Moss", "Di Park", "Bo Lin"]);
        assert_eq!(n(order(&r, &f, PersonCol::Age, false)), ["Di Park", "Ada Moss", "Cy Vance", "Bo Lin"]);
        assert_eq!(n(order(&r, &f, PersonCol::Job, false)), ["Cy Vance", "Di Park", "Ada Moss", "Bo Lin"]);
        assert_eq!(n(order(&r, &f, PersonCol::Job, true)), ["Ada Moss", "Cy Vance", "Di Park", "Bo Lin"]);
        assert_eq!(n(order(&r, &f, PersonCol::Health, false))[0], "Di Park");
        let f = Filter { class: Some(Class::Middle), ..Default::default() };
        assert_eq!(n(order(&r, &f, PersonCol::Name, false)), ["Ada Moss", "Di Park"]);
        let f = Filter { status: Some(Status::Ill), ..Default::default() };
        assert_eq!(n(order(&r, &f, PersonCol::Name, false)), ["Di Park"]);
        let f = Filter { search: "apex".into(), ..Default::default() };
        assert_eq!(n(order(&r, &f, PersonCol::Name, false)), ["Cy Vance", "Di Park"]);
    }

    /// A status from an activity: illness and homelessness weigh first.
    #[test]
    fn status_of_activity() {
        assert_eq!(Status::of(&Activity::AtHome, true, false, true), Status::Asleep);
        assert_eq!(Status::of(&Activity::AtHome, false, false, true), Status::AtHome);
        assert_eq!(Status::of(&Activity::Working, false, true, true), Status::Ill);
        assert_eq!(Status::of(&Activity::Idle, false, false, false), Status::Homeless);
        assert_eq!(Status::of(&Activity::Idle, false, false, true), Status::Out);
        assert_eq!(Status::of(&Activity::Arriving, false, false, false), Status::Arriving);
    }

    /// Column and state names parse from the API's words.
    #[test]
    fn names_parse() {
        for c in BizCol::ALL {
            assert_eq!(BizCol::parse(c.key()), Some(c));
        }
        for c in PersonCol::ALL {
            assert_eq!(PersonCol::parse(c.key()), Some(c));
        }
        for s in BizState::ALL {
            assert_eq!(BizState::parse(s.key()), Some(s));
        }
        for s in Status::ALL {
            assert_eq!(Status::parse(s.key()), Some(s));
        }
        assert_eq!(BizCol::parse("profit"), Some(BizCol::Profit));
        assert_eq!(PersonCol::parse("works at"), Some(PersonCol::Job));
        assert_eq!(class_parse("rich"), Some(Class::High));
        assert_eq!(BizState::parse("under construction"), Some(BizState::Building));
    }
}
