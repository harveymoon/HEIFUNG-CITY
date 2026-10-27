//! [land] The land market: population pressure and the density it allows,
//! sale prices, who owns what (firms with branches and merged plots, people
//! with several homes), the forms merged plots are built in, and the
//! super-elite (fictional billionaires) with their mansions, NIMBY pull and
//! escorted convoys. The data lives here; the `World` side is
//! `world/estate.rs`.
//!
//! Everything is saved with serde defaults: a save from before this module
//! loads with an empty market, no owners and a cold pressure grid that
//! warms on its first hourly sample.

use crate::buildings::BuildingId;
use crate::citizens::{CitizenId, HouseholdId};
use crate::market::BusinessKind;
use crate::traffic::CarId;
use crate::vehicle::VehicleModel;
use crate::zoning::{polygon_area, Lot, LotId};
use glam::Vec2;
use serde::{Deserialize, Serialize};
use slotmap::{new_key_type, SlotMap};
use std::collections::BTreeMap;

new_key_type! {
    /// A firm or a person owning one or more properties.
    pub struct OwnerId;
}

// ── tuning ───────────────────────────────────────────────────────────────────

/// Side of a pressure cell, metres.
pub const PRESSURE_CELL: f32 = 64.0;
/// Weight of one hourly sample in the running average while pressure
/// falls: about two days of memory, so a quiet weekend does not empty a
/// district's prospects.
pub const PRESSURE_ALPHA: f32 = 1.0 / 48.0;
/// ... and while it rises: half a day, so a town growing fast is not built
/// a week behind its people (a single rush hour still moves it little).
pub const PRESSURE_ALPHA_UP: f32 = 1.0 / 12.0;
/// Pressure (people present plus people moving through a cell, smoothed)
/// below which land stays low: one or two storeys.
pub const PRESSURE_FLOOR0: f32 = 4.0;
/// Pressure for each floor above two, at a mobility of one.
pub const PRESSURE_PER_FLOOR: f32 = 2.5;
/// [pt-growth] People out on foot (walking, cycling, waiting at a stop,
/// on a bench, loitering, sightseeing) per cell, smoothed: about a day and
/// a half to rise and four days to fall, so a busy main street is a busy
/// main street all week, not just at rush hour.
pub const FOOT_ALPHA_UP: f32 = 1.0 / 36.0;
pub const FOOT_ALPHA_DOWN: f32 = 1.0 / 96.0;
/// What one person out on foot adds to the land density, over what they
/// add as someone present: the street life developers build for.
pub const FOOT_WEIGHT: f32 = 2.0;
/// Floors pressure can justify at most.
pub const PRESSURE_MAX_FLOORS: u8 = 40;
/// Floors over the old neighbour-density cap pressure may add: towers rise
/// beside other buildings, not in a field.
pub const PRESSURE_OVER_NEIGHBOURS: u8 = 5;
/// People a town needs before pressure caps its heights: a hamlet keeps
/// the neighbour rule alone (and small test towns build as they always did).
pub const PRESSURE_MIN_POP: usize = 300;
/// Height at or under which a block counts as low density (shacks,
/// trailer parks, suburbs, McMansions, mansions).
pub const LOW_DENSITY_FLOORS: u8 = 3;

/// [pt-growth] Lot assembly: where the land density asks for at least
/// `ASSEMBLY_MIN_FLOORS`, developers buy a row of small, low plots (each
/// under `ASSEMBLY_SMALL_LOT` m², at most `ASSEMBLY_MAX_OLD_FLOORS` high,
/// three to `ASSEMBLY_MAX_LOTS` of them in a compact block) and put one
/// bigger building across them: five bungalows become a block of flats.
pub const ASSEMBLY_MIN_FLOORS: u8 = 4;
pub const ASSEMBLY_SMALL_LOT: f32 = 1100.0;
pub const ASSEMBLY_MAX_OLD_FLOORS: u8 = 3;
pub const ASSEMBLY_MIN_LOTS: usize = 3;
pub const ASSEMBLY_MAX_LOTS: usize = 5;
/// The assembled block may be this much less compact than a merge.
pub const ASSEMBLY_HULL_SLACK: f32 = 1.15;
/// [pt-growth] Foreclosure: a block of homes at most this full ...
pub const FORECLOSE_OCCUPANCY: f32 = 0.35;
/// ... for this many days in a row while people wait for homes they can
/// afford is sold cheap to a developer, who cuts the rent (or rebuilds).
pub const FORECLOSE_DAYS: u8 = 5;
/// What a foreclosed building sells for, against its price.
pub const FORECLOSE_DISCOUNT: f64 = 0.6;

