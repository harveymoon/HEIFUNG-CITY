# Leftovers sweep and shallow-systems audit (2026-11-21, r13-leftovers)

Round 13 is a cleanup round. This sweep checks every "Left:" note agents wrote at the end of their todo.md DONE entries (111, extracted by the coordinator) and the 2026-11-20 code audit (2 items) against today's code. It also lists the shallow placeholder systems that deeper systems now lean on.

**Counts (113 rows):** CLOSED 32 · FIXED 7 · FILE 31 · DROP 26 · handed to another round-13 branch 17 (trip-review 9, r13-detail 5, r13-catalogue 3).
FILE rows are grouped into 14 todo items under "Round 13 sweep: filed leftovers" in todo.md. Several rows feed the same item.

What the classes mean:
- **CLOSED**: already done by a later round, or already tracked by an open todo item (the row says which).
- **FIXED**: fixed on this branch, with a test where it makes sense.
- **FILE**: real but too big for this sweep; now a todo item.
- **DROP**: not worth doing (the row says why).
- **handed to**: in the area of another round-13 agent (trip-review: citizen routing, traffic movement, bike and pedestrian paths; r13-detail: close-up render detail, yards, props; r13-catalogue: goods and business-kind facts).

"todo:N" is the line in todo.md before the r13-docs archive (2026-11-21). Finished entries now live in `docs/archive/todo_done_2026-11-21.md`; search there by the quoted note.

## Fixes on this branch

| What | Where | Test |
|---|---|---|
| A save whose land-value grid cannot be read (`n: 0`, a short `values`, `cell: 0`) now loads with a fresh grid, where it used to panic on the first read | `LandValue::sane`, `World::after_load` | `a_broken_land_value_grid_loads_fresh` |
| A compressed save may unpack to at most 200x its size (64 MB to 2 GB); past that, the load fails with an error instead of running out of memory | `save_text`, `max_save_text` | `a_save_that_unpacks_too_far_is_refused` |
| Garbage trucks and billionaire-convoy cars say what they are doing in the car inspector and labels, not "passing through" | `car_journey`, `car_label` | `a_garbage_truck_is_not_passing_through` |
| Incinerators are rows of the Power page's plant table (their MW were in the supply but in no row) | `plant_rows`, `GridAsset::Incinerator`, `ui/detail.rs` | `an_incinerator_is_a_row_in_the_plant_table` |
| The Companies window's corp rules and the park actions borrow the world mutably only on a change or click. Before, they did it every frame the window was open, which ran the sim in the frame (r7-sim-thread LEFT 1) | `ui/estate.rs`, `ui/parks.rs` | (UI) |
| Plural trades take no article in lot histories ("artists' studios", not "an artists' studios") | `names::a_or_bare`, `lot_memory.rs` | `plurals_take_no_article` |
| Harbour Pastel boathouses go up only on lots that meet the water | `buildings/choose.rs` | `no_boathouses_inland` |
| The Music slider is greyed out and says "none yet" (the game has no music; it looked live) | `audio.rs` | (UI) |

## Every note and audit item

