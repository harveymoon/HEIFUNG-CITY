//! Small procedural name generators. Kept in the sim crate so saves can carry
//! names without depending on the heavier procgen crate.

use crate::zoning::LandUse;
use rand::seq::IndexedRandom;
use rand::Rng;

const FIRST: &[&str] = &[
    "Kai", "Mira", "Jax", "Yuki", "Dex", "Nova", "Rin", "Sable", "Ezra", "Lux", "Tomo", "Vesper",
    "Ilya", "Nyx", "Orin", "Zara", "Kade", "Suki", "Ravi", "Juno", "Theo", "Ash", "Lior", "Wren",
    "Idris", "Mei", "Cass", "Bram", "Ines", "Rook",
];

const LAST: &[&str] = &[
    "Okafor", "Volkov", "Tanaka", "Reyes", "Lindqvist", "Nakamura", "Diallo", "Castellano",
    "Marsh", "Ivanova", "Osei", "Sato", "Kowalski", "Mbeki", "Haddad", "Nguyen", "Fontaine",
    "Petrova", "Achebe", "Moreau", "Varga", "Quon", "Delacroix", "Baptiste", "Ferreira",
];

const HANDLE: &[&str] = &[
    "Static", "Ghost", "Pixel", "Null", "Byte", "Glitch", "Echo", "Zero", "Fuse", "Cipher",
    "Patch", "Drift", "Vex", "Halo", "Signal",
];

/// A family name shared by a household.
pub fn family_name(rng: &mut impl Rng) -> String {
    LAST.choose(rng).unwrap().to_string()
}

/// A member of the `family` household: their own first name (and maybe a
/// street handle), the family's last name.
pub fn member_name(rng: &mut impl Rng, family: &str) -> String {
    let first = FIRST.choose(rng).unwrap();
    if rng.random_bool(0.2) {
        let handle = HANDLE.choose(rng).unwrap();
        format!("{first} \"{handle}\" {family}")
    } else {
        format!("{first} {family}")
    }
}

pub fn citizen_name(rng: &mut impl Rng) -> String {
    let first = FIRST.choose(rng).unwrap();
    let last = LAST.choose(rng).unwrap();
    if rng.random_bool(0.2) {
        let handle = HANDLE.choose(rng).unwrap();
        format!("{first} \"{handle}\" {last}")
    } else {
        format!("{first} {last}")
    }
}

const CORPS: &[&str] = &[
    "Aramund", "Vellane", "Koridan", "Halcyon", "Namara", "Oberon", "Vantage", "Zenith",
    "Helix", "Meridian", "Kessler", "Nocturne", "Argent", "Sōma", "Fulcrum",
];

const RES_PREFIX: &[&str] = &[
    "Kowloon", "Lotus", "Harbor", "Ember", "Cinder", "Jade", "Onyx", "Mercury", "Aurora",
    "Tide", "Marrow", "Umbra", "Saffron", "Cobalt",
];
const RES_SUFFIX: &[&str] = &["Stacks", "Towers", "Habitat", "Block", "Heights", "Hive", "Terrace"];

const SHOP: &[&str] = &[
    "Noodle Bar", "Pharma", "Chrome Shop", "Mart", "Ramen", "Repairs", "Tattoo", "Bodega",
    "Optics", "Synth Grill", "Dumplings", "Laundromat", "Print Shop", "Vapor",
];
const SHOP_PREFIX: &[&str] = &[
    "Lucky", "Golden", "Neon", "Night", "Blue", "Red", "Mama Chen's", "Uncle Bo's", "Hex",
    "Big Sky", "Rust", "Velvet",
];

const CLUB: &[&str] = &["Club", "Bar", "Lounge", "Arcade", "Karaoke", "Den", "Cabaret"];
const CLUB_NAME: &[&str] = &[
    "Afterglow", "Voltage", "Lotus", "Mirage", "Overdrive", "Blackout", "Siren", "Halo",
    "Static", "Velour", "Chrome Moon", "Null Pointer", "Dopamine", "Tokyo Drift",
];

