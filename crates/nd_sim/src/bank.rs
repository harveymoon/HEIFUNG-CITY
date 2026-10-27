//! [ledger] The city's money as accounts, and the one door it moves by.
//!
//! Every balance in the sim is an account: each household's purse, each
//! citizen's pocket, each business's till, each property owner (firms,
//! developers, residents' property companies, mega-corps through their
//! cash, the university through its funds), each tower developer, each
//! tourist's wallet, the air taxi company, the independent landlords as one
//! pool, the treasury and one **Outside World** (wages earned out of town,
//! goods bought and sold abroad, visitors' money, owners out of town). Money
//! moves only by `World::transfer(from, to, amount, kind)`, which takes it
//! off one balance and puts the same amount on another, so the sum of all
//! balances plus the outside world is always zero: nothing is made or lost
//! inside the city, and every edge is a booked flow to or from the outside.
//!
//! *Exact.* Balances are kept here as whole ten-thousandths of a dollar
//! (`i64`, [`PER_DOLLAR`]); the money fields the rest of the sim reads
//! (`Household::money`, `Business::money`, `Economy::treasury`, ...) are
//! mirrors written from them after every move. Sums of `f32` drift by a cent
//! here and there, sums of integers do not, so conservation holds exactly.
//! A mirror changed behind the door's back (a direct `money +=`) no longer
//! matches its account: the next move or the daily reconcile books the
//! difference as [`Kind::Untracked`] from the outside world and the debug
//! check reports which account drifted and the last kind it booked.
//!
//! *Cheap logging.* No row per move: today's totals by (kind, payer class,
//! payee class) in one dense array, closed into a sparse day summary at
//! midnight (60 days kept and saved); each account keeps a short ring of its
//! last moves for the inspectors and the remote (not saved); one account may
//! be traced in full on demand.
//!
//! *Saves.* Only the outside world's, the landlords' and the totals are
//! saved: every other balance is its mirror, read back into its account on
//! load (`World::open_ledger`, booked as [`Kind::Opening`]), so an old save
//! migrates from its money fields with no special case.

use crate::buildings::BuildingId;
use crate::citizens::{CitizenId, HouseholdId};
use crate::economy::{Flow, FLOWS};
use crate::estate::OwnerId;
use crate::tourism::TouristId;
use serde::{Deserialize, Serialize};
use slotmap::{Key, KeyData};
use std::collections::VecDeque;

/// Ledger units per dollar: balances are whole ten-thousandths of a dollar,
/// fine enough that a fraction of a cent moved a tick is not rounded away.
pub const PER_DOLLAR: f64 = 10_000.0;
/// Moves remembered per account (the inspectors' "recent transactions").
pub const RING: usize = 50;
/// Per citizen pocket and tourist wallet: they are many and move little.
pub const RING_SMALL: usize = 24;
/// Day summaries kept.
pub const HISTORY_DAYS: usize = 60;
/// Moves kept by the full trace of one account.
pub const TRACE_CAP: usize = 100_000;
/// Drift reports kept.
pub const DRIFTS_KEPT: usize = 64;

/// Dollars to ledger units, rounded to the nearest unit (0 for NaN).
pub fn units(d: f64) -> i64 {
    if d.is_finite() {
        (d * PER_DOLLAR).round() as i64
    } else {
        0
    }
}

/// Ledger units to dollars.
pub fn dollars(u: i64) -> f64 {
    u as f64 / PER_DOLLAR
}

/// Who holds money.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub enum Account {
    /// Everything beyond the city limits.
    Outside,
    /// The city's own money (`Economy::treasury`).
    Treasury,
    /// The independent landlords who own the buildings no owner record
    /// holds: one pool.
    Landlords,
    /// The air taxi company (`AirCompany::money`).
    Airline,
    Household(HouseholdId),
    /// A citizen's pocket money (`Citizen::savings`).
    Person(CitizenId),
    /// The business in this building (`Business::money`).
    Business(BuildingId),
    /// A property owner of its own: a firm, developer, residents' property
    /// company (`Owner::money`), a mega-corp (`MegaCorp::cash`) or the
    /// university (`Campus::funds`). Owners that are a household or a
    /// person resolve to that household or person (`World::owner_account`).
    Owner(OwnerId),
    /// A tourist's wallet (`Tourist::money`).
    Tourist(TouristId),
    /// A supertall developer (`TowerDeveloper::capital`), by its index.
    Tower(u16),
}

