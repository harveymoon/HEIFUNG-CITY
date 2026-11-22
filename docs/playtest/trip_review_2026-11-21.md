# Trip review 2026-11-21 [trip-review]

Harvey: "send an agent to track a few people for a few of their excersions review the path carefully
and their movements through the city and make sure it feels natural and realistic. specifically how
do bikes navigate around the city? were are pedestrians walking? I see some strange things happen
with bikers when in intersections they seem to take some zig-zag moves that seem unnatural."

Done on a copy of Harvey's newest autosave (midnight starting day 81, tick 115,200, 16,768 people, saved 2026-11-20 00:05) in
`O:/projects/Heifung_District/runs/trip-review/saves`. Nothing touched his running game.

## How

- A headless probe, `crates/nd_sim/examples/trip_review_probe.rs`:
  - `trace <save> <ticks> <out.csv> [ids | driver]`: every tick, the position, heading, path index and
    activity of chosen people, and each planned path once when it is set (`<out>.paths.txt`).
  - `paths <save> <ticks>`: every bike ride and walk that sets off, measured through every junction
    it passes (net turn, the sum of all turns, sideways jogs) and along its whole length:
    **wiggles** (a turn of more than 25 degrees one way within 3 m, then more than 25 degrees back
    within the next 5 m) and **back-steps** (heading more than 100 degrees off the way it went 4 m
    before and back on it 4 m after: forward, back and on again), per km. Footbridge stairs (a
    switchback or a spiral is meant to turn) are left out and counted apart.
  - `spots <save> <ticks> <out> x,y ...`: every new bike path through chosen junctions, for
    plotting before and after.
  - `roads`, `near`: the network as lines; the nodes and arms round a point.
- The same probe built from the code before and after the change, run on the save from the same
  tick. Plots (matplotlib, top-down, streets grey with their kerb width, footpaths green,
  crosswalks blue, crosswalks replaced by a footbridge red, bike paths red) in
  `O:/projects/Heifung_District/shots/trip-review/plots`.
- The game (release build of this branch) on port 5232 with `ND_HOME` in the run folder, for
  follow-cam and street-level screenshots (`O:/projects/Heifung_District/shots/trip-review`).

## The people

One sim day (1,440 ticks from midnight starting day 81). Trips listed as mode, purpose, path length / as the
crow flies. "Start-back" / "end-back": metres ridden past the door and back at the start / end.

### 1. Ash Volkov (1v3), 54, cyclist, no car; home Ember Heights, works at Neon Fire HQ
- Trips: riding home (2.8 km / 0.93 km), to a grocer (3.0 km / 1.5 km), a second grocer (1.6 km),
  a walk to the bus stop, back home by bike (1.9 km).
- Natural: keeps the shoulder on the straight; turns at the big junctions on the ring roads; uses
  the footpaths across the parks where they join.
- Not natural (before): set off riding 98 m and 81 m the wrong way along the home street to its far
  node and back past the door; ended 81 m past home and back; at the curving one-way by
  (-114, 610) climbed the stairs of the **footbridge over the side street** while riding down the
  main street, went over the deck and came down the switchback onto the street it had left
  (`plots/1v3_node319.png`, `plots/spot0_before_after.png`).
- After: no start-back; the footbridge detour is gone for riders along the street. Left: the home
  trip still ends 80 m past the door and back: the home fronts a footpath, its door is 47 m off the
  last leg (beyond the 25 m the end trim reaches).

### 2. Ilya Nakamura (2v3), 25, cyclist, takes the bus too; home Mercury Block, works at Sector 7 Fire Station
- Trips: home (2.1 km), to work (1.4 km / 0.77 km), a bakery and back to work, a walk to the stop,
  a walk, home (1.6 km).