const CIVIC: &[&str] = &[
    "Precinct", "Clinic", "Ministry of Records", "Transit Authority", "Data Exchange",
    "Public Works", "Court", "Water Board", "Fire Station", "Library",
];

const PARK: &[&str] = &["Commons", "Garden", "Plaza", "Greenway", "Yard", "Memorial"];

pub fn building_name(rng: &mut impl Rng, land_use: LandUse) -> String {
    match land_use {
        LandUse::Residential => {
            if rng.random_bool(0.3) {
                format!("Habitat {}-{}", rng.random_range(1..40), (b'A' + rng.random_range(0..6)) as char)
            } else {
                format!("{} {}", RES_PREFIX.choose(rng).unwrap(), RES_SUFFIX.choose(rng).unwrap())
            }
        }
        LandUse::Commercial => format!("{} {}", SHOP_PREFIX.choose(rng).unwrap(), SHOP.choose(rng).unwrap()),
        LandUse::Industrial => format!("{} Fab {}", CORPS.choose(rng).unwrap(), rng.random_range(1..20)),
        LandUse::Entertainment => {
            if rng.random_bool(0.5) {
                format!("{} {}", CLUB.choose(rng).unwrap(), CLUB_NAME.choose(rng).unwrap())
            } else {
                format!("{} {}", CLUB_NAME.choose(rng).unwrap(), CLUB.choose(rng).unwrap())
            }
        }
        LandUse::Civic => {
            let c = CIVIC.choose(rng).unwrap();
            if c.contains(' ') {
                c.to_string()
            } else {
                format!("{c} {}", rng.random_range(1..30))
            }
        }
        LandUse::Park => format!("{} {}", RES_PREFIX.choose(rng).unwrap(), PARK.choose(rng).unwrap()),
    }
}

const CITY_FIRST: &[&str] = &[
    "Neon", "Chrome", "Vanta", "Halcyon", "Cobalt", "Ember", "Static", "Saffron", "Onyx", "Lumen",
    "Iron", "Velvet", "Cinder", "Argent", "Mercury", "Juniper", "Obsidian", "Amber", "Silica", "Port",
];

const CITY_LAST: &[&str] = &[
    "Bay", "Heights", "Reach", "Harbor", "Junction", "Hollow", "Crossing", "Point", "Falls", "Gate",
    "Basin", "Ridge", "Sprawl", "Quarter", "Haven", "Vale", "Springs", "Landing", "Strand", "Row",
];

/// [identity] A name to offer for a new city ("Cobalt Reach"); the player
/// may keep it or type their own.
pub fn city_name(rng: &mut impl Rng) -> String {
    format!("{} {}", CITY_FIRST.choose(rng).unwrap(), CITY_LAST.choose(rng).unwrap())
}

// -- [pt-visual] Names that say what the business is (F38: a tow company
// was "Big Sky Laundromat"). Picked by a hash of the building's seed, so
// naming draws nothing from the world's random stream.

use crate::market::BusinessKind;