impl Account {
    /// "household 12:3", "business 4:1", "treasury": the remote's spelling
    /// (ids as the remote writes them, "idx:version").
    pub fn label(self) -> String {
        fn k(d: KeyData) -> String {
            let f = d.as_ffi();
            format!("{}:{}", f & 0xFFFF_FFFF, f >> 32)
        }
        match self {
            Account::Outside => "outside".into(),
            Account::Treasury => "treasury".into(),
            Account::Landlords => "landlords".into(),
            Account::Airline => "airline".into(),
            Account::Household(h) => format!("household {}", k(h.data())),
            Account::Person(c) => format!("person {}", k(c.data())),
            Account::Business(b) => format!("business {}", k(b.data())),
            Account::Owner(o) => format!("owner {}", k(o.data())),
            Account::Tourist(t) => format!("tourist {}", k(t.data())),
            Account::Tower(i) => format!("tower {i}"),
        }
    }

    /// Read back what `label` wrote ("household 12:3"; also "household:12:3").
    pub fn parse(s: &str) -> Option<Account> {
        let s = s.trim();
        let (what, id) = match s.split_once([' ', ':']) {
            Some((a, b)) => (a, b.trim()),
            None => (s, ""),
        };
        let key = || -> Option<KeyData> {
            let (i, v) = id.split_once(':')?;
            let (i, v): (u64, u64) = (i.parse().ok()?, v.parse().ok()?);
            Some(KeyData::from_ffi((v << 32) | i))
        };
        Some(match what.to_ascii_lowercase().as_str() {
            "outside" => Account::Outside,
            "treasury" | "city" => Account::Treasury,
            "landlords" => Account::Landlords,
            "airline" => Account::Airline,
            "household" => Account::Household(key()?.into()),
            "person" | "citizen" => Account::Person(key()?.into()),
            "business" | "building" => Account::Business(key()?.into()),
            "owner" | "company" => Account::Owner(key()?.into()),
            "tourist" => Account::Tourist(key()?.into()),
            "tower" => Account::Tower(id.parse().ok()?),
            _ => return None,
        })
    }

    /// The slot behind a keyed account, as `KeyData::as_ffi`.
    fn ffi(self) -> u64 {
        match self {
            Account::Household(h) => h.data().as_ffi(),
            Account::Person(c) => c.data().as_ffi(),
            Account::Business(b) => b.data().as_ffi(),
            Account::Owner(o) => o.data().as_ffi(),
            Account::Tourist(t) => t.data().as_ffi(),
            _ => 0,
        }
    }
}

/// Who an account belongs to, for the daily totals and the money-flow page.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub enum Class {
    Outside,
    City,
    Household,
    Person,
    Business,
    /// Firms, developers, residents' property companies, tower developers.
    Company,
    Corp,
    University,
    Landlords,
    Tourist,
    Airline,
}

pub const CLASSES: usize = 11;

impl Class {
    pub const ALL: [Class; CLASSES] = [Class::Outside, Class::City, Class::Household, Class::Person, Class::Business, Class::Company, Class::Corp, Class::University, Class::Landlords, Class::Tourist, Class::Airline];

    pub fn index(self) -> usize {
        self as usize
    }

    pub fn label(self) -> &'static str {
        match self {
            Class::Outside => "outside world",
            Class::City => "city treasury",
            Class::Household => "households",
            Class::Person => "pockets",
            Class::Business => "businesses",
            Class::Company => "companies",
            Class::Corp => "mega-corps",
            Class::University => "university",
            Class::Landlords => "independent landlords",
            Class::Tourist => "tourists",
            Class::Airline => "air taxis",
        }
    }
}

