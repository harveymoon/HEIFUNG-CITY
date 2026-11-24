# Photographer / inspector report, 2026-11-24 (r16-photographer)

Harvey's newest autosave (day 81 00:00, "pit", 16,768 people, River valley, Volcanic climate, light snow),
copied into my own folder and loaded in my own instance (branch `r16-photographer` at main `13f51cb2`: all
of round 15 and the other round 16 agents; port 5350, High graphics, 1920 x 1080, photo mode). I ran it
from day 81 00:00 to day 82 01:20 (17,071 people). Orbit and walk views, a follow with the chase camera,
drive mode, the inspector and the City Data pages. I pinned the light per view (the hour is given), and
for the last views cleared the sky (`set_weather cloud 0.12`). Ids and positions come from `inspect`,
`find`, `containers`, `terrain_at`, `ledger` and `report`; positions are sim metres `[x, y]` for
`camera {look_at}`.

- Shots: `O:/projects/Heifung_District/shots/r16-photographer/` (report shots named below; `b_try_*`
  are working shots). Beautiful shots (no fixes needed): `.../r16-photographer/beautiful/` (8).
- The tour as a readable PDF (title, itinerary and map, a section per stop, findings, summary, gallery):
  `O:/projects/Heifung_District/shots/r16-photographer/heifung_city_tour_2026-11-24.pdf` (36 pages),
  built by `O:/projects/Heifung_District/runs/r16-photographer/build_tour_pdf.py` from `tour.json`.
- Machine load: the coordinator built the release exe during my first ~30 minutes, so my own build took
  33 minutes; the tour ran after that. At 4x the game held 60 fps (frame p50 16.4 ms, p95 19.4 ms, 2
  frames over 100 ms in 30 s); at 64x the sim reached 12.8 ticks/s of 16 asked (frame p95 40.8 ms, max
  1.2 s). The load froze one frame for 7.1 s (the open load item).

## Summary

1. **Round 15 holds up.** No cliffs on the Neon Market hill, rivers and streams full width between stone
   quays, empty landlocked lots drawn as meadows, every container depot at or under its room and stacks
   four high, the port crane's boom raised with no ship, the money books balancing to the cent. None of
   the "broken" problems of the last two reports is left.
2. **The city is beautiful at dusk and night** (Afterglow Mile, the circus viaducts, Capsule's domes,
   Signal Row): 8 gallery shots.
3. **What is wrong now is smaller:** 4 wrong (Volcanic snows at any temperature; an alert that mixes
   yesterday and today; Money flow's headline inflated by a load's opening balances; container yards
   with capacity 0), 3 unrealistic (a blank three-storey strip mall; a beach bar in the snow; parking
   fines a sixth of the city's income), 7 polish.
4. No crashes, no hangs, no `sim_fault`.

## Top 5 (ranked)

1. **F1 Volcanic climate snows at any temperature, so it never rains** (wrong). Shot: `b_try_rain_neon_3.png`
2. **F3 Money flow counts a load's opening balances as money in from outside** (wrong). Shot: `cd_economy_money_flow.png`
3. **F5 Three-storey strip mall drawn as a blank box** (unrealistic). Shots: `f_strip_mall_blank_box_573-3.png`, `s03_stream_211_787_day.png`
4. **F2 "Units saturated" alert after midnight says today, means yesterday** (wrong). No shot (API).
5. **F14 Parking fines are 17 % of the city's income** (unrealistic). Shot: `cd_economy_cash_flow.png`

## Findings

### Wrong

#### F1. Volcanic climate snows at any temperature; it never rains

- **Where:** the whole city. `set_weather {cloud: 0.95, temp_c: 8, spell: "storm"}` replied
  `precipitation: {kind: "snow", intensity: 0.87}`; `status.weather.temp_c` 8.0 with snow falling.
- **Why it matters:** snow at 8 °C is wrong, and because a Volcanic city never rains, the wet glossy
  roads and neon on wet streets that the renderer can draw never appear in this city.
- **Cause:** `nd_app/src/render/climate.rs precip_now` takes the kind from `look(climate).precip` only;
  `nd_procgen/src/climate.rs` gives Volcanic `Precip::Snow`.
- **Fix:** choose rain or snow from `w.weather.temp_c` (snow under about 1 °C, a sleet band), with the
  climate only as a bias.
- **Shot:** `b_try_rain_neon_3.png` (a "storm" at 8 °C: flakes, dry road).

#### F2. "Emergency units saturated" after midnight says "today" but means yesterday

- **Where:** `report` alerts, day 82 00:30 and 01:20: "all 8 ambulances busy for 13.1 h today (up to 0
  calls waiting at once, 0 unanswered calls today because every unit was out)".