/// Most properties merged into one building (the primary lot included).
pub const MAX_MERGED_LOTS: usize = 4;
/// A merged footprint whose convex hull is more than this much bigger than
/// its plots is refused (the plots do not form one compact block).
pub const MERGE_HULL_SLACK: f32 = 1.06;

/// Billionaires need this many people in town...
pub const ELITE_MIN_POP: usize = 900;
/// ...and inequality at least this high (Gini of earners' incomes)...
pub const ELITE_MIN_GINI: f32 = 0.40;
/// ...and the top twentieth earning at least this many times the median.
pub const ELITE_MIN_TOP_RATIO: f32 = 3.5;
/// Chance per day one arrives (or a local makes it big) when all holds.
pub const ELITE_DAILY_CHANCE: f64 = 0.05;
/// People per billionaire the town can carry, and the hard cap.
pub const ELITE_PEOPLE_EACH: usize = 2500;
pub const ELITE_MAX: usize = 6;
/// Quiet land: pressure at most this much, or this share of the median
/// pressure on the town's built lots if that is more (a city of ten
/// thousand hums everywhere; quiet is relative), and nuisance and fouled
/// ground at most these.
pub const ELITE_QUIET_PRESSURE: f32 = 4.0;
pub const ELITE_QUIET_SHARE: f32 = 0.3;
pub const ELITE_MAX_NUISANCE: f32 = 0.03;
pub const ELITE_MAX_CONTAMINATION: f32 = 0.05;
/// Floors of other buildings within `ELITE_DENSITY_RADIUS` at most.
pub const ELITE_MAX_NEARBY_FLOORS: u32 = 150;
pub const ELITE_DENSITY_RADIUS: f32 = 150.0;
/// No industry or nightlife this close.
pub const ELITE_NUISANCE_CLEARANCE: f32 = 160.0;
/// The grounds a mansion wants: plots are added (up to `MAX_MERGED_LOTS`)
/// until the estate is at least this big, and a site under the minimum
/// is passed over.
pub const ELITE_MIN_AREA: f32 = 1400.0;
pub const ELITE_WANT_AREA: f32 = 2600.0;
/// Billionaires like company: a site this close to another mansion is
/// preferred.
pub const ELITE_ATTRACT_RADIUS: f32 = 650.0;

/// NIMBY: the land value lift round a mansion, its reach (capped), how
/// fast it fades per day, and how many cheap homes nearby are priced out
/// at most (two a day).
pub const NIMBY_RADIUS: f32 = 200.0;
pub const NIMBY_MAX_RADIUS: f32 = 260.0;
pub const NIMBY_LIFT: f32 = 0.22;
pub const NIMBY_DECAY: f32 = 0.96;
pub const NIMBY_PRICE_OUT_CAP: u8 = 6;
pub const NIMBY_PRICE_OUT_PER_DAY: u8 = 2;
/// Inside a NIMBY radius nothing is redeveloped taller than this.
pub const NIMBY_HEIGHT_CAP: u8 = 3;

/// Net worth (dollars) from which the richest billionaire gets a police
/// escort in front of the convoy.
pub const POLICE_ESCORT_WORTH: f64 = 20e9;

// ── forms ────────────────────────────────────────────────────────────────────

