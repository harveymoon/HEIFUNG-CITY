# Code review: `crates/nd_sim/src/world/`

Read-only review of the `World` systems (about 60k lines in about 100 files) at `main` 8e68a3e (2026-10-08). The work was split into seven slices: services/society/air, people/social/trips, plan/placement, estate/economy, vehicles/transit, power/water/industry, and tests. Every slice was read in full, and usages were grepped across `crates/`, nd_app included. All paths are relative to `crates/nd_sim/src/world/` unless shown otherwise. Line numbers are as of that commit.

Each finding has a severity (**H**igh, **M**edium, **L**ow) and is marked as one of:
- **[V] Verified**: I read the code path and grepped its callers.
- **[S] Suspected**: the logic reads wrong, but I did not run it.

`cargo check -p nd_sim --all-targets` is clean apart from one unused import (`ramp_tests.rs:6`). That means no *private* dead code. Dead `pub` items had to be found by grep (§3.3).

---

## Top 20 actions

### (a) Bug fixes
1. **H. Friend loans can never happen.** `social.rs:1162-1213` runs after `daily_household` (`trips.rs:117-125`) has pooled every pocket into the household purse. After that, savings are at most 40, so the borrower and lender conditions can never be met. Move `loans` before `daily_update`, or base it on household money. (§1.2) **FIXED 36a7595**
2. **H. Taking over a merged building leaves dangling ids.** `take_clearing` (`construction.rs:497-513`) removes the building without calling `estate_building_gone`. Its extra lots, owner properties, reservations and the mansion entry keep the dead id, and indexing them can panic later. (§1.1) **FIXED 36a7595**
3. **H. Campus annexes never get built on plots that had a building.** The cleared lot is claimed by `claim_reserved` into the **main** university building (`campus.rs:422` and `estate.rs:429-446`). The `pending` entry then stays forever and counts toward `ANNEX_CAP`. Give campus reservations their own kind. (§1.4) **FIXED 36a7595**
4. **H. Rebuilds leave stale lot keys.** `remap_lot_keys` (`plan.rs:1358`) does not remap campus `pending`, social gardens and murals, or street-event race `Spot.lot`. After any road or zoning implement, those keys name unrelated new lots. `demolish_with` (`plan.rs:1553`) places evacuees at the stale `b.lot` centre instead of `at`. The `Expropriated` and `BuildingRemoved` events carry stale keys out to the remote API. (§1.1) **FIXED 36a7595**
5. **H. Newcomers inherit a stranger's state.** `elite_visitor` (`estate.rs:1068-1106`) and `student_arrives` (`campus.rs:273-311`) build citizens from `..template`, a clone of a random adult. They copy `drunk`, `asleep`, `wakefulness`, `packed_meal` and `commute`. Add a `Citizen::newcomer(...)` constructor and use it in `immigrate` too. (§1.4) **FIXED 36a7595**
6. **M. Corp levy double-counts the skim** (`corp.rs:450-453`). It should be `levy.min(c.cash.max(0.0))`. **FIXED 36a7595**
7. **M. Ferry day-trippers are charged three fares for two legs.** `ferry.rs:350` books both legs, then `water.rs:1427-1433` books the return again. (§1.6) **FIXED 36a7595**
8. **M. Some taxi, crash and parking bugs:**
   - A cab parked at a full rank keeps `job = ToRank` and is never re-rota'd (`taxis.rs:779-786`). **FIXED 36a7595**
   - Crash blame takes the citizen from `drivers` but the cause from `cars[0]`, so the wrong person is arrested (`driving.rs:441`). **FIXED 36a7595**
   - Injured drivers of cars that were not written off keep driving (`driving.rs:394-406`). **FIXED 36a7595**
   - One parking search is counted twice in `parking_street_today`, so lots open early (`cars.rs:901` and `922`). (§1.5) **FIXED 36a7595**
9. **M. `INDUSTRY_CHOICES` is missing Mining, Smelting and FactoryFarming** (`industry_bridge.rs:12`). The remote API rejects them and the Zone tool never offers them. Build the list from `IndustryKind::ALL`. **FIXED 36a7595**
10. **M. Two placement paths skip site checks.**
    - `build_power` skips the `min_thickness` check, so heavy plants can go on kerb strips (`power.rs:68-88`). Call `placement_problem` first. **FIXED 36a7595**
    - `build_parking` and `build_taxi_company` ignore `estate.reserved` and rubble (`parking.rs:47`, `taxis.rs:57`; the `vacant` list in `develop.rs:531` does not filter either). They can build on a plot an owner bought. **FIXED 36a7595**

### (b) Safe refactors (behaviour identical)
11. **One building-classification module.** Replace `Building::is_special`, `World::is_civic_structure`, `is_city_building`, `city_use` and about 8 inline chains with explicit, documented predicates. Doing this exposes the drift listed in §2.1, which can then be fixed in deliberate follow-up commits. **FIXED 36a7595 (the three city-building checks: `Building::is_city_structure`; the inline chains remain)**
12. **One "vacant buildable lot" or "site plot ok" predicate** in place of about 20 copies that disagree on reserved, rubble, frozen and under-facility (§2.2). The disagreements are what cause item 10. **FIXED 36a7595 (`World::site_clear` / `lot_free_for` with a `SiteRule`)**
13. **One `crate::hash` module.** It replaces 9 copies of `mix`, `unit`, `key_of` and `hash01`, plus a `Stream` salt registry for the 6 ad-hoc sub-RNGs (`erng`, `srng`, `society_rng`, `arng`, `trng`, `rng_for`). This also removes the `erng(4)` collision between `rich.rs:66` and `estate.rs:965`. (§2.3) **FIXED 36a7595 (the `erng(4)` collision only: `estate::erng_salt`)**
14. **Integration-test fixture.** Move `demo_world` and its helpers into `tests/common/mod.rs`. Today `demo_city.rs` is pulled in with `#[path]` by 4 other test files, so its 8 tests run in **5 binaries**. Also add a `world/test_support.rs` (§5.3). **FIXED 7a2ba59 (`tests/common`; `world/test_support.rs` not yet)**
15. **Remove dead code and fix wrong comments** (§3.3 and §3.4).

### (c) Bigger restructures
16. **Move world-RNG draws off the shared stream** (§4.1 and §5.2). First: the weather (`market.rs:127`, which every system then feels), through-traffic (`water.rs:787-804`, every tick), per-citizen trip draws (`trips.rs`, 34 sites), `power.rs:556`, parking (`cars.rs:907`, `952`, `982`, `1014`), `enforcement.rs:98` and `market.rs:818`. This is the root cause of the flaky demo tests.
17. **Save the per-system RNG streams.** `Services`, `Society` and `Air` hold `Option<SimRng>` streams marked `serde(skip)`, so a save/load mid-run diverges from an uninterrupted run. Then add a save → load → continue determinism test and an exhaustive `from_ron(to_ron(w)).to_ron() == to_ron(w)` round-trip test (§5.4). **FIXED b96a96e+fd6698e (streams saved; the save/load/continue test is in `tests/determinism.rs` but `#[ignore]`d: traffic, rail and land-value work in progress are not saved yet)**
18. **Make the multi-seed harness robust.** Pool the society_demo 911 drill over 5 clones of a day-7 world, and run demo_city over 5 seeds, instead of tuning thresholds against a single seed (§5.1).
19. **Speed up the per-frame and per-hour hot spots** (§4):
    - `project_check` clones the whole `World` and rebuilds on every `plan_version` bump, and `holo.rs` calls it.
    - The plant-group bonus is O(P³) per grid hour and per Power-window frame.
    - `height_cap` and the other "floors within R" scans are O(buildings) per lot.
    - `parcel_holds_civic` is O(lots) per parcel per frame.