/// What money moved for. The treasury's own flows (`Flow`) are kinds too, so
/// the Economy window's ledger reads off the same door.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Kind {
    Flow(Flow),
    /// Earnings: wages, gig money and pensions, paid in from outside.
    Wages,
    /// A business's wage bill.
    Payroll,
    /// A household's everyday living costs, spent out of town.
    Living,
    /// Between a household's purse and its members' pockets.
    Pocket,
    /// A home's rent, to the building's owner.
    Rent,
    /// A business's rent for its unit, to the building's owner.
    ShopRent,
    /// An owner's upkeep of what it lets.
    Upkeep,
    /// Mortgage payments to the banks out of town.
    Loan,
    /// An owner's or company's profit paid out (to its investor, its
    /// founder, or out of town).
    Dividend,
    /// Groceries, shopping and goods bought by residents.
    Shopping,
    /// Rounds at venues, tickets, outings.
    Outings,
    /// Sales to passers-by and walk-in custom from out of town.
    WalkIn,
    /// Goods one business buys from another.
    Supplies,
    /// Goods bought from outside.
    Imports,
    /// Goods sold to outside.
    Exports,
    /// Building work and materials paid by private owners.
    Building,
    /// A property changing hands.
    PropertySale,
    /// Capital put in: deposits, stakes, start-up money.
    Investment,
    /// Cars and bikes bought and sold.
    Cars,
    /// Private fares: taxis, ferries, air taxis.
    Fares,
    /// Private fees: parking, trash, docks, jetties, beaches, services.
    Fees,
    Theft,
    /// Between people: gifts, help from friends, a stake in a friend's plan.
    Gifts,
    Tuition,
    /// Hospital and care bills.
    Healthcare,
    /// What visitors spend: hotel nights, meals, sights.
    Tourism,
    /// Money the player gave the treasury (a grant, the dev console).
    Grants,
    /// A new account's money, brought in from outside (immigrants, new
    /// businesses' start-up float, visitors arriving).
    Arrival,
    /// A closed account's money, gone out of town with it (emigrants,
    /// businesses wound up, visitors leaving).
    Departure,
    /// A mirror changed outside the door: the difference, booked so the
    /// sum still holds. Zero in a healthy city (the debug check).
    Untracked,
    /// Balances read in from a save or an older one's money fields.
    Opening,
}

/// The kinds that are not the treasury's flows, in order after them.
pub const PRIVATE: [Kind; 31] = [
    Kind::Wages,
    Kind::Payroll,
    Kind::Living,
    Kind::Pocket,
    Kind::Rent,
    Kind::ShopRent,
    Kind::Upkeep,
    Kind::Loan,
    Kind::Dividend,
    Kind::Shopping,
    Kind::Outings,
    Kind::WalkIn,
    Kind::Supplies,
    Kind::Imports,
    Kind::Exports,
    Kind::Building,
    Kind::PropertySale,
    Kind::Investment,
    Kind::Cars,
    Kind::Fares,
    Kind::Fees,
    Kind::Theft,
    Kind::Gifts,
    Kind::Tuition,
    Kind::Healthcare,
    Kind::Tourism,
    Kind::Grants,
    Kind::Arrival,
    Kind::Departure,
    Kind::Untracked,
    Kind::Opening,
];

pub const KINDS: usize = FLOWS + PRIVATE.len();

impl Kind {
    pub fn index(self) -> usize {
        match self {
            Kind::Flow(f) => f.index(),
            k => FLOWS + PRIVATE.iter().position(|p| *p == k).expect("every kind is listed in PRIVATE"),
        }
    }

    pub fn from_index(i: usize) -> Option<Kind> {
        if i < FLOWS {
            Flow::ALL.get(i).map(|f| Kind::Flow(*f))
        } else {
            PRIVATE.get(i - FLOWS).copied()
        }
    }

    pub fn all() -> impl Iterator<Item = Kind> {
        (0..KINDS).filter_map(Kind::from_index)
    }