| # | Where | Note | Class | Finding |
|---|---|---|---|---|
| 1 | todo:191 | [feat/camera] Owning several homes has no data | CLOSED | `holdings_of` lists every owner record (`properties_of`), so a rich person's second homes (`rich.rs second_home_for`, [sweep-sim]) get OWNS pins. |
| 2 | todo:209 | [feat/look] Street lamps cast no light, no wet roads | CLOSED | [fix/haze]: lamp light by tier and wet roads in rain (todo "Look-dev leftovers"). |
| 3 | todo:210 | [feat/identity] Metro bores unchanged | DROP | Metro tunnels are open-topped channels on purpose so the Underground view looks in (`nd_procgen::metro::tunnels` "no roof"). |
| 4 | todo:228 | [fix/flags] Coplanar tail in other styles | FILE | Newer families test themselves; the old styles' tail is still only in the ignored `survey_coplanar`. Filed (low). |
| 5 | todo:229 | [fix/flags] Out-of-town tow truck loops 1-2.5 h through jams | handed to trip-review | Traffic movement. A wreck is craned off after `TOW_GIVE_UP` (a day) as a fallback. |
| 6 | todo:231 | [feat/kerbs] Sharp corners no cut; one-way ramps no pavements | CLOSED | [fix/review] curb cuts at sharp corners; ramp streets without pavements by design. |
| 7 | todo:275 | LLM dialogue lines for spotlight citizens | CLOSED | [r5-talk]: talking to a citizen through the committee's LLM backend. |
| 8 | todo:303 | Save-load-continue determinism | CLOSED | `tests/determinism.rs a_saved_city_loads_and_goes_on_exactly_as_the_running_one`. |
| 9 | todo:316 | [fix/sweep-visual] Parclo/cloverleaf, street-over-highway; pad gaps on steep junctions | CLOSED | Already the open item "Rework interchanges as an interchange stamp tool". |
| 10 | todo:319 | [fix/sweep-sim] HQ crown on needle towers; nominal deposits | FILE | Crown done (`hq_crown_reads_on_a_needle_tower`). Deposits are still 0.4% of the price with no loan (`corp.rs DEPOSIT_SHARE`). Filed. |
| 11 | todo:320 | [fix/sweep-sim] Owned buildings skip densify | CLOSED | Developers' and builders' own buildings densify now (`estate/growth.rs`, [pt-growth]/[companies]). Private owners holding out is intended. |
| 12 | todo:343 | No lot under the ramps | CLOSED | [r5-layered] undercroft bays (parking, stalls) under raised roads. |
| 13 | todo:349 | [r4-style] Cyberpunk family, street props, Lantern/Skyline/Glass looks, HK styles | CLOSED | Lantern, Skyline Luxe and Glass Campus looks done in round 4. Street props handed to r13-detail. |
| 14 | todo:350 | [r4-viewer] No per-building camera presets; turntable alone | DROP | Nice to have. Nothing depends on it. |
| 15 | todo:358 | [r5-ferry-islands] Islands: emergency units, residents' cars, one crossing, next boat, free extra sailings, Ironbank stranded | FILE | Filed as "Islands and ferries carry their real traffic". |
| 16 | todo:360 | Rest of "Layered city" | CLOSED | Same as #17 (r5-layered's own LEFT), filed there. |
| 17 | todo:369 | [r5-layered] Many levels, under-deck parking spaces, bays under zoned lots, camps policy, mood | FILE | Filed as "Layered city, phase 2". |
| 18 | todo:373 | [r4-crossings] Riders ride steep ramps as drawn; button rule not reopened | handed to trip-review | Bike/pedestrian paths at crossings. |
| 19 | todo:376 | [r4-horizon] Far towns are placeholders; draped ribbons; flat map far ground | CLOSED | Covered by steps 3/4 ("Planet", "Regions") of the open big-world item. |
| 20 | todo:377 | [r4-tiles] Districts not paintable on bought land; Buy land not in Build window; second ring; ring regenerated whole | FILE | Second ring and far land are covered by big-world steps 3/4. Districts on bought land and a Build-window entry are filed. |
| 21 | todo:383 | [r4-ramp] Hilly-land ramp check uses ground plus level | DROP | Off by well under a metre on hills, and no flag about it since. |
| 22 | todo:389 | [r4-metroloop] Metro trips planned at 23:xx after the last train; under-deck ambiguity | handed to trip-review | Citizen routing. |
| 23 | todo:391 | [r4-timetable] Where commute time goes; job/home choice should weigh the commute | FILE | Filed with the job-side growth item. |
| 24 | todo:393 | [r4-timetable] Air taxis fixed hours; Commuter preset buses vs depot; ferry waits untimed | FILE | Ferry waits folded into the islands/ferries item. Air taxi hours and the Commuter depot note dropped (by design / play advice). |
| 25 | todo:395 | [r5-stairs-hillside] Busy street/hillside not in the stairs choice; spiral 0.3 m into lot; stairs not slower | handed to trip-review | Pedestrian paths and walk times. |
| 26 | todo:402 | [r5-sound2] No lightning flash to match thunder | FILE | Small render feature (ThunderClock already returns `Strike`s). Filed (low). |
| 27 | todo:411 | [r6-perfect-city] Unemployment creep; offshore islands unused; bus share ~1% | FILE | Filed as "Job-side growth". |
| 28 | todo:417 | [r6-architects] Housing block has no homes; chapel site rule; thumbnails | FILE | The housing block having no homes is the real gap. Filed (low). |
| 29 | todo:418 | [r7-airport] No free site in a 38k city; field on a cliff; rail/metro passengers counted not simulated; flyers statistical; GSE static | FILE | Shallow: passengers by rail/metro and residents flying away are counts, not trips. Filed. |
| 30 | todo:420 | [r8-containers] Lorry lots take lorries off the map; reach stacker; deck 24 boxes | DROP | Cosmetic: a follow note is shown, and the deck limit is a draw budget. |
| 31 | todo:424 | [r10-chunk-hitch] 64x frames over 100 ms; fire redraw per tenth; slow settled chunk; draw calls +8% | CLOSED | [r11-adopt-frames] brought day-81 64x to 0 frames over 100 ms. The rest is in r11's notes. |
| 32 | todo:425 | [r11-adopt-frames] (Method note: how frames were measured) | DROP | Describes how frames were measured; nothing left to do. |
| 33 | todo:425 | [r11-adopt-frames] 38k batch frame ~28 ms in sync_citizens | FILE | Folded into "World copy and batch cost at 38k" (perf, low). |
| 34 | todo:425 | [r11-adopt-frames] The world copy (130/290 ms) caps max speed | FILE | Filed as "World copy and batch cost at 38k". |
| 35 | todo:435 | [r7-citizen-story] Click a trip to draw its path; wanders unlogged; slow trips; tickets | FILE | Clicking a trip in the diary to draw its path is filed (low). Slow trips are trip-review's; the rest dropped. |
| 36 | todo:438 | [r8-parks] Walks are spokes per lot; seam details; one car park; burst of park sites on old load | FILE | Filed (low) as "Parks as one promenade". |
| 37 | todo:442 | [r7-sim-thread] (1) windows taking &mut world every frame; (2) batch frames; (3) world copy; (5) save-load exact | FIXED | (1) Corp rules and park actions now write only on a change or click; skyline, lines, committee and the metro tool already only wrote on a click; the junction editor pauses time by design. (2) by r11, (3) filed (#34), (5) closed (#8). |
| 38 | todo:443 | [r9-stacks-look] Buried units have no door; roofs not linked down; no galleries | FILE | Filed (low) with the Kowloon look polish. |
| 39 | todo:446 | [r10-worn-paths] No shortcuts in the walk sim; tracks hide under steep pads; yards don't wear | handed to trip-review | Pedestrian routing (desire paths need walkers off the graph). |
| 40 | todo:447 | [r12-grime] Wear baked at rebuild; fixed 3.2 m streaks; convex-only fringe | handed to r13-detail | Close-up detail ("Unify the close-up detail passes"). |
| 41 | todo:448 | [r12-yards] Window things behind parapets; step offset; pastel tarp; no balcony things | handed to r13-detail | Yards and props. |
| 42 | todo:449 | [r11-lot-memory] Ghost stylised; no lot inspector; "an artists' studios" | FIXED | Grammar fixed (`names::a_or_bare`: plural trades take no article). The ghost and the lot inspector dropped (polish). |
| 43 | todo:454 | [r10-walk-kerb] 3 small walk networks on his save (Ember Sands etc.) | handed to trip-review | Pedestrian connectivity. |
| 44 | todo:455 | [r10-port-boxes] Most stored boxes never drawn; stock not kept near capacity | CLOSED | [r11-box-stock] stock/room pass (`world/box_stock.rs`). |
| 45 | todo:457 | [r11-parking] Parked cars never block a lane; last resort at kerb line; Varga Circus jam | handed to trip-review | Traffic movement. |
| 46 | todo:460 | [r11-box-stock] (a) yard room comes from the renderer; (b) tiny yards lose money; (c)-(e) | FILE | (a) is a shallow coupling: the sim reads `Containers::drawn_room`, so headless and in-game runs differ. Filed. (b)-(e) dropped. |
| 47 | todo:461 | [r12-hunger] (a)-(e): lost bookings, grocery imports, few food openings, no hunger layer, derelict homes never repaired | FILE | Hunger map layer and derelict homes never repaired are filed. The rest are observations. |
| 48 | todo:462 | [r12-odd-lots] Doubled vertices, slivers, yard spill; terrain shows through flat floors on hillside lots | FILE | Filed as "Hillside lots: cut the terrain to the pad". The rest dropped (harmless, or reshapes lots under buildings). |
| 49 | todo:490 | Grime toggle follow-up | handed to r13-detail | Close-up detail. |
| 50 | todo:498 | [r4-hunger] Remaining hungry are drivers on very slow trips; junction 943 phase | handed to trip-review | The known slow-traffic issue. |
| 51 | todo:512 | [fix/haze] Bevy Atmosphere; building-lot lamps | DROP | Atmosphere needs physical light units (a whole relight). Lot lamps are props (r13-detail's area). |
| 52 | todo:526 | [r5-laptop-nav] Touch-screen pinch not read; hi-res wheel pans in Auto | DROP | Touchpad pinch arrives as Ctrl+wheel and is read. Only a touch-screen laptop would need it, and the Zoom setting covers the wheel. |
| 53 | todo:530 | r12-hunger raised tick p99 on his save | CLOSED | Measured on this sweep's probe (see Probe): p99 is within run-to-run noise of main. The extra work is real food demand. |
| 54 | todo:531 | Headless runs lose half his population to hunger | CLOSED | [r12-hunger] fixed the exodus. |
| 55 | todo:554 | [r7-cablecar] Transfers with other lines; funicular; riders not visible | FILE | Filed (low) with the transit polish. |
| 56 | todo:580 | [fix/pt-traffic] RVs are records, not vehicles, so nothing tows them | DROP | Needs RVs as vehicles; little gameplay for the cost. |
| 57 | todo:623 | [feat/pt-ux] Tunnel ribbons hide behind buildings; platform figures are capsules | FILE | Filed (low) with the transit polish (Underground view). |
| 58 | todo:625 | [feat/pt-ux] No big tow truck for RVs; heli crew is the base's first staff | DROP | Tied to #56. The heli crew is harmless. |
| 59 | todo:626 | [feat/pt-ux] A planned (unbuilt) road is bulldozed whole | DROP | An unbuilt plan has no junction pieces, and undo restores it. Low value. |
| 60 | todo:627 | [feat/pt-ux] No U-turn option in the junction editor | CLOSED | [round1/r2-traffic] U-turns open with the left-turn arrow, which the editor controls. |
| 61 | todo:635 | [fix/pt-visual] Lot pads/streets cut 13-40 m into a mountain; streets grade slopes | FILE | Folded into "Hillside lots: cut the terrain to the pad". |
| 62 | todo:725 | [w1/start-saves] step_citizens/step_traffic cost; tow queue; 20-50k untested | CLOSED | [r6-perf] re-profiled 15k/38k; [round1/r2-traffic] tow backlog. |
| 63 | todo:725 | [w1/start-saves] Sim on the render thread; shift-start ticks; chunk regen; midnight deliveries | CLOSED | [r7-sim-thread], [r10-chunk-hitch], [r6-perf]. |
| 64 | todo:747 | [r5-drive] Traffic lane changes ignore the player's car; no engine sound | handed to trip-review | Traffic movement. Engine sound dropped. |
| 65 | todo:755 | [feat/capsule-look] Capsule industry is not a look | DROP | By design: Tech industry gives the high-tech look. |
| 66 | todo:756 | [feat/chase-cam] Footbridge deck can hide the walker | DROP | Minor camera case. |
| 67 | todo:769 | [w1/roads] Tow-job queue not touched | CLOSED | [round1/r2-traffic] (tow backlog 2,358 to 180). |
| 68 | todo:771 | [round1/r1-roads] Concave joins refused | CLOSED | The note sits on an open item ("Backland often refused ... concave"), which already tracks it. |
| 69 | todo:777 | [w1/roads] Parclo/cloverleaf loop ramps | CLOSED | Open item "interchange stamp tool". |
| 70 | todo:799 | [r5-talk] Not tried on a real model; keyword stand-in; no trip log; no streaming | DROP | Mock-only by rule. The keyword stand-in is the offline fallback by design. |
| 71 | todo:809 | [r5-megablocks] Only ~25% of trips stay inside; residents not preferred in hiring; ... | FILE | A resident preference in hiring is folded into "Job-side growth". The rest dropped. |
| 72 | todo:822 | Walk-in sales/repairs/haulier fees from outside; landlords one pool; recent-move rings empty after load | FILE | Filed as "Money still crosses the town edge". |
| 73 | todo:827 | Mega-mall cramped; malls slow; no "mall" kind; no mall summary | handed to r13-catalogue | Business kinds. The cramped mega-mall was fixed by p2-look-leftovers. |
| 74 | todo:828 | Forced mall takes any shop site; signs are colour boards; one-row car park | DROP | Dev command only. Signs are r13-detail's. |
| 75 | todo:830 | New looks and per-story mansions (Round 4) | CLOSED | Done in round 4 (r4-brick, garden, oldstone, harbour, glass, lantern, patchwork, skyline, solarpunk). |
| 76 | todo:832 | [r4-solarpunk] Street props, bike lanes, a solarpunk park | handed to r13-detail | Street props. |
| 77 | todo:834 | [r4-harbour] Boathouses inland; piers vs depth; no shops on terraces; refitted towers keep floors | FIXED | Boathouses now only on lots that meet the water (`choose.rs`, test `no_boathouses_inland`). Refitted towers filed (#79). The rest dropped. |
| 78 | todo:836 | [r4-brick] No construction look; flat sodium pools; pub/union hall don't change businesses | handed to r13-catalogue | Pub/hall effect on businesses is catalogue facts. The visual bits go to r13-detail. |
| 79 | todo:838 | [r4-garden] Facelift turns towers into houses (sim keeps floors); close heads; strip malls | FILE | Filed as "Looks painted over towers". |
| 80 | todo:840 | [r4-oldstone] No way to keep old Historic look in a custom district; clock hands fixed | DROP | Clock hands would need a per-frame mesh. The custom-district menu is rare. |
| 81 | todo:842 | [r4-glass] Quad screen reads weakly; no street props | handed to r13-detail | Props. |
| 82 | todo:844 | [r4-lantern] Strings don't span roads; refitted towers; night market menu | FILE | Refitted towers folded into #79. The rest dropped. |
| 83 | todo:847 | [r4-patchwork] Slopes; stilts vs depth; shantytown upgrade path; camps under raised ways | FILE | Filed as "Shantytown upgrade path". |
| 84 | todo:849 | [r4-skyline] Window lights follow sim clock; residents fall with tier; sky villa looks like an office | DROP | By design or taste. |
| 85 | todo:852 | [r4-brick] The other stories' estates | CLOSED | `district::look_mansion` covers every look but Kowloon/Neon (see #88). |
| 86 | todo:852 | [r4-oldstone] The other stories' estates | CLOSED | As #85. |
| 87 | todo:856 | [r4-solarpunk] The other stories' estates (Glass, Skyline, Patchwork, Kowloon/Neon) | CLOSED | Glass, Skyline and Patchwork are done. Kowloon/Neon is in #88. |
| 88 | todo:857 | [r4-patchwork] Skyline Luxe's and Kowloon/Neon estates | FILE | Skyline is done. Kowloon/Neon have no estate style; filed (low). |
| 89 | todo:874 | Malls; backland; landlords' pool; builders' vehicles; cabs "passing through" | CLOSED | Malls and backland done in G2. Cabs read their job (`car_journey` Taxi). The landlords' pool is folded into #72. |
| 90 | todo:875 | Half of pay from outside; city staff unpaid; income tax not withheld | FILE | Folded into "Money still crosses the town edge". |
| 91 | todo:881 | Growth waits for backland zoning; pit on spoil rim; road plots; landlocked lot | CLOSED | Pit dug in by p2-look-leftovers. The rest is by design. |
| 92 | todo:882 | Pit at pad height; conveyor and trucks static; no hole while building | DROP | Cosmetic. |
| 93 | todo:887 | [r4-leisure-places] Kinds open slowly; karaoke loses money; drive-through props; tax lines; dates | handed to r13-catalogue | Business-kind facts. |
| 94 | todo:889 | [r4-leisure] New kinds so sub-zones grow; multi-seed run | CLOSED | [r4-leisure-places] added 17 kinds. |
| 95 | todo:892 | [round2/e2-tenants] Malls; companies ranks only owners' businesses; growth into units rare; shop menu | CLOSED | Malls in G2. "Biggest companies" ranks every owner by size (Companies item 2). |
| 96 | todo:895 | Tuition through rent; no per-district income tax | DROP | Works as is. A per-district income tax is a design choice for Harvey. |
| 97 | todo:908 | [r5-trash] Landfills don't expand; local firms dispose out of town; incinerator MW not in the Power table; recycling column | FIXED | Incinerators are rows of the Power page's plant table (`GridAsset::Incinerator`). Out-of-town disposal is folded into #72. The rest dropped. |
| 98 | todo:917 | [round1/r1-roads] Ground beside a hillside path dips ~2 m (lot terracing) | FILE | Folded into "Hillside lots". |
| 99 | todo:923 | [round1/r1-roads] A plot touching two city lots joins only the first | DROP | Rare. Placing again joins it. |
| 100 | todo:940 | [round1/r2-traffic] A city vehicle with no errand reads "passing through" | FIXED | Garbage trucks and convoy escorts say what they do (`car_journey`, `car_label`; test `a_garbage_truck_is_not_passing_through`). |
| 101 | todo:945 | [round1/r2-traffic] Cars whose street went are re-parked without driving | DROP | Only after a street is removed. Rare. |
| 102 | todo:954 | [round1/r3-visual] Green layer on one raised beach lot (58:5) | DROP | One lot on a month-old save. The photographer would re-flag it. |
| 103 | todo:956 | [round1/r3-visual] Figure, lamp and bin in the river at (470,2) | DROP | Same: an old save, not seen since. |
| 104 | todo:974 | [r4-containers] Scrap yard waits; no rail imports; generic deck boxes; 20 ft trailer for 40 ft record | CLOSED | [r8-containers] real boxes on decks and chassis at their size. The rest depends on the save. |
| 105 | todo:989 | Harvey picks the logo | DROP | Harvey's choice, not code. |
| 106 | todo:994 | [round1/r2-traffic] Wave-1 list: concave lots, parclo, save-load, tow backlog | CLOSED | Each is done or has its own open item. |
| 107 | todo:994 | [round1/r2-traffic] Tows a day low; car ferries have no car queues | FILE | Car queues folded into the islands/ferries item. Tow trucks in jams handed to trip-review. |
| 108 | todo:996 | Round-1 list: K19, K36, K30, rage U-turns, level crossing, insight store size | DROP | Old-save specifics. Saves are compressed now ([saves]). U-turns are trip-review's. |
| 109 | todo:1000 | A shot of the air taxis; Pages setting | DROP | Harvey's GitHub setting. The photographer can take the shot. |
| 110 | todo:1011 | (empty: the "Left from the flag review:" heading) | CLOSED | Not a note. Its sub-items are already open todo items. |
| 111 | todo:1014 | [r4-edge-bridges] Far water steps; stepped sea ice; rivers under a cell | DROP | Far-field cosmetics past 3 km. |
| A1 | land_value.rs:238 | A save with a bad land-value grid panics | FIXED | `LandValue::sane()`; `after_load` replaces a broken grid before anything reads it (test `a_broken_land_value_grid_loads_fresh`). |
| A2 | save.rs:27 | A compressed save has no unpack limit | FIXED | `save_text` reads through `take()` with `max_save_text` (200x packed size, 64 MB to 2 GB) and errors past it (test `a_save_that_unpacks_too_far_is_refused`). |

## Shallow systems: placeholders that deeper systems lean on

Found by grepping for TODO / FIXME / "for now" / "placeholder" / "stub" / "stand-in" / "fake" / "estimate" / "nominal", and by following the LEFT notes into the code. Most "stand-in" and "stub" hits are ordinary words (a road stub, the committee's offline stand-in). The ones that matter:

| Placeholder | File:line | What leans on it | How bad | Done |
|---|---|---|---|---|
| Half of every household's earnings arrive from "outside" (only a payroll wage is paid by the employer); the city's own staff are paid from nowhere; income tax is a treasury-to-outside flow | `world/trips/household.rs:118` | Household budgets, rent, hunger (who is broke), the treasury, the ledger's money balance | High: the town's money does not close, so business and treasury health are partly invented | Filed ("Money still crosses the town edge") |
| A corp's deposit is 0.4% of the price, "the rest is the bank's", but there is no loan or repayment | `corp.rs:63` (`DEPOSIT_SHARE`); reused ×4 at `world/megablocks.rs:411` | Corp growth, HQ towers, megablock purchases, the antitrust cap | Medium: a corp's cash barely limits what it buys | Filed |
| The sim reads each container yard's room from what the renderer last drew | `containers.rs:569` (`drawn_room`), `world/box_stock.rs:52` | Box stock, storage fees, yard closures, lorry traffic | Medium: a headless run and the game keep different stock for the same city (a sim input from render) | Filed |
| Airport passengers by rail and metro are counted, and residents flying away are a number (`daily_pax`), not trips | `airport.rs:885` | Airport fees, station loads, the airport inspector | Low-medium: the airport looks busy, but no citizen flies | Filed |
| The car ferry's car count is the old per-deck estimate; extra sailings cost nothing; ferry waits are not timed | `ports.rs:162` (`car_deck`) | Island traffic, ferry economics, the Lines window | Medium on island maps | Filed (islands/ferries) |
| The Music slider was shown and saved, but the game has no music | `audio.rs:113` | Nothing (it looked live) | Low (confusing) | FIXED (greyed, "none yet") |
| The sim caps floors under a deck by the deck's planned level; the drawing uses the measured soffit | `pillars.rs:90`, `pillars.rs:123` | Building heights under raised roads | Low: can disagree by a metre or two on hills | Filed (Layered city phase 2) |
| Far-horizon towns have no names or trade; neighbouring towns for ships and trains are fixed name lists | `ports.rs:87`, `world/rail_travel.rs:35` | Trade flavour text, the horizon | Low | Covered by big-world steps 3/4 |
| A district look over towers draws low buildings while the sim keeps the floors | `nd_procgen` look menus | What the player sees vs homes and jobs | Medium (visible mismatch) | Filed |
| Buses' leg times are an estimate at 6 m/s until a bus has timed the leg | `transit.rs:99` | Trip planning before the first run | None: replaced by measured times as buses run | Keep |
| The fishing catch rate is calibrated to a nominal 450-minute trip | `fishing.rs:54` | Fish output | None: a calibration, not a stand-in | Keep |
| The talk/committee offline stand-in matches topics by keyword | `talk.rs`, `committee/` | Chat when no model is set | None: the offline fallback by design | Keep |

## Checks

- Full suite after merging main (e7d23df9): 28 of 28 "test result: ok" groups, no failures (`cargo test --profile fast --workspace`).
- Probe on a copy of Harvey's newest save (day 81, 16,768 people), `perf_probe measure` 720 ticks, fast profile, `ND_FINGERPRINT=1`, main and branch back to back: both end at pop 17,422, treasury 5,392,060.50, 1,079 cars, with the **same fingerprint 76915f1e984451b2**, so the sim is unchanged. Tick mean/p50/p99/max ms: main 52.0/49.7/142.5/469, branch 50.2/47.9/125.1/419 (a loaded machine; an earlier main run gave p99 93 to 124, so p99 is noise here).