/// `(prefixes, trades)`: a name is `"{prefix} {trade}"`, or a whole name
/// from the trade list when it starts with `=`.
fn trade_words(kind: BusinessKind) -> Option<(&'static [&'static str], &'static [&'static str])> {
    use BusinessKind as B;
    const STREET: &[&str] = &["Lucky", "Golden", "Neon", "Night", "Blue", "Red", "Mama Chen's", "Uncle Bo's", "Hex", "Big Sky", "Rust", "Velvet"];
    Some(match kind {
        B::Grocery => (STREET, &["Mart", "Bodega", "Grocer", "Corner Store", "Market", "24/7", "Provisions", "=Kwik-E-Fresh", "=Spam & Eggs"]),
        B::Bakery => (STREET, &["Bakery", "Bakehouse", "Buns", "=Yeast of Eden", "=Knead to Know", "=Rise & Grind", "=The Upper Crust", "=Bread Pitt", "=Loafing Around"]),
        B::GadgetShop => (STREET, &["Chrome Shop", "Optics", "Repairs", "Gadgets", "Electronics", "=Jailbreak Junction", "=Byte Me", "=Ctrl+Alt+Deal", "=Second-Hand Cyberware", "=The Dongle Hut"]),
        B::Office => (
            &["Aramund", "Vellane", "Koridan", "Halcyon", "Namara", "Oberon", "Vantage", "Zenith", "Helix", "Meridian", "Kessler", "Nocturne", "Argent", "Fulcrum"],
            &["Holdings", "Partners", "Systems", "Group", "Capital", "Logistics", "Analytics", "Consulting", "Data", "Synergies"],
        ),
        B::TowYard => (
            &["Big Sky", "Rust", "Hex", "Uncle Bo's", "Night Owl", "Lucky", "Iron"],
            &["Towing", "Tow & Haul", "Recovery", "Wreckers", "=Hook, Line & Sinker Towing", "=Tow Mater's", "=Yank & Bank Towing", "=The Tow Must Go On", "=Sorry We Towed You", "=Hooked on Towing", "=Tow-tally Legal Recovery"],
        ),
        B::Mechanic => (STREET, &["Garage", "Auto Repair", "Motors", "Body Shop", "=Grease Lightning", "=Wrench & Pray", "=Check Engine Light Ltd", "=Pistons & Prayers"]),
        B::CarDealer => (&["Velvet", "Golden", "Chrome", "Zenith", "Big Sky", "Honest Ray's"], &["Motors", "Auto Sales", "Car Lot", "Autos", "=Lemons & Limes Pre-Owned", "=No Refunds Motors", "=Mileage May Vary"]),
        B::TaxiCompany => (&["Lucky", "Yellow", "Neon", "Night", "Checker", "Blue"], &["Cabs", "Taxi", "Cab Co.", "Rides", "=Meter Running", "=Fare Game Taxis"]),
        B::TrashCompany => (&["Rust", "Iron", "Big Sky", "Green", "Night"], &["Waste", "Disposal", "Haulage", "=Bin There Done That", "=Trash Bandits", "=Refuse to Lose", "=Junk in the Trunk Hauling"]),
        B::BikeShop => (STREET, &["Cycles", "Bikes", "Spokes", "=Spoke Signals", "=Chain Reaction", "=Pedal Pushers"]),
        B::Hotel => (&["Golden", "Neon", "Velvet", "Grand", "Pacific", "Capsule", "Lotus"], &["Hotel", "Inn", "Suites", "Lodge", "=The Snooze Pod", "=Hotel Insomnia", "=The Bedbug Arms"]),
        B::FishMarket => (&["Harbour", "Tide", "Salt", "Lucky", "Old Wharf"], &["Fish Market", "Fishmongers", "Seafood", "=Oh My Cod", "=The Codfather", "=Something Fishy"]),
        B::SeafoodRestaurant => (&["Harbour", "Lobster", "Pearl", "Salt", "Tide", "Coral"], &["Grill", "Oyster Bar", "Kitchen", "=The Codfather", "=Hook & Ladle", "=Squid Pro Quo", "=Fishful Thinking"]),
        B::GolfRange => (&["Green", "Fairway", "Neon", "Night"], &["Driving Range", "Golf Range", "Tees", "=Grip It & Rip It", "=Fore Play Golf", "=Tee Time Tower"]),
        B::Gallery => (&["Blank", "White Cube", "Velvet", "Static", "Chrome"], &["Gallery", "Contemporary", "Art Space", "=Glitch Gallery", "=The Pretentious Wall"]),
        B::ArtStudio => (&["Rust", "Loft", "Warehouse", "Static"], &["Studios", "Collective", "Atelier", "=Starving Artists Co-op", "=Paint by Numbers"]),
        B::Cafe => (&["Blue", "Velvet", "Static", "Neon", "Lotus", "Night Owl"], &["Café", "Coffee", "Espresso Bar", "=Bean There", "=Brew-tal Coffee", "=Grounds for Divorce", "=Deja Brew", "=The Daily Grind"]),
        B::Bookshop => (&["Old", "Velvet", "Ink", "Night", "Paper"], &["Books", "Bookshop", "Booksellers", "=Shelf Indulgence", "=Paperback Writer", "=Read Between the Wines"]),
        B::ScrapYard => (&["Rust", "Iron", "Torch", "Eastside", "Breakwater"], &["Scrap", "Salvage", "Metals", "Breakers", "=Cut & Run Salvage", "=Heavy Metal Recycling", "=Scrap Happy"]),
        B::Fab => (&["Aramund", "Koridan", "Helix", "Kessler", "Fulcrum", "Argent"], &["Fab", "Parts", "Precision", "Machining"]),
        B::Assembly => (&["Aramund", "Koridan", "Helix", "Kessler", "Fulcrum", "Zenith"], &["Assembly", "Electronics", "Circuits", "=Some Assembly Required"]),
        B::VertFarm => (&["Green", "Leaf", "Stack", "Jade", "Sky"], &["Greens", "Vertical Farms", "Grow Tower", "=Lettuce Turnip the Beet", "=Kale Storm"]),
        B::GrainFarm => (&["Golden", "Dust", "Prairie", "Old"], &["Grain", "Fields", "Farms", "=Amber Waves"]),
        B::ProteinVat => (&["Soylent", "Vat", "Helix", "Meridian"], &["Proteins", "Biomass", "Cultures", "=Totally Meat Inc.", "=Mystery Meat Labs"]),
        B::Dairy => (&["Cloud", "Moon", "Pale", "Golden"], &["Dairy", "Creamery", "=Udderly Synthetic"]),
        B::Mill => (&["Old", "Stone", "Iron", "Golden"], &["Mill", "Flour Mill", "Milling"]),
        B::OilWell => (&["Black Gold", "Crude", "Argent", "Kessler"], &["Oil", "Petroleum", "Drilling", "=Drill Baby Drill"]),
        B::Refinery => (&["Argent", "Kessler", "Oberon", "Black Gold"], &["Refining", "Petrochem", "Refinery"]),
        B::ContainerYard => (&["Meridian", "Harbour", "Vantage", "Iron"], &["Containers", "Freight", "Box Yard", "=Thinking Inside the Box"]),
        B::FreightForwarder => (&["Tallyman", "Brackwater", "Northbound", "Quayside", "Ninefold", "Gantry"], &["Forwarding", "Consolidators", "Freight Agents", "Shipping & Forwarding", "=Box & Bundle Forwarding", "=Many Hands Consolidated"]),
        B::FleetLot => (&["Meridian", "Rust", "Iron", "Night"], &["Fleet Lot", "Truck Park", "Depot", "=Idle Hands Truck Park"]),
        B::Fishery | B::FishingPort => (&["Saltwind", "Tide", "Grey", "Old Wharf"], &["Fisheries", "Fishing Co.", "Trawlers"]),
        B::FishPlant | B::FishGutting | B::FishStickFactory => (&["Saltwind", "Tide", "Grey", "Captain"], &["Fish Processing", "Seafoods", "Fish Works", "=Gut Feeling Seafoods", "=Fin-ished Products"]),
        B::CowFarm => (&["Big Sky", "Dust", "Prairie", "Red"], &["Ranch", "Cattle Co.", "=Moo-ving Pictures Ranch"]),
        B::ChickenFarm => (&["Sunny", "Red", "Golden"], &["Poultry", "Hens", "=Fowl Play Farms", "=Cluck Norris Poultry"]),
        B::Slaughterhouse => (&["Iron", "Red", "Kessler"], &["Meats", "Packing", "Abattoir", "=Grim Reaper Meats"]),
        B::StripMine => (&["Iron", "Rust", "Kessler", "Argent"], &["Mining", "Quarry", "Minerals", "=Hole Lotta Love Mining"]),
        B::Smelter => (&["Iron", "Rust", "Kessler", "Argent", "Cinder"], &["Smelting", "Foundry", "Metals", "=Heavy Metal Works"]),
        B::Feedlot => (&["Kessler", "Big Sky", "Meridian"], &["Feedlots", "Livestock", "Agri-Corp"]),
        B::ChemicalWorks => (&["Kessler", "Oberon", "Argent", "Toxic"], &["Chemicals", "Chemworks", "Industrial Solvents", "=Better Living Through Chemistry"]),
        B::CargoHub => (&["Meridian", "Vantage", "Harbour"], &["Cargo Hub", "Freight Terminal"]),
        // [r4-leisure-places] Places to eat and go out: made-up names only.
        B::Diner => (&["Starlite", "Route 9", "Chrome", "Mama Chen's", "Uncle Bo's", "Night Owl", "Sunrise", "Rust Belt"], &["Diner", "Grill", "Luncheonette", "=The Greasy Spoon", "=Over Easy", "=Bottomless Cup", "=The Chrome Counter"]),
        B::NoodleBar => (&["Lucky", "Golden", "Red Lantern", "Steam", "Jade", "Mama Chen's", "Midnight"], &["Noodle Bar", "Ramen", "Noodle House", "=Slurp Station", "=Broth & Order", "=Oodles", "=Hand-Pulled Hong's"]),
        B::PizzaPlace => (&["Big Sky", "Brick Oven", "Uncle Bo's", "Night", "Velvet", "Corner"], &["Pizza", "Pizzeria", "Slice", "=Pie Hard", "=Crust Fund", "=Slice Slice Baby", "=Deep Dish Dreams"]),
        B::FastFood => (&["Rocket", "Zap", "Big Sky", "Chrome", "Nite-Lite", "Turbo"], &["Burger", "Drive-Thru", "Fry Shack", "=Burger Barn 24/7", "=Grease Monkey Burgers", "=Fries Before Guys", "=Clucky's Drive-Thru"]),
        B::FoodHall => (&["Central", "Junction", "Old Depot", "Iron", "Market Street", "Signal"], &["Food Hall", "Eats", "Kitchens", "=Hall of Plenty", "=Fork in the Road", "=Twelve Kitchens"]),
        B::StreetFood => (&["Night", "Neon", "Ember", "Lantern", "Alley", "Smoke"], &["Stalls", "Street Eats", "Food Trucks", "Hawkers", "=Wok This Way", "=Skewer Row", "=Smoke & Wheels"]),
        B::FineDining => (&["Maison", "Atelier", "Obsidian", "Velvet", "Silica", "Ember"], &["Dining Room", "Table", "Brasserie", "=Nine Courses", "=Salt & Silver", "=The Long Table", "=Umami Noir"]),
        B::BubbleTea => (&["Boba", "Pearl", "Cloud", "Neon", "Lychee", "Static"], &["Tea Bar", "Juice Bar", "Bubble Tea", "=Tapioca Express", "=Pulp Friction", "=Bubble Trouble", "=Juiced Up"]),
        B::Bar => (&["Rusty", "Velvet", "Night Owl", "Last Call", "Copper", "Iron", "Blue"], &["Bar", "Tavern", "Taproom", "Pub", "=The Broken Neon", "=Last Orders", "=The Drowned Rat", "=Hops & Hazards"]),
        B::KaraokeBox => (&["Starlight", "Neon", "Echo", "Pop", "Golden", "Lucky"], &["Karaoke", "Karaoke Box", "Sing Rooms", "=Mic Drop", "=Off Key", "=Encore Box", "=High Notes"]),
        B::Cinema => (&["Odeon Row", "Starlite", "Neon", "Palace", "Reel", "Lumen"], &["Cinema", "Picturehouse", "Screens", "Multiplex", "=Reel Big", "=The Flickers", "=Silver Screen 6"]),
        B::Theatre => (&["Royal Argent", "Old Lyric", "Velvet", "Crown", "Cobalt", "Juniper"], &["Theatre", "Playhouse", "Stage", "=The Fourth Wall", "=Curtain Call", "=Limelight House"]),
        B::ConcertHall => (&["Ember", "Static", "Iron", "Cobalt", "Amp", "Feedback"], &["Hall", "Live", "Ballroom", "Music Hall", "=The Loud Room", "=Amp & Anvil", "=Feedback Loop"]),
        B::BowlingAlley => (&["Lucky", "Strike", "Big Sky", "Chrome", "Starlite", "Rust"], &["Lanes", "Bowl", "Bowling", "=Pin Pals", "=Split Decision", "=Gutter Glory", "=Ten Pin Alley"]),
        B::Arcade => (&["Pixel", "Neon", "Glitch", "Hex", "Turbo", "Byte"], &["Arcade", "VR Arcade", "Game Hall", "=Insert Coin", "=Extra Life", "=High Score Hall", "=Respawn Point"]),
        B::Gym => (&["Iron", "Steel", "Pulse", "Grind", "Titan", "Chrome"], &["Gym", "Fitness", "Strength", "Athletic Club", "=No Pain Gym", "=Sweat Box", "=The Iron Temple", "=Rep Factory"]),
        B::ComedyClub => (&["Laugh", "Velvet", "Brick Wall", "Heckle", "Punchline", "Basement"], &["Comedy Club", "Comedy Cellar", "Stand-Up", "=The Laugh Track", "=Heckle & Jive", "=Open Mic Night", "=The Groan Zone"]),
        _ => return None,
    })
}