/// What a building on merged or bought plots was built as, for the
/// renderer and the inspector. Ordinary buildings have none.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Form {
    /// A family bought two plots and built one big house with a pool.
    McMansion,
    /// A billionaire's house with grounds and a garden.
    Mansion,
    /// A firm's plots merged under one building of its usual kind (an oil
    /// field across every plot, a bigger factory or datacentre).
    Merged,
    /// A hotel grown over its neighbours: valet parking, a roof pool and
    /// sky-bridges.
    MegaHotel,
    /// A gallery grown into a museum.
    Museum,
    /// [supertall] A league tower (skyscraper, supertall or megatall) on a
    /// site its developer assembled; `Plot::tower` has its height and form.
    Tower,
}

impl Form {
    pub fn label(self) -> &'static str {
        match self {
            Form::McMansion => "McMansion",
            Form::Mansion => "mansion",
            Form::Merged => "merged plots",
            Form::MegaHotel => "mega hotel",
            Form::Museum => "museum",
            Form::Tower => "tower",
        }
    }

    /// Lived in by the household that owns it: no rent.
    pub fn owner_occupied(self) -> bool {
        matches!(self, Form::McMansion | Form::Mansion)
    }
}

/// The form a business grows into when it buys its neighbours, by kind.
/// `None` for kinds that open branches instead, or do neither. Galleries
/// (added by the economy branch) are matched by label, so this keeps
/// working whichever lands first.
pub fn expansion_form(kind: BusinessKind) -> Option<Form> {
    use BusinessKind as K;
    match kind {
        K::Hotel => Some(Form::MegaHotel),
        K::OilWell | K::Refinery | K::Fab | K::Assembly | K::GrainFarm | K::VertFarm | K::Dairy | K::ProteinVat | K::Mill | K::ContainerYard => Some(Form::Merged),
        K::GadgetShop | K::Office | K::Venue | K::Grocery => Some(Form::Merged),
        K::Gallery => Some(Form::Museum),
        _ => None,
    }
}

/// Kinds that grow by opening branches across the city rather than by
/// buying the plots next door: taxi firms, pizza (bakeries' hot food) and
/// tow companies. Labels of the economy branch's kinds that work the same
/// way (mechanics, bike shops) are matched too.
pub fn branches(kind: BusinessKind) -> bool {
    use BusinessKind as K;
    matches!(kind, K::TaxiCompany | K::Bakery | K::TowYard) || matches!(kind.label(), "car repair shop" | "bike shop" | "trash company")
}

// ── per-building record ──────────────────────────────────────────────────────

/// A building's place in the land market: the extra plots it stands on,
/// the form it was built in and who owns it. Default for every building
/// from before (one plot, no form, independently owned).
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Plot {
    /// Plots beyond `Building::lot` the building spans. Each stays a lot
    /// for zoning and points at this building.
    #[serde(default)]
    pub extra_lots: Vec<LotId>,
    #[serde(default)]
    pub form: Option<Form>,
    /// `None`: an independent owner (the residents' landlord, the business
    /// itself).
    #[serde(default)]
    pub owner: Option<OwnerId>,
    /// Plots bought and waiting for the building on them to come down
    /// before this one is rebuilt across them.
    #[serde(default)]
    pub pending_lots: Vec<LotId>,
    /// The form to build in once the pending plots are clear.
    #[serde(default)]
    pub pending_form: Option<Form>,
    /// The household that bought it and moves in when it is done (McMansions
    /// and mansions).
    #[serde(default)]
    pub buyer: Option<HouseholdId>,
    /// [supertall] A league tower: its height, form and developer.
    #[serde(default)]
    pub tower: Option<crate::towers::Tower>,
}

impl Plot {
    pub fn merged(&self) -> bool {
        !self.extra_lots.is_empty()
    }

    /// Every plot including `primary`.
    pub fn lots(&self, primary: LotId) -> Vec<LotId> {
        std::iter::once(primary).chain(self.extra_lots.iter().copied()).collect()
    }
}