- **Why it matters:** 13.1 h "today" at 00:30 is impossible, and next to "0 calls waiting" it reads as
  nonsense; the player cannot tell whether to build a hospital.
- **Cause:** `nd_app/src/remote/report.rs saturated_alerts` takes `saturated_min_yesterday` in the first
  3 h of the day but the message says "today" and pairs it with today's `queue_max_today` and
  `unanswered_why_today`.
- **Fix:** in that window say "yesterday" and use yesterday's queue and unanswered counts (keep them at
  midnight), or drop the yesterday fallback.

#### F3. Money flow counts a load's opening balances as money in from outside

- **Where:** City Data, Economy, Money flow, "yesterday" and "today": "$1.6B in from outside". The
  ledger's `opening_balances` was $1,580,138,721 from outside on day 81 (the day I loaded) and
  $1,517,537,924 on day 80 (Harvey's own load); `investment` was $62M and everything else is small.
- **Why it matters:** the headline figure of the page is 25 times every real inflow together, on any day
  the game was loaded, which is most days a player looks.
- **Cause:** by design every load reads balances back as `Kind::Opening` moves from `outside`
  (`bank.rs`, `World::open_ledger`); the page sums every kind that crosses the edge.
- **Fix:** leave `Kind::Opening` out of the page's edge totals and bands, or show it as its own line
  ("books opened on load").
- **Shot:** `cd_economy_money_flow.png`

#### F4. Eight container yards report capacity 0 but hold boxes

- **Where:** `containers`: 8 of 18 depots have `capacity: 0`: Vantage Box Yard 576:3 at (233.5, 216.8)
  (`ground_capacity` 15, 1 box stored, a `storage` fee entry), Iron Containers 641:3 (1 box), Vantage
  Containers 1710:1 at (380.5, 152.8) (3 boxes), Thinking Inside the Box 1720:1, Meridian Freight
  1800:1, Iron Box Yard 1804:1, Iron Containers 1836:3, Meridian Box Yard 1988:1 (0 boxes).
- **Why it matters:** a yard with room for 0 that holds 3 boxes is over its room by the r15 rule; either
  the number is wrong or these yards will be drained or never used. (r16-tidy moved the room to the sim
  this round, so this may be new.)
- **Fix:** check how `capacity` is derived for small/storage yards against `ground_capacity` and
  `Containers::layout_room`; make the reported room agree with the drawn ground.

### Unrealistic

#### F5. A three-storey strip mall drawn as a blank box

- **Where:** Namara Logistics **573:3**, lot 965:29, at (242.9, 800.1), Static Gardens; style "strip mall",
  look "night market", 3 floors; tenants Sunrise Diner 1850:1, Over Easy 1886:1, Vellane Synergies 591:3.
  `camera {look_at: [243, 800], distance: 60, pitch: 0.25, yaw: -0.6}`.
- **What you see:** a windowless tan box with one dark slot, no shopfronts, on a parking podium whose
  face steps in a saw-tooth pattern.
- **Why it matters:** two diners and an office in a building with no door or window; it reads as a
  missing mesh.
- **Fix:** give the strip-mall style shopfronts and windows at every height (or cap the style at one
  floor and pick another for taller multi-tenant blocks); count how many strip malls are over 1 floor.
- **Shots:** `f_strip_mall_blank_box_573-3.png`, `s03_stream_211_787_day.png`

#### F6. A beach bar in the snow; a stream that springs from a concrete wall

- **Where:** Palmetto Shack **1080:1** (beach bar, lot 1413:3) at (206, 814). The stream beside it
  (13.1 m wide, current 2.24 m/s, flowing north-west to the lake) starts at about (235, 825):
  `terrain_at` gives water at (240, 815) and none at (235, 828) or south of it.
- **What you see:** a palm, a thatched bar and pastel parasols on sand at 2 °C in falling snow; the
  stream comes out of a quay wall as if from a pipe.
- **Fix:** climate-aware leisure props (patio heaters, fire pits, no palms in cold climates; like the
  surfboards of report 2026-11-22 #7); draw a culvert mouth or spring at a stream's head.
- **Shot:** `f_stream_end_211_830.png`

#### F14. Parking fines are a sixth of the city's income

- **Where:** City Data, Cash flow, yesterday's books (day 80): income tax +$175,532, **parking fines
  +$52,550**, subway fares +$25,070, property tax +$24,969, business tax +$17,721.
- **Why it matters:** fines bring in more than property and business tax together; real cities make a
  few per cent of their income from fines. It may also push the player's budget.
- **Fix:** check the fine per ticket and how often a parked car is ticketed (`parking_fines` in the
  ledger); compare with the car count.
- **Shot:** `cd_economy_cash_flow.png`

### Polish

#### F7. Smoke plumes read as black tears in the sky at dusk and night

- **Where:** on the horizon from Signal Row (-925, -111) looking north-east and from Afterglow Mile
  (-619, -7); by day the same spots are grey plumes (`f_far_terrain_notch.png`, top right).
- **What you see:** dark V-shaped streaks against the lit evening sky.
- **Fix:** light the smoke from the sky / ambient (a little emissive at night) and fade it into the fog.
- **Shots:** `s08_signal_row_night.png` (top right), `s07_afterglow_mile_dusk.png` (top left)

#### F8. The walk eye can start inside a tree crown, or standing on the lake

- `camera {mode: walk, look_at: [-925, -111], yaw: 0.3}` put the eye inside a tree crown
  (`s08_signal_row_walk_night.png`). A `camera` with `look_at: [60, 700]` and no `mode` kept walk mode and
  stood the eye on the lake (pos y 1.7 over water).
- **Fix:** extend r14-photofix's push-out of parked cars to trees and props; move a walk eye placed on
  water to the nearest shore.

#### F9. Riders waiting at a red bunch up side by side and overlap

- **Where:** junction 1801 at (491.4, 505.3), Pylon Boulevard, day 81 07:15-07:25, following Suki Okafor
  **1494:5**.
- **What you see:** a dozen riders in one blob; a bike passes through a rider, two heads share a body.
- **Why:** r14's `room_ahead` keeps riders apart on the same line (within 0.9 m across), not side by side.
- **Fix:** spread queued riders across the shoulder / stop box; or a small render offset by citizen id.
- **Shot:** `s09_ride_suki_4.png`

#### F10. Saw-tooth skirts still on ParkHaus 8

- **Where:** ParkHaus 8 **515:3** at (-1006, 894). White triangular skirts hang off the downhill end; the
  retaining wall below steps in wedges. The rest of the Neon Market hill reads well.
- **Fix:** finish the old F4 for long buildings across a slope (wall to the ground, or step the building).
- **Shot:** `s06_parkhaus8_plinth.png`

#### F11. Route 88 / Neon Parade interchange: loose wall wedges and improvised ramp pads

- **Where:** (-1130, 400). Already open as "F3 interchange" in `todo.md`; listed for the shots.
- **Shots:** `s11_route88_interchange.png`, `s11_route88_interchange_b.png`

#### F12. Snowflakes near the camera are drawn huge

- A flake passing close to the camera becomes a big white six-pointed star across the view.
- **Fix:** cap a flake's size on screen or fade flakes within a few metres of the eye.
- **Shot:** `s01_arrival_overview_dawn_b.png` (left of centre)

#### F13. `drive {start}` replies with no street or speed limit

- The start reply had `street: null`, `limit_kmh: null` with `on_road: true` (at (400.4, 497.7) on Pylon
  Boulevard); the next call gave "Pylon Boulevard", 50.
- **Fix:** fill them from the start position.

## What looks good

- **Hillside (r15-hillside):** the Neon Market hill from r13's own camera (`s06_neon_market_hill_day.png`):
  terraces, a ring road and walls where the ground steps; no cliffs, no floating lot shelves.
- **Water (r15-water):** full-width streams between stone quays at (20, 580) and (211, 787), small waves,
  clear shallows; the jetty at the stream mouth reaches the water (`s03_stream_20_580_day.png`).
- **Landlocked lots (r15-lots):** 710 landlocked (55.1 ha), 632 empty, 128 with car access: r15's after
  numbers. Lot 446:5 is a meadow (`f_lot_446-5_meadow.png`).
- **Freight (r15-freight):** every depot at or under its room (Yard 21 19/20, Cargo Hub 2 23/23, the port
  48/48), stacks four high (`s04_cargo_hubs_day.png`). Lorries on one point: 11 on 3 points at 07:00
  day 81 (all standing in bays), 5 on 2 points of 129 lorries at 00:50 day 82.
- **Port crane:** boom raised with no ship (`s05_port_crane_close.png`).
- **Money (r15-money):** books balance to the cent; on day 82 78 % of income tax was withheld in town;
  net +$218,986 on day 81.
- **Night:** the ground reads away from the lamps; Afterglow Mile, Signal Row and the circus viaducts at
  night are the best views in the game.
- **Chase camera and drive mode:** smooth and fun; limits read 50 km/h.
- **Status yesterday figures:** 0 on this pre-r15 save until the first midnight (new fields), then
  1,423 bus riders / $2,519.5 fares; City Data showed the right 3,116 from history all along.

## Not filed again (known)

- Hunger and slow trips: 3,269 starving, 6,523 hungry at day 82 00:30; Suki Okafor's grocery run took
  573 min by car ("no space, left the car on the street"), her commute 199 min.
- Load freeze (7.1 s frame), F3 interchange (F11 above, for the shot), lighthouses inland.

## Report shots

| # | File | What |
|---|------|------|
| 1 | `b_try_rain_neon_3.png` | F1: a "storm" at 8 °C snows |
| 2 | `cd_economy_money_flow.png` | F3: $1.6B "in from outside" |
| 3 | `f_strip_mall_blank_box_573-3.png` | F5 |
| 4 | `f_stream_end_211_830.png` | F6 |
| 5 | `cd_economy_cash_flow.png` | F14 |
| 6 | `s08_signal_row_night.png` | F7; good: Signal Row at night |
| 7 | `s08_signal_row_walk_night.png` | F8 |
| 8 | `s09_ride_suki_4.png` | F9 |
| 9 | `s06_parkhaus8_plinth.png` | F10 |
| 10 | `s11_route88_interchange.png` | F11 |
| 11 | `s01_arrival_overview_dawn_b.png` | F12 |
| 12 | `s06_neon_market_hill_day.png` | good: the hill |
| 13 | `s03_stream_20_580_day.png` | good: quays and the jetty |
| 14 | `s04_cargo_hubs_day.png` | good: yards in their room |
| 15 | `s05_port_crane_close.png` | good: crane boom raised |

## Beautiful shots (`beautiful/`)

| File | Caption |
|------|---------|
| `afterglow_mile_dusk_river_bend.png` | Afterglow Mile at dusk: viaduct rings, the neon tower, the river bend |
| `jade_terraces_towers_at_dusk.png` | Jade Terraces' towers lighting up, the supertalls far off |
| `capsule_domes_at_dusk.png` | Capsule Corp's domes at dusk, the far towns lit on the hills |
| `circus_viaducts_and_river_at_night.png` | The circus viaducts and the river at night, a footbridge full of people |
| `lake_and_twin_supertalls_late_afternoon.png` | The Signal Wharf lake and the twin supertalls, late afternoon |
| `walled_quarter_supertall_at_night.png` | The Walled Quarter's supertall over the industrial belt at night |
| `signal_row_roundabout_in_snow_at_night.png` | Signal Row's roundabout park in the snow at night |
| `founders_campus_hilltop_ring.png` | Founders' Campus on its walled hilltop ring |

## Counts by severity

- wrong: 4 (F1, F2, F3, F4)
- unrealistic: 3 (F5, F6, F14)
- polish: 7 (F7-F13)

14 findings (F11 is already open in `todo.md`).