/// A name for a `kind` business, chosen by `seed`; `None` for kinds named
/// elsewhere (venues, parking, marinas, the shore's trades, city services).
pub fn business_name(kind: BusinessKind, seed: u64) -> Option<String> {
    let (pre, trades) = trade_words(kind)?;
    let h = crate::hash::mix2_salted(seed, kind as u64, 0x4E41_4D45);
    let t = trades[(h % trades.len() as u64) as usize];
    if let Some(whole) = t.strip_prefix('=') {
        return Some(whole.to_string());
    }
    let p = pre[((h >> 20) % pre.len() as u64) as usize];
    Some(format!("{p} {t}"))
}

/// True when `name` came from the generic land-use generator
/// (`building_name`: "Lucky Laundromat", "Aramund Fab 3") and says nothing
/// that fits a `kind` business, so the business may rename it. Names a
/// player gave, a firm's branch name or a fitting trade are kept.
pub fn name_misfits(name: &str, kind: BusinessKind) -> bool {
    let shop = SHOP_PREFIX.iter().find_map(|p| name.strip_prefix(p).and_then(|r| r.strip_prefix(' '))).filter(|t| SHOP.contains(t));
    let fab = CORPS.iter().any(|c| name.strip_prefix(c).and_then(|r| r.strip_prefix(" Fab ")).is_some_and(|n| n.parse::<u32>().is_ok()));
    if shop.is_none() && !fab {
        return false;
    }
    let Some((_, trades)) = trade_words(kind) else { return false };
    if fab {
        return !matches!(kind, BusinessKind::Fab | BusinessKind::Assembly);
    }
    let t = shop.unwrap_or_default();
    // The generic trades that do fit a kind keep their name.
    let fits = match kind {
        BusinessKind::Grocery => matches!(t, "Mart" | "Bodega"),
        BusinessKind::GadgetShop => matches!(t, "Chrome Shop" | "Optics" | "Repairs"),
        _ => trades.contains(&t),
    };
    !fits
}

