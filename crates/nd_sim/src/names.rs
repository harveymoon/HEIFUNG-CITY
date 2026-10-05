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
    "Arasaka", "Tessier", "Kiroshi", "Halcyon", "Nakatomi", "Oberon", "Vantage", "Zenith",
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