    pub fn label(self) -> &'static str {
        match self {
            Kind::Flow(f) => f.label(),
            Kind::Wages => "wages and pensions",
            Kind::Payroll => "payroll",
            Kind::Living => "living costs",
            Kind::Pocket => "pocket money",
            Kind::Rent => "rent",
            Kind::ShopRent => "shop rent",
            Kind::Upkeep => "upkeep and running costs",
            Kind::Loan => "mortgages",
            Kind::Dividend => "profits paid out",
            Kind::Shopping => "shopping",
            Kind::Outings => "outings",
            Kind::WalkIn => "walk-in sales",
            Kind::Supplies => "supplies",
            Kind::Imports => "imports",
            Kind::Exports => "exports",
            Kind::Building => "building work",
            Kind::PropertySale => "property sales",
            Kind::Investment => "investment",
            Kind::Cars => "cars and bikes",
            Kind::Fares => "private fares",
            Kind::Fees => "private fees",
            Kind::Theft => "theft",
            Kind::Gifts => "gifts and help",
            Kind::Tuition => "tuition",
            Kind::Healthcare => "healthcare",
            Kind::Tourism => "tourism",
            Kind::Grants => "grants",
            Kind::Arrival => "arrivals",
            Kind::Departure => "departures",
            Kind::Untracked => "untracked",
            Kind::Opening => "opening balances",
        }
    }

    /// The remote's spelling: the label's first word or two, snake_cased
    /// ("rent", "shop_rent", "income_tax").
    pub fn key(self) -> String {
        self.label().replace([' ', '-'], "_")
    }

    /// Read back a `key` (or a label).
    pub fn parse(s: &str) -> Option<Kind> {
        let s = s.trim().to_ascii_lowercase().replace([' ', '-'], "_");
        Kind::all().find(|k| k.key() == s || format!("{k:?}").to_ascii_lowercase() == s.replace('_', ""))
    }

    /// The headline group on the money-flow page.
    pub fn group(self) -> &'static str {
        match self {
            Kind::Rent | Kind::ShopRent | Kind::Upkeep | Kind::Loan | Kind::Flow(Flow::PropertyTax) => "property",
            // [e2-tenants] Whole buildings changing hands at their value
            // (a block is worth months of the city's rent): a group of its
            // own, so a day's one sale does not flatten the rent beside it.
            Kind::PropertySale => "sales",
            Kind::Wages | Kind::Payroll | Kind::Pocket => "wages",
            Kind::Dividend | Kind::Investment => "profits",
            Kind::Flow(_) => "city",
            Kind::Shopping | Kind::Outings | Kind::WalkIn | Kind::Supplies | Kind::Imports | Kind::Exports | Kind::Tourism => "trade",
            _ => "other",
        }
    }
}

/// One move as an account saw it: positive in, negative out.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Tx {
    pub tick: u64,
    pub kind: Kind,
    pub other: Account,
    pub units: i64,
}

impl Tx {
    pub fn dollars(&self) -> f64 {
        dollars(self.units)
    }
}

/// An account's balance and its last moves.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Entry {
    pub units: i64,
    /// The slot key the entry belongs to (`KeyData::as_ffi`), for keyed accounts.
    #[serde(skip)]
    key: u64,
    #[serde(skip)]
    pub ring: VecDeque<Tx>,
}

impl Entry {
    fn push(&mut self, tx: Tx, cap: usize) {
        if self.ring.len() >= cap {
            self.ring.pop_front();
        }
        self.ring.push_back(tx);
    }
}

/// Entries by slot index, each knowing its key's version.
#[derive(Clone, Debug, Default)]
struct Slots(Vec<Option<Entry>>);

impl Slots {
    fn idx(ffi: u64) -> usize {
        (ffi & 0xFFFF_FFFF) as usize
    }

    fn get(&self, ffi: u64) -> Option<&Entry> {
        self.0.get(Self::idx(ffi))?.as_ref().filter(|e| e.key == ffi)
    }

    fn get_mut(&mut self, ffi: u64) -> Option<&mut Entry> {
        self.0.get_mut(Self::idx(ffi))?.as_mut().filter(|e| e.key == ffi)
    }

    /// The key of an older account still in this slot.
    fn stale(&self, ffi: u64) -> Option<u64> {
        self.0.get(Self::idx(ffi))?.as_ref().map(|e| e.key).filter(|k| *k != ffi)
    }

    fn open(&mut self, ffi: u64) -> &mut Entry {
        let i = Self::idx(ffi);
        if self.0.len() <= i {
            self.0.resize_with(i + 1, || None);
        }
        self.0[i] = Some(Entry { units: 0, key: ffi, ring: VecDeque::new() });
        self.0[i].as_mut().unwrap()
    }

    fn close(&mut self, ffi: u64) {
        if let Some(s) = self.0.get_mut(Self::idx(ffi)) {
            if s.as_ref().is_some_and(|e| e.key == ffi) {
                *s = None;
            }
        }
    }

    fn keys(&self) -> impl Iterator<Item = u64> + '_ {
        self.0.iter().flatten().map(|e| e.key)
    }

    fn sum(&self) -> i64 {
        self.0.iter().flatten().map(|e| e.units).sum()
    }

    fn len(&self) -> usize {
        self.0.iter().flatten().count()
    }
}

/// One cell of a day's totals.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Cell {
    pub kind: Kind,
    pub from: Class,
    pub to: Class,
    pub units: i64,
    #[serde(default)]
    pub n: u32,
}

