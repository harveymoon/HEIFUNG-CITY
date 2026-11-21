# Photographer / inspector report, 2026-11-20 (r12-photographer)

Harvey's autosave (day 81, "pit", 16,768 people, River valley, Volcanic climate) loaded into my own instance
(branch `r12-photographer` at 37390d0c, port 5222) and run at 4x from 00:00 to about 11:00 on day 81. I used orbit
and walk views, pinned the lighting to night (21:30) and day (10:30-11:00), and set a snow storm
(`set_weather cloud 0.95`). Ids and positions below come from `inspect`, `find` and `terrain_at`. Positions
are sim metres `[x, y]`, so they can go straight into `camera {look_at}`. All screenshots are in
`O:/projects/Heifung_District/shots/r12-photographer/`.

## Summary

1. A lot of zoned land is dead: 776 of 2,802 lots (28%) are "landlocked" and 687 of those are empty (51 ha). Many of them have a front street and `car_access: true`, so the landlocked flag looks wrong, not the roads.
2. Freight looks broken up close: Yard 21 holds 133 boxes against room for 25, drawn 7 high, with tank containers inside each other and the crane beam through them. Up to 11 lorries sit on one exact point at Cargo Hub 2.
3. Geometry on slopes and at grade changes is rough: the Route 88 / Neon Parade interchange has a sliver ramp and loose wall slabs, building plinths end in saw-tooth skirts over drops, and lot ground pads spread in triangular shards.
4. Some styles turn up in the wrong place: 38 "lighthouse corner blocks" and "converted boathouses" in Signal Wharf, only 2 of them on the water. The port's quay crane boom reaches right across the river.
5. Smaller problems with inspector and API values: an empty alert message, an "unreachable" construction site that is open for business, "derelict" towers full of people, speed limits like 46.8 km/h, and a stale `status.lighting.hour`.

## Top 5 (ranked)

1. **Landlocked lots with street access, shown as dark dead ground** (wrong). Shots: `01_landlocked_waterfront_lot_446-5_dead_ground.png`, `02_cargo_hubs_landlocked_backlots_2281-2284.png`
2. **Container yard overstock drawn as overlapping stacks; lorries stacked on one point** (wrong). Shot: `03_yard21_overstock_tanks_overlap_crane_clips.png`
3. **Route 88 / Neon Parade interchange: sliver ramp through the overpass, loose wall slabs** (wrong). Shots: `07_route88_interchange_floating_wall_slabs.png`, `08_route88_ramp_sliver_through_overpass.png`
4. **Foundation plinths on slopes end in saw-tooth skirts hanging over drops** (wrong). Shot: `09_parkhaus_515-3_plinth_sawtooth_over_drop_night.png`
5. **Lighthouse and boathouse styles on inland homes** (unrealistic). Shot: `05_lighthouse_blocks_on_inland_homes_signal_wharf.png`

## Round-12 work already in flight (to avoid double reports)

- **r12-hunger:** I saw it, so I am not reporting it as a finding: `starving_citizens` 3,197, `hungry_citizens` 7,403 of 17,339, and `food_sellers_empty` growing from 69 to 125 over the morning. Top concern: food, with 62,035 complaints this week.
- **r12-odd-lots:** the Null Pointer Karaoke lot (55:3 / lot 1115:1) still looks as Harvey described (shot 06). Finding 1 overlaps with this work: many grey "filler" slabs are landlocked lots. The jetties ending on mud (finding 7) belong there too.
- **r12-grime:** soft edges may cover some of finding 10 (hard-edged shard pads), but not the shard shapes themselves.
- **r12-yards:** nothing seen that belongs only to it.
- **Earlier work:** the depot overstock was fixed by r11-box-stock, which reduces old saves over time. On this save the Port of Saltmarsh still holds 1,442 boxes against capacity 60. Finding 2 covers how the overstock is drawn, which is a separate problem.

## Findings

### 1. Landlocked lots that have a street: 687 empty dead lots (wrong)