// ── owners ───────────────────────────────────────────────────────────────────

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum OwnerKind {
    /// A business with branches or merged plots.
    Firm { kind: BusinessKind },
    /// A developer buying out low buildings where the city presses for more.
    Developer,
    /// A household owning its home (a McMansion).
    Household { household: HouseholdId },
    /// A billionaire.
    Elite { citizen: CitizenId },
    /// A citizen who owns what they founded (a taxi firm's founder).
    Person { citizen: CitizenId },
    /// [megacorp] A mega-corporation grown out of a company of `kind`
    /// (`crate::corp::MegaCorp`, found by this owner's id).
    MegaCorp { kind: BusinessKind },
    /// [megacorp] A university and the buildings round its campus
    /// (`crate::corp::Campus`).
    University { campus: BuildingId },
    /// [ledger] A resident's property company: what they buy to let is the
    /// company's, the tenants pay it, it pays the founder a dividend
    /// (`World::settle_owners`).
    Company { founder: CitizenId },
}

impl OwnerKind {
    /// The citizen behind the owner, for a person or a billionaire.
    pub fn citizen(&self) -> Option<CitizenId> {
        match self {
            OwnerKind::Elite { citizen } | OwnerKind::Person { citizen } => Some(*citizen),
            _ => None,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Owner {
    pub name: String,
    pub kind: OwnerKind,
    /// Every property, oldest first.
    pub properties: Vec<BuildingId>,
    /// Paid for properties, cumulative.
    #[serde(default)]
    pub spent: f64,
    pub since_day: u32,
    /// [sweep-sim] What an investor's holdings paid them yesterday (rent
    /// from tenants, a share of a business's profit), and in all.
    #[serde(default)]
    pub income_day: f32,
    #[serde(default)]
    pub earned: f64,
    /// [ledger] The owner's own money (firms, developers, residents'
    /// property companies): the mirror of its ledger account. Mega-corps
    /// keep theirs in `MegaCorp::cash`, the university in `Campus::funds`,
    /// residents in their household's purse.
    #[serde(default)]
    pub money: f64,
    /// [ledger] Rent collected today and yesterday.
    #[serde(default)]
    pub rent_today: f32,
    #[serde(default)]
    pub rent_yesterday: f32,
}

// ── the super-elite ──────────────────────────────────────────────────────────

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum EliteOrigin {
    /// A local who made a fortune.
    Local,
    /// Visited and stayed.
    Visitor,
}

/// A billionaire: a citizen with a fortune, an owner record, a mansion
/// (once built) and a garage of supercars. Fictional people only: names
/// come from the town's own generator.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Elite {
    pub citizen: CitizenId,
    pub owner: OwnerId,
    /// Dollars.
    pub net_worth: f64,
    pub origin: EliteOrigin,
    pub mansion: Option<BuildingId>,
    pub cars: Vec<VehicleModel>,
    /// Black escort cars round the supercar (2..5 by net worth).
    pub escorts: u8,
    /// The very richest ride behind a police car.
    pub police: bool,
    pub since_day: u32,
}

impl Elite {
    /// Escort cars for a fortune: two from a billion, five from forty.
    pub fn escorts_for(net_worth: f64) -> u8 {
        let b = (net_worth / 1e9).max(1.0);
        (2.0 + (b.log10() / 40f64.log10()) * 3.0).round().clamp(2.0, 5.0) as u8
    }

    /// Bodyguards on foot: one per escort car.
    pub fn guards(&self) -> u8 {
        self.escorts
    }
}

/// The pull of a mansion on the land round it, fading day by day.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Nimby {
    pub center: Vec2,
    pub radius: f32,
    /// Land value lift at the centre, fading.
    pub strength: f32,
    pub mansion: BuildingId,
    /// Cheap homes still allowed to be priced out (counts down).
    pub price_out_left: u8,
}

/// A billionaire on the road: the supercar, the black cars behind it and
/// the police car in front.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Convoy {
    pub citizen: CitizenId,
    pub boss: CarId,
    pub escorts: Vec<CarId>,
    pub police: Option<CarId>,
    /// Where the boss got out, and until when the guards stand there.
    #[serde(default)]
    pub stop: Option<(Vec2, u64)>,
}