- Natural: the same commute both ways, about 1.8x the crow, on the arterials.
- Not natural: at (-660, 30) the north-south street passes the avenue without a junction, so the
  ride goes up to the apex of a V of streets and comes back down the other arm (a hairpin) to get
  onto the avenue (`plots/2v3_after_a.png`). The network's doing, not the bike's; it reads as a
  detour.
- Before: 26 heading flips of more than 120 degrees in one tick over 4.8 km.

### 3. Wren Sato (13v3), 26, cyclist; home Cinder Terrace, works at Harbor Terrace
- Trips: home (2.1 km), to work (1.6 km / 0.97 km), an evening out (300 m), a park ride (1.1 km),
  walk to a bench, walk home.
- The cleanest rider: direct routes (1.3-1.7x the crow), 5 flips in 6.6 km before, 2 in 5.2 km after.

### 4. Ines Haddad (16v3), 34, cyclist and bus; home Harbor Towers, works at Harbor Parking Authority
- Trips: walk to the stop, a long evening ride (2.9 km / 1.8 km), a grocer (390 m), home (3.1 km),
  a bakery (1 km).
- Natural: long rides keep to the arterials. Not natural (before): 10.5 m start-back, 14 m end-back;
  21 flips in 8.4 km.

### 5. Lux Reyes (140v7), 32, on foot and by bus; home Habitat 27-C, works at Old Town Medical Centre
- Trips: home, to the stop, work (176 m), back to the stop, home; walks of 150-200 m, the longest
  800 m to a grocer.
- Natural: walks the pavements and crosses at the zebras; cuts through blocks over lot paths.
- Not natural: the in-block walk follows each lot's setback loop and the gates between lots, a
  step out and back at every gate: 8 wiggles in a 150 m walk (`plots/140v7_t2.png`). Not fixed
  (see "Left").

### 6. Theo "Fuse" Haddad (7242v3), 25, on foot and by bus; home Cobalt Block, works at Argent Holdings
- Trips: to the stop, a 2.1 km walk home that takes 426 ticks (7 game hours: walking runs at
  5.6 m per one-minute tick, the known motion / schedule scale gap), to the stop, work, back.
- Not natural: the 2 km walk (rather than the bus) and the in-block notches as for Lux.

### 7. Mira "Signal" Marsh (7867v5), 48, on foot; home Aurora Towers, works at Jade Noodle Bar
- Trips: home, two strolls, a bakery, work (450-770 m, 1.4-2.1x the crow).
- Natural: strolls end in parks; pavements and zebras. Same in-block notches.

### 8. Yuki Lindqvist (85v5), 29, owns a car, walked all day; home Tide Hive, works at Jurynth City Block
- Every trip was under the drive threshold (the bus twice); 1.3-2.5x the crow on foot.

### 9. Kade Nguyen (56v3), 30, driver (owns a car and a bike); home Marrow Habitat, works at Kwik-E-Fresh
- Drives to work and back (76-168 ticks a leg, lanes and junctions as traffic), parks, and walks
  the last stretch: 215 m from the car to a home 41 m away (5.2x), up a footbridge and round by the
  footpath, because the home fronts a path and the car parks on the nearest street
  (`plots/56v3_walk_from_car.png`). As designed ("homes on paths"), but it looks odd.

## Where the bikes go

- On a street: the shoulder of the direction ridden, 0.75 m in from the kerb (half width less half
  the 1.5 m shoulder). On a footpath: 0.5 m left of its middle (walkers keep right).
- Routes: A* over every walkable way (`RoadNetwork::foot_path`), by distance only: no lanes, no
  one-way rule, footpaths and streets alike. So bikes ride the wrong way up one-way streets (at the
  kerb on their own right, facing the traffic), and on Harvey's ring-and-spoke city with overpasses
  that do not join, rides run 1.6-3.4x the crow.
- At a signalised junction they stop short of the crosswalk while their approach is red and call
  the phase; they give way to walkers on a zebra; the bold run a red in a gap. At the destination
  they ride to the door and go in with the bike (no dismount is shown).