/// [r4-leisure-places] Whether `name` says it is a `kind` business: one of
/// its trade words ends it, or it is one of the kind's whole names.
pub fn name_says_trade(name: &str, kind: BusinessKind) -> bool {
    let Some((_, trades)) = trade_words(kind) else { return true };
    trades.iter().any(|t| match t.strip_prefix('=') {
        Some(whole) => name == whole,
        None => name.ends_with(t),
    })
}

/// The indefinite article for `word` by its sound: "an" before a vowel
/// sound ("an observatory", "an hour", "an 8-floor block", "an EV"), "a"
/// otherwise ("a university", "a one-way street", "a 12-floor block").
pub fn article(word: &str) -> &'static str {
    let w = word.trim_start();
    let lower = w.to_lowercase();
    let first = lower.chars().next();
    // Numbers: eight, eleven, eighteen, eighty..., eleven thousand...
    if first.is_some_and(|c| c.is_ascii_digit()) {
        let digits: String = lower.chars().take_while(|c| c.is_ascii_digit() || *c == ',').filter(|c| *c != ',').collect();
        let an = digits.starts_with('8') || ((digits.starts_with("11") || digits.starts_with("18")) && digits.len() % 3 == 2);
        return if an { "an" } else { "a" };
    }
    // Initialisms said letter by letter: "an EV", "an HQ", "a TV".
    let token: String = w.chars().take_while(|c| c.is_alphanumeric()).collect();
    if token.len() >= 2 && token.chars().all(|c| c.is_ascii_uppercase()) {
        return if token.starts_with(['A', 'E', 'F', 'H', 'I', 'L', 'M', 'N', 'O', 'R', 'S', 'X']) { "an" } else { "a" };
    }
    // Silent h, and vowels said as a consonant.
    const AN: [&str; 4] = ["hour", "honest", "honour", "heir"];
    const A: [&str; 9] = ["uni", "use", "usu", "uti", "ure", "eu", "one", "once", "ewe"];
    if AN.iter().any(|p| lower.starts_with(p)) {
        return "an";
    }
    if A.iter().any(|p| lower.starts_with(p)) {
        return "a";
    }
    if first.is_some_and(|c| "aeiou".contains(c)) { "an" } else { "a" }
}