impl Convoy {
    pub fn cars(&self) -> impl Iterator<Item = CarId> + '_ {
        self.escorts.iter().copied().chain(self.police)
    }
}

// ── statistics ───────────────────────────────────────────────────────────────

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct EstateStats {
    pub buyouts_total: u32,
    pub merges_total: u32,
    pub branches_total: u32,
    pub mcmansions_total: u32,
    pub mansions_total: u32,
    pub elites_total: u32,
    pub priced_out_total: u32,
    pub convoys_total: u32,
    pub buyouts_today: u32,
    /// [relocate] Businesses that moved after a buyout, and those that
    /// found nowhere they could afford.
    pub relocated_total: u32,
    pub relocate_failed_total: u32,
    /// [rich] Second homes and businesses the well-off bought.
    pub investments_total: u32,
    /// [pt-growth] Rows of small plots developers bought together and
    /// rebuilt as one bigger building, and the plots that went into them.
    pub assemblies_total: u32,
    pub assembled_lots_total: u32,
    /// [pt-growth] Developer-owned buildings rebuilt taller again.
    pub redensified_total: u32,
    /// [pt-growth] Half-empty blocks their owners lost to a developer.
    pub foreclosures_total: u32,
}

// ── the pressure grid ────────────────────────────────────────────────────────

/// People present plus people moving through, per 64 m cell, as a running
/// average of hourly samples; and how well each cell moves people.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Pressure {
    pub half: f32,
    pub n: usize,
    pub value: Vec<f32>,
    /// Share of cars standing in traffic, averaged like `value`.
    pub congestion: Vec<f32>,
    /// How well the cell moves people, `0.5..1.5`: transit near, traffic
    /// flowing. Recomputed daily.
    pub mobility: Vec<f32>,
    pub samples: u32,
    /// [pt-growth] People out on foot per cell, smoothed over days
    /// (`FOOT_ALPHA_UP` / `FOOT_ALPHA_DOWN`). Empty in old saves until the
    /// next hourly sample.
    #[serde(default)]
    pub foot: Vec<f32>,
    /// Apply the caps however small the town (tests); otherwise they
    /// wait for `PRESSURE_MIN_POP` people.
    #[serde(default)]
    pub always: bool,
}

impl Pressure {
    pub fn new(half: f32) -> Self {
        let n = ((half * 2.0) / PRESSURE_CELL).ceil().max(1.0) as usize;
        Self { half, n, value: vec![0.0; n * n], congestion: vec![0.0; n * n], mobility: vec![1.0; n * n], samples: 0, foot: vec![0.0; n * n], always: false }
    }

    /// [sweep-sim] Grow the grid to `half` (an old save's smaller grid),
    /// cells kept in place when the growth is whole cells; new cells start
    /// cold. Returns whether it changed.
    pub fn grow_to(&mut self, half: f32) -> bool {
        let off = (half - self.half) / PRESSURE_CELL;
        if !self.ready() || off < 0.5 || (off - off.round()).abs() > 1e-3 {
            return false;
        }
        let off = off.round() as usize;
        let n = ((half * 2.0) / PRESSURE_CELL).ceil().max(1.0) as usize;
        let pad = crate::land_value::pad_square;
        self.value = pad(&self.value, self.n, n, off, 0.0);
        self.congestion = pad(&self.congestion, self.n, n, off, 0.0);
        self.mobility = pad(&self.mobility, self.n, n, off, 1.0);
        self.foot = pad(&self.foot, self.n, n, off, 0.0);
        self.half = half;
        self.n = n;
        true
    }

    pub fn ready(&self) -> bool {
        self.n > 0 && self.value.len() == self.n * self.n && self.mobility.len() == self.n * self.n && self.congestion.len() == self.n * self.n
    }