- **Where:** across the city. Best example: lot **446:5**, Commercial, 3,537 m², waterfront with 181 m of shore, centred at about (103, 750) beside the lake in the Signal Wharf / Jade Hive area. Its card reads `landlocked: true`, `usable: false`, but also `car_access: true`, `car_segment: 2157`, `front_segment: 2157`, `entrance: (65.0, 753.9)`, depth 56 m, width 182 m, `sub_zone_auto_pick: neighbourhood`.
- **Numbers:** `find lots` returns 2,802 lots. 776 are landlocked (57.7 ha): 340 Residential, 189 Commercial, 112 Industrial, 64 Park, 50 Leisure, 21 Civic. 687 of them have no building (51.3 ha). Of the 40 largest empty ones, **26 report `car_access: true` with a front segment**, and 7 are waterfront. Other examples:
  - 1472:3 Res 3,518 m² at (-599, 481), segment 122
  - 931:5 Ind 2,609 m² at (-181, 320), segment 2102
  - 2046:3 Leisure at (-1259, 675), segment 798
  - 2077:3 Com at (-1020, 952), segment 840
  - 124:3 Res at (246, 742), segment 1797
- **Back lots:** the cargo hub frontage at (-420..-240, -360) shows the same problem as a layout. A row of shop lots faces the street, and behind it a second row of Commercial lots has no street of its own: 2281:1, 2282:1, 2283:1, 2284:1 (1,007 m² each), 2276:1, 2275:1, 2274:1, 2273:1, 2261:1, 2260:1. They draw as a long dark strip between the yards and the grass (shot 02).
- **What you see:** big flat dark-grey or grey-paved slabs next to busy streets and along the lake. Nothing ever grows on them, and they are near-black at night.
- **Why it's wrong:** the lot has a road at its front, but the growth model never uses it. In other places the zone depth makes rear lots that cannot be used, and the city fills with dead ground.
- **Shots:** `01_landlocked_waterfront_lot_446-5_dead_ground.png`, `02_cargo_hubs_landlocked_backlots_2281-2284.png`
- **Fix direction:** compare the test behind `landlocked` with the one behind `car_access` / `front_segment`. One may need a minimum frontage, or the flag may be stale after road edits. Re-run it when roads change. When slicing zones, merge rear slivers into the front lot, or leave them unzoned/green instead of zoned-but-dead. Draw unusable lots as grass, not paving.

### 2. Freight: overstocked yard drawn as overlapping stacks, and lorries stacked on one point (wrong)

- **Where:** **Yard 21** (1472:1, freight yard, lot 2814:11) at (-347, -446), next to Cargo Hubs 1 and 2.
- **Numbers:** the depot holds `stored: 133` against `capacity: 25` and `ground_capacity: 75`, with `going_out: 133`. The Port of Saltmarsh (111:1) holds `stored: 1442` against `capacity: 60`.
- **What you see (shot 03):** stacks 6-7 high. Tank containers (the round ones) sit inside each other and lie across 40 ft boxes. The yard gantry's top beam passes through the top tier, and one box face draws see-through.
- **Lorries:** `find cars` showed **11 lorries at exactly (-313.9, -406.6)**, Cargo Hub 2's (1907:1) loading point, all at 0 km/h. Ids include 178:797, 264:1141, 324:1063, 626:329, 647:229 and 689:255. There were 3 more at (-347.5, -414.2) (Cargo Hub 1: 35:909, 1251:57, 1252:77) and 3 at (750.0, 360.4) (111:1145, 445:905, 1210:119). Truck 647:229 was then "driving back empty" from Hub 2 to Hub 1, 51 m apart, by a road loop of about 250 m, carrying `fuel` with amount 0.
- **Why it's wrong:** boxes that overlap and a crane that clips through the stack look broken. Lorries parked inside each other are the same class of bug as the old car-in-car one.
- **Shot:** `03_yard21_overstock_tanks_overlap_crane_clips.png`
- **Fix direction:**
  - Cap the drawn stack height at the crane's lift height. Draw the surplus as a separate block, or turn the surplus away (r11's overflow yards).
  - Snap tank frames to slots like dry boxes.
  - Give depot loading bays a queue, so the next lorry waits at the kerb behind instead of on the same point.
  - Check why lorries run empty between two hubs next to each other.

