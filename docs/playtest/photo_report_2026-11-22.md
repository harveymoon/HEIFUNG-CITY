# Photographer / inspector report, 2026-11-22 (r13-photographer)

Harvey's newest autosave (day 81 00:00, 16,768 people, Volcanic climate, light snow), copied and loaded
in my own instance (branch `r13-photographer` at main `e70d61d1` plus the todo commit, port 5262, High
graphics, 1920 x 1080, photo mode). I ran it from day 81 00:00 to day 82 10:10 (about 18,000 people
by then). I used orbit and walk views, pinned the lighting to night (21:30) and day (11:00), and once
cleared the sky (`set_weather cloud 0.1`) to see the washing come out. Ids and positions come from
`inspect`, `find`, `yards` and `terrain_at`. Positions are sim metres `[x, y]`, so they can go
straight into `camera {look_at}`. All screenshots are in `O:/projects/Heifung_District/shots/r13-photographer/`.
The 15 report shots are named below; the other files there are working shots.

The frame rate was low the whole time: 3.5-4 fps on High (p50 200-250 ms), with another agent's game
and several builds on the machine. Same family as last report's finding 16; not filed again.

## Summary

1. **The hills under Neon Market / Afterglow Mile are dug away.** Streets and the lots along them
   are graded up to 50-63 m below the natural hill. What is left of the hill stands as cliffs, lot
   floors stick out from them as flat shelves over the drop, and plinth walls stand alone. This is
   last report's finding 4 (saw-tooth plinths) grown much worse. It is the most broken-looking
   place in the city.
2. **Cyclists ride inside each other.** Two or three riders share one exact position, and in the
   chase camera a rider's head and body poke out of the rider in front.
3. **The bike junction fix looks right.** I followed a rider through three turns, with no zig-zag,
   no back-step and no flip of heading. The known left-overs (riding through parked cars, the
   chord cutting corners) are still there, as trip-review said.
4. **Round 12 work reads well up close:** the grime, soft lot edges, and washing that comes in and
   out with the weather. Two polish problems: the stain decal on pastel stucco reads as rows of
   little hearts, and the green damp band on pavements looks painted on in a snowy city.
5. Most of last report's findings look the same (landlocked lots, the yard overstock, lorries on one
   point, the crane boom, shards, lighthouses inland, black night ground, speed limits). Four look
   better: the road height fields are back in `inspect`, Clucky's site gets its deliveries, there
   are half as many derelict buildings, and no alert has a blank message.

## Top 5 (ranked)

1. **Hillside earthworks: dug-away hills, cliffs and lot shelves floating over the drop** (broken). Shots: `w07_hillside_floating_shelves.png`, `w06_neon_market_cliff_west.png`, `w04_neon_market_day_cliff.png`
2. **Cyclists stacked on one point, riding inside each other** (wrong). Shot: `b1_follow_15565_25.png`
3. **Heart-shaped stain decal on pastel stucco** (polish, but it shows on every Harbour Pastel wall). Shot: `y03_cinder_stacks_yard_close.png`
4. **Hunger demand starts at zero after a load** (wrong, small). No shot (API numbers).
5. **Mustard-yellow asphalt on some streets by day** (unrealistic). Shot: `b1_follow_15565_3.png`

## Work in flight this round (to avoid double reports)

- **r13-detail** (close-up props / washing): findings 3, 8 and 9 are in its area. I saw them on main
  as of today (`e70d61d1`).
- **Trip-review (merged):** its "Left" list already has bikes riding through parked cars, riders
  drawn on the chord between ticks, and bikes against one-way streets. I saw the first two (shot
  `b1_follow_15565_3.png`) and did not file them again.

## Findings

### Broken

#### 1. Hillsides dug 50-63 m below the natural ground; cliffs, floating lot shelves and loose walls

- **Where:** Neon Market / Afterglow Mile, west and north-west of (-1006, 894). Best view:
  `camera {look_at: [-1150, 920], distance: 300, pitch: 0.35, yaw: -1.6}`.