20. **Split the large files** along the lines in §3.1: `services.rs`, `social.rs`, `trips.rs`, `plan.rs`, `water.rs`, `estate.rs`, `cars.rs`, `driving.rs`. Do it after items 11–13 so that the moves are pure moves.

---

## 1. Correctness

### 1.1 Lots, buildings and rebuilds (plan, construction, placement)

| Sev | Where | Problem | Fix |
|---|---|---|---|
| H [V] | `construction.rs:497-513` `take_clearing` | Calls `buildings.remove(old)` directly. `estate_building_gone` never runs, so `extra_lots` keep `building = Some(dead)`, and `owner.properties`, `reserved`, nimby and `mansion` dangle. Reached via `take_over_lot` → `begin_demolition` → `place_special` (`trains.rs:246`). | Take `plot` out of the old building and call `estate_building_gone(old, &plot, ..)`, or route through one `remove_building_record(bid)`. **FIXED 36a7595** |
| H [V] | `plan.rs:1358` `remap_lot_keys` | The doc says every kept key follows its lot, but it only remaps rubble, camps and `pending_footfall`. These are not remapped: campus `pending` (`corp.rs:550`), `social.gardens` and `murals` (`social.rs:785`, `804`; `social.rs:1616-1628` only checks `contains_key`), and race `Spot.lot` (`street_events.rs:214`, used at `nights.rs:156` and `262`). A fresh SlotMap re-issues the same keys, so all of these silently point at other lots. | Remap them with the same `map`. Add a test that rebuilds and checks every stored `LotId` still names a lot at the old position. **FIXED 36a7595** |
| M [V] | `plan.rs:1553` `demolish_with` | `center = lots.get(b.lot).map(center).or(at)` prefers the stale old key over `at` during a rebuild, so evacuees are put on an unrelated lot. | `at.or_else(\|\| lots.get(b.lot).map(..))`. **FIXED 36a7595** |
| M [V] | `plan.rs:1263-1265`, `1595` | `Expropriated(bid, b.lot)` and `BuildingRemoved(bid, b.lot)` carry the old key after a rebuild. The remote API exports them (`nd_app/src/remote.rs:362`). | Emit `LotId::default()` (or map through `carried`) when `at.is_some()`. **FIXED 36a7595** |
| M [V] | `plan.rs:93-99`, `1024-1051` `add_road`, `add_rail`, `add_path`, `add_zone` | They restore `self.project` without calling `plan_changed()`. `effective_plan()` and `planned_zoning` then ignore the active project while `preview()` does not. A refused implement silently drops the scratch project. Each call burns a project number. | Call `plan_changed()` after restoring. Return `Option<GestureId>` when `implement_refused` is set. |
| M [V] | `plan.rs:289-303` + `767` | `implement_project` moves the active project aside, so `conflict_warnings` never sees it. | Pass the parked-aside project into `conflict_warnings`. |
| M [V] | `placement.rs:279-284` `release_city_lot` | Replaces the lot with a fresh `make_lot`, so `shore` is reset. This runs after `mark_shore_lots` in a rebuild. | Carry `shore` over, as `landlocked` already is. |
| M [V] | `water.rs:527-544` `charge_bridges` | Charges the *net* `metres - bridge_metres`, so a removed bridge pays for a new one. Viaducts are charged, but the preview (`bridge_cost`, used only in tests) says 0. An old save (field defaults to 0, never migrated) pays for every bridge on its first implement. | Charge per new deck segment. Set `bridge_metres` from the network in `after_load` when the field is absent. Share one deck-metres function between preview and charge. |
| M [S] | `plan.rs:1510`, `1228`, `pillars.rs:107` | Expropriation and buyouts debit nothing and compensate no one. Possibly intended. | Document it, or add one `buy_out(bid, at, why)` that charges (see §2.4). |
| L [V] | `interchange.rs:260-264` `add_diamond` | Adds 3-5 gestures, so one Ctrl+Z removes only the last ramp. | Use one `edit_project(\|p\| p.push_many(..))`. |
| L [S] | `plan.rs:1194-1248` | Extras and primaries are matched in `old_lots` order. An extra can take the primary's new lot, which turns an expropriation into a silent demolition. | Two passes: primaries first. |
| L [V] | `pillars.rs:285-297` | `pillars_after_load` follows the drawn points, while a rebuild follows smoothed, water-split and sampled points. `pillar_gaps` (`serde(skip)`) are lost on load. | Extract `built_way_points(...)` and use it in both places. |
| L [V] | `mod.rs:957` `validate` | Checks lot → building but not building → lot. `ports[*].lot` is not validated. | Re-link or drop buildings whose `b.lot` does not point back. |
| L [S] | `grounds.rs:163` `settle_path_links` | Rebuilds without `project_check`. A structure fronting only the trimmed path is removed, and `demolish_with` never calls `refresh_power` or bumps the subway version. | Refresh the power and subway state in `demolish_with` for power and metro buildings. |
| L [V] | `develop.rs:751-757` `build_on_named` | A rezoned park is removed with `buildings.remove` directly, bypassing `estate_building_gone`. | Same helper as `take_clearing`. **FIXED 36a7595** |

### 1.2 People, social, trips, places