### 3. Route 88 / Neon Parade interchange: sliver ramp and loose wall slabs (wrong)

- **Where:** about (-1130, 400), Signal Row / west edge.
  - Neon Parade avenue pieces **2229** ((-1125.3, 489.1) to (-1125.8, 351.8)) and **2230** (to (-1126.0, 289.4)): level `cutting`, 2+2 lanes.
  - Route 88 ramps **2226** and **2227** ((-1095.0, 392.3) to (-1103.9, 382.7)): `cutting`, one-way, 1 lane.
  - Ramps 2232 and 2233 ((-1144.5, 323.2) to (-1161.2, 350.9)): at grade.
  - Highway pieces 2203 and 2239 at y 411 / 429.
- **What you see:** a ramp comes up as a thin sloped slab through the overpass abutment, and a tow truck drives along it (shot 08). Short concrete retaining-wall pieces stand alone on the grass beside the western ramps, and the cutting walls cross each other (shot 07).
- **Why it's wrong:** the walls are not connected to the road they should hold back, and the ramp deck is too thin and clips the bridge.
- **API gap:** `inspect {segment}` returns `levels`, `heights`, `inner_heights` and `grade_crossings` as null for these pieces, although REMOTE.md lists them, so I could not check the heights.
- **Shots:** `07_route88_interchange_floating_wall_slabs.png`, `08_route88_ramp_sliver_through_overpass.png`
- **Fix direction:**
  - Where a cutting ramp meets an at-grade ramp next to an overpass, check how the cutting walls are trimmed and capped at each end.
  - Wall pieces shorter than a few metres, or not next to their road, should not be drawn.
  - Restore the height fields in `inspect {segment}`.

### 4. Plinths on slopes end in saw-tooth skirts over drops (wrong)

- **Where:** **ParkHaus 8** (515:3, car park, 3 decks, lot 2030:3) at (-1006, 894), and the night market Mirage Den next to it (1895:1, lot 2032:3), in Neon Market / Afterglow Mile.
- **Numbers:** `terrain_at` at lot 2030:3's corners gives `ground_height` 36.7 everywhere and `natural_height` 40.7-48.7, slope up to 0.5. Lot 2032:3 is graded in two steps, 31.7 and 36.7, against natural 37.0-43.7.
- **What you see:** the garage stands on a tall pale plinth whose lower edge is a row of jagged teeth hanging over a steep dark drop (shot 09, night and snow). The same thing shows on the hillside round (-599, 481), where the Ember Towers / Habitat 21-A blocks and a parking garage sit on podiums with ragged bottoms.
- **Why it's wrong:** the skirts read as torn geometry, not as retaining walls.
- **Shot:** `09_parkhaus_515-3_plinth_sawtooth_over_drop_night.png`
- **Fix direction:** run the skirt down to the terrain under each edge vertex, with a straight bottom edge at the lowest ground under that edge plus a little extra depth. Or draw a retaining wall plus an earth slope instead of a hanging curtain. Where the drop is more than a couple of floors, step the plinth.

### 5. Lighthouse corner blocks and converted boathouses far from the water (unrealistic)

- **Where:** Signal Wharf ("The Promenade" story, `harbour pastel` look). Within 450 m of (-250, 680) there are **38 such buildings, and only 2 are on waterfront lots**: 680:1 Marrow Towers and 690:1 Tide Towers at x = 26. Inland examples:
  - Lighthouse blocks: 758:1 Onyx Terrace (-288, 639), 742:1 Marrow Heights (-271, 535), 734:1 Cobalt Towers (-156, 590), 768:1 (-136, 584), 197:3 (-44, 639)
  - Boathouses: 828:1 Cobalt Towers (-178, 533), 730:1 (-62, 601), 804:1 (-16, 625)
- **What you see:** red-and-white striped lighthouse towers with lanterns (lit at night) on terraces two or three streets from the shore, seven or more in one view.
- **Shot:** `05_lighthouse_blocks_on_inland_homes_signal_wharf.png`
- **Fix direction:** allow `lighthouse corner block` and `converted boathouses` only on lots with `shore.waterfront` (or within about 40 m of the shore), allow at most one lighthouse per stretch of shore, and give inland lots another pastel style.