    pub fn index(&self, p: Vec2) -> Option<usize> {
        if !self.ready() || !p.is_finite() {
            return None;
        }
        let x = ((p.x + self.half) / PRESSURE_CELL).floor();
        let y = ((p.y + self.half) / PRESSURE_CELL).floor();
        if x < 0.0 || y < 0.0 || x >= self.n as f32 || y >= self.n as f32 {
            return None;
        }
        Some(y as usize * self.n + x as usize)
    }

    pub fn cell_center(&self, i: usize) -> Vec2 {
        let (x, y) = (i % self.n, i / self.n);
        Vec2::new(-self.half + (x as f32 + 0.5) * PRESSURE_CELL, -self.half + (y as f32 + 0.5) * PRESSURE_CELL)
    }

    /// Fold one hour's counts (people present, people moving, cars, cars
    /// standing) into the averages. The first sample sets them outright,
    /// so an old save is warm after one hour.
    pub fn sample(&mut self, present: &[f32], stalled_share: &[f32]) {
        if !self.ready() || present.len() != self.value.len() {
            return;
        }
        let first = self.samples == 0;
        for (v, s) in self.value.iter_mut().zip(present) {
            let a = if first { 1.0 } else if *s > *v { PRESSURE_ALPHA_UP } else { PRESSURE_ALPHA };
            *v += (s - *v) * a;
        }
        let a = if first { 1.0 } else { PRESSURE_ALPHA };
        for (v, s) in self.congestion.iter_mut().zip(stalled_share) {
            *v += (s - *v) * a;
        }
        self.samples = self.samples.saturating_add(1);
    }

    /// [pt-growth] Fold one hour's people out on foot into `foot`. An old
    /// save's empty grid starts from this sample.
    pub fn sample_foot(&mut self, on_foot: &[f32]) {
        if !self.ready() || on_foot.len() != self.value.len() {
            return;
        }
        if self.foot.len() != on_foot.len() {
            self.foot = on_foot.to_vec();
            return;
        }
        for (v, s) in self.foot.iter_mut().zip(on_foot) {
            let a = if *s > *v { FOOT_ALPHA_UP } else { FOOT_ALPHA_DOWN };
            *v += (s - *v) * a;
        }
    }

    /// A cell blended with its eight neighbours (half and half), so a
    /// block and the street beside it agree.
    fn blended(&self, grid: &[f32], p: Vec2) -> f32 {
        let Some(i) = self.index(p) else { return 0.0 };
        if grid.len() != self.n * self.n {
            return 0.0;
        }
        let (x, y) = ((i % self.n) as i64, (i / self.n) as i64);
        let mut sum = 0.0;
        let mut k = 0.0;
        for dy in -1..=1i64 {
            for dx in -1..=1i64 {
                if dx == 0 && dy == 0 {
                    continue;
                }
                let (nx, ny) = (x + dx, y + dy);
                if nx < 0 || ny < 0 || nx >= self.n as i64 || ny >= self.n as i64 {
                    continue;
                }
                sum += grid[ny as usize * self.n + nx as usize];
                k += 1.0;
            }
        }
        let ring = if k > 0.0 { sum / k } else { 0.0 };
        grid[i] * 0.5 + ring * 0.5
    }

    /// [pt-growth] People out on foot at `p`, smoothed over days.
    pub fn foot_at(&self, p: Vec2) -> f32 {
        self.blended(&self.foot, p)
    }

    /// [pt-growth] The land density score at `p`: the people present
    /// (`at`) plus the street life (`FOOT_WEIGHT` x `foot_at`). Developers
    /// build to it (`floors_at`); the Land density map layer shows it.
    pub fn density_at(&self, p: Vec2) -> f32 {
        self.at(p) + FOOT_WEIGHT * self.foot_at(p)
    }

    /// The density score of cell `i` (unblended), for the map layer.
    pub fn density_cell(&self, i: usize) -> f32 {
        self.value.get(i).copied().unwrap_or(0.0) + FOOT_WEIGHT * self.foot.get(i).copied().unwrap_or(0.0)
    }

    /// Smoothed pressure at `p`: the cell blended with its eight
    /// neighbours, so a block and the street beside it agree.
    pub fn at(&self, p: Vec2) -> f32 {
        self.blended(&self.value, p)
    }