impl Cell {
    pub fn dollars(&self) -> f64 {
        dollars(self.units)
    }
}

/// A closed day: its non-empty cells.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct DaySummary {
    pub day: u64,
    pub cells: Vec<Cell>,
}

impl DaySummary {
    /// The day's total of one kind.
    pub fn of(&self, kind: Kind) -> f64 {
        dollars(self.cells.iter().filter(|c| c.kind == kind).map(|c| c.units).sum())
    }
}

/// Today's totals, dense: (kind, from class, to class) → units and moves.
#[derive(Clone, Debug)]
pub struct Totals {
    units: Vec<i64>,
    n: Vec<u32>,
}

impl Default for Totals {
    fn default() -> Self {
        Totals { units: vec![0; KINDS * CLASSES * CLASSES], n: vec![0; KINDS * CLASSES * CLASSES] }
    }
}

impl Totals {
    fn cell(kind: Kind, from: Class, to: Class) -> usize {
        (kind.index() * CLASSES + from.index()) * CLASSES + to.index()
    }

    fn add(&mut self, kind: Kind, from: Class, to: Class, u: i64) {
        let i = Self::cell(kind, from, to);
        self.units[i] += u;
        self.n[i] += 1;
    }

    /// The non-empty cells.
    pub fn cells(&self) -> Vec<Cell> {
        let mut v = Vec::new();
        for (i, (&u, &n)) in self.units.iter().zip(&self.n).enumerate() {
            if n == 0 {
                continue;
            }
            let (k, rest) = (i / (CLASSES * CLASSES), i % (CLASSES * CLASSES));
            let Some(kind) = Kind::from_index(k) else { continue };
            v.push(Cell { kind, from: Class::ALL[rest / CLASSES], to: Class::ALL[rest % CLASSES], units: u, n });
        }
        v
    }

    fn clear(&mut self) {
        self.units.iter_mut().for_each(|x| *x = 0);
        self.n.iter_mut().for_each(|x| *x = 0);
    }

    fn from_cells(cells: &[Cell]) -> Self {
        let mut t = Totals::default();
        for c in cells {
            let i = Self::cell(c.kind, c.from, c.to);
            t.units[i] += c.units;
            t.n[i] += c.n;
        }
        t
    }
}

impl Serialize for Totals {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        self.cells().serialize(s)
    }
}

impl<'de> Deserialize<'de> for Totals {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let cells: Vec<Cell> = Vec::deserialize(d)?;
        Ok(Totals::from_cells(&cells))
    }
}

/// A mirror that no longer matched its account (`Kind::Untracked`).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Drift {
    pub tick: u64,
    pub account: String,
    pub dollars: f64,
    /// The last kind the account booked before the drift showed.
    pub last_kind: Option<Kind>,
}

/// How an account's mirror reads, for the door to compare and write.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Mirror {
    F32(f32),
    F64(f64),
    /// No field outside the ledger (the outside world, the landlords).
    None,
}

impl Mirror {
    /// Does it show `u`, exactly as the door writes it back?
    pub fn shows(self, u: i64) -> bool {
        match self {
            Mirror::F32(m) => dollars(u) as f32 == m,
            Mirror::F64(m) => dollars(u) == m,
            Mirror::None => true,
        }
    }

    pub fn units(self) -> i64 {
        match self {
            Mirror::F32(m) => units(m as f64),
            Mirror::F64(m) => units(m),
            Mirror::None => 0,
        }
    }
}

/// The ledger.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Bank {
    /// The outside world, the treasury, the landlords and the air taxi
    /// company. Only the outside world's and the landlords' balances are
    /// really needed in a save (the others have mirrors).
    pub fixed: [Entry; 4],
    pub today: Totals,
    pub history: VecDeque<DaySummary>,
    /// [Kind::Untracked] booked since the city began, and how often.
    pub untracked_units: i64,
    pub untracked_moves: u32,
    pub drifts: VecDeque<Drift>,
    /// The minute every move is stamped with (`World::tick` sets it).
    #[serde(skip)]
    pub now: u64,
    /// Moves today, all kinds.
    #[serde(skip)]
    pub moves_today: u64,
    /// Accounts read in since the load (or the start): new ones after it
    /// are `Arrival`s, before it `Opening`s.
    #[serde(skip)]
    pub opened: bool,
    /// Reconcile after every tick and report drifts (the debug check).
    #[serde(skip)]
    pub check: bool,
    /// Every move of one account, kept in full (debug).
    #[serde(skip)]
    pub trace: Option<Account>,
    #[serde(skip)]
    pub trace_log: VecDeque<Tx>,
    /// Moves that named an account that no longer exists (paid to or from
    /// the outside world instead).
    #[serde(skip)]
    pub orphans: u64,
    #[serde(skip)]
    households: Slots,
    #[serde(skip)]
    persons: Slots,
    #[serde(skip)]
    businesses: Slots,
    #[serde(skip)]
    owners: Slots,
    #[serde(skip)]
    tourists: Slots,
    #[serde(skip)]
    towers: Vec<Entry>,
}