### 6. Port of Saltmarsh: quay crane boom spans the whole river (unrealistic)

- **Where:** Port of Saltmarsh (111:1, lot 930:7) at (-345, -44), berth (-398.0, -68.2), freight route to the open sea every 240 min, `ships: []`.
- **What you see:** the ship-to-shore crane's boom runs straight across the river and ends over the far bank. There is no ship, and the stacks stand 5-6 high in single rows.
- **Shot:** `04_port_saltmarsh_crane_boom_spans_river.png`
- **Fix direction:** limit the boom outreach to about 50-60 m, or to half the water width at the berth. Raise the boom when no ship is alongside. Point the crane at the berth.

### 7. Jetties that end on the mud bank (wrong, belongs to r12-odd-lots)

- **Where:** Signal Wharf at about (-359, 591) to (-330, 600). The karaoke lot's jetty and the wooden deck next to it both stop on the brown bank, short of the water.
- **Shot:** `06_signal_wharf_karaoke_lot_jetties_on_mud.png`
- **Fix direction:** extend a jetty to the water line, or don't place one when the shore point is dry.

### 8. Ground pads in triangular shards; buildings float with no street (polish / unrealistic)

- **Where:**
  - Founders' Campus (district 6 Iron Commons, centroid (1203, 633)), viewed at (1220, 660)
  - Downtown Signal Row (district 7, centroid (-925, -111))
- **What you see:** beige paving pads cut in thin radial triangles that spill into the grass. Blocks stand in open beige fields with no street in front, and on the hillside the pads are tilted under the buildings. Downtown, the pads end in jagged points along the roundabouts, and an empty grass disc fills one roundabout.
- **Shots:** `10_founders_campus_shard_lot_pads_floating_blocks.png`, `12_signal_row_downtown_jagged_ground_pads.png`
- **Fix direction:** the radial slicing leaves sliver lots. Merge slivers below a minimum angle or width, clip the pads to the lot, or draw only the building's own apron. r12-grime's soft edges will hide the hard edge but not the shapes.

### 9. Merged warehouse with a skewed roof (polish)

- **Where:** **Meridian Fab 13** (293:3, `rusted warehouse`, 3 floors, merged over lots 556:1, 557:1 and 566:3) at (-261, 96).
- **What you see:** one huge brown roof plane stretched over the irregular merged footprint. The ridge runs off to a corner, so the roof looks like a wedge. The same shot shows a big dark paved lot round a small building across the road, and grey fragments on the grass at the bottom right.
- **Shot:** `11_meridian_fab_13_skewed_roof_and_dark_paved_lot.png`
- **Fix direction:** for merged footprints, roof the bounding rectangle's main part (or each original lot) separately, and use a flat or saw-tooth roof for non-rectangular halls.

### 10. Construction site "unreachable" but already open, and the alert message is blank (wrong)

- **Where:** **Clucky's Drive-Thru** (2010:3, fast-food drive-through, high tier, land value 0.81) at (-671.1, 105.4), lot 632:1 (159 m²), under the Iron / Copper Circus viaducts in Jade Terraces.
- **Numbers:**
  - The lot reads `car_access: true`, `front_segment: 1861` (Cinder Street, at grade, two-way) and `landlocked: false`.
  - `under_construction` reads `blocked: "no_deliveries"` ("delivery trucks can't get here"), progress 0.16, 1 of 4 deliveries.
  - Yet the business shows `closed: false`, `staffing: 1.0` and 3 jobs, with no stock.
  - The `construction_unreachable` alert (rank 16) has `"message": ""`.
- **Shot:** none; it is hidden under the viaduct ring. Use `camera {look_at: [-671, 105], distance: 110}`.
- **Fix direction:**
  - Fill in the alert's message.
  - Don't staff or open a business before its building is finished.
  - Find why deliveries can't route to segment 1861: possibly the viaduct above blocks the lane graph, or it's the lot's access point.

### 11. "Derelict" towers full of people (unrealistic / naming)