    pub fn mobility_at(&self, p: Vec2) -> f32 {
        self.index(p).map_or(1.0, |i| self.mobility[i])
    }

    /// Floors the pressure at `p` justifies: two in a quiet place, one
    /// more per `PRESSURE_PER_FLOOR` over `PRESSURE_FLOOR0`, scaled by how
    /// well the city moves people there.
    pub fn floors_at(&self, p: Vec2) -> u8 {
        if self.samples == 0 {
            // Not measured yet (an old save, the first hour): no opinion.
            return PRESSURE_MAX_FLOORS;
        }
        // [pt-growth] The land density: people present and street life.
        let over = (self.density_at(p) - PRESSURE_FLOOR0).max(0.0);
        let floors = 2.0 + over / PRESSURE_PER_FLOOR * self.mobility_at(p);
        floors.clamp(2.0, PRESSURE_MAX_FLOORS as f32) as u8
    }
}

// ── the market ───────────────────────────────────────────────────────────────

/// Everything the land market keeps (`World::estate`).
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct EstateMarket {
    pub pressure: Pressure,
    pub owners: SlotMap<OwnerId, Owner>,
    pub elites: Vec<Elite>,
    pub nimby: Vec<Nimby>,
    pub convoys: Vec<Convoy>,
    /// Plots bought and waiting to be cleared, by the building that will
    /// span them.
    pub reserved: BTreeMap<LotId, BuildingId>,
    pub stats: EstateStats,
    /// [relocate] Businesses that lost their building to a buyout and are
    /// looking for a new place (`World::relocate_waiting`).
    pub moving: Vec<Moving>,
}

/// [relocate] A business bought out of its building (road works, a city
/// building, a developer's rebuild, a neighbour's expansion), packed up
/// with its cash, books and owner, looking for a vacant lot of its land
/// use nearby. Kept a couple of days; if nothing turns up it is gone.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Moving {
    pub business: crate::market::Business,
    /// The building's name, kept for the new one.
    pub name: String,
    pub land_use: crate::zoning::LandUse,
    /// Where it stood and how much ground it had.
    pub from: Vec2,
    pub area: f32,
    pub jobs: u16,
    /// Its owner record (not a developer's), to hand the new building to.
    pub owner: Option<OwnerId>,
    /// Its staff, hired back when the new place opens.
    pub staff: Vec<CitizenId>,
    /// "road works", "a developer's rebuild", ...
    pub why: String,
    /// Where it stood, in words, taken when it packed up ("next to Lucky
    /// Bodega"), while the old neighbours still stood.
    #[serde(default)]
    pub from_words: String,
    pub since_tick: u64,
}

impl EstateMarket {
    pub fn elite_of(&self, c: CitizenId) -> Option<&Elite> {
        self.elites.iter().find(|e| e.citizen == c)
    }

    pub fn is_elite(&self, c: CitizenId) -> bool {
        self.elite_of(c).is_some()
    }

    pub fn in_nimby(&self, p: Vec2) -> bool {
        self.nimby.iter().any(|n| p.distance(n.center) < n.radius)
    }
}

/// The social class of a citizen: the three income classes, plus the
/// super-elite on top. Income-class counts and the Gini are untouched: a
/// billionaire is counted as High there and adds their income to the Gini.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum Class {
    Low,
    Middle,
    High,
    Elite,
}

impl Class {
    pub fn label(self) -> &'static str {
        match self {
            Class::Low => "low income",
            Class::Middle => "middle class",
            Class::High => "high income",
            Class::Elite => "super-elite",
        }
    }
}

// ── geometry ─────────────────────────────────────────────────────────────────

/// Do two plots share an edge (not just a corner)? At least 3 m of
/// common outline (`geom::shared_boundary`).
pub fn lots_touch(a: &Lot, b: &Lot) -> bool {
    a.polygon.len() >= 3 && b.polygon.len() >= 3 && crate::geom::shared_boundary(&a.polygon, &b.polygon) >= 3.0
}