- **Numbers (`terrain_at`, ground vs natural height):**
  - A street of Leisure lots along y ≈ 925: 2065:3 Smoke Hawkers at (-1136, 929): ground 20.7 m,
    natural 71.0 m. 2020:5 Lot 93 (car park) at (-1122, 928): 19.4 / 68.6. 2066:3 (-1153, 925):
    22.1 / 73.5. 2067:3 (vacant) at (-1166, 924): 22.8 / 75.5. 2068:3 (-1179, 930): 24.2 / 77.7.
  - The homes just behind them sit about 35 m higher: 1994:5 Kowloon Heights at (-1133, 900):
    55.7 / 67.6. Landlocked lot 1149:7 at (-1107, 913): 53.6 / 64.7.
  - Street 816 at (-1300, 900): ground 30.8, natural 93.7 (63 m cut). Street 827 at (-1200, 1000):
    25.6 / 90.7.
  - A 200 m grid of the map shows other deep cuts:
    - x -2000..-1200 along y = 400: 19-47 m
    - x -2000..-1400 along the rail trunk, y = -400 / -600: 20-52 m
    - x 1400-1600 at y 400-800: 13-35 m
    - Almost everywhere else the cut is within ±5 m.
- **What you see:**
  - The district stands on a ring of grey cliffs and tall plinth walls over a flat, graded plain
    (w04, w05).
  - Up close (w06), green terrain spikes poke up between walls. Free-standing wall slabs as tall as
    the cliff stand on their own, like the U-shaped "frame" of three walls round a hill at the top
    right of w06. Concrete lot floors jut out of the hillside as flat shelves, with a few boxes on
    them, over a 30 m drop.
  - **w07:** the yurt camp on landlocked lot 1149:7 stands on a big flat floor whose far edge hangs
    in the air over the cliff down to the 20 m street.
- **Why it's broken:** cutting streets 50-60 m into a hill with vertical sides is not a real way to
  build a city. Lot floors over thin air and walls that hold up nothing read as missing or torn
  geometry.
- **Shots:** `w07_hillside_floating_shelves.png`, `w06_neon_market_cliff_west.png`, `w04_neon_market_day_cliff.png`
- **Guess at the cause:**
  - The built-ground grading (`crates/nd_sim/src/landscape/built_ground.rs`, `grade_free_ground`)
    seems to pull whole areas down to the street level with no cap on the depth of the cut.
  - The lot floors now lie flat (r12-odd-lots), so on a slope they stand out from the hill as
    shelves. r12-odd-lots LEFT already noted the opposite case: terrain showing through flat floors.
  - Fix directions:
    - Cap a cut at a few metres and let the street climb (or tunnel) instead.
    - Step the lots as terraces with retaining walls that reach the ground.
    - Clip a lot floor to the ground under it, or give it a wall down to the ground.
    - Drop plinth or skirt walls that do not touch the building's own footprint.
- **Compared with last report:** finding 4 (ParkHaus 8 at (-1006, 894), saw-tooth skirts) is this
  same hillside. It now reads as a whole district on cliffs, not one ragged plinth.

### Wrong

#### 2. Cyclists stacked on one point and riding inside each other

- **Where / ids:** round the junction at (491, 505) on Pylon Boulevard, day 81 08:00:
  - **17416:1** and **17496:1**, both heading home, both at exactly (471.88, 533.37), heading 1.95
  - **8311:1**, **9414:1** and **14575:3**, all shopping, all at exactly (539.77, 512.32), heading -0.02
  - **8659:3** and **10776:3** at (481.87, 508.28) and (481.85, 508.33)
- **What you see:**
  - Following 15565:1 (Vesper Haddad) with the chase camera, there is always a second rider inside
    hers: a pink or red shirt and a second head stick out of the green-shirted rider (shots
    b1_follow_15565_14 and _25).
  - In `b1_follow_15565_25.png` about a dozen riders run in a tight single file along the edge, nose
    to tail, at a bend.
- **Why it's wrong:** the same class of bug as cars inside cars and lorries on one point (last
  report's finding 2). Riders who leave together (same household, same shop trip) follow the same
  path tick by tick and are drawn on top of each other.
- **Shots:** `b1_follow_15565_25.png`, `b1_follow_15565_14.png`
- **Guess at the cause / fix:**
  - Riders (`crates/nd_sim/src/world/riders.rs`, `step_rider`) have no spacing or gap rule against
    each other.
  - Give riders on the same leg a small offset across the lane or a following gap, or stagger
    their start times by a few seconds.
  - A render-only offset (a hash of the citizen id) would hide most of it.