impl Bank {
    fn fixed_index(a: Account) -> Option<usize> {
        match a {
            Account::Outside => Some(0),
            Account::Treasury => Some(1),
            Account::Landlords => Some(2),
            Account::Airline => Some(3),
            _ => None,
        }
    }

    fn slots(&self, a: Account) -> Option<&Slots> {
        Some(match a {
            Account::Household(_) => &self.households,
            Account::Person(_) => &self.persons,
            Account::Business(_) => &self.businesses,
            Account::Owner(_) => &self.owners,
            Account::Tourist(_) => &self.tourists,
            _ => return None,
        })
    }

    fn slots_mut(&mut self, a: Account) -> Option<&mut Slots> {
        Some(match a {
            Account::Household(_) => &mut self.households,
            Account::Person(_) => &mut self.persons,
            Account::Business(_) => &mut self.businesses,
            Account::Owner(_) => &mut self.owners,
            Account::Tourist(_) => &mut self.tourists,
            _ => return None,
        })
    }

    pub fn entry(&self, a: Account) -> Option<&Entry> {
        if let Some(i) = Self::fixed_index(a) {
            return Some(&self.fixed[i]);
        }
        if let Account::Tower(i) = a {
            return self.towers.get(i as usize);
        }
        self.slots(a)?.get(a.ffi())
    }

    fn entry_mut(&mut self, a: Account) -> Option<&mut Entry> {
        if let Some(i) = Self::fixed_index(a) {
            return Some(&mut self.fixed[i]);
        }
        if let Account::Tower(i) = a {
            return self.towers.get_mut(i as usize);
        }
        let ffi = a.ffi();
        self.slots_mut(a)?.get_mut(ffi)
    }

    /// An account's balance in units (0 when it has none yet).
    pub fn balance_units(&self, a: Account) -> i64 {
        self.entry(a).map_or(0, |e| e.units)
    }

    pub fn balance(&self, a: Account) -> f64 {
        dollars(self.balance_units(a))
    }

    /// The account's last moves, newest last.
    pub fn recent(&self, a: Account) -> Vec<Tx> {
        self.entry(a).map_or_else(Vec::new, |e| e.ring.iter().copied().collect())
    }

    fn cap(a: Account) -> usize {
        match a {
            Account::Person(_) | Account::Tourist(_) => RING_SMALL,
            _ => RING,
        }
    }

    /// Move `u` units (> 0) from `from` to `to`. Both entries exist
    /// (`sync` first). The one place a balance changes.
    pub fn post(&mut self, from: (Account, Class), to: (Account, Class), u: i64, kind: Kind) {
        if u == 0 || from.0 == to.0 {
            return;
        }
        let tick = self.now;
        if let Some(e) = self.entry_mut(from.0) {
            e.units -= u;
            e.push(Tx { tick, kind, other: to.0, units: -u }, Self::cap(from.0));
        }
        if let Some(e) = self.entry_mut(to.0) {
            e.units += u;
            e.push(Tx { tick, kind, other: from.0, units: u }, Self::cap(to.0));
        }
        self.today.add(kind, from.1, to.1, u);
        self.moves_today += 1;
        if let Some(t) = self.trace {
            let tx = if t == from.0 {
                Some(Tx { tick, kind, other: to.0, units: -u })
            } else if t == to.0 {
                Some(Tx { tick, kind, other: from.0, units: u })
            } else {
                None
            };
            if let Some(tx) = tx {
                if self.trace_log.len() >= TRACE_CAP {
                    self.trace_log.pop_front();
                }
                self.trace_log.push_back(tx);
            }
        }
    }