/// Convex hull, counter-clockwise (Andrew's monotone chain).
pub use crate::geom::convex_hull;

/// One plot standing for several: the convex hull of their outlines,
/// framed along the primary plot's frontage and entered by its front edge.
/// `None` when the plots do not make one compact block (the hull is more
/// than `MERGE_HULL_SLACK` times their area).
pub fn merged_lot(primary: &Lot, others: &[&Lot]) -> Option<Lot> {
    merged_lot_within(primary, others, MERGE_HULL_SLACK)
}

/// `merged_lot` with the compactness test at `slack` (a tower's site may
/// be a little less compact: `towers::TOWER_HULL_SLACK`).
pub fn merged_lot_within(primary: &Lot, others: &[&Lot], slack: f32) -> Option<Lot> {
    if others.is_empty() {
        return Some(primary.clone());
    }
    let all: Vec<Vec2> = primary.polygon.iter().chain(others.iter().flat_map(|l| l.polygon.iter())).copied().collect();
    let hull = convex_hull(&all);
    if hull.len() < 3 {
        return None;
    }
    let area: f32 = primary.area() + others.iter().map(|l| l.area()).sum::<f32>();
    let hull_area = polygon_area(&hull).abs();
    if hull_area > area * slack {
        return None;
    }
    let along = (primary.corners[1] - primary.corners[0]).normalize_or_zero();
    if along == Vec2::ZERO {
        return None;
    }
    let inward = Vec2::new(-along.y, along.x);
    let o = primary.corners[0];
    let (mut u0, mut u1, mut v0, mut v1) = (f32::MAX, f32::MIN, f32::MAX, f32::MIN);
    for p in &hull {
        let d = *p - o;
        let (u, v) = (d.dot(along), d.dot(inward));
        u0 = u0.min(u);
        u1 = u1.max(u);
        v0 = v0.min(v);
        v1 = v1.max(v);
    }
    let at = |u: f32, v: f32| o + along * u + inward * v;
    let corners = [at(u0, v0), at(u1, v0), at(u1, v1), at(u0, v1)];
    let center = crate::geom::polygon_centroid(&hull);
    let mut lot = primary.clone();
    lot.polygon = hull;
    lot.corners = corners;
    lot.center = center;
    lot.landlocked = false;
    Some(lot)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn escorts_scale_with_net_worth() {
        assert_eq!(Elite::escorts_for(1.0e9), 2);
        assert!(Elite::escorts_for(8.0e9) >= 3);
        assert_eq!(Elite::escorts_for(40.0e9), 5);
        assert_eq!(Elite::escorts_for(400.0e9), 5);
    }

    #[test]
    fn hull_of_two_squares_is_their_rectangle() {
        let pts = [Vec2::new(0.0, 0.0), Vec2::new(10.0, 0.0), Vec2::new(10.0, 10.0), Vec2::new(0.0, 10.0), Vec2::new(20.0, 0.0), Vec2::new(20.0, 10.0), Vec2::new(5.0, 5.0)];
        let h = convex_hull(&pts);
        assert_eq!(h.len(), 4, "{h:?}");
        assert!((polygon_area(&h) - 200.0).abs() < 1e-3, "counter-clockwise, 200 m²");
    }

    #[test]
    fn a_cold_grid_has_no_opinion_and_warms_in_one_sample() {
        let mut p = Pressure::new(300.0);
        let busy = Vec2::new(10.0, 10.0);
        assert_eq!(p.floors_at(busy), PRESSURE_MAX_FLOORS);
        let mut present = vec![0.0; p.n * p.n];
        let i = p.index(busy).unwrap();
        present[i] = 60.0;
        let zeros = vec![0.0; p.n * p.n];
        p.sample(&present, &zeros);
        assert!(p.floors_at(busy) > 6, "busy cell justifies height: {}", p.floors_at(busy));
        assert_eq!(p.floors_at(Vec2::new(-250.0, -250.0)), 2, "a quiet corner stays low");
    }
}