- **Numbers:**
  - `find buildings` shows 365 with `derelict: true`; the alert says 355.
  - 124 of them are residential and 119 have residents: **5,916 people, about 35% of the city**, in 3,343 homes.
  - 246 derelict buildings have workers.
- **Examples:**
  - Habitat 8-B 57:1 (455, 685): 20 floors, 60 homes, 140 residents
  - Harbor Towers 312:1 (398, 525)
  - Tide Hive 8:1 (286, 706)
  - Harbor Heights 1:1 (401, 664): condition 0.09, 120 residents, style `the Stacks`
- **Why it's odd:** in plain language "derelict" means abandoned, but these are the most crowded blocks in the city.
- **Fix direction:** if this means "run down", rename it (dilapidated / condemned) in the inspector and alerts. If derelict should mean empty, decay should move people out first.

### 12. Long queues at roundabouts (wrong-ish, traffic)

- **Numbers:** the `stuck_vehicles` alert reported 42-47 vehicles standing still for over 180 s. Queues of 15-53 cars at roundabouts round (0, 657), (-40, 571) and (-66, 585) in Signal Wharf show "giving way to the roundabout", stalled 403-407 s. Examples: queue cars 235:1079, 156:1163 and 165:875. The head of a jam moves between cars, so it is not a deadlock, but the roundabouts there are overloaded.
- **Fix direction:** check the roundabout entry rules (exit_full / give_way) at these single-lane circuses, and add a second circulating lane or signals.

### 13. Inspector speed limits like 46.8 km/h (polish)

- **Numbers:** `inspect road` shows `speed_limit_kmh` values of 46.8 (avenues, streets, ramps), 28.8 (Arcade Circus), 29.47 and 45.64 (ramps), and 38.44 (Iron Circus). Highways show 90.
- **Fix direction:** these are m/s × 3.6 (13 m/s = 46.8). Round to the nearest 5 or 10 km/h, or set the limits in km/h.

### 14. `status.lighting.hour` is stale (polish, API)

- At 00:06 and again at 02:17, with `follow_clock: true`, `status.lighting.hour` was 14.0. Report the clock's hour when following the clock.

### 15. Transit barely used (unrealistic / balance)

- **Numbers:** 5 bus routes and 25 buses, with 211 riders today in a city of 17k. Fares 524.5 against upkeep 5,101.5 per day. Rail: one station (Court Station 1471:1 at (-950, -460)), 13 riders, upkeep 700 per day.
- **Fix direction:** check mode choice against parking pain (666 drivers parked on the street for lack of spaces) and stop coverage. The station looks great (shot 14) but nobody uses it.

### 16. Frame hitches during play (wrong, performance)

- `frame_stats` max frame: 12.7 s right after the load, and **36.5 s** later at 4x around 10:00. p95 was 66-74 ms, about 15-31 fps on High with Harvey's game sharing the GPU. One screenshot was never written during a hitch (it was retried).
- **Fix direction:** check `frame_stats.slow` / `long_frames` on this save at 4x. It may be r11-adopt-frames territory.

### 17. Night ground is pitch black away from lamps (polish)

- Away from street lamps, terrain and the landlocked slabs (finding 1) render near-black at night. Big black holes read as missing geometry in the night overviews.
- **Fix direction:** raise the night ambient on terrain slightly, or add moonlight fill.

## What looks great

- `13_GOOD_iron_copper_circus_viaducts.png`: the stacked Iron / Copper Circus rings over parks and plazas at (-650, 200) read as a real megacity interchange.
- `14_GOOD_court_station_and_park.png`: Court Station's platform canopy, the arcade and the zoo / park behind it at (-950, -462) are clean and believable.
- `15_GOOD_walk_pylon_boulevard_street_level.png`: street level on Pylon Boulevard (399, 506): grimy towers, overhead wires, buses, taxis and a delivery robot. Walk mode really sells the city.

## Counts by severity

- broken: 0
- wrong: 8 (findings 1, 2, 3, 4, 7, 10, 12, 16)
- unrealistic: 4 (findings 5, 6, 11, 15)
- polish: 5 (findings 8, 9, 13, 14, 17)

17 findings in all (finding 7 overlaps r12-odd-lots).