- Walkers: pavements (0.35-0.75 m right of the middle), crosswalks at junctions and zebras, lot
  paths and gates through blocks, park paths, footbridges where built. None seen crossing mid-block
  except the planned jaywalkers. The in-block network is the zig-zag one (above).

## The bike zig-zag at junctions: cause

The ride's line was built in `World::route_path` (crates/nd_sim/src/world/trips/travel.rs): each piece
of street offset to the rider's kerb line, and consecutive pieces joined at the corner where the two
lines meet (or, when the lines were near parallel, by both their end points). The rider steers
straight at each vertex and takes its heading from the leg being ridden (`step_rider`). That went
wrong in four ways, all in junctions:

1. **Widths differ, nearly straight on** (an avenue going on as a street, a one-way meeting a
   two-way): the near-parallel fallback put the end of one kerb line and the start of the next at
   the junction's middle, a sideways step of 1.5-3.5 m.
2. **Short pieces** (a divided road's carriageways meet 5-10 m apart, a footpath's end cuts a street
   a few metres from the corner, stubs between two nodes): the kerb lines are offset further (up to
   7.75 m) than the pieces are long, so their corners came out of order: forward past the junction,
   back, and on (e.g. node 319: down 2 m past the turn, 2.5 m across, 4.6 m back up, then off).
3. **Corner behind the rider**: between lines at a shallow angle (a street onto a footpath, a bend
   and a width change) the corner lay behind where the ride already was, so the bike turned round
   for a metre or two.
4. **Riding along a street past a side street whose crosswalk is a footbridge**: the leg passing
   the side street's mouth was taken for a footpath crossing that street, so the bike was sent up
   the bridge's stairs and back down (and at a zebra, made to wait as a walker would).

On top, a tick moves a bike 18 m, and the renderer draws it on the chord between tick positions
with the heading eased from the old leg's to the new: a tick that ended on one of those short back
legs showed the bike swinging round and back.

Measured on the save (2 sim hours of new rides from the same tick, before → after):

| | before | after |
| --- | --- | --- |
| bike path wiggles per km | 0.95 (4,143 in 4,346 km) | **0.04** (174 in 4,139 km) |
| bike path back-steps per km | 0.46 (1,991) | **0.01** (39) |
| junction passes with turns over 20 degrees beyond the net turn | 22.5% | 15.0% (the rest are real dog-legs between junctions a few metres apart) |
| junction passes by footbridge stairs | 3,051 | 644 |
| walk path wiggles / back-steps per km (unchanged code) | 4.23 / 5.25 | 4.23 / 5.24 |

The traced riders, before: 74 heading changes of more than 120 degrees in one tick in 27.5 km
(2.7/km). After: 41 in 20.6 km (2.0/km); what is left are real hairpins of the network (the V at
(-660, 30), U-turns at dead ends) and the first ticks of rides that set off before the change.

## What was fixed

- `route_path` (travel.rs): through a junction (three or more ways at a node) the way in ends a
  junction radius out (the widest street's half width plus a metre, still inside the zebra, so the
  rider meets the crosswalk on the straight and stops at its line as before) and the way out starts
  as far along; between them a quadratic curve through the kerb corner (it turns one way only), or,
  where that corner is out of order, a cubic leaving along the way in and arriving along the way
  out (a smooth shift for a width change). A hairpin whose ends nearly meet is a sharp corner (no
  loop). Pieces under 12 m beside a junction (up to 30 m of them) are ridden as part of it. Plain
  bends of one street keep their kerb corner (the existing test that a turning rider never crosses
  the lanes still holds), unless it is out of order.
- `rides_along_street` (riders.rs): a rider within a street's half width of its middle and within
  about 35 degrees of its line, at a junction, rides through: the crosswalk or footbridge over a
  side street's mouth is not theirs (`bike_over_footbridges`, and the walker-style crossing in
  `rider_stop`).