    /// Make sure `a` has an entry matching its mirror `m`: open it (its
    /// money arrives from outside), close an older account left in its slot
    /// (its money leaves), or book a mirror changed outside the door.
    pub fn sync(&mut self, a: Account, class: Class, m: Mirror) {
        if a == Account::Outside {
            return;
        }
        if let Account::Tower(i) = a {
            while self.towers.len() <= i as usize {
                self.towers.push(Entry::default());
            }
        }
        let ffi = a.ffi();
        if let Some(old) = self.slots(a).and_then(|s| s.stale(ffi)) {
            self.close_key(a, old);
        }
        let fresh = self.slots(a).is_some_and(|s| s.get(ffi).is_none());
        if fresh {
            self.slots_mut(a).unwrap().open(ffi);
            let kind = if self.opened { Kind::Arrival } else { Kind::Opening };
            self.book_outside(a, class, m.units(), kind);
            return;
        }
        let have = self.entry(a).map_or(0, |e| e.units);
        if !m.shows(have) {
            let delta = m.units() - have;
            let last_kind = self.entry(a).and_then(|e| e.ring.back()).map(|t| t.kind);
            let kind = if self.opened { Kind::Untracked } else { Kind::Opening };
            self.book_outside(a, class, delta, kind);
            if kind == Kind::Untracked && delta != 0 {
                self.untracked_units += delta;
                self.untracked_moves += 1;
                if self.drifts.len() >= DRIFTS_KEPT {
                    self.drifts.pop_front();
                }
                self.drifts.push_back(Drift { tick: self.now, account: a.label(), dollars: dollars(delta), last_kind });
            }
        }
    }

    /// `u` units from the outside world to `a` (or back when negative).
    fn book_outside(&mut self, a: Account, class: Class, u: i64, kind: Kind) {
        if u > 0 {
            self.post((Account::Outside, Class::Outside), (a, class), u, kind);
        } else if u < 0 {
            self.post((a, class), (Account::Outside, Class::Outside), -u, kind);
        }
    }

    /// A keyed account whose holder is gone: its balance leaves town and
    /// the entry is dropped.
    pub fn close(&mut self, a: Account, class: Class) {
        let u = self.balance_units(a);
        if self.entry(a).is_none() {
            return;
        }
        self.book_outside(a, class, -u, Kind::Departure);
        let ffi = a.ffi();
        if let Some(s) = self.slots_mut(a) {
            s.close(ffi);
        }
    }

    fn close_key(&mut self, like: Account, ffi: u64) {
        let d = KeyData::from_ffi(ffi);
        let (a, class) = match like {
            Account::Household(_) => (Account::Household(d.into()), Class::Household),
            Account::Person(_) => (Account::Person(d.into()), Class::Person),
            Account::Business(_) => (Account::Business(d.into()), Class::Business),
            Account::Owner(_) => (Account::Owner(d.into()), Class::Company),
            Account::Tourist(_) => (Account::Tourist(d.into()), Class::Tourist),
            _ => return,
        };
        self.close(a, class);
    }

    /// Every keyed account with an entry, for the reconcile.
    pub fn keyed(&self) -> Vec<Account> {
        let mut v = Vec::new();
        v.extend(self.households.keys().map(|k| Account::Household(KeyData::from_ffi(k).into())));
        v.extend(self.persons.keys().map(|k| Account::Person(KeyData::from_ffi(k).into())));
        v.extend(self.businesses.keys().map(|k| Account::Business(KeyData::from_ffi(k).into())));
        v.extend(self.owners.keys().map(|k| Account::Owner(KeyData::from_ffi(k).into())));
        v.extend(self.tourists.keys().map(|k| Account::Tourist(KeyData::from_ffi(k).into())));
        v
    }

    pub fn outside_units_mut(&mut self) -> &mut i64 {
        &mut self.fixed[0].units
    }

    pub fn has(&self, a: Account) -> bool {
        self.entry(a).is_some()
    }

    /// The sum of every balance, the outside world's included: zero, always.
    pub fn sum_units(&self) -> i64 {
        self.fixed.iter().map(|e| e.units).sum::<i64>()
            + self.towers.iter().map(|e| e.units).sum::<i64>()
            + self.households.sum()
            + self.persons.sum()
            + self.businesses.sum()
            + self.owners.sum()
            + self.tourists.sum()
    }

    /// Accounts with an entry.
    pub fn accounts(&self) -> usize {
        4 + self.towers.len() + self.households.len() + self.persons.len() + self.businesses.len() + self.owners.len() + self.tourists.len()
    }