#### 3. Washing and yard things: lone grill far from the house; a jetty still short of the water

- **Where:** **Tide Towers** (690:1) at (26.5, 653.3), a waterfront lot in Signal Wharf. Its
  `yards` spots are `step 4, yard 8`. Its things: plant, bikes, project_bike, grill, surfboard,
  kid_bike, chalk.
- **What you see (y01):**
  - One grill stands alone in the middle of a huge empty beige pad, about 30 m from the building.
  - The household's wooden jetty stops on the brown mud bank, short of the water. This is a real
    water lot, so the jetty is allowed after r12-odd-lots, but it still does not reach the water.
- **Shot:** `y01_tide_towers_yard.png`
- **Guess:**
  - The yard spot picker (`render/yards.rs` / `remote/yards.rs` spots) can choose a point far out on
    a big lot. Keep yard things within a few metres of the walls or the back door.
  - For the jetty: extend it to the water line, or don't place one when the end point is dry (last
    report's finding 7 at another lot).
  - This is in r13-detail's area (main as of today).

#### 4. Hunger demand starts at zero after a load

- **Numbers:** right after loading the day-81 save at 00:00, `report people.hunger.demand` read
  `hungry_counted_at_midnight: 0`, `share: 0.0`, `town_hungry: false`, `hungriest_cells: []`.
  Yesterday's grocery figures were all 0. Yet 6,800 people were hungry and 3,183 had gone 2+ days
  without a meal (the `starving_citizens` alert, rank 1).
  - At the next midnight (day 82 00:10) it read `hungry_counted_at_midnight: 8712`, share 0.483,
    `town_hungry: true`, eight hungriest cells, and 2 food businesses opened for hunger. So the
    system works, but it loses the first day after every load.
- **Why it's wrong:** someone who loads a starving city gets no food demand for up to a whole game
  day, while people leave after 4 hungry days.
- **Guess:** the midnight hunger count and yesterday's grocery stats are not saved (r12-hunger
  fields without `#[serde(default)]` persistence, or rebuilt only at midnight). Save them, or
  recount on load.

#### 5. "Storm" weather label on a clear sky

- After `set_weather {cloud: 0.1, wind: 0.3, temp_c: 12}`, the reply said `label: "storm"` and
  `spell: "storm"` with no rain or snow. Ten game hours later `status.weather.label` read "clear"
  (cloud 0.41).
- **Guess:** `set_weather` keeps the running spell, so the label comes from the spell, not the sky.
  Dev command only. Low priority, but the label is misleading.

### Unrealistic

#### 6. Mustard-yellow asphalt on some streets by day

- **Where:** the long avenue east of Pylon Boulevard, from (550, 512) to (1040, 350), seen while
  following cyclist 15565:1 at 08:00-08:30 (lighting pinned to 11:00).
- **What you see:** the carriageway and the paving next to it are an olive / mustard yellow, not
  grey (b1_follow_15565_3, _14). Other streets at the same moment are a normal grey (g01 on Pylon
  Boulevard itself, b1_follow_15565_25 further on).
- **Shot:** `b1_follow_15565_3.png`
- **Guess:** a ground-tint layer drawn over the road, maybe the volcanic ash or dirt / contamination
  tint, foot wear, or r12-grime's carriageway dirt. Roads should get at most a light dirt tint;
  check which layer tints the asphalt.

#### 7. Surfboards at a hundred homes in a cold, snowy, mostly inland city