/// `word` with its indefinite article: "an observatory", "a park".
pub fn a(word: impl AsRef<str>) -> String {
    let word = word.as_ref();
    format!("{} {word}", article(word))
}

#[cfg(test)]
mod business_name_tests {
    use super::*;

    #[test]
    fn tow_company_is_not_a_laundromat() {
        assert!(name_misfits("Big Sky Laundromat", BusinessKind::TowYard));
        assert!(!name_misfits("Lucky Mart", BusinessKind::Grocery));
        assert!(name_misfits("Lucky Ramen", BusinessKind::Grocery));
        assert!(name_misfits("Aramund Fab 3", BusinessKind::OilWell));
        assert!(!name_misfits("Aramund Fab 3", BusinessKind::Fab));
        assert!(!name_misfits("Juno's Towing", BusinessKind::Bakery), "a name the generator did not make stays");
        for seed in 0..200u64 {
            let n = business_name(BusinessKind::TowYard, seed).unwrap();
            assert!(n.contains("Tow") || n.contains("Recovery") || n.contains("Wreckers"), "{n}");
            assert!(!name_misfits(&n, BusinessKind::TowYard), "{n}");
        }
        // Every kind with names names every seed the same way twice.
        for k in BusinessKind::ALL {
            assert_eq!(business_name(k, 7), business_name(k, 7));
        }
    }

    #[test]
    fn articles_follow_the_sound() {
        for (w, art) in [("observatory", "an"), ("aquarium", "an"), ("opera house", "an"), ("park", "a"), ("university", "a"), ("hour", "an"), ("hospital", "a"), ("one-way street", "a"), ("8-floor block", "an"), ("18-floor block", "an"), ("80-floor block", "an"), ("12-floor block", "a"), ("110-lot park", "a"), ("EV", "an"), ("HQ", "an"), ("TV", "a"), ("Observatory", "an"), ("", "a")] {
            assert_eq!(article(w), art, "{w}");
        }
        assert_eq!(a("observatory"), "an observatory");
    }
}