- `trim_route_ends` (travel.rs; also the ride to a bus stop with the bike, trains.rs): a ride (and
  a walk along the streets) starts level with the door on its first leg and ends level with the
  destination on its last, when the door is abreast of that leg within 25 m.
- Deterministic: geometry only, no random draws; the save format is unchanged (rides in progress
  keep their old path until they end).
- Tests (`world/trip_review_tests.rs`, `riders.rs`): every turn and straight at crossroads of 1+1,
  2+2 and mixed widths turns one way only and never steps back; straight over from an avenue to a
  street shifts smoothly (never over 30 degrees off, one S at most); a dog-leg over an 8 m stub
  never heads back; riding along an avenue past a side street's footbridge stays on the street; the
  end trim; a rider's heading through a left and a right turn in the sim never swings back.

Plots (before left, after right, 60 rides each):
`plots/spot0_before_after.png` (the side-street footbridge, node 319),
`plots/spot1_before_after.png` (a junction with a 7 m stub),
`plots/spot2_before_after.png` (a one-way bend meeting an avenue),
`plots/spot3_before_after.png` (street to footpath),
`plots/spot4_before_after.png` (a one-way pair and an avenue),
`plots/spot5_before_after.png`, `plots/spot6_before_after.png`.

## Screenshots

In `O:/projects/Heifung_District/shots/trip-review` (the release build of this branch, 07:30 on
day 81 of the copy; the before state is shown by the probe plots above: a second 30-minute release
build of the old code was not worth the time):

- `after_junction278_overview.png`: the junction of the one-way pair and the avenue at (-311, 541),
  ringed by footbridge decks, with riders on the shoulders.
- `after_chase_8300v1_0..4.png`: chase camera behind Rin Fontaine (8300:1) riding down the curving
  one-way past node 319 (-114, 610). Frames 0-2: she stays on the shoulder past the side street's
  footbridge stairs (before the change every rider here went up those stairs and back down).
  Frame 3: she rides **through a yellow car standing at the kerb**: the shoulder line is where cars
  park and taxis wait (filed).
- `after_high_7447v1_0..3.png`: Ezra Baptiste (7447:1) at the same junction as the overview; the
  follow camera ended up beside a deck pillar, so these show little. His positions tick by tick
  ((-310, 531), (-303, 523), (-307, 527), (-311, 529)) went out and back: he was wheeling up the
  ring of decks, as a rider on a footpath's line over the street does (not a rider along it).

## Performance

The same 720 ticks from the save's tick, the probe built before and after, two runs each
(load 3.5 s): 25.8 s and 26.5 s on average, about 31.0 and 31.9 ms a tick. The difference is within
the run-to-run spread (25.3-27.2 s) on a loaded machine. The curves add vertices only to new rides
(a route of n junctions gets about 4n more points); the end trim and the side-street test are per
ride planned and per crossing near a rider.

## Left (filed in todo.md)

- Walk routes through blocks follow the lot setback loops: a notch at every gate (4.2 wiggles and
  5.2 back-steps per km of walk path). String-pull the in-block stretches, keeping the crossing end
  points `crossing_between` needs.
- Bikes ride against one-way streets (routing ignores lane direction).
- Rides to path-fronted lots can end far from the door (Ash: 80 m past and back).
- Long detours on Harvey's layout (overpasses without junctions, footpaths only partly joined).
- The renderer draws a rider on the chord between ticks (18 m apart): it cuts junction corners.
  Interpolating along the path would follow the curve.
- Bikes ride through cars parked at the kerb and taxis waiting there (the shoulder line is the
  parking lane): pass them on the outside, or ride a bike line just outside the parked cars.
- Walkers' long trips: a 2.1 km walk home took Theo 7 game hours (motion runs at 1.4 m/s against a
  one-minute tick); he could have taken the bus.
- A driver parks on the nearest street and walks 5x the distance home round a path-fronted lot
  (Kade: 215 m for 41 m).