- `yards find surfboard` near (0, 300) returns 100 homes (the list's cap), the farthest 608 m away.
  Examples:
  - Cobalt Hive 220:3 at (-190, 529): its only hobby thing is a surfboard at the step
  - Cinder Stacks 730:1 at (-62, 601): three surfboards
  - Habitat 2-E 724:1 at (-155, 618)
- The climate is Volcanic, it was snowing, and the water nearby is a lake and a river.
- **Guess:** the hobby picker ignores climate and distance to open water. Give surfboards only to
  homes near a beach or the sea, in warm climates. Use other hobbies (skis or a sled in snow, a
  fishing rod by the river).

#### 8. A converted boathouse inland, facing a retaining wall

- **Where:** **Cinder Stacks** (730:1, style `converted boathouses`, Harbour Pastel, The Promenade)
  at (-62, 601), Signal Wharf.
- **What you see (y03):** a boathouse with a big glazed boat door facing a sunken one-way street's
  retaining wall, 200 m from the water.
- **Shot:** `y03_cinder_stacks_yard_close.png`
- This is a new example of last report's finding 5 (lighthouse and boathouse styles inland). It
  still stands, and the lighthouse towers inland still show in y01.

### Polish

#### 9. The pastel stucco's stain decal reads as rows of little hearts

- **Where:** every Harbour Pastel wall I looked at. Clearest on Habitat 12-D (835:1) at
  (-75.5, 605.6), next to Cinder Stacks, and on Tide Towers (690:1).
- **What you see:** small dark "∪" / heart shapes repeated over the walls in a loose grid (y03 top
  left, y01 on the blue block).
- **Shot:** `y03_cinder_stacks_yard_close.png`
- **Guess:** a grime or stain texture tile with one blob shape repeated at a small scale. Use
  larger, softer, irregular stains, and vary them by building seed. r12-grime may own this texture.

#### 10. A green damp band on pavements, painted flat, in a snowy city

- **Where:** shopfronts on Pylon Boulevard, walk view at (399, 506), by day and night.
- **What you see:** a saturated green band covers the paving along the wall and the step, with a
  hard outer edge, like paint (g01, w01). It shows the same in snow at -2 °C.
- **Shot:** `g01_walk_pylon_day.png`
- **Guess:** r12-grime's moss / damp on shaded ground. Tone the green down to a grey-green, fade it
  over the first 20-30 cm from the wall, and weaken it in cold or dry climates.

#### 11. Grime streaks repeat at the same storey rhythm on every tower

- **Where:** Harbor Heights (1:1) area at (401, 664).
- **What you see:** the rust streaks and dark blotches on concrete panels repeat identically panel
  after panel, floor after floor (g03, right).
- **Shot:** `g03_harbor_heights_grime.png`
- Already in r12-grime's LEFT ("streaks at a fixed 3.2 m storey"). Listed so the shot is on file.

#### 12. Pavement clutter blocks the footway; the walk eye starts inside a parked car

- **g01:** on Pylon Boulevard at (399, 506), a stack of blue cabinets and crates and an upright
  corrugated panel stand in the middle of the pavement, leaving a gap of about half a metre.
- **g02:** `camera {mode: walk, look_at: [399, 506], yaw: 1.57}` put the eye inside a parked car's
  roof, which filled the view.
- **Shot:** `g01_walk_pylon_day.png`
- **Guess:** shopfront clutter should keep a clear walking strip along the kerb side. Walk mode
  could push the eye out of parked cars, as it does for walls.

## Checked and fine

- **Bikes at junctions (trip-review fix):** I followed **15565:1** (Vesper Haddad) on a 100-point
  ride from (553, 512) east and then south, sampling every second:
  - Headings went smoothly -0.02 → -0.30 → -0.39 through a bend at (930, 503), then -0.39 → -1.41
    at the junction at (1020, 465).
  - No back-steps, no sideways steps and no heading flips, so the zig-zag Harvey saw is gone.
  - At the bend near the level crossing (b1_follow_15565_14) she stayed on the street; another rider
    took the ramp beside it, which looked like their own route.
  - Not checked: the start and end at the door. Still there, as trip-review listed: riding through
    parked cars (b1_follow_15565_3), and the chord cutting corners.
  - 2,952 people were riding at 08:00 (47.6% of trips under way), so bikes are a big part of the
    street picture now.
- **Washing follows the weather (r12-yards):** in the snow, `yards` read `laundry_out: false`,
  `wet: true`. After clearing the sky, by 10:10 it read `laundry_out: true`, and lines with clothes
  stand in the yards (Cinder Stacks 730:1, y03; Cobalt Hive 220:3; Mercury Towers 723:1). The
  tool bench and the kids' bikes sit where they should.
- **Grime and soft edges (r12-grime):** walls look weathered and lived in (g03, w01 at night). Lot
  paths have soft grass fringes instead of hard edges (y02, y03).
- **Odd lots (r12-odd-lots):** I saw no gravel wedges. The one dock I checked closely (Tide Towers)
  is on a real water lot. See finding 3 for its length.
- **Hunger and food (r12-hunger):** the demand system works after its first midnight (finding 4).
  215 groceries and 35 bakeries; hungry_now 7,057 at day 82 00:10 (40%). Grocery trips that found
  bare shelves: 1,500 on day 81. Population still grew 16,768 → about 18,000 in the day. This
  matches r12-hunger's LEFT (b), so not filed again.
- **Night views:** Signal Row at night (w02), the Iron / Copper Circus viaducts at night (n01) and a
  night walk along a downtown street (n02) all look good.

## Fixed or better since the last report (2026-11-20)

- **F3 API gap:** `inspect {road}` now returns `levels`, `heights`, `inner_heights` and
  `grade_crossings` (e.g. street 816 heights [30.12, 30.82]). The interchange geometry itself
  (p02) looks about the same; I could not tell whether the sliver ramp is gone.
- **F10 Clucky's Drive-Thru (2010:3):** no longer blocked. 3 of 4 deliveries, progress 0.33, crew
  3/3. It still reports `closed: false` while under construction.
- **F10 / alerts:** no alert has an empty `message` any more.
- **F11 derelict towers:** 168 buildings with `derelict: true` (was 365); the alert says 159. Not
  checked whether people still live in them.
- **Unhoused / free homes:** at 23:10 day 81 the alert read 289 unhoused with 1,626 homes free; by
  00:10 it read 55 unhoused with 1,775 free. Probably just the housing step's timing; not filed.

**Look the same (not filed again):**
- F1: 778 landlocked lots, 689 of them empty, 611 with `car_access: true`. Lot 446:5 unchanged.
  Also 665 lots with no building report `vacant: false`.
- F2: Yard 21 holds 133 boxes against capacity 25; 4 lorries share (-347.5, -414.2) at Cargo Hub 1.
- F5: lighthouses and boathouses inland (y01, finding 8).
- F6: the crane boom at the Port of Saltmarsh still reaches the far bank, now with a ship alongside
  (p03).
- F7: a jetty short of the water, at Tide Towers now (finding 3).
- F8: shards at Founders' Campus (o01).
- F13: speed limit 46.8 km/h.
- F14: `status.lighting.hour` read 11.0 at 10:10 with `follow_clock: true` (the game was paused).
- F17: black ground at night (w03).

## Report shots (15)

| # | File | What |
|---|------|------|
| 1 | `w07_hillside_floating_shelves.png` | finding 1: yurt pad hanging over the cliff |
| 2 | `w06_neon_market_cliff_west.png` | finding 1: cliffs, shelves, loose walls |
| 3 | `w04_neon_market_day_cliff.png` | finding 1: the district on a ring of cliffs |
| 4 | `b1_follow_15565_25.png` | finding 2: stacked cyclists |
| 5 | `b1_follow_15565_14.png` | finding 2, and the bike check at the level crossing |
| 6 | `b1_follow_15565_3.png` | finding 6: mustard asphalt; known: riding through parked cars |
| 7 | `y03_cinder_stacks_yard_close.png` | findings 8, 9; washing out (fine) |
| 8 | `y01_tide_towers_yard.png` | finding 3; F5 and F7 still |
| 9 | `g01_walk_pylon_day.png` | findings 10, 12 |
| 10 | `g03_harbor_heights_grime.png` | finding 11; grime reads well |
| 11 | `o01_founders_campus.png` | F8 still |
| 12 | `p03_port_crane_recheck.png` | F6 still |
| 13 | `w02_signal_row_night_orbit.png` | good: Signal Row at night |
| 14 | `n01_circus_viaducts_night.png` | good: the viaduct rings at night |
| 15 | `n02_walk_signal_row_night.png` | good: night walk downtown |

## Counts by severity

- broken: 1 (finding 1)
- wrong: 4 (findings 2, 3, 4, 5)
- unrealistic: 3 (findings 6, 7, 8)
- polish: 4 (findings 9, 10, 11, 12)

12 findings in all (finding 11 is already in r12-grime's LEFT; findings 3, 8 and 9 touch r13-detail's area).
