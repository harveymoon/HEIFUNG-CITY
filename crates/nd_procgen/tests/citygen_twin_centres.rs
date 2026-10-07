//! [mapgen] The twin centres example city on two seeds (see
//! `citygen_common`).
//!
//! cargo test --release -p nd_procgen --test citygen_twin_centres -- --nocapture

mod citygen_common;

use citygen_common::{check_city, check_deterministic, SEEDS};
use nd_procgen::citygen::CityPreset;

#[test]
fn deterministic() {
    check_deterministic(CityPreset::TwinCentres);
}

#[test]
fn seed_1() {
    check_city(CityPreset::TwinCentres, SEEDS[0]);
}

#[test]
fn seed_2() {
    check_city(CityPreset::TwinCentres, SEEDS[1]);
}