    /// Close the day: today's totals into the history.
    pub fn roll(&mut self, day: u64) {
        let cells = self.today.cells();
        if self.history.len() >= HISTORY_DAYS {
            self.history.pop_front();
        }
        self.history.push_back(DaySummary { day, cells });
        self.today.clear();
        self.moves_today = 0;
    }

    /// Today's cells.
    pub fn today_cells(&self) -> Vec<Cell> {
        self.today.cells()
    }

    /// Today's total of one kind.
    pub fn today_of(&self, kind: Kind) -> f64 {
        dollars(self.today.cells().iter().filter(|c| c.kind == kind).map(|c| c.units).sum())
    }

    /// The last closed day.
    pub fn yesterday(&self) -> Option<&DaySummary> {
        self.history.back()
    }

    /// The closed day `day`, if still kept.
    pub fn day(&self, day: u64) -> Option<&DaySummary> {
        self.history.iter().find(|d| d.day == day)
    }

    /// Start (or stop, `None`) tracing one account in full.
    pub fn set_trace(&mut self, a: Option<Account>) {
        self.trace = a;
        self.trace_log.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn moves_balance_exactly_and_land_in_the_totals() {
        let mut b = Bank { opened: true, ..Bank::default() };
        b.sync(Account::Treasury, Class::City, Mirror::F32(100.0));
        assert_eq!(b.balance(Account::Treasury), 100.0, "an account opens at its mirror");
        assert_eq!(b.sum_units(), 0, "its money came from outside");
        for _ in 0..1000 {
            b.post((Account::Treasury, Class::City), (Account::Landlords, Class::Landlords), units(0.1), Kind::Rent);
        }
        assert_eq!(b.balance_units(Account::Landlords), 1000 * 1000, "a thousand dimes are exactly $100");
        assert_eq!(b.balance_units(Account::Treasury), 0);
        assert_eq!(b.sum_units(), 0);
        let rent = b.today_cells().into_iter().find(|c| c.kind == Kind::Rent).unwrap();
        assert_eq!((rent.from, rent.to, rent.n), (Class::City, Class::Landlords, 1000));
        assert_eq!(b.recent(Account::Landlords).len(), RING, "the ring keeps the last few");
        b.roll(3);
        assert!(b.today_cells().is_empty());
        assert_eq!(b.day(3).unwrap().of(Kind::Rent), 100.0);
    }

    #[test]
    fn a_mirror_changed_behind_the_door_is_booked_as_untracked() {
        let mut b = Bank::default();
        b.sync(Account::Treasury, Class::City, Mirror::F32(50.0));
        b.opened = true;
        assert_eq!(b.untracked_units, 0, "read in before the books opened: an opening balance");
        b.sync(Account::Treasury, Class::City, Mirror::F32(80.0));
        assert_eq!(b.balance(Account::Treasury), 80.0);
        assert_eq!(b.untracked_units, units(30.0));
        assert_eq!(b.drifts.back().unwrap().account, "treasury");
        assert_eq!(b.sum_units(), 0);
    }

    #[test]
    fn labels_and_kinds_read_back() {
        let k = KeyData::from_ffi((3u64 << 32) | 12);
        let a = Account::Household(k.into());
        assert_eq!(a.label(), "household 12:3");
        assert_eq!(Account::parse("household 12:3"), Some(a));
        assert_eq!(Account::parse("household:12:3"), Some(a));
        assert_eq!(Account::parse("treasury"), Some(Account::Treasury));
        for k in Kind::all() {
            assert_eq!(Kind::parse(&k.key()), Some(k), "{k:?}");
            assert_eq!(Kind::from_index(k.index()), Some(k));
        }
        assert_eq!(Kind::parse("shop_rent"), Some(Kind::ShopRent));
    }

    #[test]
    fn the_totals_save_sparse_and_load_back() {
        let mut b = Bank { opened: true, ..Bank::default() };
        b.sync(Account::Treasury, Class::City, Mirror::F32(10.0));
        b.post((Account::Treasury, Class::City), (Account::Outside, Class::Outside), units(2.5), Kind::Flow(Flow::PlantUpkeep));
        let text = ron::to_string(&b).unwrap();
        let back: Bank = ron::from_str(&text).unwrap();
        assert_eq!(back.today_cells(), b.today_cells());
        assert_eq!(back.fixed[0].units, b.fixed[0].units, "the outside world's balance is saved");
    }
}