| Sev | Where | Problem | Fix |
|---|---|---|---|
| H [V] | `social.rs:1162-1213` `loans` | Can never fire. See action 1. Existing debts are never repaid either. | Run before `daily_update`, or use `households[h].money`. Add a test. **FIXED 36a7595** |
| M [V] | `mod.rs:671` `social_concern` | `serde(skip)` and only set at :25 in `hourly_social` (`social.rs:711`). After a load, `pick_topic` has no `Topic::City`, so conversations diverge. | Set it in `after_load`. **FIXED b96a96e+fd6698e (saved; read in `after_load` for older saves)** |
| M [V] | `trips.rs:210`, `social.rs:1146` | The "move up a tier" search and the friend flat tip-off don't filter `b.annex`, so households can move into dorms or frat houses. They also compare `b.rent`, not `rent_due`. | Add a shared `Building::is_free_home()` (§2.2). |
| M [V] | `benches.rs:138`, `social.rs:762-768` | `park_with_room` and `shared_place` skip `park_open_for_visits`, so people are sent to shut beaches. | Add a `park_visitable(lot)` helper. |
| M [V] | `social.rs:767` | Comment says "Never where an enemy of either is now". No such check exists. | Implement it via `take_crowds`, or drop the comment. |
| M [V] | `social.rs:829-840` `plan_outings` | Records the meet-up, memories and `meetups += 1` even when neither person set off. | Check `is_traveling()` as the pickup branch does. |
| M [S] | `rides.rs:340-350` `school_run` | The parent's trip and the onward leg are set up before `join_ride` can fail, so the parent drives off without the child. | Check seats and distance first, or undo on failure. |
| L [V] | `social.rs:657` | `avoid_hits` is bumped once per candidate building scanned, not once per choice. | Bump once per `choose_place`. |
| L [V] | `trips.rs:994`, `places.rs:419-433` | Any shut or full destination (grocer, park, friend's home) files `Concern::Nightlife`. | Pick the concern from the trip's purpose. |
| L [V] | `social.rs:2010` vs `2013` | Protest news says "today", members remember "tomorrow". It fires on D+1. | Say "tomorrow". |
| L [V] | `rides.rs:322` | `hour..(hour+4).min(23)` misses hour 23 and shifts that run past midnight. | Use `(0..4).any(\|k\| ..(hour+k)%24)`. |
| L [S] | `trips.rs:1301-1304` | A failed `origin_for` sets `Idle` without `leave_building`, so a visitor stays counted. | Call `leave_building` first. |
| L [V] | `benches.rs` `seats_after_load` | Uses the seat key as the bench key, so sitters on old saves never meet. | Look up via `walk.benches_in(lot)`. |
| L [S] | `crate::pedestrians` `occupants`, `buttons`; `Social::avoid_hits` | `serde(skip)`, but the comment claims a load "carries on exactly". | Save them, or correct the comment. |

### 1.3 Services, society, enforcement, chases, air

| Sev | Where | Problem | Fix |
|---|---|---|---|
| H [V] | `services.rs:1328` `civic_value` | Batteries and city parking (both `is_civic_structure`) fall through to `Placeable::CargoHub.cost()`. Plants use base `cost()`, not site-scaled `cost_at`. Repair bills are therefore wrong. | Use `self.placed_as(id).map_or(0.0, \|p\| p.cost())` (`services.rs:362`). **FIXED 36a7595** |
| M [V] | `services.rs:447`, `2245` | `incidents_today` and `resolved_today` are `[u32; 3]` indexed by `kind.index()`. Crash is 3, so `get_mut` silently drops crashes, while `unanswered_today` counts them. | Index by `kind.slot()`, or use `[u32; 4]`. **FIXED 36a7595 (by `slot()`: crashes count with the police)** |
| M [V] | `chases.rs:281-318` | A homicide suspect tagged after a chase is arrested, but `homicide_solved` never runs: no news, the family never hears. | Call `homicide_solved` in the tag branch. |
| M [V] | `air.rs:436`, `867` | Air ambulances are sent to on-road crash victims. `parked` tests `c.inside == Some(building)`, so the incident resolves `Gone` with no transport. | Skip `on_road`, or land at `inc.pos` and use the on-road presence test. |
| M [S] | `services.rs:2383-2394` | `daily_services` overwrites `c.income` for every employed citizen before `daily_update`. Elites' incomes are restored only later, in `daily_estate`. | Apply the bonus separately, or move the pass. |
| M [V] | `enforcement.rs:98` | `patrol_target` draws from the world RNG every 10 ticks and on every arrival. | Use `srng()` or a hash. |
| L [V] | `enforcement.rs:479` | An unpaid clamp fine goes to the tow business. The city never receives it and `fines_total` misses it. | Credit the city in `pay_release`. |
| L [S] | `enforcement.rs:895` `rebook_booted` | Officer clamps are rebooked as `Ordered`, losing the Unpaid wait and fine. | Store the reason on the spot. |
| L [V] | `services.rs:~1165`, `950` | `recover_lost_units` and `divert_for` requeue by hand and leave stale fields. | Call `self.requeue(id)`. |
| L [V] | `services.rs:668-673` | With `emergencies` off, `hourly_services` returns before `rubble.retain`, so rubble never clears and blocks the lot forever. | Hoist the retain. |
| L [V] | `chases.rs:348` | `CalledOff` is counted as lost. | Add a separate counter. |
| L [S] | `chases.rs:244` | `t.route.len() - 1` underflows on an empty route. | Use `checked_sub`. |

### 1.4 Estate, towers, corps, campus, market, tourism, districts

| Sev | Where | Problem | Fix |
|---|---|---|---|
| H [V] | `campus.rs:422` + `estate.rs:429-446` | Annex plot reservations are claimed into the main university building (action 3). `campus_grow_now` takes the same path. | Use a separate reservation kind; `claim_pending` builds the annex. **FIXED 36a7595** |
| H [V] | `estate.rs:1068-1106`, `campus.rs:273-311` | `..template` newcomers (action 5). They also return `None` in a town with no adults. | Add `Citizen::newcomer`. **FIXED 36a7595** |
| H [V] | `corp.rs:450-453` | `levy.min(c.cash.max(0.0) + skim)` after `c.cash += skim` counts the skim twice. | `levy.min(c.cash.max(0.0))`. **FIXED 36a7595** |
| M [V] | `towers.rs:411-418`, `440-446` `break_ground` | Redevelops the primary plot without `pack_business`, so its business (cash and staff) is lost. | Call `pack_business` first, as the other three acquire flows do. **FIXED 36a7595** |
| M [V] | `corp.rs:469-499` + `towers.rs:381` | A corp's league-tower HQ is paid out of a tower developer's capital and given to the corp for 0. It sets `last_start_day`, which blocks market towers. A non-commercial fallback site gives an HQ with no Office. | Add a `TowerOrder` path that charges corp cash, is limited to Commercial, and leaves the gap alone. |
| M [V] | `estate.rs:213-225` `give_to`, `301-323` `buyable` | Sellers are never paid. `Person`, `Firm` and `Developer` holdings are buyable, so an investor's second home (bought with a real deposit, `rich.rs:90`) can be taken for nothing by a firm expansion, a tower, an HQ, or the remote `buy_out_and_rebuild`. | Pay the previous owner in `give_to`, or make Person-owned property unbuyable. **FIXED 36a7595 (`settle_sale` at every market buyout)** |
| M [V] | `estate.rs:362-363`, `390` `prune_estate` | The keep-list is built before dead elites are pruned. A dead billionaire's Elite owner keeps the mansion and firms forever (unbuyable), the NIMBY entry stays, and the police escort is never reassigned. | Release the elite's properties, drop the NIMBY entry, and call `refresh_police_escort()`. **FIXED 36a7595** |
| M [V] | `campus.rs:119` | `prune_campuses` leaves the University owner holding the annexes (unbuyable, still tagged) and the reservations in place. | Release them on prune. |
| M [V] | `districts.rs:301-339` `district_stats` | Iterates lots, so merged buildings are counted once per extra lot (2-4×). | Skip unless `b.lot == lot_id`. **FIXED 36a7595** |
| L [V] | `market.rs:818` | World-RNG draw on every purchase. | Use a hash. |
| L [V] | `corp.rs:456` | The levy is booked as `BusinessTax` but not added to `economy.tax_today`. | Add it. |
| L [V] | `corp.rs:811-829` `divest` | Can sell the flagship while `corp.flagship` still points at it. | Skip the flagship, or clear it. |
| L [S] | `market.rs:353` | `self.buildings[y]` indexes from the cached `yards` (`serde(skip)`, possibly stale mid-hour). | Use `get_mut`. |
| L [V] | `districts.rs:227-250` | The industry retool runs in Historic districts, against the module doc. | Check Historic first. |
| L [V] | `tourism.rs:276-279` | `break` with no way in drops the hour's remaining arrivals uncounted. | Add them to `turned_away_today`. |
| L [V] | `estate.rs:1002-1006`, `1066`, `rich.rs:90`, `181`, `campus.rs:236`, `270`, `pressure.rs:352`, `estate.rs:701`, `762` | Money appears or vanishes with no ledger flow (elite seed money, the investor rent share, student stipends, branch and merge fees). The branch fee can push a business negative. | Book it through `economy` flows, or document it as intentional. |
| L [V] | `rich.rs:66` + `estate.rs:965` | Both use `erng(4)` on the same midnight tick, so an elite spawn day is always an invest day too. | Use distinct salts (part of the §2.3 registry). **FIXED 36a7595** |

### 1.5 Vehicles and transit

| Sev | Where | Problem | Fix |
|---|---|---|---|
| M [V] | `driving.rs:441` | `at_fault` takes the cause from `cars.first()` but the person from `drivers.first()` (citizens only). A stranger's red-light crash gets the citizen victim arrested. | Use `traffic.cars[cars[0]].citizen` if that is a citizen. **FIXED 36a7595** |
| M [V] | `cars.rs:901-904` + `922-925` | One search increments `parking_street_today` twice (circling, then gave up). That counter drives `maybe_open_parking`. | Count once. **FIXED 36a7595** |
| M [V] | `taxis.rs:779-786` | Rank full → `park_cab_at_kerb` keeps `job = ToRank`. The cab blocks the rank in `at_rank` and `run_rota` never moves it again. | Set `job = Idle` in the fallback. **FIXED 36a7595** |
| M [V] | `driving.rs:394-406` | Injured drivers whose car was not wrecked keep `Activity::Driving`, while a Medical incident is opened at the crash. | Stop and unseat them, as the wreck branch does. **FIXED 36a7595 (a hurt driver's car is written off: they wait by it)** |
| M [S] | `driving.rs:371-378` + `taxis.rs:404` | A cab can be written off with a fare aboard. The rider stays `Riding` until the tow, the rider is never rolled for injury, and the cab is "recovered" to its yard. | Set the rider down, roll for injury, and remove the cab. |
| L [V] | `moto.rs:338`, `nights.rs:603` | Clearing `profile.assigned` re-runs `assign_drivers`: drunk trips are double-counted and the memory is written twice. | Restore the stored profile instead. |
| L [V] | `taxis.rs:935-939` `sell_cab` | Removes an on-road cab mid-street. | Use `retire_vehicle`. |
| L [V] | `tolls.rs:101-131` | Taxis, tow trucks, recovery trucks and dealer cars pay tolls on nobody's account, and city tow trucks pay the city. | Charge the company, or exempt them. |
| L [V] | `cars.rs:1387` `vehicles_after_load` | The orphan test uses `t.owned.is_some()` instead of `== Some(id)`. | Tighten it. |
| L [S] | `taxis.rs:441-474` | `serve_requests` frees cabs while `requests` is taken out, so a freed cab can't see a waiting rider. | Free them after the requests are put back. |
| L [S] | `buses.rs:173-179` | Riders on a bus whose alight stop is removed lose their trip and idle until their old decision tick. | Retarget `alight`, or set `next_decision_tick`. |
| L [S] | `tow_moves.rs:226-263` | `retire_vehicle` mid-stunt with the move dropped: the stunt never ends. | End the stunt first. |
| L [V] | `taxis.rs:618` vs `306` | Fare charged at straight-line ×1.25, quoted at ×1.3. | Use one formula. |

### 1.6 Power, water, ports, fishing, industry

| Sev | Where | Problem | Fix |
|---|---|---|---|
| M [V] | `ferry.rs:350-354` + `water.rs:1427-1433` | Day-trippers pay 2 fares at boarding and are charged again from `boarding` when the ferry sails. | Keep a separate carried-but-not-charged count. **FIXED 36a7595** |
| M [V] | `industry_bridge.rs:12` | 8 of 10 industry kinds are listed (action 9). | Derive the list from `IndustryKind::ALL`, and test that it covers every kind. **FIXED 36a7595** |
| M [V] | `power.rs:68-88` `build_power` | Re-implements `lot_problem` minus `min_thickness`. Reached directly from `place_on_lot` and the remote API. | Call `placement_problem` first and delete the inline copy. **FIXED 36a7595** |
| L-M [V] | `power.rs:182`, `202` `plant_rows` | `pending: construction.is_some()`, but a heavy plant clearing its lot has `demolition` set instead, so it shows as running at 0 MW. | Use `!b.operating()`, as the battery rows do. |
| L [V] | `water.rs:1039-1066` `place_port` | Debits and expropriates before the "no street" failure, which keeps the money and the takings. | Check first, or roll back. |
| L [V] | `fishing.rs:339-347` | Stats count the full catch while only `min(room)` reaches the stock. | Record the amount actually stored. |
| L [V] | `water.rs:1431` | Ferry overflow riders vanish (`riders.min(CAPACITY)` after removing all of `boarding`). | Put the remainder back. **FIXED 36a7595** |
| L [S] | `power.rs:584` `plant_cluster` | `self.lots[field[0]]` unchecked. The renderer calls it every frame. | Guard with `get`. |
| L [V] | `ferry.rs:377` | `ferry_riders_today` is counted on the stranded sweep, when no boat ran. | Count only real sailings. |
| L [V] | `shore.rs:540-544` | A jetty load whose buyer is gone is dropped. The dock paths re-dispatch it. | Make jetties consistent with docks. |
| L [V] | `cargo_hubs.rs:27`, `164` | Hub build and upkeep are booked without an item, upkeep as `RailUpkeep`, inside `step_cargo_hubs` after the day record closes. | Add a `daily_cargo_hubs` beside `daily_ports` using `debit_item`. |

### 1.7 Save/load and `#[serde(default)]`

- Every `World` field added since v1 has `#[serde(default)]`. The fields without it (`config`, `clock`, `rng`, `master`, `next_gesture`, `plan_version`, `roads`, `zones`, `lots`, `buildings`, `citizens`, `traffic`, `economy`, `land_value`, `next_seed`) are original v1 fields. **No missing defaults were found on World.**
- **H. Per-system RNG streams are not saved.** `Services::rng`, `Society::rng` and `Air::rng` are `Option<SimRng>` with `serde(skip)`, re-seeded from `(seed, load tick)` by `srng` (`services.rs:54`), `society_rng` (`society.rs:10`) and `arng` (`air.rs:30`). A loaded game diverges from the same run left going. **FIXED b96a96e+fd6698e (streams: fix/review-sim-core `rng_save`, the world stream folded into it; `Transit::departures` saved too; `Air::grumbled` still skipped)** `Air::grumbled` is skipped too, so people grumble twice a day across a load. Save them with an `Option` variant of `rng_serde` (`mod.rs:1445`).
- **Not repaired on load.** `after_load` does not call `prune_social`, `prune_corps` or `prune_campuses`, which only run at midnight. Nothing cleans incidents or units, society offenders and camps, taxis, cargo hubs or fishing that point at missing ids. `validate` covers only the original systems.
- **Pruned only daily.** `social_concern` and `bridge_metres`: see the tables above.

---

## 2. Duplication (proposed single homes)

### 2.1 "What kind of building is this?"
- **Overlapping predicates:**
  - `Building::is_special` (`buildings.rs:424`): power, depot, rail, port, service, metro, special annex. **No CargoHub, no city_parking.**
  - `World::is_civic_structure` (`plan.rs:1502`): adds city_parking and CargoHub, **drops annex**.
  - `is_city_building` (`placement.rs:57`): `is_civic_structure || vehicles.city_lots`. Redundant with `b.city_parking` since `after_load:871`. `taxes.rs:44`, `market.rs:23`, `cars.rs:969` and `parking.rs:83` read the map, not the flag.
  - `city_use` (`placement.rs:213`) and `civic_value` (`services.rs:1328`): each has its own case list, and the latter has the bug above.
- **Inline "ordinary grown building" chains, each slightly different:** `mod.rs:1114` (`height_cap`; misses port, metro and cargo hub, so docks count towards density), `air.rs:171-183`, `industry.rs:142`, `services.rs:2737`, `plan.rs:973`, `corp.rs:124`, `corp.rs:278`, `towers.rs:203`, `districts.rs:217` (redundant `port` check), `develop.rs:435`, `estate.rs:303`.
- **Working depot:** `b.depot && !b.is_derelict() && b.operating()` appears 4 times (`buses.rs:238`, `308`, `develop.rs:96`, `survey.rs:92`). `committee/report.rs:277` and `proposals.rs:301` drop `!is_derelict`.
- **Home:** a `world/kinds.rs` (or methods on `Building`) with `is_city_structure()`, `is_grown()`, `counts_for_density()`, `working_depot()`, `is_free_home()` and `placed_as()`. Make `city_parking` the only source of truth and keep `vehicles.city_lots` for its data only. **FIXED 36a7595 (`is_city_structure`, `is_special`, `is_civic_structure`, `is_city_building` now agree)**

### 2.2 "Can something be built or bought here?"
- **Vacant buildable lot:** about 20 copies. Some forget `reserved`, `is_rubble`, `district_frozen` or `landlocked`:
  - `develop.rs:531` (`vacant`, no reserved or rubble filter), `760`
  - `relocate.rs:118`, `campus.rs:447`, `corp.rs:514`, `672`
  - `estate.rs:486`, `677`, `811`, `1153`
  - `towers.rs:216`, `pressure.rs:571`
  - `parking.rs:47` (no reserved or rubble check: bug), `taxis.rs:57` (same bug)
  - `tourism.rs:373`, `nights.rs:139`, `shore.rs:219`
- **Site plot ok:** 7 copies of "vacant, or buyable and unmerged with floors ≤ N": `estate.rs:486-497`, `811-823`, `1152-1160`, `towers.rs:214-223`, `corp.rs:512-521`, `campus.rs:447-458`, `corp.rs:672`.
- **New building site boilerplate** (`Building::new` → construction → insert → `BuildingBuilt` → `order_materials`): `estate.rs:878`, `towers.rs:420`, `corp.rs:591`, `782`, `campus.rs:493`. Add one `raise_site(...)`.
- **Free home:** 6 copies: `trips.rs:210`, `241`, `social.rs:1146`, `campus.rs:722`, `pressure.rs:885`, `estate.rs:1059`.
- **Placement checks:** `build_power` (`power.rs:68-88`) copies `lot_problem` (`services.rs:124`). `structures_in_way_at` (`plan.rs:936`), `road_takes` (`pillars.rs:241`) and `roundabout_takes` (`plan.rs:509`) use three different road widths. Use `deck_half_width` with one `lots_under_way(..)`.
- **Facility and port footprints:** the footprint union appears at `plan.rs:473`, `1155` and `pillars.rs:116`. `under_facility` (`mod.rs:1099`) rebuilds every footprint per call and ignores ports; `no_parcel_why` adds ports by hand.
- **Home:** `fn lot_free_for(&self, lid, &SiteRule) -> bool` in `world/placement.rs`, plus cached `structure_footprints()` in `refresh_facility_cache`. **FIXED 36a7595 (`site_clear` / `lot_free_for`; footprints not cached yet)**

### 2.3 Hashing and RNG
- **Splitmix-style mixers:** `committee/mod.rs:968`, `corp.rs:319`, `streets.rs:91`, `world/nights.rs:14` and `world/social.rs:108` are identical. `fishing.rs:249`, `shore.rs:137` and `world/waterfront.rs:52` are another identical trio. `world/pressure.rs:47` is a murmur variant. `pedestrians.rs:92` has `hash01`.
- **`unit`:** `corp.rs:327`, `towers.rs:253`, `nights.rs:21`.
- **Key-to-u64 helpers:** `districts.rs:359`, `tourism.rs:797`, `pressure.rs:56`, `moto.rs:23`, `nights.rs:25`, `cars.rs:1624` `id_hash`, `driving.rs:138` `car_hash`.
- **Sub-RNG constructors:** `erng(salt)` (`estate.rs:25`, a fresh generator per salt and tick), `trng()` (`taxis.rs:30`; a fresh generator **per call**, so two calls in one tick give the same numbers), cached `srng`, `society_rng` and `arng`, and `rng_for` (`lib.rs:135`; a linear XOR mix that can collide across salt and tick pairs).
- **Home:** `crate::hash` with `mix`, `unit`, `key64`, `hash01`, and a `Stream` enum of registered salts plus `World::rng_at(stream, key)`. See §5.2.

### 2.4 Flows re-implemented per branch
- **Buyout and take-over:** `take_over_lot` (`plan.rs:1510`), rebuild expropriation (`plan.rs:1258-1271`) and pillar buyouts (`plan.rs:1228`). Merge into one `buy_out(bid, at, why, memory)`. `check_take_over` (`plan.rs:1527`) is an empty hook.
- **Nearest building or service:**
  - nearest working service: 8 copies (`services.rs:2079`, `2151`, `2304`, `1594`, `air.rs:453`, `876`, `journeys.rs:497`, `enforcement.rs:73`)
  - nearest building to a point: `concerns.rs:494`, `driving.rs:341`, `pedestrians.rs:727`
  - witness search: `driving.rs:348` and `pedestrians.rs:736`
  - nearest port or yard: `trains.rs:28` and `water.rs:1232` are the same function
  - `cargo_hubs.rs:38`, `industry.rs:396`, `shore.rs:395`, `taxis.rs:197` (which equals `free_cabs_near(p, 1)`)
  - Many of these have no id tie-break. Add one `nearest_of(ids, p, reach)` with an id tie-break, plus `nearest_service(kind, p, accept)`.
- **Car handling:**
  - impound a car: `driving.rs:1124`, `1184`, `nights.rs:510`, `583`
  - drive own car to X: `cars.rs:551`, `driving.rs:899`, `nights.rs:257`, `moto.rs:171`
  - release to home: `moto.rs:319`, `nights.rs:594`, `driving.rs:1197`
  - first street at a node: 8 copies in `cars`, `driving` and `moto`
  - lanes into or out of a node: about 8 copies
  - `building_lot(x).and_then(car_segment..)`: 14 or more copies
  - Homes: `cars.rs` and `driving.rs` for the first three, `RoadNetwork::street_at` for streets, `TrafficNet::lanes_from` and `lanes_into` for lanes, and `car_stop(b)` next to `truck_stop` in `market.rs` for the last.
- **Services:** `send_unit` and `send_second` share about 30 lines (`services.rs:1735-1807`). `return_to_station` and the `send_home` tail are the same. `open_incident` and `requeue` both set `work_left`. The `city_has` check is inlined at `services.rs:1444`.
- **Money:**
  - The purse has three conventions: household first (`enforcement.rs:255`), savings first (`air.rs:798`), and clamped (`services.rs:2125`). `air.rs:248` re-implements `wealth_of`.
  - The income formula appears at `trips.rs:745` and `services.rs:2390`, the latter with an education bonus the hire path lacks.
  - The import refund appears 4 times: `water.rs:1239`, `shore.rs:509`, `cargo_hubs.rs:65`, `market.rs:626`.
  - Sum of plot prices appears 4 times: `estate.rs:741`, `855`, `towers.rs:375`, `corp.rs:580`.
  - `sale_price` and `value_of_new` share a table.
- **Ownership lookups:** 8 functions: `owner_of`, `owner_for_citizen` (a linear scan), `properties_of`, `firm_of`, `investment_income`, `corp_of`, `campus_of`, `holdings_of`. There are also two sources of truth, `Business::owner` and `plot.owner`. Add one `owners_of_citizen(cid)` and retire `Business::owner`.
- **Floors within R:** `mod.rs:1111`, `towers.rs:200`, `estate.rs:1198`. Add one `floors_within(center, r, except)` over a cell grid.
- **People free to go out:** `places.rs:323`, `social.rs:689` and `rides.rs` disagree on thresholds. The on-shift test is copied in 5 files. Add `on_shift(cid, hour)` and `free_for_company(cid)`.
- **Plants and ferries:** the plant-field BFS appears at `power.rs:573` and `plant_groups.rs:94` with different "finished" filters. The ferry rotation is copied at `ferry_routes.rs:309` and `water.rs:1384`, and the two already disagree on retry timing.
- **Naming:** two unrelated "pressure" systems. `estate::Pressure`, a people-density grid, should be renamed `Density`; it is separate from `crate::pressure`, the business pressure.

---

## 3. Structure

### 3.1 Files to split

| File | Lines | Proposed split |
|---|---|---|
| `services.rs` | 3427 | `services/{mod (srng, hourly, step, step_incident, resolve, daily), coverage (232-398), incidents (399-625), staffing (631, 1097-1143), dispatch (823-984, 1589-1808), units (985-1092, 1144-1323, 1376, 1809-1977), fire (1324-1375, 1472-1588, + scenes.rs), care (1978-2234), events (2300-2374), tests (~1000 lines from 2438)}`. Placement code (59-230, 362) moves to `placement.rs`. |
| `social.rs` | 2117 | `social/{mod, encounters, talk, outings, ties, groups, works, petitions}` (line ranges in the social slice: 1-158, 160-306, 308-607 + 2068-2117, 609-905, 1022-1288, 907-986 + 1290-1630, 1632-1885, 988-1020 + 1887-2034). |
| `trips.rs` | 1809 | `trips/{household, population, decide, arrive, travel, modes}`. `arrive` (252 lines) splits further into park arrival, turned away at the door, and seat arrival. |
| `plan.rs` | 1761 | `projects.rs` (105-323), `parcels.rs`, `project_check.rs`, `rebuild.rs` (`rebuild_from_master`, remap, `LotGrid`, `same_layout`, `carry_traffic`). Removal lifecycle goes to `construction.rs`, and `strand_*` to the traffic code. `rebuild_from_master` (244 lines) becomes `lay_master_roads`, `swap_network`, `recut_lots`, `carry_buildings` (two passes), `pull_down_for_works` and `after_relayout`. |
| `water.rs` | 1733 | `heights`, `bridges`, `land_extent`, `highway`, `dock_access`, `docks`, `ships`. |
| `estate.rs` | 1453 | `estate/{density, owners, growth, elite, convoys}`. |
| `cars.rs` | 1646 | `kerbs`, `vehicle_routes` (generic exits and retire, used by 5 systems), `parking_hunt`, `car_market`, `commute` (`begin_trip`, `end_trip`). |
| `driving.rs` | 1740 | `drivers`, `wrecks` (beside `tow_moves`), `takeovers`, `driving_tests`. |
| `mod.rs` | 1471 | `tick()` (181 lines) could become `daily()`, `hourly()` and `per_tick()` with the minute phases as named constants. Move the save code (`to_ron`, `after_load`, `validate`, `rng_serde`) to `world/save.rs`. |

**Long functions**, in addition to the above:
- `pedestrians.rs:377` `step_walker` (287 lines)
- `market.rs:125` `hourly_economy` (259): split into `hourly_prelude`, `produce_hour`, `walk_in_trade`, `restock` with `restock_one`, `export_surplus` and `market_stats`
- `social.rs:375` `converse` (189)
- `develop.rs:337` `daily_update` (181)
- `fishing.rs:163` `step_fishing` (172)
- `power.rs:394` `dispatch_power` (168): split into `plant_supply`, `battery_headroom`, `run_batteries`, `book_grid_hour`, `share_supply` and `dark_hour_moods`
- `interchange.rs:89` `plan_diamond` (166)
- `trips.rs:578` `immigrate` (162)
- `riders.rs:123` `rider_stop` (153)

### 3.2 Modules in the wrong place
- **Placement:** service placement lives in `services.rs` (59-230). `build_power` is in `power.rs`, `build_parking` in `parking.rs`, `build_taxi_company` in `taxis.rs`, `build_cargo_hub` in `cargo_hubs.rs`. All should go through `placement.rs`.
- **Parking:** `daily_city_lots` and `set_parking_price` are in `enforcement.rs:902-920` but belong in `parking.rs`.
- **Midnight work:** it runs from three places. `daily_*` is called from `tick`, `daily_air` runs inside `step_air`, and enforcement and cargo-hub resets run inside their step functions. Route all of it through `tick()`'s day block so the order relative to `close_day_record` is explicit.
- **Tests:** tests unrelated to journeys sit in `journeys.rs:541-570`.
- **Test-only copies:** `#[cfg(test)]` copies of production code, `industry.rs:608` `truck_arrived` and `654` `send_truck_back`, are what `industry_tests.rs:239` and `261` test, not the real `lorry_arrived`.

### 3.3 Dead code (grep across `crates/`, nd_app included)

**No callers:**
- `plan.rs:793` `implement_blockers`
- `power.rs:171` `plant_cost_on`
- `cargo_hubs.rs:320` `hubs_near`
- `junctions.rs:321` `arrows_from`, `:326` `MOVES`
- `campus.rs:71` `is_student`
- `chases.rs:405` `chases_now`
- `crate::services` `chase_of_car` and `dispatchers_on_shift`
- `crate::air` `is_pad` and `noise_at`
- `crate::taxi` `cab_of_car`

**Test-only `pub`:** `tie_between`, `car_occupants`, `best_shop`, `parking_trouble_spot`, `has_project`. Make them `pub(crate)` or `#[cfg(test)]`.

**Unreachable:**
- the cargo branch in `vehicle_arrived` (`cars.rs:605`; `mod.rs:1413` routes cargo first)
- the `(None, Some(k))` arm of `unhappy_neighbours` (`dirt.rs:185`)
- `ShipKind::FishingBoat` arms (never spawned): `waterfront.rs:45`, `175`, `184`
- the empty `boat_home` (`waterfront.rs:135`)
- `check_take_over` (`plan.rs:1527`)

**`let _ = x;` leftovers:**
- `develop.rs:34`, `campus.rs:591`, `estate.rs:801`
- `rich.rs:150-189`, `market.rs:247`
- `cars.rs:994`, `1041` (unused `target` parameter)
- `nights.rs:563` (`_for_chase`), `nights.rs:756`
- `water.rs:1316-1320`, `construction.rs:144`, `junctions.rs:144`
- `social.rs:648` (`friends_regular`, computed at a cost per tie per candidate, then discarded)

### 3.4 Comments that are now wrong (house `[branch]` tags kept)
- **Doc comments on the wrong item:**
  - `plan.rs:1445-1449`: the `is_civic_structure` doc sits on `parcel_holds_civic`.
  - `plan.rs:1738-1740`: the `shrink_about_centroid` doc sits on `closest_on`.
  - `estate.rs:88-91`: the `pressure_floors` doc sits on `pressure_active`.
  - `industry.rs:58-61`: the doc sits on `UNLOAD_SECONDS`.
  - `mod.rs:539-544`: the `crowds_cache` doc sits on `waiting_cache`.
  - `market.rs:730-731`: the old `best_shop` doc is duplicated.
- **Comments that claim things that are no longer true:**
  - "Never draws the world RNG": `driving.rs:7-8` (`send_recovery` → `route_from_edge_to` shuffles `self.rng`), `relocate.rs:14`, `ferry.rs:24-25`.
  - "Never where an enemy is": `social.rs:767`.
  - The `remap_lot_keys` doc says every key follows its lot (`plan.rs:1351`).
  - "Historic districts are never refitted": `districts.rs:13`.
  - Coal plants are said to be read into `dirty` (`dirt.rs:1-2`, `108`).
  - "via `truck_arrived`": `industry.rs:13`.
  - "plus those on their way": `industry.rs:380`.
  - "and on an ordinary day's average": `power.rs:310`.
  - "None until the industry model is in": `industry_bridge.rs:32`.
  - The `vehicle_arrived` doc on through cars: `cars.rs:600`.
  - "longest out first" and "idlest" vs slot order: `taxis.rs:857`, `997`.
  - "No bus is coming" given to rail and subway riders: `buses.rs:278`.
  - "Pupils stay until..." is on the wrong block: `services.rs:783`.
  - The index named in the `free_units` doc: `services.rs:821`.
  - `Holding::Owns` scope: `gifts.rs:28`.
  - A field `Spot` no longer has: `social.rs:115`.
  - A 4-tuple that has 3 fields: `places.rs:143`.
  - `road_name_at` does no lookup: `interchange.rs:309`.
  - `release_racer`'s `keep_role`: `nights.rs:559`.

---

## 4. Performance hot spots

| Sev | Where | Cost | Fix |
|---|---|---|---|
| H | `plan.rs:818` `project_check` ← `nd_app/src/render/holo.rs:362` | Clones the **entire World** (citizens, traffic) and runs `rebuild_from_master` on every `plan_version` bump, including every drag amend. | Cache by `(plan_version, roads.version)`. Run the trial on a slim clone (plan, roads, lots and civic buildings only), or debounce the trial to drag-end and the implement. |
| H | `plant_groups.rs:165` via `power.rs:162`, `401` | `plant_group_bonus` per plant = `plants_of` O(B) + `flood` O(P²) `lots_touch` (0.5 m sampling) → about O(P³) per grid hour. `plant_rows` does it about 3× per plant every frame while the Power window is open (`nd_app/src/ui/detail.rs:427`). | One union-find grouping, cached on a `serde(skip)` field keyed on `roads.version` plus a building or lot change counter. |
| M | `mod.rs:1108` `height_cap`, `towers.rs:200`, `228-264`, `estate.rs:1198` | O(buildings) per lot, called per candidate lot by develop, estate, corp, campus and towers. `tower_sites` also rescans for vertiports per lot (`towers.rs:113`). | A per-day floors grid of `DENSITY_RADIUS` cells, with vertiport centres cached. |
| M | `plan.rs:1450` `parcel_holds_civic`, `mod.rs:1099` `under_facility` | O(lots) per parcel, called per frame for about 400 parcels (`nd_app/src/tools.rs:1751`). `under_facility` rebuilds every footprint per call and runs per lot in about 15 loops. | Use `p.lot` (O(1)) and cached footprints. |
| M | `develop.rs:16` `update_economy` | About 15 full passes over buildings and 5 over citizens, every 10 ticks. | One pass each. |
| M | `services.rs:1488` `incident(id)` | Linear over up to 500 resolved incidents, called many times per active incident per tick. `step_incident` and others `.cloned()` whole incidents every tick. | `binary_search_by_key` (ids ascend). Copy out only the fields needed. |
| M | `services.rs:672`, `729` | `refresh_coverage` (4 Dijkstras + per-lot pass) runs hourly regardless of `coverage_signature`. `by_car` is rebuilt every tick (`812`) though it is maintained incrementally. | Gate on the signature, and drop the per-tick rebuild. |
| M | `enforcement.rs:163` `patrols_look` | O(vehicles × patrols) every 2 ticks. | Segment index over kerb spots. |
| M | `rides.rs:39` `passengers_of`, `267`; `step_passengers` | Full citizen scans per seat check, per pickup and per tick. | A `BTreeMap<driver, Vec<passenger>>` in `Rides`. |
| M | `places.rs:131` `choose_place` | Every Visit decision scans all buildings × ties × `inside.contains`, and allocates per candidate. | Distance cull first, sorted `inside`, hoist the citizen's place book. |
| M | `trips.rs:1485`, `753` | Parking-fee estimate scans all parking per trip. `job_search` is O(jobless × buildings), with an RNG draw each. | Spatial parking index, and rank jobs once per call. |
| M | `water.rs:780-863` | `step_through_traffic` rebuilds `highway_ends()` and counts through cars every tick. `city_leaves()` (a HashSet over all lots) is rebuilt on every exit search (about 6 callers, twice per `route_from_edge_to`). | Cache on `(roads.version, plan_version)`, and keep a through-car counter. |
| M | `cars.rs:867` `look_for_space`, `taxis.rs:800-900` `run_rota` | All buildings per hunting arrival. `ranks()` × 2 building scans per idle cab every 5 ticks. | Index of parking businesses, and ranks once per rota. |
| M | `industry.rs:396` `fleet_lot_with_room`, `market.rs:654` `truck_count()` | Scans all cars plus all buildings per lorry arrival or per dispatch. | Incremental counters. |
| M | `tourism.rs:703` `tourist_pick_shop` | All buildings per visitor decision. | An hourly tourist-shop list, like `seller_index`. |
| M | `construction.rs:333`, `437`, `plan.rs:1580` | Rebuild demolition is O(D × (lots + citizens)). | Pass the `LotGrid` in; collect the people inside once. |
| L | `grounds.rs:146` `refresh_closed_grounds` | A full building scan and a Vec allocation **every tick** (called before `clock.advance`). | Recompute on construction or demolition state changes. |
| L | `journeys.rs:121` `step_journeys`, `driving.rs:191`, `232`, `pedestrians.rs:82` | Per-tick scans of all cars or walkers, plus fresh HashMap and Vec allocations. | Note at spawn, and reuse buffers. |
| L | `services.rs:994` `units_out()` inside the per-unit loop; `1017` `start_patrol` O(B × 500); `subway.rs:268` `prune_subway` every tick; `fishing.rs:190` clones Port and Jetty every tick; `water.rs:1335` clones every Port every 10 ticks | Assorted. | Hoist, index, borrow. |
| L | `estate.rs:778` `build_mcmansions` O(H × owners); `campus.rs:152` `enrol` O(C × students); `corp.rs:622` `corp_candidates` (clones the Dijkstra table per buy); `corp.rs:427` `drive_cache` evicts the fallback origin daily | Daily, so modest. | Use sets, compute once per day, borrow the table. |

Caches checked and fine: `crowds_cache` and `waiting_cache` are keyed by tick. `plan_cache` is refreshed in `plan_changed`, apart from the `add_*` helpers (§1.1). `seller_index` is keyed by hour. `LandValueWork` is deliberately dropped on clone and saved.

**HashMap iteration: no outcome-affecting case found.** Every HashMap or HashSet in `world/` is used for lookups, sorted afterwards, or summed over integer-valued f32s (`air.rs:174`). Ordered state uses SlotMap or BTreeMap.

---

## 5. Tests

### 5.1 Why demo_city, society_demo and sim_checks are borderline
- **One shared stream.** The world RNG `self.rng` is drawn at about 80 sites (`trips.rs` 34, `develop.rs` 10, `cars.rs` 9, `water.rs` 6, `mod.rs` 5, and others). Several draw a *variable number* of times:
  - one per vacant lot (`develop.rs:580`)
  - one per citizen in the dark (`power.rs:556`)
  - through-traffic short-circuits on the live count (`water.rs:787`)
  - one per mode option (`trips.rs:1527`)
  - one per parking (`cars.rs:982`, `1014`)
  So any new draw anywhere shifts everything downstream. The weather is drawn from that same stream (`market.rs:127`), and it feeds trips, fishing and the economy, so it amplifies every shift.
- **Churn in git history.** 19 non-merge commits touched the three files. 8 of them loosened, re-windowed or reseeded an assertion:
  - society_demo: da5de71, e0d7a35, 811c3bb, 1a02e03 (`median < 15` became `<= 15`)
  - demo_city broke-business share: 88d924a < 5% → d3eac82 < 8% → 10800c2 → da153c4 < 8.5%
  - sim_checks benches: b43b0c5 (2 days became 2.5)
- **society_demo** (`tests/society_demo.rs:230-240`):
  - With about 7 drill fires, "≥ 90% within 30 min" means all 7 must make it.
  - The nearest-rank median over 7 values is decided by one call.
  - Crime and illness picks are still list-index based (`:99`, `:124`), the fragility 811c3bb fixed for fires. One LCG feeds all kinds, so one slow fire shifts every later pick.
  - `slot()` folds organic crashes into the crime bucket (`:172` vs `:231`).
  - `served.offenders < bare.offenders` and the overflowing comparison are single integers from two runs that diverge everywhere.
- **demo_city** (`:141-165`): `late < 0.085` sits next to a measured spread of 1-12% (per the test's own comment). "Every household size 1-5" depends on rare draws in `household_ages` (`trips.rs:537`). The parking test (`:222-255`) prints tickets and tows but never asserts them, and silently skips the office placement.
- **sim_checks:** `:93` jumps to 00:22, when few people walk, and then asserts walkers exist. It hard-codes the private `LAND_VALUE_PHASE`. The benches guard `most >= 2` depends on organic park visits.

**Making them robust without loosening:**
1. Migrate the RNG (§5.2) first, then re-baseline once.
2. **society_demo:** run to day 7 with services, `clone()` 5 times, give each clone a different drill seed, and assert on the *pooled* median and the within-30 share over 35 or more calls per kind. Pick every drill target by nearest-to-a-hashed-point. Index results by `kind` and report crashes separately. Add mechanism checks:
   - call to dispatch ≤ 2 min when a unit is free;
   - travel time ≤ 1.5 × free-flow route time + 3 min.
3. **demo_city:** run seeds `[7, 11, 23, 42, 99]` in parallel with `thread::scope` and pool business-days. With about √5 less noise the threshold can go back to < 7%. Assert household sizes on pooled arrivals. Assert tickets, tows and releases are > 0, and `expect` the office placement.
4. **sim_checks:** advance to 08:22 (`tick % 1440 == 502`) and expose `land_value_in_progress()`. Fill the benches on purpose with injected `Visit(park)` decisions via the `Late` mind.
5. **Per-system invariant checks** instead of totals, where possible. For example: bins in depot cover are emptied within 48 h, and jailed offenders do not offend.

### 5.2 RNG design
- Add `crate::rng`, with `enum Stream { Weather, Through, Trip, Mode, Develop, Immigration, Names, Cars, Power, Services, Society, Air, Estate, Taxis, ... }` and registered salts. This rules out collisions like `erng(4)`.
- **Keyed, stateless draws (the default):** `World::rng_at(stream, key) = SimRng::seed_from_u64(splitmix(seed ^ splitmix(stream ^ splitmix(tick ^ splitmix(key)))))`, keyed by entity (`citizen.seed`, `lot.as_ffi()`, node). They are save-safe by construction, and one system's draw count can no longer shift another's.
- **Persistent sequential streams** where a real sequence is wanted (names, immigration batches): a saved `World.streams` with `rng_serde`. These replace the unsaved cached `Option<SimRng>` streams.
- **Migration order:**
  1. weather (`market.rs:127`)
  2. through traffic (`water.rs:787`, `797`, `804`)
  3. `trips.rs`: decision timing at `821`, `858`, `1072`, `1100`, `1110`; mode at `1527`; car buy and sell at `1565-1598`; leisure at `1726-1747`; destinations at `193`, `207`, `249`, `429`, `434`, `473`; bench at `925`
  4. `power.rs:556`
  5. `develop.rs` (`536` start index, `580` per-lot roll, others)
  6. `mod.rs:1145` `pick_floors`
  7. `cars.rs` (`503`, `512`, `907`, `952`, `982`, `1014`) and colours
  8. immigration (`trips.rs:488-662`)
  9. cosmetic names to `Names` (`develop.rs:783`, `trains.rs:195`, `power.rs:106`, `127`, `parking.rs:58`, `buses.rs:37`, `water.rs:1070`); `parking.rs:53` floors is keyed by lot
  10. `industry.rs:151`, `207`, `217`, `market.rs:818`, `enforcement.rs:98`, `water.rs:1453`

  Each step changes golden behaviour once; land them together, then re-baseline.

### 5.3 Test hygiene
- **H. demo_city runs 5 times.** `tests/demo_city.rs` (8 tests) is `#[path]`-included by `society_demo.rs`, `sim_checks.rs`, `probe_society_civic.rs` and `probe_society_units.rs`, so its tests run in 5 binaries. That is 5 chances to flake and about 7 eight-day demo runs per full suite. `busy_city()` (up to 40k ticks) is built up to 15 times. Fix:
  - Move `demo_world`, `Late`, `ghosts`, `place` and `place_near` into `tests/common/mod.rs`.
  - Keep one copy of the M4 late-mind test; `demo_city.rs:393` and `sim_checks.rs:52` duplicate each other.
  - Share `busy_city` via `OnceLock` and clone it per test. **FIXED 7a2ba59**
- **Duplicated unit-test helpers.** Proposed home: `world/test_support.rs` (`#[cfg(test)]`). Copies:
  - `square`: about 25 copies with two different signatures
  - `rect`: 5 copies
  - `finish_all`: 8 identical copies (`tests.rs:13`, `car_tests.rs:13`, `districts_tests.rs:22`, `places_tests.rs:13`, `social_tests.rs:12`, `taxi_tests.rs:20`, `ped_highway_tests.rs:20`, `under_construction_tests.rs:23`)
  - `run(w, ticks)`: about 15 copies
  - `run_until`: 4 copies
  - `civic_lot`: `driving.rs:1311`, `services.rs:2467`, `events_tests.rs:67`, and `society.rs:128`, which differs silently (min area 500 vs 420)
  - `station`, `building_at`, `lot_near`: 3 copies
  - the quiet `WorldConfig {emergencies: false, society: false, flat, ..test_town()}` literal: about 20 copies

  `test_support.rs` should also provide `quiet_config` and `full_config`, `HOUR` and `DAY`, and a crate-wide `World::check_invariants()`, promoted from `car_tests.rs:84`, run by `run()` in debug.
- **Systems on by default.** No test sets `social: false`. 36 worlds in `tests.rs` run with emergencies, society and social on (e.g. the 5-day runs at `tests.rs:111` and `838`), so random fires can hit unrelated assertions.
- **Tests that pass without testing.** `corp_tests.rs:544`, `economy_tests.rs:352`, `estate_tests.rs:504` and `towers_tests.rs:236` read `saves/flags/flag_patch.ron` (gitignored) and pass with "skipped" when it is missing. `rebuild_regressions.rs:125` passes when `ND_SAVE` is unset. Commit a trimmed legacy fixture under `tests/fixtures/`, and make the env-gated test panic with a hint. **FIXED 7a2ba59**
- **Magic tick counts:** `events_tests.rs:273`, `sim_checks.rs:93`, `society_demo.rs:98`, `110`, `121`, `166`.
- **Slow unit tests** (multi-day): `tests.rs:111`, `838`, `districts_tests.rs:198`, `economy_tests.rs:289`, `estate_tests.rs:142`, `towers_tests.rs:197`, among others.

### 5.4 Important behaviour with no test
- **H. Save → load → continue.** Run A, round-trip into B, run both M ticks, compare `to_ron()`. This fails today because of the unsaved streams (§1.7). **FIXED 7a2ba59 (`#[ignore]`d with the reasons: state outside `world/` is not saved yet)**
- **H. Whole-world determinism** on the demo world with RuleBasedMind (2 days twice → identical RON). Only per-subsystem determinism tests exist. **FIXED 7a2ba59**
- **M. Exhaustive round trip.** Assert `from_ron(w.to_ron()).to_ron() == w.to_ron()` on a world with one of every feature. These fields have no round-trip test: `taxis`, `cargo_hubs`, `services` (incidents, units), `driving`, `journeys`, `scene`, `identity`, `rides`, `crosswalks`, `road_log`, `air`, `pillars`, `streets`.
- **M. Dangling ids.** Inject one ghost id per system, load, and run 1440 ticks under `check_invariants()`. Also call `prune_social`, `prune_corps` and `prune_campuses` from `after_load`.
- **Regression tests, one per bug fixed** (each also lists its test in the slice tables above):
  - loans
  - the campus annex on a standing plot
  - newcomer state
  - the low-cash levy
  - a tower over a business
  - investor buyout payment
  - elite death and the escort
  - campus removal
  - merged-plot district stats
  - `civic_value` for a battery or parking lot
  - crash stats
  - a homicide chase tag
  - an air medic sent to a road crash
  - rebuild key aliasing (campus pending, gardens, murals, race spots)
  - `add_road` with an active project
  - implementing a parked project
  - `add_diamond` undo
  - bridge replacement and an old-save bridge
  - a waterfront city lot keeping `shore`
  - `take_clearing` on a merged building
  - day-trip fares
  - `INDUSTRY_CHOICES` coverage
  - `build_power` vs `placement_problem`
  - a cab at a full rank
  - `at_fault` with a stranger
  - a taxi wreck with a rider
  - `parking_street_today` counting
  - a bus stop removed with riders aboard
  - rubble clearing with emergencies off
  - the production lorry path (instead of the `#[cfg(test)]` copy)
