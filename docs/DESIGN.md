# District 2040: design from the drawing board

A procedural cyberpunk city builder that we keep tinkering with. This document is the
living design doc. `IDEAS.md` next to it is the backlog where new ideas land first.

## Pillars

1. **Everything procedural.** Terrain, lots, buildings, names, citizens, their
   personalities, signage, even the ads. Data files in `assets/` drive the generators
   and hot-reload while the game runs.
2. **A city that is alive at street level.** Citizens are individuals with homes,
   jobs, schedules and moods. You can click on anyone and read what they are thinking.
3. **Minds are pluggable.** Every citizen is driven by a `Mind`. Today that is a cheap
   rule-based mind with templated thoughts. The same interface takes an LLM-backed mind
   for "spotlight" citizens. The simulation never waits on a model.
4. **Deterministic, headless simulation.** The sim crate has no engine dependency. Same
   seed, same inputs, same city. It runs in tests, in a CLI, or under Bevy.
5. **Cheap to change.** Each system lives in its own crate with a small interface, so
   replacing traffic, the economy or the renderer never means rewriting the rest.

## Lessons taken from Citybound

Kept:
- Rust for the simulation, data-driven building grammar, planning tools that draw
  roads as polylines and zones as polygons, the "inspect anything" window, a flat
  stylised look that reads well from far away.

Changed:
- No custom actor framework or code generator. Plain structs and an ECS only at the
  engine edge.
- The simulation ticks on its own fixed clock. Rendering frame rate never drives it.
- Saves are serde snapshots, not memory dumps. A crash costs seconds, not the city.
- Real renderer with HDR and bloom so neon actually glows.
- Maintained 2026 toolchain: stable Rust, Bevy 0.19, egui.

## Architecture

```
crates/
  nd_sim      engine-independent simulation (roads, zoning, lots, buildings,
              citizens, economy, clock, save/load, Mind trait)
  nd_procgen  generators: terrain, parcels' buildings (47 styles from a parts
              toolkit), streets, seamless textures, palettes
  nd_minds    Mind implementations: RuleBasedMind, LlmMind (+ backend trait)
  nd_app      Bevy app: rendering, camera, planning tools, egui UI, hot reload
assets/
  palette.ron           world and glow colors
  rules/buildings.ron   building grammar per land use and the style weights per tier
docs/
  DESIGN.md, IDEAS.md
```

Data flow per frame: `nd_app` steps `nd_sim::World` N times according to sim speed,
drains `SimEvent`s, and creates or updates Bevy entities for anything that changed.
Citizens are rendered from their simulated positions every frame.

## Simulation model (v0)

- **Clock**: one tick is one simulated minute. 1440 ticks per day.
- **Planning**: roads, zone polygons and paint strokes are *gestures*. The master plan
  is what is built; a project is a named stack of deltas on top of it with undo and
  redo. Several projects can be pending at once, as in Citybound: the active one is
  edited and drawn bright, the others (`World::parked_projects`) are drawn as dim
  holograms and keep their own histories. Control points of any road or zone can be
  dragged, inserted or deleted; bulldozing marks a gesture for removal. Implementing
  one project applies its deltas over the master as the projects implemented before it
  left it (so where two projects change the same gesture or paint the same parcels,
  the one implemented later wins, with a warning) and rebuilds roads, parcels, zones,
  lots and lanes. Buildings whose lot survives the rebuild are kept, the rest are
  demolished. A save from before named projects loads its one project as "Project 1".
- **Roads**: a node/segment graph with per-direction lane counts. New polylines snap to
  existing nodes and split segments they cross. Walking uses A* over nodes.
- **Lanes and traffic**: every segment becomes offset lanes per direction, shortened at
  junctions. Each junction gets connector lanes for every movement (U-turns only at
  dead ends); lane arrows on each arriving street permit some of them. Defaults follow
  the lane count and the turning counts, revised every 600 traffic seconds: two lanes
  are a left-only lane plus straight-and-right (a shared left-and-straight lane while
  under 30% turn left), three or more get a left lane, straight lanes and a
  straight-and-right lane, with a second left lane when over 40% turn left. The player
  can set arrows per approach (`lane_use`); highway ramps keep every lane open. Arrows
  are painted in every arriving lane before the stop line (`street::lane_arrows`): one arrow
  per lane, 8 m back (a second 25 m further on long approaches), 5.4 m long with 0.4 m shafts
  and filled heads for each allowed movement, so a left-only lane reads apart from a
  straight-and-right one from the top-down camera. Road paint (lane lines, stop lines,
  zebras, arrows) is its own unlit surface (`Surface::Paint`), full at night and brighter by
  day, instead of sharing the glow material that is dimmed to a third in daylight and made
  the markings read dark grey on sunlit asphalt. Junctions with three or more approaches run an
  actuated signal: opposite approaches share a green (left turns yield to oncoming
  traffic), a green with nobody left to serve is cut after 5 s when another approach
  waits, and phases nobody waits for are skipped. A green opens with a protected left
  arrow (up to 7 s, oncoming red) when left turners queue in a lane of their own and
  their head has been yielding, or when any left turner has yielded 20 s; a starving
  left also ends an endless green. On a one-lane approach a car going straight or right
  behind a waiting left turner pulls onto the shoulder at low speed, passes on the
  right, stops short of the crossing if it must, and merges back; on wider approaches
  followers change lanes out from behind the turner. Cars path over
  the lane graph with A* (lane changes are steps between parallel lanes; every vehicle
  on a lane adds to its cost, so routes spread out), follow the intelligent driver
  model, queue behind each other and stop at red lights. They move into the lane their
  next turn needs as soon as the gap is IDM-safe for them and the follower there, drift
  across over a few seconds, and overtake buses at stops and slower trucks when a
  parallel lane is free. New cars pull out of the kerb only into a gap (the next driver
  lets them in after 15 s); a car stalled 40 s before a full exit, or 10 s at a lane
  change it cannot make, takes another way, which breaks gridlock rings. Lane changes stop at the solid line before a junction or a roundabout junction
  (the last 20 m, more at speed, up to 32 m) and never happen inside one: routing plans
  them on pieces with room before that line (`SHORT_CHANGE_COST`, `SOLID_CHANGE_COST`), and
  a car not in the lane its turn needs by the line carries on in its own lane and finds
  another way (it misses the turn) instead of stopping for a gap. Only with no other way at
  all does it wait at the line: then the driver behind in the next lane leaves it a gap as the
  queue moves (the zip), and two cars side by side each wanting the other's lane swap places.
  A change is drawn as a glide: the sideways offset eases along a half cosine over 2.5-3 s of
  driving (8 to 45 m) with the nose along it. Cars already in the junction count against the
  room on its exit, so a run of cars cannot fill the box behind a standing tail. Traffic physics runs 1.5 seconds per sim minute so a car at 50 km/h
  moves at a believable pace on screen at 1x; trips therefore take sim-time tens of
  minutes, the usual city-builder compromise. Right-hand traffic; lane sets are centred
  on the road; junction curves stay inside the junction; cars refuse to enter a
  junction whose exit lane is full; a vehicle 30 traffic minutes on one route replans from where it stands, and nothing is ever taken off the road for being slow: jams are part of the city and drivers feel them. Pedestrians walk on the
  right-hand pavement.
- **Roundabouts** (`Gesture::Roundabout { center, radius, lanes }`, Road tool preset): a
  one-way ring of 1 to 3 lanes, 24 to 240 m across, traffic keeping the middle on its left
  (anticlockwise on screen). The gesture is stamped over the plan and never edits road
  gestures: at rebuild `Plan::built_roads` cuts the streets and paths *older than the ring*
  out of the circle (`roads::layout_rings`, by gesture order), so the old junction goes,
  signals and all; roads drawn after it are kept inside and out and join it where they
  meet it. The ring goes down first with a vertex at every join and the arms snap onto
  those vertices. So undo, bulldoze or a radius change in the Edit tool just rebuild.
  Highways pass over, track is not cut. The middle is open land with a mountable apron
  along the inner kerb: parcels are cut along the ring's inner edge and any roads inside,
  so it can be zoned and built on, and rings can nest when the inner one's pavement
  clears the outer one's by a lane and a pavement (partial overlaps are refused).
  Buildings under the ring road come down as for any road works. In the lane network
  (`traffic/roundabout.rs`) ring lanes run at `Ring::speed` (sqrt(2 r), 7 to 13 m/s), nothing
  is signalised. Arms on the outer perimeter turn right onto the ring (into its outer
  lanes) and are left from the outer lane; arms joining from the open middle (roads
  inside the ring) turn left onto it, into the inner lane, and are left from the inner
  lane; every ring lane continues. Ring pieces are flagged `Segment::ring`, so a
  three-lane ring is never taken for a highway carriageway (one-way, three lanes). Entries give way with gap acceptance: a car about to reach the
  line goes unless a ring vehicle that will pass the entry (not leave by it) is in the
  junction or arrives less than 3 s after it, looking back round the ring as far as a ring
  car covers in 9 s (at least 45 m);
  ring traffic never yields and only follows a car merged ahead. A ring car that has stood 20 s at the end of a ring lane
  behind a ring that does not move gets off at the arm it is at when that arm has room
  (`Traffic::bail_off_ring`): a one-lane ring packed solid otherwise never moved again. Passing an arm in a
  lane on that arm's side costs routing a little, the more lanes in from the side the
  more (`RING_OUTER_THROUGH` per lane), so the first exits keep to the outside and later
  ones go round further in, changing back out between junctions; ring arrows are fixed
  (the adaptive junction arrows leave ring nodes alone). Hovering the tool shows the ring,
  the road it takes up, the joins and the lots under the ring road (red); clicking stamps it
  into the active project.
- **Driving** (`traffic.rs` `DriverProfile`, `world/driving.rs`, `takeover.rs`): turns follow a
  circular arc tangent to both lanes (the fillet of their corner), long vehicles swing wide, and
  cars are drawn along the poses of every physics substep (`Car::pose_at`) rather than a straight
  blend across a 4 s tick, so nothing cuts corners. Each driver gets a profile from age,
  personality (caution, hedonism) and drunkenness: desired speed, IDM headway and acceleration,
  reaction lag, signal compliance (late ambers, reds), lane-change eagerness, skill (weave). The
  old drive slower with slower reactions; rebellious youth faster, closer, through late ambers.
  Citizens drink at venues (hedonists only), sober up over hours; rebellious drunks who own a car
  drive anyway (`citizens::wants_to_drive_drunk`) and weave over the lines. Crashes: running
  into the car ahead (profiled drivers cannot brake harder than 8 m/s per s), red lights run into
  green cross traffic, drunks running off the road, weaving sideswipes, misjudged gaps. Minor
  ones stop both cars a minute or three; bad ones (closing speed) write cars off and may hurt
  people: a witness calls 911, a `Crash` incident brings the police (who may arrest a drunk or
  red-light driver) and each injured person gets an on-road `Medical` incident and an ambulance;
  a crashed car is pushed onto the shoulder of a kerb lane and the traffic behind eases past it at
  a crawl where its body fits beside it (`traffic/hazard.rs`; no room, as for a crash in an inner
  lane or a junction, and it changes lanes or waits and re-routes). Once the police are on scene
  (or have gone, or 45 minutes after a crash nobody reported) the nearest tow company with a truck
  free sends a recovery truck (with none in town, one from out of town comes in from the map edge,
  by any road end that reaches the wreck). It drives there with amber beacons (no siren: nobody
  pulls over), and within 250 m (or 1 km to go along its route) takes the siren's off-law moves
  round the queue (the opposing lane when clear, the kerb past standing traffic; never a red
  light), stops just behind the wreck (on the lane before when there is no room on its own) and
  hauls it to the yard or off the map. A bad crash can roll a car onto its side or roof (drawn
  so); the crew rights it (`RIGHTING_MIN`) before hooking it. A crash whose call stays open two
  hours has its tow called anyway. The inspector shows a wreck's wait ("Waiting for a tow"), then
  "Truck inbound" with a locate button, and draws the truck's road to it while it is selected. A truck not there within an hour is called off and the
  wreck hooked where it lies (a truck from the far side); only a wreck no road leads away from is
  craned off after a day. (The out-of-town tow used to aim only for the nearest road end, often
  behind the wreck with no way to turn, so wrecks stood for a day.) Street
  takeovers: on ~30 % of nights (with a crew) at 22:00 a meet is planned; rebellious young locals
  drive over, outsiders roll in off the highway as a pack ignoring lights, the junction is closed,
  cars ring it and the flashiest spin donuts in tyre smoke, a crowd walks over, a neighbour calls
  the police; police on scene break it up (some arrests, cars impounded), else it ends after
  45-80 minutes and everyone leaves in convoy.
- **Moto packs** (`world/moto.rs`, `street_events::MotoPack`): from day 10, at 20:00 on 25% of
  evenings with no takeover planned (by a hash of the seed and the day), when the takeover crew
  has members who ride a two-wheeler, a pack ride is planned 20-150 minutes later. Up to four
  local gang members on bikes ride to a road end at the city's edge (the meet) and wait at the
  kerb; once they are in (or after 45 minutes) a club of 3-7 riders from out of town rolls in
  there as a column, two abreast where the street has two lanes in, and the locals pull out
  into it. They all take the same route through town: past the nightlife nearest the middle of
  town (else the lot nearest it), then off the map at the farthest road end. Riders drive a pack
  profile (`moto_profile`: 1.3-1.45x the limit, close, hard on the throttle, overtaking and
  changing lanes through ordinary traffic); the leader runs every light, the rest a late red
  now and then. The column stays loose but together: a rider more than 60 m ahead of the pack
  eases off, one as far behind opens up. Locals peel off home once through the nightlife.
  Three ticks after they set off a witness calls it in on most rides (`CrimeKind::RecklessRiding`);
  a police car on patrol, or the one sent, that comes within 150 m of a local rider gives chase
  (60%, once a ride) with the chase system, the rider fleeing on their bike. Riders are marked
  `GangRole::pack` ("MOTO PACK" in the car inspector, `moto_pack` in the API); the ride is in the
  news and the event record (reds run, chase). API: `incident {kind: "moto_pack", at}`.
- **Street races** (`nd_sim::street_events`, `world/nights.rs`): a young town has no scene, so
  neither races nor raves happen on their own before day 10 (`SCENE_FROM_DAY`; the API forces one
  any time). From then, at 23:00, on 35% of nights when three or more locals aged 18-42 at home
  with a fast car of their own (tuned, a coupé, a superbike, an
  expensive saloon or wedge; rebelliousness 0.45+) are free and no takeover is on, a race is planned
  for 23:30-01:00. The crew (up to 8, flashiest first) drives to an empty lot or a surface car park
  (the meet: near their homes, on cheap land) and parks up in rows, nose to the street
  (`Spot::bay`, a `Ring` stunt); onlookers walk over. Once everyone is in (or most after 40
  minutes, a pair after 80) they show the cars off for ten minutes, then the flag drops: every car
  pulls out onto the same route over the streets to another such lot 450 m to 2.4 km away (550 m
  by road at least) with a racing profile (`DriverProfile::racing`: 1.55x the limit, hard on the
  throttle, through every light; a `GangRole` with `race` set and phase `Racing`, held at the end
  of its route instead of parking). Cars park up at the finish in the order they arrive (the
  winner first) and party there for 50-90 minutes. On most nights (by a hash of the race) a
  neighbour at the meet calls it in when the engines go: a `StreetRace` crime at the finish. Police
  on scene there wait for the first car in, then bust it: each racer at the party may be arrested
  (40%, at most three; the car impounded and towed like a takeover's), every other racer caught is
  fined 250 from their household (`Flow::Fines`), and one may floor it, handed to the chase system
  (`try_chase`, the incident's suspect set to them). Then everyone drives home. The race is in the
  record, the news and the inspector ("STREET RACER · racing to Lot 24", the place they came in).
- **Warehouse raves** (`world/nights.rs`): an old warehouse is a cheap (low tier) industrial
  building standing (not a dock, plant, station or service). At 22:00 each is weighed by the
  culture living within 900 m (`RavePull`): young night owls (18-34, hedonism 0.55+) / 30 plus
  artists (`pressure::is_artist`) / 6, plus 0.25 when the city has an art scene; 1 or more is a
  scene. The strongest with a scene, if it has not held one in four days and the city none in two,
  opens at 23:00 with a chance of 12% per point of culture (at most 30%). Night owls and artists
  within 1.6 km who are at home or idle (nobody is pulled out of a bar or a shift) walk over (two
  in three take up the word; more word goes round every
  20 minutes until 01:00, 70 guests at most) and queue along the pavement from the door; the door
  lets two in every two minutes (`Activity::Visiting` the warehouse, mood up, a memory) for 8 on
  the door, half of it the hire of the place to whoever runs the warehouse. Beams
  sweep the sky over the roof and coloured light pulses to the beat. After the first hour the
  neighbours within 220 m who are home lose sleep and mood every half hour (a Noise concern), and
  one may call the police (15% plus 4% per neighbour woken, at most 85%): an `illegal rave` crime at
  the door. Police on scene shut it down, fine the promoter 400 (a quarter of the time taking them
  in); otherwise it winds down between 03:00 and 04:30. A city without a scene near its warehouses
  never has one (tested).
- **Road wear and maintenance** (`nd_sim::road_wear`, `world/roadworks.rs`): every street piece
  the city looks after (not paths, track, bored tunnels, highway carriageways or anything past the
  map edge) wears 0.0033 a day with age (about 300 days to crumble) plus 0.00012 per vehicle sample
  per lane per 100 m (vehicles seen on the piece every 10 minutes, lorries and buses x4: a lane
  holding a car per 100 m all day adds about 0.003), bridges 35% faster.
  Wear is kept per segment with each piece's midpoint, so road works carry it to the pieces that
  are still there and new road starts smooth (`RoadCondition::remap`). At midnight the crews work
  down a queue of pieces worn past 0.3 until the day's allowance is spent: the allowance is the
  funding level (0-150%, City Data's Economy › Taxes Road maintenance slider, default 100%) times what the
  network needs to stand still (lane-metres x 0.033 a day); resurfacing costs 6 per lane-metre per
  unit of wear and leaves the piece like new; the job the money runs out on is patched part way.
  The queue is honest about whose street gets fixed: priority is wear x (0.35 + 1.3 x land
  value), so at the same wear dear land goes about three times ahead of cheap land (rich
  neighbourhoods call the council and the council answers). Cut the line and the poor streets
  crumble first (tested: at a third of the need, the dear side stays near its start while the
  cheap side wears on). Spending is booked as `Flow::RoadMaintenance`. From 0.35 wear the lanes
  are rough (`Lane::rough`): drivers keep up to 35% under the limit (`limit_at`) and misjudge gaps
  and run off the road up to 2.5 times as often (the same rolls, so no new draws). Worn pieces are
  drawn with dark wheel ruts (from 50%), cracks (from 40%), patches (from 55%) and potholes (from
  75%), placed by a hash of the piece's midpoint; the Road condition layer colours every street
  green to red; hovering a street with Inspect shows its surface, what it does to traffic and its
  place in the crews' queue.
- **Road rage and trip time** (`traffic::road_rage`, `World::step_rage`): every drive is timed
  from the tick it appears, against the minutes its route should take in free flow (90 % of each
  lane's limit plus 10 s per junction). A private driver running far over (from about 1.4 to 3.4
  times the expected minutes plus five, by patience and temper from their personality) grows
  angry over the next stretch; anger is laid over the calm profile (`DriverProfile::angry`:
  shorter headway, harder acceleration, eager lane changes, ambers run, a little less skill, so a
  slightly higher crash risk) and angry cars blink a red ring. It only builds during the trip,
  goes with the car on arrival and adds to that trip's pain (and a memory). Professionals and
  takeover packs keep their heads.
- **Why is it standing there** (`traffic/why.rs`): `Traffic::stop_reason` turns the step's coarse
  `Car::wait` and `blocked_by` into a plain reason naming the vehicle responsible (queued behind,
  red light, people on a crossing, giving way, turning left against oncoming traffic, exit full,
  lane change, at the kerb, bus stop, unloading, crash, takeover), derived read-only on demand;
  `Traffic::jam_chain` follows it to the head of the queue (or round a gridlock ring). The
  inspector shows both and draws the chain; `unexplained` would be a bug. A car that cannot make
  the last lane change of a route at a carriageway's end off the map leaves in its own lane after
  a few seconds (`drop_dead_end_changes`): the highway's lanes used to funnel into one at the map
  edge and queue for minutes.
- **Parcels and zones**: parcels are cut along every street and path as soon as it is
  built, zoned or not (`zoning::generate_parcels`, the land within 60 m of the
  corridors), at a home's lot size; land behind the rows is backland. Zoning gives
  parcels a land use: a zone polygon (Residential, Commercial, Industrial,
  Entertainment, Civic, Park) zones every parcel whose centre it holds, and the zone
  brush or marquee zones the parcels it touches (a `Gesture::ParcelZone`, keyed by
  each parcel's centre and frontage on a quarter-metre grid, found again by its old
  centre if road works reshape it). Gestures apply oldest first, so the newest wins
  (`plan::parcel_zoning`). Only parcels the use may go on take it: parks anywhere
  (backland and rough ground too), homes on a street or a footpath, everything else on
  a street, industry and civic sites not on scraps under 200 m², with their deep
  yards allowed to take the backland behind them; nothing but parks on rough ground.
  Rough ground is water or a sheer cliff: ground up to `MAX_GRADED_SLOPE` (0.7) takes any
  lot, levelled to a pad at its street and cut and filled into the ground, as long as
  the land rises no more than `MAX_LOT_RELIEF` (18 m) across it; [hillside] steeper
  ground, up to `MAX_TERRACE_SLOPE` (1.8) and `MAX_TERRACE_RELIEF` (40 m), is a hillside
  (`Parcel::hillside`, `Landscape::lot_ground`) terraced for homes, shops and clubs (the
  cliffside villa, the stair street), not for works yards or civic grounds. Parcels are
  cut within the configured square, grown on a landscape to take in every street drawn
  past it (an island off the coast) up to the map edge (`World::parcel_half`).
  [stable-parcels] Parcels are kept between rebuilds and saved with the city (as
  outlines, `zoning::ParcelCut`): road works recut only the land within `PARCEL_REACH` +
  `RECUT_REACH` of a road piece that changed (`zoning::recut_parcels`, which compares the
  road pieces, node degrees and water the last cut was made from), and every other
  parcel, lot, building, plant and pad stays exactly as it was. A lot whose parcels all
  came through unchanged keeps its id, outline and building. The ground under the
  player's buildings (`World::is_city_building`, not docks or stations) is locked: the
  parcels around it stop at its outline, it is never reshaped by the zoning or the
  works (only trimmed where a street or track is laid over it), and it is levelled last
  so a neighbour's pad never buries it. [parcel-size] Slivers along a road and the
  backland behind them are mended into one parcel, backland on the water folds into the
  lot in front, and no parcel is over `MAX_PARCEL_AREA` (2,400 m², 4,800 m² on the
  water); cuts made before saved parcels (an old save's load) keep the old rules, so
  its lots and parcels still agree.
  Every zoned parcel becomes a lot; uses with lots bigger than a home's join small
  neighbouring parcels (works yards and civic sites two or three, a shop a small
  neighbour) and industry and civic grounds fold in the backland behind them
  (`zoning::join_parcels`). Industrial strokes can carry an industry sub-zone that is
  set on implement.
- **Free placement** (`world/placement.rs`, [free-placement]): everything the Build
  window puts on a lot (plants, batteries, services and attractions, depots, metro
  entrances, the cargo hub, city parking) goes on any lot, whatever its zone, or on
  unzoned ground with road access: no zoning first. On unzoned ground a lot is made for
  it on placement, from the parcel under the pointer, or joined with its unzoned
  neighbours as civic ground would be when one parcel is too small
  (`World::placement_site`). [big-sites] On a lot too small for it (a stadium, which
  wants 2,400 m², a coal plant) it takes in as few neighbouring lots as it needs
  (`World::joined_site`): what grew on them is bought out and the ground becomes one
  lot. A city parking lot fills its plot (spaces, cost and upkeep by area; the
  multi-storey car park the same in decks), and one put next to the city's own lot of
  the same kind joins it. Only physical rules refuse: road access, a street out front
  where vehicles come and go, the lot's size, the site (river, coast, hot spot), ground
  under a station; parks are kept (but a metro entrance). The ground under a city
  building (`World::is_city_building`) is the city's own: its lot's use is Civic with a
  zone of its own outline (`take_for_city`), so the inspector and the API say "Civic"
  (`World::shown_use`), zoned growth never claims it, zone tallies leave it out and the zone tools refuse to
  paint it while the building stands. The building itself keeps its own use
  (`World::city_use`, its `land_use`): plants, batteries, depots, the cargo hub and trash
  depots work as industry, a stadium as entertainment (a night out), a city car park as
  commercial, services and metro entrances as civic, whatever ground they stand on, so
  their power draw, wages and venue role do not depend on where the player put them. Rebuilds keep it so (`build_lots_from_parcels`
  pins one Civic lot per city building); an older save's city buildings are switched
  over on load and on the first rebuild without moving anything. The Bulldoze tool (or
  `World::bulldoze_building`) takes a city building down at once, and its lot goes back to
  what the master plan has there, or is unzoned ground again (`release_city_lot`).
- **Buildings**: a vacant lot develops when demand for its land use is high. Floors
  and capacity (homes or jobs) depend on land use and a seed. Geometry is generated
  from the seed, so a save stays small.
  [pt-growth] What the inspector says matches the mesh (F8, F25): civic buildings are two to
  five storeys at any tier (the high-tier multiplier gave ten-floor "Data Exchange"s drawn as
  low monuments); the styles that draw a fixed low height (villas, the monument plaza, the
  library, the precinct) are never picked for a site of more than three floors and the
  courthouse fades past five (`adjacency::site_fit`); and "the city's tallest" for an ordinary
  building is ranked by its drawn height (`BuildingTops`, the renderer's walls and roof), not
  floors at a storey height.
- **Homes and jobs in balance**: demand counts working-age adults only (children and
  retirees neither work nor look for work). The job gap (adults the empty and coming homes
  will bring, at the town's adults per household, against permanent and coming jobs) and
  adult unemployment pull commercial and industrial land; joblessness and a coming job
  shortage hold back the residential trickle (people already waiting for a home still pull).
  With no job free and over 5% of adults out of work, households mostly stop arriving.
- **Which business opens**: a new commercial, industrial or entertainment building opens as
  what the market can carry, not a random kind. Each kind has a cover (takings over wages
  and inputs, smoothed daily, first day skipped); more of a kind are built only while its
  cover is at least 1.05 and its shelves hold under 0.75 days of output. Grocers are built to
  the town's need (two thirds of everyone's groceries); a kind new to town is tried once, in a
  building of at most 20 jobs; offices (publicly funded) take the rest. A building
  redeveloped while its business loses money reopens as what the market wants.
- **How businesses open and close today** (the whole picture, `world/develop.rs`,
  `world/industry.rs`, `world/market.rs`): every development round (a) opens a parking lot
  near the last driver who could not find a space once four or more circled today (a surface
  lot on cheap land, a car park on middle land, never on dear land); (b) lets a citizen with
  savings found a taxi company near trips that wanted a cab; (c) opens one business a
  pressure asks for (below); then (d) tries each vacant lot with a chance of its land use's
  demand x 0.06. The business on a new building comes from, in order: a district's policy
  (hotels where visitors lack beds, the trades a district favours); for industrial land the
  sub-zone the ground, the neighbours and the player's bias pick (`industry::weights`, then
  `business_weights` inside it: farms by soil, wells over oil, fishing by water, logistics by
  ports, refineries only with crude in reach; kinds cluster by `industry::affinity` and the
  procgen adjacency table), vetoed if that kind covers under 0.8 of its costs; and otherwise
  the market (`business_kinds_viable`: cover at least 1.05 with shelves under 0.75 days
  full, a first one of a kind in a building of at most 20 jobs, grocers to the town's need,
  offices always). Kinds that need their site or a pressure (`BusinessKind::needs_site`) are
  never the market's blind pick. A business pays a share of its staff's wages each night;
  five days deep in debt (under -300) it closes, its staff are laid off, and a week later the
  building is demolished; a cheap building on dear land is redeveloped after three days.
- **Business pressure** (`nd_sim::pressure`, `world/pressure.rs`): pressures open the trades
  a city needs for reasons the market does not see. At midnight each reads a cause, smoothed
  (40% of today), into `wanted` (businesses the cause would keep busy) against `have` (open
  and being built), giving a `gap`; `strength` is the gap clamped to 0..1 (one business of
  unmet need is full). A gap of 0.75 held two days opens one business of the kind on the
  vacant lot that suits it (allowed land uses, area, distance to its anchors, the land value
  it likes); with enough of them the gap closes. A business that cannot pay its way closes as
  any other; its kind then waits ten days (`COOLDOWN_DAYS`). The pressures:
  - *tow company* (`TowYard`): cars a day towed by the company from out of town, 6 per
    company (locals take the jobs they can, so `wanted` = local companies + outside tows / 6);
    industrial or commercial land near the city's recent crashes. Reads the tow counts only.
  - *car repair shop* (`Mechanic`): wrecks a day plus one service a day per 90 cars, 9 a shop;
    one-storey garages on cheap commercial or industrial land near the crashes. Repairs pay
    90 a wreck and 30 a service, spread over the shop day by capacity; the car-owning
    households pay the bill at midnight.
  - *car dealership* (`CarDealer`): one per 250 wealthy households plus one per 25 cars bought a
    day; on commercial land near the rich. Cars bought in town go to a dealership within 4 km
    first (the old gadget-shop fallback stays), the dealer keeps 25%, and wealthy households
    within reach pay 2 a day for leases and valeting. The forecourt is drawn full of rows of
    one new model in one colour.
  - *trash company* (`TrashCompany`, `crate::trash`): buildings out of every city depot's
    reach, 150 per company; industrial land near them. A local company serves every
    uncovered building within 1.8 km (cheap homes only at a fee of 1.0 or less), from its yard
    with up to four trucks; the household fee falls with the companies in reach
    (`company_fee`: 1.2 alone, 0.92 with a rival, 0.75 with two more, never under 0.6; the
    hauler from out of town charges 1.5 and only takes the rich).
  - *bike shop* (`BikeShop`): bike owners / 900 + walks of 25 minutes or more a day / 2000 +
    bikes bought a day / 60; commercial land near the riders. Bikes are bought at a bike shop
    within 2.5 km first; repairs (one a day per 25 owners, 12 each) pay too.
  - *slaughterhouse*: livestock a day from the cattle and chicken farms, 110 each; industrial
    land within 1.5 km of the farms. *Fish gutting*: fish landed a day, 90 a shed; *fish-stick
    factory*: fillets a day, 80 a factory, plus 0.6 per gutting shed open (its fillets need a buyer); *fish market*: 0.8 per small and 1.6 per large
    fishing port; all near a fishing port. *Seafood restaurant*: 0.8 per small marina, 1.8
    per large, 0.5 per fishing port, within 900 m of one.
  - *golf driving range*: comfortable and rich adults / 650; entertainment or commercial lots
    of 2,800 m² or more near them.
  - *artists' studios* and *galleries* (the art scene, below).
  - *marina* (advice only: the player builds them): wealthy households within 900 m of the
    water / 150.
  City Data's Economy › Overview "What the city's businesses want" lists every pressure with its bar,
  `have` and `wanted`, why, and why nothing opens when it is held (cooling down, no lot, a
  player-placed kind). The inspector says why a business is where it is (its `origin`, or for
  the market's picks what picked it) and how it is doing (paying its way, struggling, days in
  debt, short-staffed, out of an input, more of them than the city needs, competitors).
- **New supply chains** (`market.rs` recipes, per job-hour; a job adds about 50 a day at base
  prices): cattle farm grain 1.0 -> livestock 2.2, chicken farm grain 0.8 -> livestock 1.8
  (both farming sub-zone, drawn by the farming menu on open land), slaughterhouse livestock
  2.0 -> meat 2.0 (farming; drawn only where two livestock farms per slaughterhouse stand within 700 m,
  and more likely the more farms; cattle and chicken farms are a small share of the farming menu,
  since what a slaughterhouse cannot take sells out of town at a loss); strip mine -> ore 2.0 (new sub-zone `IndustryKind::Mining`: only where the ground's
  rock is near the surface, on cheap land; nuisance 0.42 over 340 m, dust 0.002 a working
  hour over 90 m; ore sells out of town, taxed with oil extraction); fishing port -> fish 3.0;
  fish gutting fish 2.0 -> fillets 2.0; fish-stick factory fillets 1.5 -> meat 2.2 (fish
  sticks counted as meat); fish market fish 2.0 -> groceries 3.0 (a grocer for
  the public); seafood restaurant: diners' tabs (x1.5), 0.3 fillets a diner-hour (half the tab
  with none); artists' studios -> art 0.5; galleries buy art.
- **Waterfront** (`world/waterfront.rs`, Build > Water): small and large marinas and fishing
  ports are docks the player places at the water's edge, checked as the shipping ports and
  ferry terminals are (`Port::plan`: the nearest shore within 90 m, the yard set back on dry
  land, a pier that reaches open water, not too steep or ragged; marinas and fishing ports
  take rivers 18 m wide (26 m for the large ones) and lakes). The city pays for the quay
  (9,000 / 26,000 / 8,000 / 22,000, upkeep 40 or 90 a day); an operator business runs it on its
  takings with a float of 1,500; one that goes under is replaced by a new operator after three
  days shut (the dock stays). A marina lets its berths (10 or 22) to the wealthy households
  within 1.5 km (20 a day); a large one also takes yachts from the rich anywhere (one per 12
  rich households, 32 a day) and has a clubhouse that is a place to go out. Marinas lift the
  land around them (+0.08 over 220 m, +0.14 over 320 m for a large one) and draw seafood
  restaurants. Fishing ports land fish (their own recipe; nuisance as fishing), draw gutting
  sheds, fish-stick factories and fish markets. Boats are port ships, not saved: trawlers put
  out between 04:00 and 07:00 (two thirds of the berths), fish for five hours and come home;
  sailing boats and (large marinas) motor yachts go out 09:00-18:00, a quarter of the berths at
  most, for a short loop on open water that keeps 5 m off the banks; 24 boats at most in all,
  never counted against the freighters and ferries.
- **The shore** (`nd_sim::shore`, `world/shore.rs`, [waterfront]). *Waterfront lots*: where a
  street or path runs within 80 m (`zoning::SHORE_REACH`, from its corridor) of a river, lake or
  the sea, the land past the usual 60 m reach that runs down to the water (and borders the road's
  own reach, so never the far bank) is parcelled too, and every chunk of land on the water that
  took some in is cut into lots that run from the road the whole way to the waterline:
  frontage-wide slices (0.38 of the depth wide, 14-36 m), never rows a lot deep, never halved
  across. Every parcel and lot knows its frontage on the water and which way it lies
  (`Lot::shore`, probed 1.5 m outside each edge against the water mask). New cities have it;
  saves from before keep the parcels they were cut with (lots reshaped under buildings would be
  pulled down) and can switch it on (`waterfront_parcels`). A waterfront lot carries a land value
  premium (+0.12 over 55 m at 30 m of frontage). *Waterfront trades* open by pressure on vacant
  waterfront lots only: beach resorts (lodging: twice a hotel's beds a floor, 1.6x the night;
  wanted by visitors in town and open beaches, scaled by the beach weather), pier restaurants
  (dear places to go out; by comfortable adults and visitors), beach bars (by bather-hours) and
  boat rentals (by visitors and the wealthy near the water); a hotel opening for want of beds
  takes a waterfront lot first and is drawn as a waterfront hotel. *Public beaches*: a park
  sub-zone painted on parcels that touch the water; laid out as sand with a boardwalk down
  towards the water and towel spots (the park's benches, so the seats system crowds it); a
  landlocked one still gets its park building from the city. It joins the walk graph only
  through a street or path it fronts or a park next to it (a beach never gates into a private
  lot; a player path drawn to a gate on its edge joins it too: beaches are `GroundsKind::Beach`
  grounds, `world/grounds.rs`); a breadth-first walk from its nodes to any pavement or path node is
  its *access*, and without it nobody goes ("no access"). Each hour: contamination over the sand or 8-50 m off it
  closes it at 0.2 (reopens under 0.12) with a sign and tape; the crowd is the residents within
  1.5 km going to their nearest open beach (8% of them at a perfect hour) plus a share of the
  visitors in town, times the beach weather (`beach_weather`: nothing under 12 °C or in a
  storm, little under a sky heavy enough to rain, building to a 12-16 h peak, nearly nobody
  after dark; Arctic beaches stay empty), thinning past its capacity (16 m² a bather). Open
  beaches lift the land (+0.16 over 240 m; less while shut or unreachable), are tourist sights
  (`AttractionKind::Beach`; visitors lay their towel on a spot of their own), draw citizens out
  to the park in the heat (`beach_pull`), and households within 700 m of one used that day gain
  a little mood at midnight. *Private jetties*: a fishing company or fish works, or a works that
  ships, on a waterfront lot builds its own small jetty (600) when it has twice that and the
  water off it is navigable (sea, lake, a river 18 m wide; `plan_jetty`); a cargo quay lands its
  imports by its own coaster at the sea price five hours on; a pier restaurant's deck and a boat
  rental's pontoon are jetties too. A fishing jetty is a trawler's berth (the fishing fleet
  model does not sail it yet).
- **Golf driving ranges**: a place to go out (`BusinessKind::is_outing`): a night out also
  finds restaurants, ranges, galleries and marina clubhouses on any land use; each charges by
  the hour at its own rate (`tab_factor`). Open 08:00-23:00, floodlit after dark.
- **The art scene**: about 4.5% of adults have an artist's calling (`artist_seed`, a hash of
  their seed, no draw from the RNG). Two artist households a day move to cheap (low tier)
  homes within 500 m of old warehouses (low-tier industrial buildings) no dearer than their
  own; up to six artists a day leave their day jobs for open studio or gallery jobs. The scene
  exists when at least four artists earn a middle income and twenty rich adults live in town;
  then studios open (one per 25 artists) on cheap commercial or industrial land near the
  warehouses, and galleries (one per 300 rich adults, at most three for every four studios)
  near the studios on middle land. A gallery holds an opening about every six days (a hash
  of the gallery and the day): at 17:00 it invites eighteen comfortable or rich adults within
  2.5 km (the rich and artists first); at 18:00 about two in three free guests go; the opening
  runs to 22:00 and draws others too (choose_place: +1.2 for guests, +0.4 for everyone).
  Visitors lift their mood; the rich buy a piece now and then (2.5x the art's base price) and,
  enjoying the evening (mood over 0.45), give 4% of what they and their household have, up to 60
  (paid from the household's purse). Donations are the
  gallery's takings and are counted; a gallery that has been given 1,500 and lost money on at
  most two of its last seven days sets `Business::wants_expand`: the hook for the land agent
  (C2) to buy the plots next door and grow it into a museum. Nothing acts on it yet.
- **Citizens**: immigrate when homes are free, take jobs when available, and move
  along roads between home, work, shops and nightlife. Each has a personality seed,
  a memory stream and a current thought.
- **Looks** (`nd_sim::looks`): each citizen has a saved `Appearance`: skin tone,
  natural hair colour and texture (the `Genes`), hair style (short, long, bun, buzz,
  bald, afro; neon-dyed mohawks for young hedonists), greying age, height, build, two
  favourite colours (eight neon, fourteen everyday) and an outfit by age and class
  (tee, hoodie, work jacket, suit, dress, long coat). `family_looks` gives a
  household's children a mix of their parents' genes, a grandparent the first
  parent's; couples and housemates are unrelated. All drawn from the seeds, never the
  world's dice; saves without looks get them on load. Rendered by
  `nd_procgen::people` (under 150 triangles, vertex colours, an emissive trim in the
  favourite colour via a palette texture) through one lit material and a bounded
  figure cache (`render/people.rs`, 1,024 meshes; the people nearest the camera
  claim figures first, distant overflow borrows one of the same silhouette).
- **Land value**: a 32 m grid summing road access, parks, civic and commercial
  amenities, industrial and nightlife nuisances, traffic noise, vacant lots and the
  tier of neighbouring homes, blurred once. Recomputed every two sim hours.
  [pt-growth] *The scale*: the terms used to sum to 3 or 4 in most of a city (every park
  parcel, every high-tier home, every 24 m of road added its full splat) and were clamped, so
  most land read 1.0. Now like adds to like with diminishing returns: each kind of term is a
  channel (`land_value::Channel`: roads, parks, homes, civic, commerce, services, transit,
  shore, landmarks, blight, nuisance) that saturates at its cap (`cap x tanh(sum / scale)`;
  roads 0.25, homes 0.15, parks 0.3, civic 0.2, commerce 0.3, services 0.25 on gentle slopes,
  as they come in many splats over a block; transit 0.3, shore 0.3 and landmarks 0.25 at slope
  one, so a single station, marina or monument counts in full; crime, camps and bins 0.1, plants
  and works 0.45 taken off), on a base of 0.32, and the raw sum (`LandValue::raw`, kept for `for_use`) is compressed to the value:
  itself up to 0.45, then `0.45 + 0.55 x (1 - exp(-(raw - 0.45) / 0.8))`, so 0.66 (high tier)
  takes a raw score of about 0.76, 0.8 about 1.26 and 0.95 about 2.9; 1.0 is never reached.
  Measured: Harvey's 6,200-person Cinder Ridge went from 71% of built lots at 0.95+ (median
  0.96) to none (median 0.72, top 0.83); the Perfect AI City at 13k from 75% to none (median
  about 0.7, top 0.9); the poorer, industrial demo district reads 0.35-0.45 at the median and
  0.68 at the top.
  *The lie of the land* (`World::splat_setting`): ground 30 m above the median lot is worth up
  to +0.08, the low ground down to -0.03, so hillsides start dearer than the flats before
  anything is built (the waterfront has its premium in `splat_shore`). Thresholds read on the
  new scale: tiers 0.38 / 0.66 as before, crime's neighbourhood factor from 0.6 down (full at
  0.2), downtown cafes and bookshops from 0.66, rooftop drone pads from 0.74 (kept to 0.68),
  towers from 0.6, McMansions from 0.45; the tiers move to where the old thresholds fall on
  the compressed scale: middle from 0.38, high from 0.58 (`Tier::MID_FROM`, `HIGH_FROM`; was
  0.66 on the plain sum). An older save's grid is recomputed on load. Power plants
  and industry are *nuisances*, kept in a second grid so land uses feel them differently
  (`LandValue::for_use`, read through `World::lot_value` for tiers and rents): homes feel
  125% of a nuisance, shops 85%, nightlife 70%, industry 30%. Every plant costs its
  neighbours something: coal 0.40 over 360 m, nuclear 0.18 over 260 m (fear, not fact),
  geothermal 0.09 over 180 m, hydro 0.05, wind and tidal 0.045 over 120-130 m, solar 0.03
  over 90 m. Industry by sub-zone: refinery 0.40/300 m, oil 0.32/260 m, manufacturing
  0.30/220 m, logistics 0.18, fishing 0.14, farming 0.08, clean tech 0.05. Working industry
  also lifts run-down land (up to +0.1, fading to nothing at 0.35): jobs bring activity.
  Contaminated ground costs everyone up to 0.6.
  **Walking access** (the player's rule: a long way to any park or night out makes a
  neighbourhood cheaper): with the strips, two multi-source Dijkstras over the walk network
  (from every node in or fronting a park, and an open entertainment lot) give each cell the
  minutes on foot to the nearest of each (`LandValue::park_min` / `nightlife_min`; a cell
  with no walk node within 48 m has none). Homes get +0.03 for a park within 4 minutes,
  nothing at 10, -0.06 from 20 or out of reach; nightlife +0.02 / 5, 0 / 12, -0.04 / 25
  (`land_value::access_value`). It is in the grid, and `for_use` takes it back out for every
  use but homes. The hover card, `terrain_at` and a lot's `inspect` show it ("parks 4 min
  walk · nightlife 12 min").
- **Tiers and rent**: buildings are low, middle or high tier. New construction takes
  the tier the land value supports (pulled down when people are waiting for something
  affordable). Full buildings on rising land upgrade; buildings on falling land decay.
  Cheap buildings wear out faster. Rent is tier base × land value × condition.
- **Class and migration**: a job pays a wage set by its building's land use and tier,
  times a personal skill factor. Income puts a citizen in a class. Newcomers take the
  best home they can afford (rent at most half their income) and are turned away if
  there is none. Three days of rent stress sends people looking for somewhere cheaper,
  or out of town once their savings are gone; the comfortable move up a tier.
- **Resources and businesses**: fourteen resources (produce, grain, flour, baked goods,
  meat, dairy, groceries, parts, electronics, goods; crude, fuel, plastics, fish). Every non-residential building
  hosts a business with a recipe: farms, protein vats, dairies, mills, fabs and
  assembly plants on industrial lots; grocers, bakeries, gadget shops and offices on
  commercial; venues on entertainment. Output per hour scales with filled jobs.
  Businesses hold stock and cash; prices rise when stock is scarce and fall when it
  piles up.
- **Industry sub-zones** (`nd_sim::industry`, `nd_sim::ground`, `world/industry.rs`):
  industrial land splits into farming (vertical farms, grain farms, protein vats, dairies,
  mills), oil extraction (oil wells: crude), refinery (crude to fuel and plastics),
  manufacturing (parts fabs), logistics (container yards; truck and bus lots), fishing
  (fisheries landing fish; fish plants turning fish into food counted as meat) and clean tech
  (assembly plants). A new industrial lot picks its kind by weight: farming by soil fertility
  and clean ground, oil only over a deposit and then strongly, fishing only by the water,
  logistics by shipping ports, freight yards, the highway and bus depots, clean tech by
  dear land, heavy kinds by cheap land; then neighbours: the same kind clusters, farms and
  fishing keep away from oil and refineries, refineries follow the wells. The player's bias
  (`World::set_industry_bias(IndustryArea::Lot | Polygon, Option<IndustryKind>)`) forces a kind
  over an area (or with `None` hands it back to the ground); standing industry of another kind
  retools on the spot. The ground is read from the landscape seed: fertility (river banks,
  plains), oil patches (about a tenth of the land), rock (noise and slope), sand (sea shore)
  and water proximity. Contamination is a saved 32 m raster (0..1): per working hour at full
  staff oil wells pour 0.01 within 60 m, refineries 0.004 within 80 m, chemical works 0.008
  within 140 m, strip mines 0.004 within 120 m, smelters 0.006 within 130 m, factory farms
  0.004 within 110 m, coal plants 0.003 within 120 m; each day an eighth of every cell seeps
  into its neighbours and 1% fades (half-life about ten weeks), parks take another 3% a day
  off the ground under them, and the player can pay to dig it out (`remediate`, 450 per cell
  of full contamination, at most 300 m round). It also drifts (`Contamination::drift`, daily,
  only cells over 0.01): up to 5% of a cell a day blows one cell downwind (the wind's strength
  times the direction the smoke drifts, `weather::wind_dir`), and 15 to 30% of a cell in or on
  the bank of a river or stream (by its current) is carried one cell downstream; nothing is
  made, what leaves the map is gone. In the game the Bulldoze tool's "Clean up ground" mode
  (a radius of 20 to 300 m) shows the circle, the stained cells, the cost and the days by the
  pointer, green or red like the Build hover (`World::cleanup_quote`: refused on clean ground,
  where a crew is already at work, or when the treasury is short); a click pays up front and
  sends a crew (`World::start_cleanup`, saved in `World::cleanups`) that digs from the middle
  outwards over 2 to 6 days (1 + amount/20 + radius/150, rounded up), clearing everything
  inside the radius it has reached each hour, what seeped back in too. The site shows a
  taped fence, fresh earth to the dig front, spoil heaps, workers in hi-vis and swinging
  excavators (`render/cleanup.rs`). The remote API's `cleanup` (with `quote`) does the same;
  `remediate` still cleans at once. Grain farms yield 0.6 + 0.8 x fertility and lose
  up to 90% on fouled ground (pasture farms and dairies 70%, vertical farms, chicken farms and
  fisheries 50%); container yards and truck lots burn fuel and earn handling fees (2 per filled
  job hour, more near a port or yard), truck lots a bay fee per parked lorry.
- **Dirty industry** (`industry::IndustryKind::dirtiness`, `world/dirt.rs`): strip mining
  (0.75), oil extraction (0.55), refineries (0.8; a chemical works with mercury cells 1.0),
  smelting (0.9: smelters turn 1.5 ore and 0.1 fuel into 1 metal, base 75, shipped out as soon as 8 hours of it stand in the yard) and factory
  farming (0.6: feedlots turn 2 grain into 12 livestock; still food) are dirty (coal
  plants keep their own pollution and nuisance figures and are not on this table). A dirty plant is felt out to 150 + 250 x dirtiness
  metres (about 290..400 m); what a neighbour suffers is level x sensitivity x (1 - d/reach)^1.5
  per plant, combined as 1 - prod(1 - hit), capped at 0.9 (its *exposure*). Level is the
  dirtiness times 0.6 + 0.4 x staffing (0.35 when shut). The compatibility table
  (`industry::sensitivity`): farming 1.0 (0.6 against a feedlot), fishing 0.9, clean tech 0.8,
  factory farming 0.3, manufacturing 0.25, logistics 0.1, the dirty kinds 0; homes 1.0, shops
  0.7, nightlife and civic 0.6, parks 0.5. Exposure takes up to 0.3 off a lot's value (so rent
  and tier), up to half a farm's or fishery's output, and up to 85% of the chance homes, shops
  or nightlife are built on a lot; new industry squares its weights by (1 - exposure), so a farm
  by a smelter is rare and a fab or a yard is not. Residents of exposed or poisoned homes (over
  0.12) lose up to 0.04 mood a day and some complain (Noise & pollution). Smelters only come
  with a working strip mine within 1500 m, two to a mine at most. Loaded lorries wear the road by their load:
  ore and metal 2.5, crude 2, livestock 1.6 times an ordinary lorry (`road_wear::vehicle_load`).
  The looks: sooty stained yards, slag and ore heaps, a manure lagoon, a poisoned settling pond,
  smoke, fumes and dust from their vents, and the contamination stains the ground in the city
  (sickly yellow-green to dead brown) and has its own map layer.
- **Truck and bus lots**: a logistics building with one bay per 90 m² (4..40). A lorry that
  has delivered parks at the nearest lot with room within 700 m, else drives back to the
  business that sent it, else to the nearest lot with room at any distance, else out of town by
  the edge: no lorry ever vanishes. A seller within 800 m of a lot with a lorry parked sends its
  next load out in the longest-parked one; at 22:00 lorries parked 18 hours or more drive home to
  their business. Refineries are only developed with working oil wells within 1200 m or a
  shipping port to land crude. A bus depot with no room left dispatches into
  the lots within 450 m (a quarter of their bays). Lots develop near depots, ports and yards.
- **Market**: a buyer whose stock plus inbound deliveries drops below half its target
  asks for the cheapest supplier, delivered price including haulage distance, strikes
  the deal, and a truck carries it. With no local seller it imports at a markup from a
  truck entering at the city edge; surplus raw outputs export at a discount. Trucks on
  the road are capped; beyond the cap deliveries are instant.
- **Hunger**: citizens lose satiety every hour and eat from their pantry at home.
  When the pantry is empty they shop for groceries; at lunch they may buy baked
  goods; the comfortable buy goods for mood. Days without food cost mood and
  eventually drive people out of town.
- **Business accounting**: businesses pay a share of their workers' wages daily.
  Deep in debt for five days means closure: workers laid off, the building goes
  derelict, and after a week it is demolished so the lot can be reused. Offices
  and civic buildings are publicly funded.
- **Cars** (`world/cars.rs`, `vehicle::Vehicles`): every private car is a record with a place:
  parked (the owner's home spaces, a building's garage, a parking business, a kerb slot, or squeezed
  onto the street when nothing was free) or on the road. A car comes into the city only with a
  household driving in (along the highway or a road off the map edge, from its far end out of sight),
  or when bought (at a dealer, the nearest goods or gadget shop, and delivered home by its driver; or
  out of town, driven in from the edge); it leaves only when sold (driven to the dealer), scrapped, or
  driven out of town by an emigrating owner. A trip by car starts at the car: the driver walks to it
  (within 320 m) and it pulls out of its kerb slot into a gap. At the far end it uses the
  destination's own spaces; else the driver weighs the nearest free kerb slots and parking lots by
  the drive there (by the route it would really take), the walk back and the fee; with nothing in
  sight it circles the streets around (counted as circling, the signal development opens lots on),
  and after four tries parks anywhere on the street. The hunt is also timed: after 20 minutes the driver looks further and at
  banned kerbs, after an hour it stops looking and leaves the car where it can (a banned kerb
  nearby, a free one within 700 m, else squeezed in where it stands), and a worn-out hunter
  standing two minutes in a queue does so on the spot instead of waiting to reach its street. Kerbs: one slot per 6.5 m on both sides of
  streets, none within 12 m of a junction, none on highways, bridges, avenues (clearways) or streets
  fronted by shops and nightlife (loading). A car keeps its space until it drives off, so a lot's
  occupancy is the cars standing in it, overnight too. Cars parked at a building that goes are moved
  to the street. Lorries return to their business or a truck lot (`world/industry`); delivery vans
  to their shop.
- **How people travel** (`World::mode_options`, `choose_mode`): per trip each citizen weighs walking,
  their bike, the bus, the train and their car (when parked nearby) in minute-equivalents: time
  (weighed by ambition), money (fares, fuel, the expected fee; weighed by income), comfort (long
  walks tire, more so for children and the old; cycling worries the cautious; the unsociable mind
  the crowd; a car is a comfortable seat, most of all to hedonists) and remembered pain, with a
  little daily whim. Pain (`Commute`, bad half-hours per trip, fading daily) comes from long walks
  and rides, slow transit and hours-long waits, jams, the hunt for a space, fees and the walk back
  from the car. Habits follow once a day: long walks buy a bike at a shop, parking misery or endless
  jams sell the car when there is another way, transit that keeps failing (or walks and rides too
  long) buys one if the household can afford it; memories record each. The report counts trips by
  mode per day.
- **Places, taste and habits** (`nd_sim::places`, `world/places.rs`): every venue, door shop and
  park has a character (`PlaceCharacter`): its vibe (dive bar, club, karaoke, lounge, arcade,
  cabaret, night market from the name and tier; café-bakery, grocer, gadget boutique, park), polish
  (tier and condition), price (tier), noise (vibe, louder when packed), crowd and the crowd's class,
  safety (crime heat around it) and cleanliness (its bin overflowing). Each citizen has a taste
  (`Taste`) from personality, age, class and budget: the rich and old want polish, safety and
  quiet and shun cheap places; young hedonists like loud, packed and cheap; the cautious avoid hot
  spots; curiosity sets how often they try somewhere new. Every visit is rated on arrival: the fit
  of character to taste, events (a fight, rude staff, overpriced, too crowded, great music, great
  food, meeting a friend, coming with family or coworkers) and luck. A `PlaceBook` per citizen keeps
  twelve places (visits, last visit, average, best and worst, regular, never-again; the least valued
  is forgotten) and eight acquaintances met at their haunts. A great first night is written down
  ("Found The Velvet Den..."), three good visits make a regular, a terrible night at a place that
  is not a habit means never again. Choice (`choose_place`, `choose_shop`): known places by
  experience, regular bonus, friends who are regulars there and distance; unknown ones by first
  impression (fit) and word of mouth (`Reputation::pull`: rating, sworn-off customers); a softmax
  over the pool, with curiosity deciding between known and new; `Intent::Explore` (rule mind:
  "looking for a drink") looks at unknown places near by. Groceries: the habitual shop unless
  another is far cheaper. A sociable citizen may bring a free household member or coworker along (5% + 20% x sociability per outing); a regular spot adds 6% to the evening's pull.
  Regulars stand an extra round (20% on the tab; drinks cost 0.9/1.0/1.4x by tier). All draws are
  seeded by citizen and tick, so replays repeat.
- **Parking rules, towing and city lots** (`crate::enforcement`, `world/enforcement.rs`): every
  street car stands in a kerb slot of its own (a driver who finds nothing legal takes a free slot on
  a street where parking is banned, or keeps circling; never on top of another car). Kerb parking is
  limited to 2 hours on every street (`Vehicles::kerb_rules` overrides per street); residents within
  150 m of home hold a permit. A parking enforcement office (Services, three tiers) sends officers,
  two a car, round the streets within its reach from 07:00 to 21:00: a car over its limit gets a 35
  ticket (household to treasury, ledger "parking fines"); an hour later, or at once if parked where
  parking is banned, the tow truck is called. *Clamps* (`[clamp]`): a car still over its limit when
  the grace runs out is clamped instead (`ParkSpot::booted`, the orange clamp) with a 90 clamp fine
  (`CLAMP_FINE`) and the tow booked for six hours later (`CLAMP_LIMIT`, `TowReason::Unpaid`); the owner
  pays it by phone when awake and able (35% an hour by a hash, 07:00-23:00, the household's money),
  the clamp comes off and the tow is cancelled; unpaid (or broke) the truck comes when the time runs
  out and the fine is owed at the pound on top of the tow fee. The inspector shows the time left;
  `report`'s parking block counts clamps, fines paid and cars towed unpaid. A car parked where
  parking is banned is still towed at once. A company in town sends a truck only when one is free; the company from out of town has four on the city's roads at most, the other jobs wait their turn (uncapped, a city short of parking had 170 tow trucks in its jam). Tow companies are businesses on industrial land
  (logistics); the nearest with a truck free sends one, which hooks the car where it stands and
  carries it to its pound (drawn on the truck as wrecks are). *Tow trucks at work* (`world/tow_moves.rs`):
  a car on a one-way street whose legal way in is a long loop (150 m+ longer than coming in from the
  far end, and at most 160 m up the street) is reached from the far junction: the truck leaves the
  lanes there and creeps up the kerb side against the traffic at 3 m/s with its ambers flashing
  (`StuntKind::Manoeuvre`), stops nose to nose with the car, hooks it and turns round in the street
  to leave the right way. A truck that has hooked its car (or a wreck) facing away from its pound or
  yard turns round in the road (a slow arc onto the other lane) when that is 120 m+ shorter than
  driving on. Traffic meeting a truck on such a move slows and eases round it, or waits where there
  is no room (the scene zones, `traffic/scene.rs`); `enforcement` counts `contraflow_total` and
  `u_turns_total`. With none in town a company from out
  of town comes in from the edge 45 minutes later and charges 180 instead of 80. The owner pays the
  fee plus 15 a day and goes to fetch the car (a pound out of town delivers it), or after three days
  it is auctioned and driven away. Crash wrecks go to the same companies. The player places parking
  lots the city runs (Buildings, Transit): small (12 spaces), large (30) or a multi-storey car park
  (120), on any lot with a street; the fee (default 2-3, `parking_price`) goes to the treasury
  (ledger "city parking fees"), attendants and upkeep are the city's.
- **Tow this car** (`World::order_tow`, `order_tow_car`; inspector button, API `tow_car`): the
  player can have any resident's car clamped. A car on the road pulls over into a free kerb slot
  within 60 m (else the street's edge where it is), off the lanes so traffic passes; its driver and
  passengers get out and walk on. An orange clamp (`ParkSpot::booted`) is drawn on the street-side
  front wheel and the car cannot be driven, sold or taken out of town; a tow job (`TowReason::Ordered`)
  goes to the nearest tow company, or one from out of town. The owner's mood drops, they remember it
  and complain about parking, and they pay the fee at the pound as for any tow. Garaged, hooked and
  impounded cars cannot be clamped; through traffic, lorries, buses, cabs and service vehicles neither.
- **Passengers** (`crate::rides`, `world/rides.rs`): a car carries up to four passengers (a
  motorcycle or scooter one). A passenger (`Activity::Passenger`) walks beside the driver to the car,
  rides in it out of sight at its position (listed in `Car::occupants`), and when the driver parks
  gets out there and walks on to their own destination; if the drive falls through they go their own
  way. Rides come from a household member or coworker brought along on an outing or a date, from
  coworkers who live at the driver's door and are due on shift (to work) or whose shift is over and
  who live near the driver (home), family out at the same place going home, a parent at home with the
  car driving a child to school (on the way to work, or now and then from home and back: the car stops
  at the school, the child gets out, it drives on), and the mode choice: a seat in the car of family
  or a coworker setting off by car from the same spot to within 300 m of where the trip goes is a
  free, comfortable option (`ModePlan::Lift`).
- **Parking businesses**: [parking-market] when drivers circle where the lots are full, a resident
  opens another (`world/parking.rs::maybe_open_parking`): the trigger is four drivers circling today
  (three while the town's parking businesses cover their costs 1.3x, ten while they lose money); the
  spot is the concerns' Parking hotspot of the last three days, then where the last driver gave up;
  nothing opens while the lots within 220 m were under 85% full at their busiest, within 150 m of a
  lot opened in the last two days, or once paid spaces match the town's cars (60 at least). The founder is a
  grown-up within 1.5 km who can pay (300 for a surface lot, 1,200 for a car park), who owns it; with
  nobody able, an operator from out of town comes only for three times the trigger. Every parking
  business sets tomorrow's fee at midnight by its busiest moment (`Building::parking_peak`): 85%
  full or more, +15% (up to 2.5x its base by tier and land value); under 40%, -10% (down to 0.6x).
  The lot is a surface lot where land is cheap (about one space per 28 m², two jobs), a
  multi-storey car park on middle land (three to five decks). It is the cheapest business in town and lives or dies on fees (a fee by
  tier and land value per car parked). Towers (high tier) bring their own spaces for about half
  their homes and jobs, middle-tier blocks a quarter, cheap buildings none. Parking lots render
  their occupancy; cars at the kerb are drawn there.
- **Pedestrian network** (`nd_sim::walk`): a second graph for people on foot. Pavements
  on both sides of every street, path centres, a crossing hub at every junction, a
  walkable setback loop inset into every usable lot with its front joined to the
  pavement, an alley cross through the Stacks, and gates between lots that share a
  few metres of boundary (never through industrial lots or gated high-end homes).
  Walkers route over it with A* (interior edges cost 15% more), so people cut through
  blocks; bikes stay on streets and paths. Every pedestrian route counts the lots it
  passes (`Lot::footfall_*`), and each hour 6% of the people who passed a grocer,
  bakery or gadget shop buy something off the shelf: walk-in trade that rewards shops
  where the feet are. The P overlay draws the in-block network.
- **Pedestrians and the rules of the road** (`nd_sim::pedestrians`, `world/pedestrians.rs`,
  `traffic/crosswalk.rs`): streets are crossed only at crossings. Every arm of a junction of
  three or more streets has a crosswalk where its zebra is painted (pavements meet round the
  corners, never through the junction); a footpath over a street and every ~120 m of a block
  longer than 170 m get a zebra; a dead end is crossed at its end. At a signalised junction
  the crosswalk across an arm shows walk while the traffic parallel to it has the green (not
  during a protected left arrow, and not in the last 4 s of a green that is about to end);
  walkers wait at the kerb in a small cluster facing the street, call the walk phase (so it
  is never skipped or cut while someone is crossing) and step out after a look. Cars stop at
  the stop line before the zebra on red; turning cars give way to anyone on the crosswalk they
  turn over, and at a zebra drivers stop for anyone on it or about to step onto it, if they can
  stop at 5 m/s². Jaywalking: the rebellious (low caution, high hedonism), the drunk and the
  late for a started shift sometimes cross against the light, or plan a route straight over
  mid-block between opposite lot fronts, when the gap in traffic looks big enough (drink makes
  it look bigger); the cautious, the old and children never do. A car that drives over an
  occupied crossing at speed is a near miss or, faster and unluckier, someone hit: the car
  stops, a witness calls 911 and an ambulance comes (`report_crash`). On the pavement people
  keep 0.35-0.75 m right of the centre line, fall in behind someone slower or standing, step
  aside for anyone else within 0.75 m (0.5 m in a crowd), using a 2 m grid of walkers per
  tick. `report.traffic.pedestrians` counts walkers waiting, jaywalkers, near misses and
  collisions today.
  Crowds are not queues: each person walks at their own pace (0.86-1.14x) and prefers a place
  across the path spread from left of centre to well right; the width they have is the pavement
  between kerb and building line, the zebra's depth, or 2.6 m either side on lot paths, parks and
  plazas (a crush spills 0.9 m further). Someone slower ahead is passed on the left (or right) where
  it fits; walkers only fall in behind when boxed in. With the walk light on, cars standing or
  crawling (under 2 m/s) across a crossing do not hold walkers back: they weave round the ends of
  the stopped cars.

  Cyclists (`world/riders.rs`) ride the road shoulder and keep the rules too: into a
  signalised junction they stop short of the crosswalk while their approach has red (and call
  their phase, as a detector would), anywhere they give way to people on a crosswalk they are
  about to ride over, and straight over a street on a footpath's line they cross like a walker
  (walk light or a gap at a zebra, and drivers stop for them on it). The rebellious, drunk or
  late may run a red when the cross traffic leaves a gap (`red_lights_run_today`); the
  cautious, old and young never do.
- **Homes on paths**: in a residential zone a path counts as frontage, so houses and
  blocks line footpaths. Such lots have no car access: a resident's car trips start and
  end at the nearest street, the last stretch is walked over the pedestrian network,
  and the car is parked at a nearby parking business for a fee (or circles, which pulls
  a lot into existence). Shops and industry still need a street.
- **Parks** (`nd_sim::park`): a park is every park parcel that touches another, so a
  zone cut by a street or a footpath is still one park, and park parcels may line a
  footpath like homes do. The planner works per connected park: each lot gets a hub
  deep inside it, spokes from the hub to its street gate and to every gap it shares
  with a neighbouring park lot (so paths run lot to lot), a loop on big dead-end lots,
  spurs to its features, bends around them, and benches every 9 to 24 m along the paths
  plus a half-ring facing the water or the stage. Features come from a budget by the
  park's total area and the land value around it, handed out largest first to the lot
  with the least feature area per square metre that has room: cheap land gets a court
  and maybe a skate park and nothing else; the middle gets playgrounds, a pond, a kiosk,
  flower beds, a dog run; dear land gets a lake with a jetty, a fountain, a bandstand, a
  carousel, a ferris wheel, a statue and formal avenues. The plan lives on the lot
  (`Lot::park`) and is rebuilt with the lots.
  Park paths and benches are part of the pedestrian network: path vertices are nodes,
  entrances join the lot loop, benches hang off the nearest path node, and park edges
  cost 8% less than pavement, so walkers cut through parks on the way elsewhere. A park
  visit no longer vanishes into a building: the visitor walks the paths to a bench and
  sits (`Activity::Sitting`, drawn seated, visible, facing the path or the water),
  then walks off the bench when the next decision comes. A bench has three seats
  0.6 m apart (`world/benches.rs`); a visitor claims a free seat when they reach the park
  and holds it until they get up, so nobody sits on anybody. A park with every seat taken
  sends the visitor to the nearest park with room within 600 m, or home. Strangers who sat
  side by side for 15 minutes or more may get talking (10% plus 30% of their mean sociability)
  and become acquaintances (`PlaceBook::meet`), as regulars of a bar do.
  **Paths into parks** (`world/grounds.rs`, `walk::add_grounds`, Cities: Skylines style): every
  park, and the grounds of a stadium, a monument's plaza, a school's campus and a railway station,
  shows gates on its outer edges (one per 40 m of edge, 1.6 m outside it; none on edges shared
  with more of the park, in the water, or within a road end's snap reach of a road). With the Path
  tool they show as small amber rings (green where a path is joined); a path point near one snaps
  onto it, the pointer ring and the gate glow green. A path built with an end on a gate is joined
  there (`World::path_links`, saved): the park takes the end as an entrance and lays a walkway from
  its hub out to it (`park::plan_parks_with`), and the walk graph gets an edge from the path's end
  to the grounds' nearest walk node (a park path, or the lot loop of a stadium or station), so
  people walk from the path into the park. The path stays the player's gesture. When the grounds go
  (rezoned, bulldozed, the lot re-cut away: nothing within 4 m of the end) the join is dropped and
  the path cut back to its last point before that end, or removed when fewer than two points are
  left, with a toast (`SimEvent::PathTrimmed`); no walk edge is left dangling. Other ground with a
  walkway network (a public beach) registers through `walk::Grounds`.
- **Construction** (`buildings::Construction`): nothing pops into existence. `build_on` opens
  a site with a build time of (1 + 0.4 × floors) days scaled by tier, clamped to 1–12 days;
  it offers no homes and a temporary crew of jobs that the unemployed commute to. [pt-growth]
  Crews work the working day only (08:00-17:00, `Clock::is_work_hours`, the hours the tower
  cranes turn), a day's share of the work in those nine hours, so the calendar time is as
  before but nothing rises at night; and each site reports at its own ten-minute slot of the
  hour (`site_slot`, a hash of the building), so the city never grows a storey in one
  synchronised jump. The inspector says when it should be done ("done about day 23 14:00",
  `Clock::after_work_hours`). Progress needs four loads of parts, one per quarter, each ordered a quarter
  ahead from a local seller or by import; a late truck stalls the site at the next quarter.
  Every 10% the renderer redraws the site; on completion the crew is released, the real
  homes and jobs open, and tenants may move in. Player-placed specials and parking lots
  complete at once. Housing and job pressure are damped by what is already under
  construction. Sites are drawn as hoarding, raft, shell to the last full floor, scaffold,
  a tower crane from six floors, pallets, cabin, mixer and hi-vis workers.
- **Demolition** (`buildings::Demolition`): nothing vanishes either. Whatever pulls a
  building down (a bulldozed zone, road works, a shop shut for a week, a take-over, a
  redevelopment) turns it into a demolition site. [pt-growth] It says why
  (`DemolitionReason`: "new development planned", "plots bought together for a new
  development", "abandoned: a new owner is taking over", "road works", "bought out by the
  city", "bulldozed: the zone was cleared", "a neighbour is expanding onto the plot", "burned
  down: clearing the ruins", "bought for a private estate", "cleared for a landmark building",
  "rebuilt for richer tenants"), and the inspector shows a red bar with the progress, when the
  lot will be clear, the reason, the crew and the rubble hauled; the wrecking crews keep the
  working day too. While the old building comes down the inspector says "coming down: 1 floor
  low residential · then Brutalist, 9 floors" (the building's fields already describe its
  successor: a hut being pulled down read as "high tier, 9 floors", F25). The site: residents, households and staff leave
  at once, and a wrecking crew (2 + floors/3 jobs) takes it down over (0.9 + 0.1 × floors)
  days, clamped to 1–3. A pure demolition also carries a placeholder `Construction`, so
  every system that skips building sites skips it, and nothing can be built on the lot
  until it is clear. Dump trucks leave as the rubble piles up (2 + floors/3 loads, at most
  10, spread evenly, the last before hand-over), as real trucks on the roads to the
  nearest freight yard or off the map's edge; the work does not wait for them. When the
  zone itself was erased the site keeps a parcel of the usual size where the building
  stood, removed with the rubble; when a road now runs over the spot, or another building
  holds it, the building is cleared at once (a demolition site already there takes on
  its rubble). Car parks, plants, depots, stations, yards and docks appear at once and
  go at once, as do sites still at groundworks.
  Redevelopment is demolition then construction: the building keeps its id, its fields
  describe the successor, the `Demolition` keeps the old look; materials are ordered only
  when the lot is clear. A civic structure placed over a grown building works from the
  moment it is placed (specials always have) and inherits the clearing: its lot shows the
  old building coming down, and the structure appears when the rubble is gone. (Making
  the structure wait would mean gating plants, depots, stations and docks on a state they
  never had; showing the clearing first gives the same picture without that.) [dock-access]
  Stations, yards and docks are the exception: they stand on footprints of their own and are
  drawn as themselves at once, what they bought out cleared away. Sites are
  drawn as the real building cut down from the top (triangles above the working level are
  dropped, those across it clipped), a raw broken slab with rebar on what is left,
  hoarding, a high-reach excavator with a crusher at the cut (a small machine on the roof
  and a tower crane above 26 m), rubble heaps that grow and are carted away, a dump truck
  at the gate and the crew; the last fifth is bare earth and the last heaps.
- **Eminent domain**: when a parcel changes shape under a building on a rebuild (area off by
  more than 12% or an outline point moved more than 3 m) the building is pulled down and
  residents remember why (`SimEvent::Expropriated`). Specials own their ground: zoned lots
  under a facility footprint go, placement over a built lot is refused, and development never
  builds under a footprint.
- **Redevelopment and density**: a Low-tier building on High-value land for three days is
  rebuilt in place at the land's tier; rents rise and the displacement rules move people on.
  [pt-growth] *How an area evolves* (Harvey's model): new ground starts as low suburbs (the
  neighbour rule and quiet land hold the first houses to one or two storeys, cheap homes to
  one); people walking the streets raise the *land density* (below); where it asks for four
  floors or more, developers *assemble* plots: a small low building (under 1,100 m², at most
  three floors) is bought with two to four small low neighbours of its use (vacant ones too,
  each under 1,650 m², built at least three days, in a compact block, hull within 15%), all
  pulled down ("plots bought together for a new development"), and one building of at least
  four floors goes up across them (`World::assemble_plots`, `Form::Merged`, at the land's tier,
  at least three quarters of what the density and the neighbours allow), so five bungalows
  become a block of flats. It must pay: the new block's land and homes or jobs at 1.2 x what
  every plot sells for. The residents and the neighbours' move out with a memory ("Kessler
  Land bought up ... for a new block of 8 floors. We had to go."), businesses pack up to move,
  a line of news. A developer's own block may be rebuilt taller again as the street gets
  busier (densification feeds on itself; `redensified_total`), and a plain buyout now builds to
  three quarters of what the land density allows, not one storey more. *Owners lose
  buildings* (`World::foreclose_empty`): a middle or high block of homes at most 35% let for
  five days while people wait for a home they can afford is sold to a developer at 60% of its
  price, who cuts the rent (one tier down, refitted). While people cannot afford a home
  (`World::short_of_cheap_homes`: low-tier housing demand over 2, or 20 people on the street)
  cheap blocks are rebuilt cheap (taller, more homes) and full ones are not refitted up. A
  busy town rebuilds a few a day (2 + one per 200 buildings, at most 6; densify tries only uses
  in demand, so homes nobody needs no longer crowd out the shops). Heights are capped only by the land
  density and the neighbours, the city's and a district's height limits, a vertiport's
  clearance and a billionaire's NIMBY; rents are kept low only by a district's look (Kowloon
  stacks). Supertalls and megatalls still go up through the skyline league where the land is
  worth 0.6+ and the density asks for nine floors or more. Stats: `EstateStats::
  assemblies_total`, `assembled_lots_total`, `redensified_total`, `foreclosures_total`.
  New buildings are capped at two floors plus one per ten neighbouring floors within 150 m,
  so high-rises appear only where the block is already dense; the land market below caps
  them again by pressure.
- **The land market** (`nd_sim::estate`, `world/estate.rs`, [land]). *Pressure*: every hour
  (:13) each 64 m cell counts the people present (walkers twice: they are also moving
  through) and the cars in it, folded into a running average (half a day to rise, two days
  to fall; an old save's cold grid takes its first sample outright). [pt-growth] *Foot
  traffic*: the same hour counts everyone out on foot (walking or cycling, at a stop, on a
  bench, loitering; sightseeing visitors; not the homeless at their camps) into
  `Pressure::foot`, smoothed over days (a day and a half to rise, four to fall). The *land
  density* is the pressure plus twice the foot traffic (`Pressure::density_at`,
  `World::land_density_at`): what the floors follow, what the Land density map layer shows
  (a log heat map per 64 m cell, quiet suburb to busy downtown) and what the inspector's
  estate line and the API (`land_density`, `foot_traffic`) report. *Mobility* per cell,
  daily: 0.8 + 0.5 x transit (bus stops within 300 m, a station or metro entrance within
  600 m) - 0.6 x the share of cars standing, clamped 0.5..1.5. A lot's pressure floors are
  2 + (pressure - 4) / 2.5 x mobility (2..40), so height follows how well the city moves
  people; `height_cap` is the lesser of that and the neighbour rule plus five floors. A new
  game is low almost everywhere. Cheap homes on land held to two floors are one storey
  (shacks, trailer parks, huts) with six homes, as many as the walk-up they replace.
  *Prices and buyouts*: every building has a sale price (land at 25 + 400 x value per m²,
  the structure by homes and jobs, tier and condition, a year of a business's recent profit
  as goodwill; mansions x3). Daily, developers (a handful of fictional firms) buy the most
  pressed low buildings (pressure floors at least twice the height and three more) while
  demand for the use is over 0.2, a few a day (1 + buildings/400, at most 4), only when the
  new building is worth 1.2 x the price: residents move out with a memory, a business
  closes, and the existing redevelopment (demolition, then construction) builds bigger.
  *Owners*: `Building::plot.owner` names an `Owner` (a firm, a developer, a household, a
  billionaire) with its properties; none means an independent owner. Selecting a building
  outlines everything its owner holds (`World::owned_with`). *Firms grow* (generic trigger
  `World::wants_to_expand`: open six days, the last three days' takings over 1.1 x costs,
  cash for the move, mostly staffed; two a day at a 20% roll): taxi companies, bakeries
  (pizza) and tow yards open branches on vacant lots of their use as far as can be (450 m at
  least) from the firm's other properties; other kinds buy the plot next door (two for oil
  and farms), even landlocked backland, vacant first then cheapest, and rebuild across them
  (`estate::expansion_form`: hotels as mega hotels, galleries as museums, the rest as a
  bigger building of their kind; oil fields are one field over every plot). Plots stay
  lots for zoning and each names the building (`plot.extra_lots`); a built neighbour is
  demolished first and joins when its rubble is gone (`EstateMarket::reserved`). A merge
  is refused unless the plots make a compact block (hull within 6% of their area); the
  renderer draws the building on `World::footprint`, the merged outline. Rebuilds carry
  extras; a building's removal frees them. *McMansions*: at most one a day (30% roll), one
  per six households earning 220+, on two touching residential plots in a block held to
  three floors on land worth 0.45+, bought by the richest such household, which moves in;
  owner-occupied homes pay no rent. *The super-elite* (fictional people only, named by the
  town's own generator): rare (5% a day, more with each one already here) and only with
  extreme disparity (900+ people, Gini 0.40+, the top twentieth earning 3.5x the median),
  one per 2,500 people, at most six. A local high earner made rich, or a visitor who stays
  (put up in the best free home meanwhile), worth 1-45 billion, buys two to four touching
  residential plots (1,400 m² at least) where pressure is under max(4, 0.3 x the town's
  median), with no nuisance or fouled ground, no works or nightlife within 160 m and little
  built round it; land value counts, parks hardly, other mansions within 650 m draw them.
  Also one or two other homes, one to three businesses and two to six supercars
  (`Class::Elite` above the three income classes; class counts and the Gini are unchanged).
  The finished mansion starts a NIMBY pull: land value +0.22 fading 4% a day within 200 m
  (260 at most), nothing redeveloped taller than three floors there, and up to two cheap
  homes a day priced out (rebuilt middle tier), six per mansion at most. *Convoys*: when a
  billionaire drives (never the bus or a bike), black escort SUVs (2 + 3 x log40 of their
  billions, 2..5) spawn on their tail and the richest (20 billion+) gets a police car ahead;
  escorts take the boss's driving style, follow every change of course and leave at the end
  of the run. Guards (one per escort car) walk round them on foot and stand at the kerb for
  an hour where the convoy stopped (drawn by `render/convoys.rs`).
  *Relocation* (`world/relocate.rs`): a business bought out of its building (road works through
  it, a city building on its lot, a developer's rebuild, a neighbour's expansion, a mansion) is
  packed up first (`EstateMarket::moving`: cash, books, stock, owner, staff). The next development
  round moves it to the nearest vacant lot of its land use within 1.5 km with at least half its
  ground, if it can pay 200 + 20 a job: same name, same owner, a building site that reopens with
  its old staff hired back if they are still out of work; "moved from ..." in the inspector and a
  news line. It looks for two days, then is gone. Kinds tied to their ground (farms, wells,
  fishing, marinas) and parking, taxi yards and cargo hubs close as before.
  *The well-off invest* (`world/rich.rs`): daily (35% roll) the richest household earning 220+ a
  day with 1,000 in the bank, not a billionaire's, buys in its top earner's name a business in
  profit (if it owns none) or else a second home in a middle or high block of up to 24 homes,
  putting 30% of its cash down; at most one property per 220 of daily income (four at most).
  *Garages*: a finished mansion takes the billionaire's supercars beyond the one they drive, a
  McMansion a second car, as real cars in the house's own spaces (`OwnedCar::spare`), never driven,
  sold when the owner is gone.
- **Mega-corps** (`nd_sim::corp`, `world/corp.rs`, [megacorp]). *Growing into one*: daily (25%
  roll) the biggest company (an owner record of a firm or a founder, or a lone business) whose
  holdings take 9,000 a day (last three days' average), employ 30 and number two (or twice the
  takings and staff in one building) becomes a mega-corp, in a town of 600 people or more, one
  per 3,000 people, four at most. It gets a coined fictional name (two syllables nobody trades
  under and a sector word), a brand colour (12, a free one first) and a logo: a shape (circle,
  ring, hexagon, diamond, triangle, square, chevron) with its initial cut into it, an 11x11
  pixel mark (`Brand::logo_bitmap`) the renderer, the trucks and the UI all draw the same.
  *Its day*: it skims 40% of each holding's profit (a till keeps 500) into its purse and pays
  the player's levy; with no HQ it buys the dearest, most pressed commercial plots downtown it
  can assemble (up to three touching, 900 m²) and raises a tower there (pressure floors + 10,
  18..40 floors; hook for feat/supertall: order it as a league tower when the city's height
  limit allows). It buys up to three things a day nearest its HQ *by road* (one Dijkstra per HQ,
  cached against the road network's version): businesses in profit (over their last three days) of any kind (a kind it does
  not own counts as 30% nearer, so it spreads into every industry), blocks of homes for its staff
  while it has under 0.6 homes a head, and vacant land of any zoning (a branded office, shop,
  hotel, bar or works on it, a kind it lacks first; at most one buy in four is land). Staff move into its housing (six households a day, nearest their work)
  and pay 30% less rent. *Balance*: the player caps how much of the town's ordinary buildings
  one corp may own (8% by default, 2-25%, at least four) and sets a levy (0-20% of its daily
  takings, booked as business tax) or bans new corps (City Data › Economy › Corporations); a
  corp over the cap sells its newest holdings, two a day, never the HQ. Its holdings are off the
  market (developers, the rich and billionaires leave them alone). *On the map*: a lit rooftop
  sign with the logo on every holding, a facade banner on the taller ones, a smaller banner on
  staff housing, and on the HQ (a twist or slab tower) two bands of light, a screen wall with the
  logo on all four sides and a spire; its lorries and vans wear a dark brand-tinted livery with
  the logo lit on the box (`Livery::Corp`). The inspector shows the owner corp with its logo.
- **Universities** (`ServiceKind::University`, `world/campus.rs`). Placed under Services
  (community college, university, institute of technology; 825 m² at least): a stone hall with a
  portico, a clock tower and a lawn quad. It founds a campus (`OwnerKind::University`, a crest
  in a free colour). Daily it enrols the town's 18-24 year olds (about two in three, nearest
  first) up to its places (120/320/800), takes 6 a day tuition, lets students arrive from out of
  town into free dorm rooms (four a day), claims parks within 336 m as campus plaza (flags in
  its colour, the crest at the gate, coloured paving bands) and, every other day, grows one
  annex within 420 m: a first dorm (a block of homes nearby bought as it stands, else a new one
  on the nearest plot, bought and cleared if need be), then administration offices, a sports
  centre, a theatre and a frat house, then dorms until 35% of its places are housed and more
  frats, up to 4/7/12 annexes by tier. [pt-growth] The campus grows outward from what it has:
  a plot next door to the hall or an annex (a shared edge, or corners within 10.5 m across a
  path or zone gap) comes first, vacant before built (a house next door before an empty plot
  across the road), then the nearest plot to any campus building, all within the reach. Dorm rooms are let only to students (45% off the rent).
  Offices, theatre and sports centre are the campus's own (special: never sold or rebuilt).
  *Students* go to class at 09:00 and stay till 15:00; at 20:30 about a third go out to a bar,
  a pizza place or the campus theatre within 600 m. Pizza places, cafés (bakeries) and bookshops
  (gadget shops) near campus see 0.05 walk-ins an hour per student (full at the gate, none at
  600 m), and a new shop or bar within 420 m of a campus with 20 students opens as a pizza
  place or a bar half the time.
- **Schools and families** [megacorp]: a household with children takes a home a school covers
  (coverage 0.3+) when one of its tier is free, and a family living out of reach of any school
  moves to a covered home of its tier or better (12% a day, while the town has a school); the
  childless choose as before. A school's land value lift follows its staffing (x0.8 empty to
  x1.2 full).
- **Railway** (`RoadKind::Rail`, `nd_sim::rail`): track is a third kind of way in the road
  network, single track 4.8 m wide. A trunk line crosses the map 480 m south of the
  centre from the start, edge to edge and on out of sight; the player lays branches off it with the Road tool's Rail
  preset. Track ends snap only to track and road ends never snap to track; a track
  crossing a street shares a node, which renders as a level crossing. Track cuts a
  corridor out of parcels but never fronts them, carries no lanes or pavements, and
  walkers' and drivers' searches ignore it. Stations and freight yards are pieces
  (`rail::Facility`), as in Cities Skylines: a footprint with a stretch of track along
  one side and a stretch of street along the other. They go anywhere, with no zone or
  lot needed. Set down within 40 m of track the piece aligns to it and fuses its rail
  side onto it; set down by a street it fuses its road side; otherwise connector stubs
  (a 1+1 street, a length of track, each reaching 24 m past the footprint) are laid as
  plan gestures so roads and track can be snapped onto their ends. The footprint
  becomes a lot of its own (zoned lots under it give way) so the building, trains,
  riders and lorries work as for any lot; the platform or apron is laid along the rail
  side (`RailLink`). Nothing runs until the rail side reaches an end of the railway and
  the street side reaches the city's roads (`RailLink::works`), recomputed on every
  rebuild; unjoined connectors show red in the Rail tool and the inspector says which
  side is missing. [dock-access] No road is needed to place one. A passenger station whose
  track is joined but whose street is not is a halt (`World::halts`): passenger trains call
  and stand their time, nobody gets on or off, it hires nobody. "The roads" are a road
  component holding zoned lots or reaching off the map (`World::network_components`). The
  free ends of an unjoined street stub carry an amber "connect a road here" ring and label
  (`nd_app::stub_marks`), and the inspector, the Build tool's hover card and the alerts say so. Track drawn by the player is eased when built: ends joining existing
  track leave or arrive along it through a 60 m turnout, corners are rounded to a 120 m
  radius, long legs stay straight, so there are no right angles or square forks.
  Trains run west to east from one end of the line to the other, calling at every
  facility in order; a dead-end spur is run to the last platform and back. Branches
  have no passing loops, so one train holds the branch at a time: others wait at the
  switch, and no new train is dispatched while one is waiting. Passenger trains come
  every two hours of clock from 05:00 to 23:00 when a station exists; freight trains
  every three hours when a yard has goods waiting, or six regardless (and an extra one 45
  minutes after the last when goods were left behind: more than a full train waiting, or an
  order that already missed one). Trains are not
  saved; they are back on the next headway. People take the train for trips over 500 m
  between two stations within 240 m of each end (`TransitLeg::Rail`), pay a fare, and
  newcomers mostly arrive by train when a station exists. With a freight yard, imports
  come by rail at 80% of the price; exports earn 20% more. [freight] The goods really ride the
  trains: an order waits out of town (`Rail::orders`, saved) until the next freight train is
  made up at the west edge of the map; its orders for the yards it calls at are loaded there,
  oldest first, into wagons of 50 units (5 to 12 wagons, enough for what waits; an order that does
  not fit waits for the next train, but an empty train always takes the first, so even a 400-unit
  order leaves). They ride in on it, come off at their yard and go on by lorry (or straight into a
  cargo hub on the siding). At each yard the train then takes on the exports from the apron, as
  much as its wagons have room for. The train inspector's manifest is what is aboard: loads with
  their yard and buyer, wagons, units aboard of capacity. A train taken off by a rebuild, or run
  out of town with goods for a yard that went, puts them back in the queue (by road when the yard
  no longer works); trains are not saved, but goods aboard are saved back into the queue. Orders
  for a yard that stops working, when no freight has run for two headways, or after twelve hours
  waiting come by road from the edge instead. Older saves' orders waiting at yards move to the
  queue on load. Stations lift land value, yards lower it.
- **Buses** (`nd_sim::transit`): the player draws lines and places a depot; without a
  depot no bus runs. A line is a loop of stops on streets; a click snaps to the nearest
  street and the stop sits on the kerb of the side clicked, so buses driving that way
  serve it (the two sides of a street are two stops). Each leg is a lane route between
  consecutive stops; legs with no road are flagged and the line does not run. Buses are
  cars of the traffic model with a `Bus` state: 12 m long, so following cars keep a
  longer gap, and a `hold` that keeps them standing at a stop for 18 traffic seconds
  while riders get off and on. From 05:30 depots send out one bus every few ticks until
  every line has the buses it asked for (ten per depot); from 23:30 buses finish the
  loop and drive back to the depot. A bus reaching a stop is rerouted in place to the
  next stop, so it never leaves the road. People without a bike take the bus for trips
  over 160 m when a line has a stop within 110 m of both ends and walking the two ends
  saves real distance: they walk to the stop, wait (`Activity::Waiting`, visible),
  board when their line's bus dwells there (`Activity::Riding`, carried with the bus),
  pay a fare into the treasury, alight at their stop and walk the last leg. Riders who
  wait three hours of clock walk instead. Buses cost upkeep per day and depots too.
  Editing: stops can be added, dragged, deleted; buses per line set; lines deleted (the
  buses are taken off and riders stranded on the kerb). Roads changing re-snap the stops
  or drop them.
- **Subway** (`nd_sim::subway`, `world/subway.rs`): the Cities: Skylines metro. A station is a
  lot taken by the city (any land use, 120 m² or more, 160 m from the next station; 15,000 to build,
  140 a day): at street level only its entrance shows, a plaza with a stairwell under an arched glass
  canopy and a pylon with the lit M (`buildings::subway::metro_entrance`); its platform lies 16 m down
  under the lot's middle. Stations are placed first (Build window, Transit, Metro, Metro station);
  the metro line tool (Transit, Metro lines; the Transit tool's Metro mode) or the `subway` /
  `metro_line` API lays a line through stations in order: one polyline of twin bores that ignores
  the street grid, laid with the
  railway's easing (`rail::smooth_track`) with a 90 m straight through every platform so a bend
  never pulls the tunnel off a station; 22 per metre to bore, 90 per km a day to run. Tunnels are
  not in the road network on purpose: track there would join the railway's searches, become entry
  points for intercity trains and make every edit rebuild parcels and lanes. Trains are the
  railway's own `Train` records (poses, stops, riders) in a map of their own, not saved: every
  `headway` minutes (default 10, 4..60) from 05:00 to midnight a four-car set leaves each terminus,
  runs at 22 m/s, stands 16 s at every platform and is gone at the far end (each run costs 2.5). Riders: a
  trip of 420 m or more with an entrance within 320 m of each end on one line (450 m for the
  Central Station) and walks that save real distance (`Subway::plan_trip`; no transfers yet) is an
  option in the mode choice (`TripMode::Subway`: the walks, two minutes of stairs, half a headway,
  the ride and its dwells; fare 2.5; the crowd-shy mind it a little less than the bus). They walk
  to the entrance, wait on the platform (shown at the entrance), board a train going their way
  (`Activity::OnMetro`, hidden underground), pay into the treasury (ledger "subway fares"), get off
  at their station and walk on. A station pulled down drops out of its lines (the tunnel is re-laid
  through the rest; a line left with one station closes) and its waiting riders go on foot. Land
  value: +0.16 within 240 m of an entrance. Upkeep (stations, tunnels, runs) is booked at midnight
  under "subway upkeep". The Underground view draws the bores with track and a lit strip in the
  line's colour, island platforms, the shafts up to the entrances and the trains
  (`nd_procgen::metro`, `render/subway.rs`); City Data's Services › Transit and the inspector show lines,
  timetables and boardings per station.
  - **Loops** (`MetroLine::looped`): a line closed back on its first station (three stations or
    more; `add_metro_line` takes the first station again at the end as a loop). Its tunnel
    (`subway::lay_loop_tunnel`) wraps round with the same platform straights and ends where it
    starts; every run goes the whole way round, one each way from the first station on the
    headway, so every pair of stations is served one way or the other (`MetroLine::serves` by
    call position; the first station is the first call and the last). Rides are timed the short
    way round. A loop that loses stations down to two runs end to end.
  - **Drawing a line station to station** (`metro_ux.rs`, `tools::metro_click`, the pure
    `subway::MetroDraft`): picking Metro lines switches the map to the Metro layer (below; leaving
    the tool puts back the layer before). A click on a station (its lot, or within 75 m of its
    platform or entrance: `World::metro_station_at`) adds it and the tunnel to it is drawn white
    (`World::metro_draft_preview` lays it as it will be laid); the station under the pointer
    previews the next leg in green (red when it would be refused) and off a station a dashed lead
    follows the pointer. The first station again closes a loop; Enter or the panel's Finish lays
    the line; a right-click or Backspace takes back the last stop (a closed loop opens first); Esc
    cancels. A click that misses every station does nothing but say so, and with fewer than two
    stations the panel says to place them first (with a button that picks Metro station).
    `World::metro_draft_add` refuses, with the reason in the status line: the last stop again, a
    station already on the line, a loop of two, and a stop whose tunnel the treasury could not pay
    for. Tunnels are bored straight under everything (streets, water, road tunnels, which they
    pass below), so any two stations connect and the line digs its own tunnel as it always did:
    there is no "no tunnel path" case to refuse or offer to dig, only the cost, shown up front.
    The Metro line panel lists the four steps with the current one lit and the line so far: its
    stations, stops, length, round trip (end to end and back, or once round a loop: running at
    22 m/s plus 16 s per call), trains out at once at the headway, the tunnel's cost against the
    treasury and the running cost a day (`subway::preview`).
- **Central Station** (`RailFacility::Central`): the grand terminus, a 150 by 64 m rail piece
  placed like a station (Buildings > Rail or Transit, or the Rail tool), one per city, 120,000 to
  build and 1,100 a day, 60 jobs. It is a passenger station on the intercity line (a 120 m platform,
  extended along the straight the track runs, so a trunk split by crossings still gets its full
  length), a metro station (lines can start or call there) and a bus terminal (a depot). People walk
  420 m to its trains and 450 m to its metro. Land value +0.32 within 480 m and +0.2 within 200 m.
  Drawn as a hall under a barrel-vaulted glass roof on steel ribs with a magenta neon ridge, a stone
  pavilion with three lit arches, a great clock and CENTRAL in neon, a clock tower with a neon crown,
  saw-tooth bus canopies over lit bays, a metro entrance in the forecourt, a taxi rank and the
  railway station's canopied platform (`buildings::subway::central_station`, `rail::platform`).
- **Power** (`nd_sim::power`): the city runs on electricity and the player supplies it.
  Every building draws megawatts by land use, floors and tier (industry most, homes
  least, high tiers more); plants draw none. Four kinds of plant go on any lot or unzoned
  ground by a road with the Power tool (no zoning needed: see Free placement below), paid
  from the treasury: coal (120 MW per 1,000 m² of site, cheap, steady,
  a brown haze, ash on the ground and a heavy land value penalty for 360 m), solar (25 MW nameplate,
  output follows the sun and is dimmed by cloud), wind (40 MW nameplate,
  output follows the wind) and nuclear (400 MW per 1,500 m², steady, very expensive to build and
  run, a smaller penalty for the nervous neighbours). [plant-scale] Coal, nuclear and geothermal
  plants are sized to their site in a capped range (`PowerPlantKind::size_range`: coal 600 to
  2,400 m², 72 to 288 MW; nuclear 900 to 3,000 m², 240 to 800 MW; geothermal 400 to 1,600 m²),
  output, cost, upkeep and jobs alike (land-value cone and ash by the square root of the size);
  the size is fixed when built (`Building::plant_scale`; plants from older saves keep the
  standard size). A bigger site past the cap makes it no bigger: the station stands on part of it
  and the rest is coal yard. On unzoned ground free placement joins just enough parcels to reach
  the plant's minimum, never past its cap (`zoning::grow_site`). [freight] Hydro and tidal plants
  are sized the same way (70 and 60 MW per 700 m², 300 to 1,866 m²: one to eight turbine bays,
  `PowerPlantKind::turbine_bays`, three at the nameplate; hydro's output still follows the
  current), fixed when built; older plants keep the nameplate. Their meshes grow by bays: a hydro
  hall of 8 m bays (a roof generator housing, a pilaster and a penstock each), a tidal house of 7 m
  modules (a sluice gate each), the same height and depth whatever the size; the dam keeps its arch
  across the river and gains intake towers (one per two bays) and spillway gates (two to four),
  the tidal barrage a caisson per bay; on a lot too narrow for one row a second hall stands in front (`waterworks::bay_rows`, `dam::dam_parts`). The coal station is composed of
  units, not stretched (`buildings::power::coal_layout`): one to four black boiler houses with
  orange-lit slits, each with its own smokestack of its own height, a start-up stack, a tall
  common stack and cooling towers (steam) on the big ones, a turbine hall, ash filters, conveyor
  galleries and coal heaps; the renderer's plumes start at the same stacks (`coal_vents`). The
  Build hover label shows a plant's MW, cost and upkeep on the site before the click, and for
  solar and wind the Sunfield or Wind Park it would join with the combined output. Weather is a
  slow random walk of cloud and wind, stepped hourly. Each hour the supply is summed, the
  regional grid sells up to 60 MW at a price to cover any shortfall, and what is left is
  shared out highest tier first, so brownouts land on the cheap districts. A building
  below 60% power counts a dark hour: its businesses produce proportionally less, its
  windows go dark, the people inside lose mood and say so ("The power is out again."),
  and the inspector marks it UNDERPOWERED. Surplus and storage (`power::dispatch`, the merit order): a surplus
  first charges the grid batteries, then is sold to the regional grid up to 120 MW at 0.5 a MWh (a
  fifth of the import price; booked at midnight under the `grid export` flow), and what is left is
  curtailed (every plant turns down by the same share); a shortfall first drains the batteries, then
  is imported. Battery packs (Buildings, Power: small 20 MWh / 5 MW, medium 80 / 20, farm 240 / 80;
  `BatteryKind`) charge and discharge at 95% each way (about 90% round trip); every pack takes or
  gives the same share of its own headroom, so they fill and empty together; they work once their
  lot is clear and run in slot order, so the dispatch is deterministic. [batteries] They play the
  day's time-of-use prices by one rule (`power::PriceBands`, `dispatch_tou`): the day's 24 import
  prices (at the temperature the day's swing gives each hour) mark its six cheapest hours (cheap) and
  six dearest (dear). A surplus always charges them first. In a cheap hour they also buy from the
  regional grid, within its import line, to fill up (only if the round trip pays: cheap under
  0.9 x 0.9025 x dear). In a dear hour a shortfall drains them before anything is imported. In
  between they hold the charge for the peak and give only what the import line cannot cover
  (nobody browns out to save a charge). A flat price day falls back to the old merit order. What
  they save is booked per hour (`Dispatch::battery_savings`: imports spared at the hour's price,
  less power bought to charge them, less exports their surplus would have earned), summed per day
  (`DayRecord::battery_savings`) and split per pack by its share of the flow
  (`Battery::savings_today/yesterday`). Placing a plant or battery
  refreshes who is powered but books no hour (no money, no battery energy, no dark hour); the hour is
  booked once, on the hour. A plant does not feed the grid while the old building on its lot is
  still being cleared; solar, wind and battery yards clear it at once. Each plant kind has an air
  and noise model beside its land-value cone: CO2 per MWh (coal 0.95 t, geothermal 0.08, the rest
  0), smog at the fence (coal 1, geothermal 0.15) and noise while running (coal 68 dB(A) down to
  solar 38; batteries 40-48). `World::plant_rows` puts each plant's and battery's output, 24-hour
  mean, upkeep, cost per MWh (upkeep over the last 24 hours' energy), CO2, smog and noise in one row
  for City Data's Services › Power and the API; `nd_sim::history` keeps a per-plant record (`PlantLog`: 72
  hours of output, a battery's charge, 90 days of energy). The treasury starts at 60,000, takes 12% of
  incomes and 5% of business revenue each day (the standard rates; see Taxes), and pays plant upkeep
  and imported power.
- **Services and emergencies** (`nd_sim::services`, `world/services.rs`): police, fire, health,
  schools and a 911 dispatch centre, plus monuments and stadiums, placed by the player in three
  tiers (cost x0.5 / 1 / 2.2, upkeep x0.6 / 1 / 1.8, jobs half / base / double, radius x0.7 / 1 / 1.4,
  vehicles 1 / 2 / 4). Coverage is by road: a multi-source Dijkstra over street nodes from every
  station with its radius as budget; a lot is fully covered with 200 m of budget left, fading to none.
  Hourly, each building may catch fire (by tier, condition, land use, x(1 - 0.7 fire coverage)), an
  occupied one may suffer a crime (by unemployment, land value, night, x(1 - 0.75 police coverage)),
  and each citizen indoors may fall ill (by age, hunger, x(1 - 0.5 health coverage)). Someone present
  calls it in after a few minutes; the dispatcher (a worker at a dispatch centre who is at the desk:
  2 min; staffed but nobody in: 8; no centre: 20, the regional line) sends the nearest idle unit whose
  station has the crew; the vehicle is a car of the traffic model, drives over the lanes, parks on
  scene, works (engines knock fire down 0.012 a minute each; crimes 25 min, patients 12), resolves (fire
  out, arrest 75 % if quick else 40 %, patient driven to the nearest hospital with a free bed for
  6-24 h) and drives home. Unanswered crimes go unsolved after 3 h, patients recover alone after 8 h;
  unanswered fires grow, jump to buildings on touching lots (4 % x intensity per neighbour every 20 min) and burn a building down at condition 0.08,
  leaving rubble for two days. Workers near a school earn up to 12 % more; school-age children whose home a school covers go there at 07:30 until it closes at 16:00. Stations, hospitals and dispatch work three round-the-clock shifts; the sick are weighted by age (small children and the old most); a hospital stay bills the household 60, the city covering what it cannot pay. Stadiums host a game on
  30-60 % of evenings; fans within 1.6 km buy a ticket and go. Upkeep is booked at midnight under
  `service upkeep`, tickets under `tickets`. A placed special buys out whatever grew on its lot.
  With no crewed station of the kind but a dispatcher at a dispatch centre, a unit comes from out of
  town 45 minutes after dispatch and is billed (fire 1500, police 600, ambulance 800); with no
  dispatcher nobody asks. Arrests go to the nearest jail with room for 2-5 days; without one the
  suspect is held at a police station for 6 hours and let go early (`released_early_total`), so the
  same people offend again. `World::report_crime(kind, at, suspect, victim)` is the hook for crimes
  decided elsewhere; `Services::arrests_of`, `jail_occupancy`, `SimEvent::Arrested/Released` report
  back. Every incident keeps a record (place, crime kind, people and their roles and where they were
  taken, unit and whether it came from out of town, fee, fire damage per building), the last 500
  resolved ones on file, shown live in City Data's Services › Emergencies and by the `incidents` API command.
  Services draw from their own random stream so the rest of the simulation is not perturbed.
- **Landmark attractions** [attractions] (`nd_sim::attractions`, `world/attractions.rs`,
  `nd_procgen::buildings::attractions`): ten landmarks under Build > Attractions, one of each per
  city and one size (`ServiceKind::LANDMARKS`, `is_landmark`): an art museum (white galleries round a
  sculpture court, a stepped spiral drum ringed in neon, an LED art wall), a natural history and
  science museum (a glass barrel-vaulted hall, a planetarium dome, a sauropod skeleton and a rocket
  outside), a city history and design museum (a colonnade under a lit pediment, a clock tower, an old
  tram on show), a pleasure pier (Santa Monica style: a lit gateway ashore, a deck 60-120 m out over
  the sea on piles with lamps, string lights and food stands, and on its head a ferris wheel, a
  coaster, a carousel and an arcade), a grand city park (the high-tier park plan of the whole site:
  lakes, a bandstand, rides, fountains; a loop path with benches; a lit gate, a glass pavilion, a
  boathouse and an obelisk), a zoo (enclosures of savanna, elephants, big cats on rocks, penguins,
  bears and flamingos round a visitors' loop, an aviary dome), an observatory (domes with telescopes
  in their slits on a terrace, a ringed planet, a green beam after dark), an aquarium (a wave-roofed
  hall glowing blue, a shark on the roof, a touch pool), a botanical garden (beds and pools round a
  domed glass conservatory with vaulted wings) and an opera and concert hall (white sails over a
  glass foyer on a podium, their edges lit). **Sites**: each needs 1,500-7,000 m² (several plots;
  `ATTRACTION_AREA`): the lot clicked takes in its neighbours like a stadium (`joined_site`, up to
  48 plots for a landmark, `Placeable::join_steps`), and a landmark's site may be notched (rows of
  plots of different widths never add up to a rectangle one at a time: `grow_site_steps(concave)`,
  `make_placed_lot_concave`; the city's ground keeps its street front through later road works).
  **Rules** (`World::landmark_problem`, shown in the build hover): one per city (`World::landmark`);
  the pier only on the sea shore, the site within 14 m of the sea (not a lake or river) with open sea
  under its whole deck (`attractions::plan_pier`); the observatory only on high ground, its site 12 m
  or more above the median ground height of every lot (`World::town_median_height`,
  `rise_above_town`; the Build pane shows the rise under the pointer). **The pier's deck** is saved in
  `World::attractions.piers` and pushed onto the built ground's decks (`add_pier_decks`), so walkers
  on it stand on the boards. **Grounds**: the grand park, zoo, botanical garden and pier lay out
  walkways and benches in `Lot::park` (`attractions::plan_grounds`, after the parks are planned), so
  the walk graph joins them like a park's, citizens walk in and sit, and their gates are Path tool
  grounds of kind `Attraction`; the pier's path runs out along the deck and round the head. Museums,
  the aquarium, the observatory and the opera show a plaza's gates. **Visitors**: the town's people
  find the museums, zoo, aquarium, garden, park and pier for an afternoon out (`Intent::Visit(Park)`)
  and the opera, pier and observatory for a night out (`Visit(Entertainment)`) while open, staffed
  (a quarter of its jobs) and under capacity (`World::landmark_for_outing`); tourists see them as
  sights of their own kind (`tourism::AttractionKind::ArtMuseum..ConcertHall`, open by the landmark's
  hours) with a pull of 18-38 points (`ServiceKind::tourist_pull`, halved when unstaffed), stand on the
  pier's head or along the park's walk, and stay an hour longer. Both pay the admission at the door
  (0-25, `ServiceKind::admission`, to the treasury under `tickets`), counted in
  `World::attractions.books` (townsfolk and tourists, today and yesterday, takings). **Hours**:
  museums by day (09/10-18/19), the aquarium to 20:00, the garden 08-20, the zoo 09-18, the pier
  10-01, the park always, the observatory 14-00, the opera 17-00. **Land value**: a landmark splats
  its own channel (`Channel::Attractions`, cap 0.5, scale 0.5: one counts nearly in full, two or three
  overlapping add up to more), 0.26-0.42 over 320-540 m, times 0.55-1.0 by staffing: the player's
  strongest lever on land value since the rescale (a grand park lifts the lots round it by a tenth or
  more and the quarter 200-340 m out by several hundredths, where a monument's lift is gone). The
  grand park, zoo, garden and pier count as parks for walking access; the opera, pier and observatory
  as nightlife. **Costs**: 45,000-95,000 to build, 420-1,100 a day, 14-40 jobs, car parks of 30-60.
  The inspector's ATTRACTION card says what it is and does, open or not and why, today's and
  yesterday's visitors and takings, its land value lift and pull, the pier's deck and the
  observatory's rise; `inspect` carries the same as `attraction`.
- **Fire engines at the scene** (`world/scenes.rs`, `traffic/scene.rs`): an engine is sent to the
  burning building's street, clear of the junctions at either end (18 m, `SCENE_JUNCTION_CLEAR`), up
  whichever side is quicker, and on arrival pulls in to the kerb on the building's side (crossing
  over if it came up the far side), its body flush with the road's edge and nose the way it came, so the crew climbs out on the pavement side; the hose crew stands between
  the engine and the building, never on the road side (`fire_scene.rs`, `CREW_OFF_ENGINE`). (It used
  to stand where its route ended, mid-lane and sometimes in the junction, with traffic driving
  through it.) The world tells the traffic each tick what stands at the roadside (`Traffic::scenes`):
  drivers coming up to an engine at work crawl past at 4.5 m/s and ease sideways round its body (up
  to 1.9 m, a little over the centre line), or stop short where there is no room; sirens are not
  held. *Second alarm*: a fire still burning at 0.6 or more once its first engine is on the way
  sounds a second alarm (once a fire): the nearest other free engine comes with its own crew,
  pulls in 11 m behind the first (`SECOND_ENGINE_GAP`) and each engine at work knocks the fire down,
  so two put it out twice as fast. Either may arrive first; if the first engine is lost the second
  takes the fire over. Counted in `ServiceStats::second_alarms_*`.
- **Lights and sirens, and car chases** (`traffic/siren.rs`, `world/chases.rs`): drivers up to
  60 m ahead of an emergency vehicle on its lane or the next lane of its route (and oncoming drivers
  on a lane it overtakes in) pull right, onto the kerb edge from a kerb lane, and slow to a crawl,
  stopping once it is within 20 m; drivers about to cross a junction it is running against the light
  hold at the line. Whether a driver does is temperament (`traffic::pulls_over`: from compliance and
  drink; about 95% of ordinary drivers, ~75% of the reckless, fewer drunks, no takeover crew).
  Sirens drive off-law: someone in the way who will not pull over is passed in the lane to the left
  or on the opposing lane when it is clear beyond the queue (oncoming speed counted), else up on the
  kerb past standing traffic, else after 4 s stuck it squeezes past at a crawl; reds are taken at
  6.5 m/s, holding at the line while cross traffic is moving in the junction. The swing is drawn on
  top of the lane (`DriveState::offlaw`). *Chases*: when the police finish at a crime scene and the
  suspect can get to their own car or motorbike (parked within 700 m, or the one they are driving),
  half the time they run for it instead of the usual arrest roll: a reckless fleeing profile (fast,
  through every light, round slower traffic the same way) heading for lots 250-1200 m off, away from
  the police car, re-chosen every 8 minutes. The unit (`UnitState::Pursuing`, incident
  `Stage::Pursuit`) chases at 1.4x the limit, re-routed every minute along the fugitive's own route
  (a route ending just past it had it braking for its own stop). Tag rules: the police car touching
  it (4.6 m centre to centre at any substep) ends it, the suspect stops, gets out and is arrested,
  the car left at the kerb; more than 280 m apart for 6 minutes or 60 minutes in, they are lost and
  drive home. Each chase is on its incident (`Incident::chase`: vehicle, metres, closest, result),
  counted in `ServiceStats::chases_*`, shown in City Data's Services › Emergencies and by `incidents`/`report`.
: the rich fly. Vertiports are placed under Transit (drone pad /
  vertiport / skyport: 2 / 4 / 6 landing slots, decks at 0.4 / 7 / 15 m); rooftop pads are rare: a high-tier
  tower of 9+ floors on land worth 0.8+ qualifies when 8+ rich adults would fly from it (its own in full,
  those within 150 m at half), and only the most-demanded towers get one, about one per nine towers
  (5 of 67 on the demo at day 12); kept at 60% of the bar and land down to 0.72; redrawn with a helideck
  on stilts, styles limited to deck-capable towers. The air taxi company keeps one eVTOL drone per vertiport slot but one. A
  rich adult on a trip of 400 m or more flies when a taxi pad is within 170 m of both ends, the fare (20 + 16/km,
  15% levy to the treasury) is in the purse and the flight beats a city drive (`consider_air`, the only hook in
  the mode choice): walk to the pad, wait, the nearest drone with the charge for the job comes, climb to 150 m,
  fly straight at 45 m/s, land, walk on. A pad takes as many aircraft as it has slots; the rest circle and land
  first come first served. Batteries drain per km and per metre climbed and charge on the stand at base; riders
  with no drone after 40 minutes drive. High-tier hospitals and the fire HQ keep an air ambulance: a critical
  patient (65+ or one in three) over 500 m from any hospital is flown, landing on the roof pad or in the street,
  10 minutes of work, then to the nearest hospital with a bed, helipad first. The police HQ helicopter circles
  crime scenes at night with a searchlight. Flying is logged on a 64 m noise grid (halved nightly) that feeds the
  land value nuisance layer, vertiports are a nuisance too, and people on the street who are not rich grumble
  (a memory, a little mood, once a day). The renderer lands aircraft on the decks the buildings drew
  (`Geometry::pads`).
- **Taxis** (`nd_sim::taxi`, `world/taxis.rs`): a taxi company is a commercial business
  (`BusinessKind::TaxiCompany`) nobody plans. Every trip that chose another way although a cab
  would have beaten it by 10% (priced with an 8-minute wait, as if a company existed) is demand
  nobody served: near a company (within 1.65 km) it is a rider that company missed, elsewhere it
  counts towards a new company (`Taxis::latent_today`). Once a day's count reaches 10 (10 more per
  company in town, and at most one company per 1,500 people), development opens a company on the
  vacant commercial lot (industrial at a pinch) nearest where those trips began, founded by the
  adult with the most savings and household money (a kilometre away counts as 200 less), who puts
  in 40% of it (up to 1,500) and remembers it ("<Surname> Cabs"). It starts with 3 cabs in
  a yard drawn as a surface lot with a lit TAXI sign, 6 staff, and lives on fares. Every cab is a
  record with a place, like a private car: in its yard, queued at a rank, stopped at a kerb, or
  on the road as a car of the traffic model (`Errand::Taxi`); one taken off the road by road works
  goes back to its yard, and the cabs of a company that closes are sold and driven off the map.
  Mode choice gains `TripMode::Taxi` for trips of 250 m or more (not children, not walks in the
  park): the walk to the kerb, the wait (the nearest free cab on duty within 1.1 km at 9 m/s, once
  the riders already waiting have taken the free cabs; else 8 min plus 15 per queued rider per cab on
  duty; over 30 minutes and the rider does not hail, which counts as demand), the ride, the fare (4 + 3.5/km of road, about
  1.3 times the straight line, paid by the household to the company; the purse must hold twice
  it), and comfort: being driven (more for hedonists and the old), at night (22:00 to 05:00, when
  the buses stop) and above all drunk; with a cab to be had, a drunk who would drive anyway weighs
  the car at -15 instead of -40, so fewer drink-drive. A rider walks to the kerb in front of their
  building and waits (visible); the dispatcher sends the nearest free cab, which stops at the kerb
  and waits for them if they are not there yet; they get in (`Activity::Riding`), are driven to the
  kerb of their destination, pay and walk in. No cab in 30 minutes (60 with one on its way): they
  give up, remember it, the nearest company counts a missed rider, and the trip goes another way.
  A cab done with a fare takes the nearest unserved rider, else waits at a rank with room (4 cabs
  each: station forecourts, the six busiest open venues, its own gate), else goes home. The rota
  puts a share of each fleet on duty by the hour (all from 20:00 to 03:00, 80% in the evening rush,
  half by day, 30% at dawn); off-duty cabs park in the yard. At midnight a company with 3 or more
  missed riders and the money buys one cab per 8 missed (420 each, up to 4 a night and 20 in all);
  one whose cabs gave under 1.5 rides each and missed nobody sells one. A rider who has booked
  makes no other plans until the cab comes or they give up. On the demo city (day 24) one company
  ran 11 cabs, about 120 rides a day, 2 to 3% of trips, most of its trade in the evening and night. Services › Transit and `report.transit.taxis`.
- **Cargo hubs** (`nd_sim::cargo_hub`, `world/cargo_hubs.rs`): a freight terminal the player places
  (Buildings, Rail: 9,000, 140/day, 24 jobs, on a lot of 900 m² or more with a street, not homes or
  parks), run by the city. Businesses within 1.8 km of a working hub import through it
  (`World::import`): by sea when a shipping port works and by rail when a freight yard does (at
  their cheaper prices, as before), else by road at 90% of the import price; the load is addressed
  to the hub. Road orders wait at the hub until 60 units fill a semi or the oldest has waited 30
  minutes, then one semi brings them all from the map edge: one long-haul trip through the city
  instead of one per order (counted as `long_haul_trips_saved`). A freight yard or port within
  500 m unloads straight into the hub (its siding or quay, no lorry); farther ones truck the loads
  over. From the shed the hub's own lorries (parcel livery) take each load to its buyer, and come
  back to park in its bays: the hub is a truck lot for the fleet code (`BusinessKind::is_truck_lot`),
  so any lorry nearby may park there too. Exports through a hub earn 6% more and leave in semis of
  80 units. A 3% handling fee is the hub's income. A hub that closes or is bulldozed sends what it
  holds on to the buyers, and readdresses loads still waiting for a train, at ports, on trains or ships to them:
  nothing is lost. Services › Transit and `report.transit.cargo_hubs`.
- **Crime, the street and the bins** (`nd_sim::{crime, homeless, trash}`, hooked in from
  `world/society.rs`, their own random stream). *Crime*: every hour each adult may turn, by class
  (poor 1, middle 0.22, rich 0.04), being jobless (x2.5) or homeless (x1.6), hunger, unhappiness,
  cheap land at home (nothing from land value 0.7 up), police coverage at home (cut by up to 80%),
  personality (boldness from the seed), youth and the offenders living within 140 m (+60% each, up
  to x4: crime spreads through a poor district nobody polices); at most 8% of adults offend. An
  active offender strikes about a third of the time a day (twice as often at night, less under
  coverage) at a building within 260 m: theft from a business (6% of its cash up to 80, two units of
  stock), burglary (a quarter of a household's money up to 100), assault (the victim falls ill
  through the medical hook; 6% are murders) or vandalism (condition). Every offence goes to
  `report_crime`; the services arrest. A jail term ends a career at 20% + 8% a day served, an early
  release from a police cell at 5% (and emboldens); released offenders who strike again make the
  reoffending rate. Offences leave decaying heat at hot spots, which take land value off the block
  and frighten the neighbours (mood; rich households near heat 4+ leave). *Homicide*
  (`World::murder`): rare. Besides the 6% of offenders' assaults, a fight between enemies (social
  layer, `Bond::Enemy`) ends in a death 3% of the time (a hash of the pair and the minute, no
  draws). Before the victim leaves the city their household and friends remember it (mood down a
  lot, a heavy Safety concern), the place's regulars and the neighbours within 150 m are shaken, a
  sober line goes in the city news ("Police are investigating") and the spot takes triple heat. It
  is reported as a `Homicide` (a violent crime, ahead in the queue): the police hold the scene for a
  150-minute investigation, then arrest 80% of the time when the killer is known (a feud), 45%
  when not (an offender), for 12-20 days in jail; an arrest is in the news and the grieving hear.
  No gore: nothing is drawn but the police at work. *Homelessness*: a
  household evicted with nowhere cheaper and no money stays on the street 70% of the time if any
  land is cheap (value < 0.36): camps of up to six households on cheap pavements, vacant lots and
  unzoned parcels, or (with a car) an RV or van at the kerb ([pt-growth] each pulls up between
  half an hour and fourteen hours after its household lost its home, a hash of the household,
  and never beside two others within 60 m, so a new cheap street does not fill with ten RVs in
  the same second); they eat charity meals, walk back to
  the camp at night, are housed again by the ordinary daily search when something affordable is
  free, by outreach from a clinic or police post nearby, or give up after ten days. Camps lower land
  value and are drawn only on cheap land. *Trash*: people throw out 1.5/1.8/2.4 kg a day by class,
  workers 0.6 to 2.8 kg by land use; bins hold three days. A staffed trash depot (Buildings >
  Services, radius 560/800/1120 m, 2/4/7 trucks of two crew) sends garbage trucks (cars of the
  traffic model, 9 t, a few minutes a stop) 06:00-18:00 to the fullest covered bins, unloading at
  the depot. Outside its reach rich buildings pay a private hauler (1.5 a household a day) whose
  trucks come in from the map edge and drive the trash out of town; everyone else's bins overflow:
  bags on the pavement, complaints and mood, land value, and rats (illness). Demo, day 8: bare,
  19 offenders and about 10 offences a day, 103 of ~350 buildings overflowing, the hauler with 88
  clients; with a police station, jail, dispatch and depot, 11 offenders, 5 arrests, nothing
  overflowing.
- **The skyline league** (`nd_sim::towers`, `world/towers.rs`, [supertall]): towers far above the
  ordinary growth, in three classes: skyscrapers (150 m up), supertalls (300 m up), megatalls (600 m
  up). Tower developers (fictional firms) gather capital daily with the city (900 per person, more
  with demand for offices or homes; one more firm per 5,000 people, four at most). Once a city has
  1,200 people, with at most one project rising (plus one per 10,000 people) and three days since the
  last, a 40% roll a day lets the richest break ground. Sites: commercial or residential plots on land
  worth 0.6+, pressure for 9+ floors and 50+ floors of neighbours within 160 m, under every height
  limit, scored by value x (pressure floors + neighbour floors / 12). The height: 6-24% over the
  tallest standing or rising (the record) when demand is 0.2+ and the money (height² x $900 plus the
  plots) is there, else a skyscraper under the record; never over the limit or what the town can
  carry (110 m + 0.05 m a person). The site: a supertall needs two plots and 1,500 m², a megatall
  three and 2,600 m² (a skyscraper one and 700 m²): touching plots of its land use are bought, vacant
  first then cheapest, never the city's, a home bought outright, a billionaire's, a merged building
  or another tower, as long as they make one compact block (hull within 18%). The grown buildings
  come down (residents move with a memory), the plots join as their rubble goes, and the podium and
  tower go up over 12 + height/15 days (a month for a skyscraper, two for a megatall). The sim counts
  floors at 4 m (88% of the height: the spire is not floors); homes (2 a floor) or offices (6 a floor)
  count all of the first 40 floors, half of the next 80 and a quarter above, capped at 900 homes or
  1,500 jobs; power is the usual per floor x 1.25 (lifts, cooling). A finished tower lifts the land
  round it (up to 460 m, by its height). Topping out writes the record book when it is the tallest and
  a line of news either way ("Kestrel Group tops out Meridian Spire at 412 m, the city's tallest").
  Towers are never bought out, refitted to a district's look, retiered into a redevelopment, given a
  drone pad or demolished for a failed tenant (offices take the floors). Height limits: the city's
  (City Data, City, Skyline), a district's (the District tool) and 120 m within 350 m of a vertiport
  the player placed; they hold ordinary buildings too (`height_cap`). Drawn by
  `nd_procgen::buildings::supertall`: a stone podium with a glass lobby and roof garden, and one of
  four forms: a stepped Art Deco spire, bundled tubes on a Y plan stepping back in a spiral to a
  needle (the Burj type), a twisting tower with a halo crown, a needle tower with neon edges; gold or
  neon on every setback, lit crowns, red aviation lights every 90 m. The site shows the concrete core
  leading, bare floor plates, glazing behind, netting and a jump crane on the core.
- **Districts** (`nd_sim::districts`, `world/districts.rs`, the District tool 9): named areas painted
  over the map with a brush or a polygon, as in Cities: Skylines, on a 16 m raster of district ids
  (saved run-length encoded). Painting applies at once, outside projects: it moves no road, lot or
  zone. Each district has policies: homes Kowloon stacks / Suburban / Eco, shops Neon strip / Tech
  campus / Night market / Tourist quarter, an industry sub-zone (an industry bias that follows the
  district's cells, under any drawn bias) and Historic. A new home or shop in a district takes its
  `Look`, saved on the building: the look sets the tier (the stacks stay cheap whatever the land, a
  campus builds a step up, suburbs never slums, market stalls a step down), the height (Kowloon may
  overshoot the density cap by four floors, suburbs three floors at most, night markets two) and the
  trades favoured among the ones the market can carry (gadget shops on a strip, offices on a campus,
  grocers and bakers in a market, hotels in a tourist quarter). Standing buildings are refitted to the
  district's look a quarter a day; industry retools the same way. Historic freezes the tier (no
  upgrade, no decay of tier, no redevelopment) and is never refitted. Eco, historic and campus
  buildings lift the land value round them. The look is drawn by `nd_procgen::buildings::district`:
  its own style menu per land use and tier (weighed by site and neighbours, so adjacency families
  keep working inside a district) and a re-tint of walls, trim, windows, sign/LED/awning colours and
  sign density: the strip is dark walls under pink and cyan signs, the campus white and glass, the
  market brick under red lanterns, the old town brick and amber. The Districts layer washes each
  district's cells in its colour with a bright border and its name and policy over the map; the
  City Data's City › Districts lists every district's figures (area, buildings by tier, homes and residents,
  jobs, land value, rent, hotels and beds, visitors staying, pull) with the policy editor.
- **Tourism** (`nd_sim::tourism`, `world/tourism.rs`): attractions are re-read every two hours:
  the landmark attractions by their pull and staffing, monuments and stadiums by tier, mid and high tier parks by area, open venues, landmark districts
  (historic, neon strip, night market, tourist quarter, with 3+ buildings) and the waterfront (built
  lots within 70 m of water). Their pull sums to the city's attractiveness; visitors per day follow it
  (diminishing past 60 points) times the ways in that work (road off the map 1, rail 0.45, ferry
  0.3, air 0.15). They arrive on the hour 08:00-21:00 by road (set down at the hotel door), rail (at
  a station), ferry (a terminal) or air (a vertiport, the well-off only), class backpacker / visitor /
  well off, and need a bed: a hotel of their kind if they can. Hotels (`BusinessKind::Hotel`,
  capsule 12 beds a floor, rooms 8, suites 6; 18 / 45 / 110 a night) open on commercial land when
  visitors are turned away (smoothed demand of 4 a day; a tourist quarter first, else the vacant lot
  nearest the sights), whatever the general commercial demand. A visitor stays 1-4 nights; by day they
  walk (or take a taxi past 1.1 km) to sights, weighed by pull and distance, stand and take photos,
  shop in gadget shops and bakeries (more on a strip or in a market), go to venues after dark, and are
  back at the hotel by 22:00; their footsteps count as footfall for the shops they pass. At midnight
  each pays the night to the hotel plus a 12% tourist tax to the treasury (ledger "tourist tax"). On
  the last day they leave the way they came. Visitors are not citizens: no home, job, rent or mind;
  they are drawn with the citizens' figure generator with a daypack and a camera. All of it rolls on
  its own seeded streams. City › Districts and City › Tourism and the `tourism` API show attractiveness, demand,
  arrivals by mode, beds, the turned away, spending by hotels / shops / venues and the tax.
- **Taxes** (`nd_sim::tax`, `world/taxes.rs`, City Data's Economy › Taxes): a rate per line, collected at
  midnight: incomes by the tier of the home (three lines, 12% standard, up to 40%), business takings
  by kind (grocers, bakeries, gadget shops, private car parks; nightlife by tier; each industry
  sub-zone; 5% standard, up to 30%); offices and the city's own businesses stay at 5%. Wages and
  prices were tuned as what is kept after the standard rates, so the treasury's standard take is
  collected at source and the defaults change nothing; only the difference from the standard reaches
  a purse (a household pays the extra out of its money every day, a business out of its takings,
  which its cover shows). Each line has a pull, `exp(-(rate - standard) x k)` bounded to 0.25..1.6
  (k = 4 for homes, 8 for businesses): the land use's demand is scaled by its lines' mean pull,
  residential lots of a tier develop by that tier's pull, arrivals by the pull of the tiers with
  free homes, a new industrial lot's sub-zone weights and a new shop's kind by their line's pull.
  Heavy income tax costs mood (and is complained about). The import duty (0..100%) is added to the
  price of every import and paid to the treasury (ledger `import duty`); imports are remembered by
  resource for a few days, and under a duty the industry sub-zones that make what the town imports
  are drawn 1 + 1.5 x duty times harder. Each line books its take (today, yesterday, a running
  average) for the window's bars.
- **Toll booths** (`nd_sim::tolls`, `world/tolls.rs`, Buildings > Transit): [toll-fit] one toll
  plaza on offer, a kiosk for every lane of the road it is put on and priced by it (1,500 and
  1,250 a lane; upkeep 15 and 22.5 a lane a day: 4,000 and 60 on a two-lane street); widening
  the road under it adds kiosks and the road works pay for them (`TollBooth::paid_lanes`). The
  old six-booth plaza (15,000, 200 a day) is kept for saves that have one. It stands across a straight stretch of street or highway, both
  directions, saved by position and found again on every road rebuild (gone with its road). The
  lanes of its piece carry a `LaneToll`: vehicles slow to 2.5 m/s (plaza 4.5) from 60 m before the
  line to 6 m past it, and pay when they cross it from behind: cars from the driver's pocket (the
  trip remembers it: it hurts like a parking fee and the next mode choice expects it), lorries
  double from their business, through traffic and out-of-town lorries just pay; buses and emergency
  vehicles go free. Routing counts a toll as 120 m of detour per unit of money, so drivers go round
  a dear toll. Takings go to the ledger as `road tolls`, upkeep as `toll booth upkeep`. Drawn by the
  app (`nd_procgen::toll`): islands with kiosks on every lane line, a canopy with neon fascia, a lit
  TOLL board and lane lights, raised barrier arms, rumble stripes; the plaza's asphalt flares out to
  six booth lanes with fanned lane lines (the sim keeps the road's lanes: a plaza is faster to pass,
  not wider).
- **Economy stats**: demand per land use, Gini of incomes, class counts, rents per
  tier, displaced, emigrated and priced-out counters, plus market stats: businesses,
  deals, imports, exports, meals sold, hungry citizens, closures, and the average
  price and stock of every resource.

- **Landscape** (`nd_sim::landscape`, `world/water.rs`): generated from a seed and
  `LandscapeParams` (preset river valley / islands / coast / delta / plains / flat; water, relief,
  branching, islands, highway). Features are saved with the world (old saves load flat): rivers and
  streams are centrelines with a width and a current (streams faster), lakes are outlines, the sea is
  an outline with islands as holes, hot spots are discs with a heat. Heights come from a 10 m raster of
  base hills carved by the signed distance to water (bed below the waterline, a flat strip then a
  slope of about one in ten up the banks); the basin centre stays dry. Road polylines are split at
  every waterline so a bridge is exactly the span; bridge segments are flagged, their ends set at deck
  height (water + 7 m) and every node raised until no segment is steeper than its grade (street 7%,
  rail 3%, path 10%); new bridge metres are charged on implement. `Ground` is a 4 m raster of the built
  ground: terrain blended into each road's profile within 22 m (where two roads' cores overlap at
  about one height, the lower holds, so the ground stays under both surfaces) and flattened to a pad
  under every lot
  (the street's height at the frontage). A long street or path leg at grade gets inner height
  samples every ~30 m (`World::road_inner`, `(t, z)` per segment, read through
  `landscape::way_height` by cars, lot pads, the built ground and the street mesh), each the land's
  height held within the grade of both ends and its neighbours: it follows the hills between its
  ends. They are not road nodes (the highway's and track's are, `sample_heights`): a street's
  vertices cut its parcels, so parcel keys stay as they were. Decks, tunnels, cuttings, set levels
  and ways in a grade crossing keep the straight line. The renderer draws the terrain from it, lifts flat-datum meshes
  onto it, raises buildings to their pad and movers to `surface_height` (decks over water). Water cuts
  parcels (`zoning::generate_parcels_with`); lots steeper than 0.2 are unusable. A one-way street of three
  or more lanes is a highway: 25 m/s, no frontage, grade-separated (crossings make no junction, only
  dropped ends join, so ramps merge with priority to the mainline). Embankments and cuttings
  slope no steeper than 1 in 2.5 (the blend to the terrain widens with the height, up to 160 m).
  Where two ways cross without a junction (`RoadNetwork::grade_crossings`) and neither is on a
  deck or in a tunnel of its own, the lower way runs through an underpass (`landscape::Underpass`):
  the ground is cut to its level for the width of its corridor plus 8 m along the whole embankment,
  the upper way crosses on a short deck (two halves meeting over the crossing, which may be the
  crest of a lifted way; registered decks, so its traffic stays up), and abutment and wing walls
  with coping follow the embankment down either side.
- **Elevation** (`Gesture::Road::elevation`, `Node::level`): every control point of a road has a
  level, metres above the ground (serde default empty: old roads are at grade); curves blend it
  along each span, water splits and roundabout cuts carry it (`plan::carry_levels`). The Road tool
  draws at a level stepped 2 m by Page Up / Page Down (-20..+24 m), shown in the sub-pane as
  "ground", "+8 m viaduct", "-2 m cutting" or "-8 m tunnel"; a point snaps onto a road only at
  about its level (the first point snaps to anything and takes that road's level: a ramp or a
  portal starts there). Crossings: two ways meet at a junction (split in `add_segment_between`,
  also at an existing node or a polyline vertex) unless `roads::grade_separated`: a highway and
  anything else, or levels 4.5 m or more apart. Then nothing joins them, not in the road graph,
  the lane network or the pavements; the upper way (higher level, else the highway) gets a vertex
  of its own over the crossing. Heights (`World::rebuild_ground`): ground plus level; nodes drawn
  off the ground are held, the rest eased to the grade (lowered toward a cutting beside them,
  raised otherwise); at every separated crossing that is not clear by 4.5 m either way the upper
  way's vertex is lifted (if the player did not set its level), so a street drawn across the
  highway at grade passes under it with the highway on a lifted deck. A piece with an end 2.5 m
  or more up is a viaduct (`Segment::viaduct`, a bridge: a deck on columns and crossheads,
  parapets, no frontage, no kerb parking; the ground is graded only near its feet); one with
  both ends 5 m or more down is a tunnel (`Segment::tunnel`: bored, no parcels cut, nothing
  above it disturbed; it joins other tunnels at underground junctions); between, an open cutting
  with retaining walls. Like the highway and its ramps a tunnel is a way for traffic only
  (`Segment::fronts_nothing`): no frontage, pavements, walk graph, crossings or kerb parking
  (legal or not), no RVs or camps; its bore is a carriageway with a safety kerb and walls. Tunnels
  show on the surface as a concrete arch portal where they come out (a headwall with an elliptical
  arch over the opening, soffit, coping, a lit band and a dark mouth; the cutting's ground is held
  at road level only `PORTAL_HOLD` (8 m) into the hill, under a lid, `terrain::portal_arch`) and a vent head
  every 150 m. A road that would meet the railway at a level that is neither at grade (within
  1 m) nor separated (a portal ramp part way down) is refused by the Road tool and the API with
  what would work (`World::road_refusal`): the level-crossing node used to take the road's level
  and drag the track into the cutting. Steep legs are warned about
  (`World::road_grade_warnings`, `steep_legs`) and still built: the ground end is raised or
  lowered to the grade. Cars ride the height of the way they are on (`car_height`), movers keep to
  the surface nearest the height they were at (`surface_height_near`), so a walker under a
  viaduct stays under it. Elevated roads pass roundabouts by.
- **Interchanges** [interchange]: slip roads (`Segment::ramp`) are grade-separated from what
  merely crosses them, like the highway (`roads::WayClass`, `roads::separated`): a street or the
  railway crossing a ramp passes under it (the ramp's vertex over it is lifted), so a street drawn
  under a highway with ramps no longer cuts the ramps into junctions (which ended the ramp there
  and made the rest a town street). A ramp meets the highway only by its ends, other ramps (forks
  and merges, and one-way streets being laid), and paths at grade (people cross a slip road at its
  foot); a street meets a ramp where an end is dropped on it or a control point is put right on
  one of its nodes (`SNAP_EXACT`, 1 m: a street drawn through a ramp's end is its terminal). Road
  ends (`RoadNetwork::end_target`) prefer a way at their own level (within 4.5 m) of everything in
  reach, so an end dropped where a viaduct passes over a street joins the one at its level; only
  with none at its level does it come up or down to another (a raised road ramps down to the
  road it is dropped on). A path end lands on the highway or a ramp only with no other way in reach. Ramp easing
  (`ease_ramp_ends`) scales the lead to the ramp: 45% of how far the ramp's next point lies along
  the highway, 15 to 40 m, then rounds the bend with a few points; it is idempotent and the
  holograms ease what they draw (`ease_ramp_way`), so the preview is what is built (a fixed 40 m
  lead on a short diamond ran each ramp alongside the highway up to the street under it and then
  hairpinned onto it over the street's asphalt). A gore's hatching stops where the ramp's first
  piece ends. The Road tool's **Interchange** preset (Build window, Roads > Highways) lays a diamond
  from two clicks across the highway (`World::plan_diamond`, `add_diamond`): the cross street
  under the highway (kept if one runs there already), and per carriageway an off-ramp leaving it
  150 m before the crossing and an on-ramp joining 150 m after (`DIAMOND_SPAN`), on its outer
  side, meeting the street at one signalled junction 55 m out (`DIAMOND_TERMINAL`), with a 40 m
  lead beside the carriageway and an easy S into the street square. The hologram is the whole
  diamond; it is refused, with a red pin and label at the spot, when the line does not cross the
  highway or crosses it more than 40 degrees off square, the street is too short for the
  terminals, the highway bends or ends near a join, a ramp would cross another road, or another
  road already joins the highway there. Remote: `add_interchange {from, to, check?, implement?}`;
  dev `rebuild_roads` lays the roads again from the plan with the current rules.
- **Raising roads while drawing, and pillars** [tools]: with the Road tool, `]` raises and `[`
  lowers the next point by 2 m (as Page Up / Page Down; the camera's bracket zoom gives way while
  the Road tool is out); `Shift+]` / `Shift+[` move the whole road drawn so far, its placed points
  and the next one, together. A raised way stands on concrete columns planned along its built
  polyline (`nd_sim::pillars::plan`, a shortest path over 1 m samples): about 24 m apart
  (`PILLAR_SPAN`), never more than 42 m between supports (`PILLAR_MAX_SPAN`, the structural
  limit; a raised end needs one within 10 m), none where the deck is under 2.5 m up (it lands on
  its ramp). Each sample is a site (`World::pillar_site`): a road, path or track below the deck
  (with its pavements and a junction's corners, 1.6 m footing clearance) or a facility or one of
  the city's own structures (plants, depots, stations, services, docks, hubs) or a park blocks a
  column; an empty lot is taken (it leaves the zoning); a grown building is bought out, and costs
  the most, so the plan shifts columns onto open ground or empty lots first. A stretch with no
  site for longer than a span may be is a gap: the preview draws the deck red over it (and a red
  band on the ground), the sub-pane says why, and implementing is refused (`World::pillar_refusal`,
  also on a trial rebuild, so a ground road laid under a viaduct where its columns stood is refused
  too). The columns are planned on the way as the network lays it (`RoadNetwork::as_built`): an
  end dropped on a road lands on it at that road's level, so a raised road ramps down to the road
  it joins and needs no column at that end; an end left in the air needs one within 10 m, and its
  refusal says so ("ends in the air over a road at ..."), apart from the span gaps. The plan view
  runs the very check implementing runs (`World::project_check`, the trial rebuild included) on
  the active project whenever it changes: what would be refused is red on the map with a red pin
  and a label at the spot, and the toolbar says "Will be refused: ..." with a button that flies the
  camera there; a refused implement reads "Project 1: 0 of 1 roads built, nothing changed. ...".
  The preview shows the columns where they will stand, orange (with the lot outlined and the
  building named in the sub-pane) where one buys out a building. A deck at least 6 m up at both
  ends of a piece (`pillars::SPAN_LEVEL`, `RoadNetwork::spans_lots`) spans over the lots under it:
  it takes no corridor from the parcels; a building taller than fits under it (`floors_under`:
  deck, roof and 3.6 m storeys) is bought out, growth there is capped to what fits
  (`World::deck_cap`, in `height_cap`), and a lot under a deck too low for a storey goes. One of the
  city's own structures under a deck refuses the road unless it is a building of storeys that fits
  (a plant, a station or a dock never). Columns are kept on the world (`World::pillars`, serde
  default) with the key of the way they carry, so a way drawn again the same keeps them where they
  stood while each is still clear; a save from before pillars plans them on load without pulling
  anything down. `terrain::viaducts` draws each column at its deck's height (a highway hoisted by
  the grading alone keeps its evenly spaced columns).
- **Intersection editor** [tools]: Edit mode, click a junction or a crosswalk (Ctrl+click still
  grabs a control point there). A modal (`ui::junction`) with a top-down orthographic render of the
  roads only (`render::junction_view`: a camera on its own render layer, which the road, lane-arrow,
  path and footbridge meshes are on as well as the main one, filming into an image egui shows; the
  wheel zooms) with each arriving street's lane arrows and the crosswalks over it, clickable, and
  beside it: signals (auto / signals / none), each phase's green (5..90 s), the protected left
  (when needed / always / never, 3..30 s), left turns allowed or forbidden for the whole junction
  or per street, lane permissions per lane (L, S, R toggles: the player's lane arrows, checked as
  `set_lane_use` checks them), and per crosswalk an upgrade. The junction's settings are a
  `junctions::JunctionRule` kept by the traffic (`Traffic::junction_rules`, anchored by the
  junction's position and the directions its streets leave it, applied on every lane rebuild
  before the player's arrows: a forced signal, the greens on `Intersection::greens`, the arrow mode
  and length, and banned lefts taken out of the approach's offered movements). Crosswalk upgrades
  are `junctions::CrossingRule`s on the world (`World::crosswalks`), applied to the walk graph after
  every build (`WalkGraph::apply_upgrades`):
  - a push-button signal (`Crossing::button`): a walker at the kerb presses it and waits; 3 s later
    traffic gets a red (a hold on every lane over the crossing) for 12 s, then at least 20 s of
    green (`pedestrians::ButtonLight`). Nobody pressing, nobody stopped;
  - a footbridge (`walk::Footbridge`): a walkway 6 m up a little to the side of the crosswalk and
    stairs from each end along the pavement, away from the junction. The walk graph gets raised
    nodes (`WalkNode::level`) and raised edges (`WalkEdge::raised`, listed in `WalkGraph::raised`):
    generic, so skywalks between buildings can join the same network. Raised nodes are never
    snapped onto (their stairs reach them). A walker on the bridge crosses no street, so holds no
    one up; crossing at grade under a bridge costs `BRIDGED_GRADE_PENALTY` (30 m) more in the
    route search, so walkers take the bridge when it costs them only a little more. Walkers are
    drawn at their level on the leg they walk (`World::walker_level`); the bridge itself is
    `nd_procgen::footbridge` (deck on columns, glass balustrades, steel rails, neon strips, stairs).
  The remote API's `intersection` command reads and sets all of it.
- **Depth view** (Underground / Ground / Sky, U cycles, top bar): Underground hides the terrain,
  water, buildings, lots, surface roads and walkers, draws surface roads as faint outlines and
  shows the tunnels (drawn as their own street layer, `StreetLayer::Underground`; each layer draws
  only its own roads' crossings, beacons and signal posts). A road tunnel is a tube
  (`street::bore_shell`): a horseshoe of concrete lining (straight walls to a 2.6 m spring line,
  then a half ellipse whose rise grows with the width, up to 4.6 m), darker lining bands every
  14 m, an invert under the carriageway and a lamp line high on each wall. Every face of it looks
  into the tube and the materials cull back faces, so the half nearer the camera is never drawn:
  from above the vault is cut away, from the side the near wall, from any angle, and the cars
  inside show against the far wall. It is built with the rest of the street meshes when the
  roads change (no per-frame work) and wears the shared Concrete material, so heat vision and the
  focus views swap it like everything else (`render/swap.rs`). Its traffic shows too:
  the cars in them: a car is in the tunnel layer by the lane it is on (`World::car_in_tunnel`),
  hidden above ground and shown below, not by its height (near a portal, where the cutting is
  graded down to the road, cars and wrecks used to show above ground and vanish below). The Tunnel preset switches to it, other presets back to
  Ground. Sky is a placeholder. The metro (see Subway) shows here: its bores, platforms, shafts and
  trains carry the tunnel tag.
- **Highway and the map edge**: two carriageways cross the map at
  y = 420 from the start; through cars enter at the map edge and leave at the far one.
  A new map has nothing else: no streets, the player builds the rest. Every way whose end reaches
  the map edge (`landscape::MAP_EDGE`) runs on straight to `WAY_REACH` (5 km out), beyond the fog
  (`SIGHT`, 2.6 km) from anywhere the camera may be (`CAMERA_BOUND`); those far ends are the only
  places vehicles from elsewhere appear and leaving ones are removed (through traffic, newcomers'
  cars, lorries, trains; ships leave the same distance out). Dead ends on the map lead nowhere.
  Newcomers need a way in: a street joined to the highway by ramps, a road off the map edge, or a
  working station; walkers arriving by road are set down at their door. Through traffic on the
  highway carries on across a road rebuild. The ground and water beyond the map are drawn out to
  the horizon, graded to the ways that run out there. Docks
  (`nd_sim::ports`) stand on the shore with a pier and a street stub, take over grown lots, and work
  once their street reaches the city. [dock-access] They need only the water's edge: one not set
  down by a street lays its own access road (`Port::stub`, a 1+1 street with pavements from its gate
  14-32 m inland, turned up to 60 degrees or shortened round water, buildings and other docks) whose
  free end is marked for the player to draw a road onto. Off the roads a ferry terminal's boats
  still sail (`Port::sails`) but carry no one (nobody steps off or is sent there; residents back
  from a day out are set down at home), nothing lands at a port, nobody is hired. A dock or station
  over zoned ground takes it out of the zone and is drawn as itself at once (it never inherits
  the clearing); a dock can be bulldozed, giving its ground back to the plan: freighters route by water (river centrelines, tributaries
  joining, mouths opening on the sea) from off the map to the berth and back, landing imports for
  lorries; ferries run terminal to terminal or out to elsewhere with fares. A ferry terminal must
  stand on water that reaches the map edge; it is given one or two towns off the map
  (`ports::RemoteTerminal`, named from a list per edge) beyond the edges its water reaches
  (`Landscape::water_routes_out`: a coast reaches only the edge its sea opens onto, so a terminal on a
  west coast sails west; a river running off both sides reaches both). Its two ferries take turns
  between those towns and the other terminals in town; a run off the map sails out over the edge
  (hurried along out of sight), stands at the far pier and comes back. Visitors who come by water
  ride in on it and step off when it ties up; on the morning boats commuters come for a shift at
  businesses near the terminal short of staff (they count as staff, the business pays them 50, and
  they walk back to the pier after five); residents living near a terminal take day trips out
  (`Activity::Abroad`) and come back on an evening boat. Everyone leaving waits on the pier for the
  next run out. One terminal is enough for all of it. Hydro (river current at
  least 0.8 m/s, output scales with current), tidal (sea shore, two tides a day) and geothermal (hot
  spot, scales with heat) plants extend `PowerPlantKind`.

## The social layer: people talking, friends and feuds, crews and clubs

`nd_sim::social` (data and pure rules), `world/social.rs` (the systems), `world/social_tests.rs`.
People who share a bench, a bar, a workplace, a bus or a few metres of pavement get talking; how
it goes builds friendships and feuds; friends and enemies then act differently; friends with a
passion in common form groups that make things in the city.

- **Ties** (a `Circle` per citizen in `World::social.circles`, at most `MAX_TIES` = 16, the
  weakest dropped first, never a partner): affinity (-1..1), familiarity (0..1), trust (0..1),
  when, how and where they met (a bench, a bar, a park, work, a friend's home, transit, the
  street, a club), talks, romance (`spark`), money lent. The label is derived: stranger,
  acquaintance (familiarity 0.08), friend (affinity 0.3 and familiarity 0.25), close friend
  (0.6 / 0.5), partner, rival (-0.25), enemy (-0.55). Household members have no ties (they are
  family). Ties and conversation notes are saved as tuples to keep saves small.
- **Compatibility** is fixed per pair (`compatibility`): the gap on each personality trait
  (sociability and hedonism weigh most), age (three decades apart rarely click), class (rich and
  poor rarely agree), the strongest shared interest (`interests`: music, sports, books, art,
  business, gardening, the neighbourhood, from personality, age and class) and a little chemistry
  from a hash of the pair.
- **Encounters** (`step_social`, every 3 ticks): everyone out of the house is put in a spot: a
  venue, a stadium, a bench, a park, their workplace, a home with a guest in, a bus or train, a
  stop, a 4 m cell of pavement. Spots are sorted, and within each a few pairs (three at most) are
  drawn by a hash of the tick and the spot; at most `TALK_BUDGET` = 48 conversations a round, so
  the cost is bounded and never O(n^2). A pair talks with a chance by setting (a friend's sofa
  0.25, a bar 0.16, a bench 0.12, the park 0.07, a bus 0.025, work 0.02, passing in the street
  0.012) times sociability, more for friends, much less for enemies (unless drunk); the same pair
  not twice within 45 minutes. Adults talk with anyone grown up, teenagers with their own age,
  small children not at all.
- **A conversation** (`converse`) has a topic from their lives (work; rent, more when either is
  squeezed; the city's concern of the week; the news; the game, at the stadium above all; a place
  they both know; gossip about someone both know; music, art and books by shared interest; the
  weather; family) and an outcome: agreement, laughter, flirting (adults near in age, single), a
  favour (someone in need, and trust between them), an argument, insults, now and then a fight
  (enemies with drink or a rebellious streak, in a bar, a park, the street or at work). The chance
  of it going well grows with compatibility, existing affinity and mood and shrinks with a touchy
  topic between classes and with drink. Affinity and trust move by the outcome and drift towards
  compatibility; familiarity grows. Becoming friends, close friends, a couple, rivals or enemies
  is remembered; every talk goes in the citizen's last six conversations. Gossip moves the
  listener's feeling for the third person towards the speaker's (by trust); a bad word about a
  place costs its reputation a little. A fight is an assault: counted as an offence and reported
  to the police (`report_crime`); both remember it.
- **What it changes**: `choose_place` adds `social_pull`: friends there now (+0.15 each, two at
  most) and friends who are regulars (+0.06), an enemy there now (-0.6) or whose regular spot it
  is (-0.2), a band playing tonight (+0.35 plus its reputation; +0.9 for the band's friends, +2
  for the band). From 11:00 to 22:00 (`plan_outings`, hourly at :25; daytime only for those
  without work) a free, sociable person may meet a free friend at a place they both like: living
  within 700 m, one calls round and they walk there together at the slower one's pace
  (`social_pace`); otherwise both go. With nowhere both like, one pops round to the other's home.
  In the park a friend sits on a friend's bench if there is room (`seat_by_friend`), and close
  friends and partners can come along on an outing (`bring_company`). At midnight: the jobless
  are taken on where a trusted friend works, a household squeezed by its rent moves to a cheaper
  flat where a friend lives, the broke borrow from a friend with savings (paid back when they can;
  a debt left too long sours it), and a partner living alone moves in with the other
  (`move_into_household`). Ties fade without contact (familiarity after 2 days, affinity after 5,
  grudges slower); a word exchanged once is forgotten after three days.
- **Groups** (`Group`): each midnight a few people with two friends or more and a strong passion
  (0.45) start a band, sports club, book club, street-art crew, startup, community garden group
  or neighbourhood association with the friends who share it (three people, two for a startup;
  neighbours within 450 m for an association). Kinds already common are less likely. A group has a
  base (the place of the right kind its members like best), meets every few days at 18:00 (those
  who turn up grow closer), recruits members' friends who care (never someone with an enemy in
  it), builds momentum from its members' passion and meetings (cooling 5% a day) and at its kind's
  threshold makes something: a gig at its venue (news, the venue's name up, a crowd drawn that
  night), a match in its park or a reading at its café (the place's name up), a mural (painted on
  the street face of a building near the members; land value +0.02 x quality near by, less
  where the land is dear), a community garden on a vacant lot (land value +0.025; lost if the lot
  is built on), a business (the members' pooled savings, 40% up to 2,500, open a venue, bakery,
  gadget shop or office on a vacant lot near the founder, who owns it), or a petition to the
  player (`Ask`: more police, lower rent, a safe crossing, clean streets, a park, jobs, quiet;
  quoted in the complaints). A petition the city answers (checked daily) cheers them up; one
  ignored for three days becomes a protest at the nearest civic building (else a park), members
  and friends marching at 11:00; after eight days it lapses. Rivals inside a group cost it
  momentum and may split it: the one with fewer friends in it walks out with those who like them
  better, starting a group of their own when there are enough. A group with no momentum for ten
  days, or fewer than two members, folds.
- **Visible** (`ui/social.rs`, `render/social.rs`, `nd_procgen::social`): speech bubbles over
  people talking when the camera is within about 320 m (an icon and the topic; within 90 m the
  line said; red for a fight; at the door of the bar when they talk inside; overlapping ones
  stack), a note over a venue while a band plays and a fist over a protest; murals and gardens.
  A mural is painted on its building as drawn (`nd_procgen::social::facade_spot`): the
  building's geometry is generated again, its walls facing the street grouped by plane,
  anything drawn within 3 m in front of a wall (windows, frames, signs, balconies, AC units)
  marks cells there off, and the largest clear rectangle (up to 16 x 12 m, at least 3 x 2.4 m,
  lower and nearer the street preferred) takes the paint; a wall with no clear stretch is
  painted round its windows. The design (`paint_mural`) keeps a crew's palette, motif (a
  synthwave sun, a figure, a face, an eye, rings, waves), rays and skyline or hills from wall
  to wall, laid out per mural from its seed: gradients and soft spray edges from vertex colours
  on a grid of up to 40 cells a side, crisp neon strokes (Glow), the crew's tag in block letters
  with a drop shadow, two lamps over the top. A second, unlit copy of the paint is shown only
  after dark (daylight under 0.35), so the mural reads floodlit at night. Poorly regarded crews
  scrawl a tag instead. Only a mural with no wall to paint (a vacant lot, a glass front) stands
  on the old free-standing wall, painted the same way.
  The person pane lists friends and foes (clickable, coloured by bond, how and when they met),
  their groups and recent conversations ("talked rent with Mara Okafor at The Blue Lantern:
  agreed"). City Data's People › Social has the day's counts, a friendships chart, pairs by
  bond, every group (members, base, works, momentum) and petitions to the player; social news
  toasts.
- **Determinism**: every roll is a hash of the pair, the tick and a salt; the world's random
  stream is drawn only when a startup opens its business (`build_on_as`, as other founders do).
- **Numbers** (the perf probe's 4,000-person city with rule minds, days 5 to 18 after loading):
  6,000 to 8,000 conversations a day, about three in four warm; 200 to 300 new friendships and 75
  to 120 feuds a day once going; after two weeks about 1,300 friend pairs, 1,000 close-friend
  pairs, 170 couples, 450 rival and 280 enemy pairs; 10 to 25 meet-ups and a few home visits,
  jobs through friends and moves-in a day; two groups formed a day, their first works after about
  five days. Cost: about 0.1 ms a tick.

## Minds and the LLM plan

```
trait MindProvider {
    fn decide(&mut self, ctx: &MindContext) -> Option<Decision>;
    fn poll(&mut self) -> Vec<(CitizenId, Decision)>;   // async results
}
```

- `RuleBasedMind` answers instantly for everyone. It produces a `Decision { intent,
  thought }`. The thought string is the exact slot an LLM fills later.
- `LlmMind` wraps a `MindBackend` (anything that can take a prompt and answer later).
  Only citizens marked *spotlight* (the ones you inspect, or notable ones) get LLM
  calls. Requests are batched and asynchronous; until an answer arrives the
  rule-based decision stands in. Answers are structured (intent + thought + optional
  new memory) so the game can act on them safely.
- Memory stream per citizen, in the style of generative agents: arrivals, job
  changes, places visited, notable events. Prompts include the last few memories,
  the personality, the time of day and the city's state.
- Later: dialogue when you click a citizen, citizen-to-citizen gossip that
  propagates memories, LLM-authored shop names and ads, district reputations.

## Rendering

- Y-up world. Sim `(x, y)` maps to world `(x, 0, y)`.
- Terrain: flat basin for the city, procedural hills around it.
- Surfaces: every primitive a generator emits lands in one of sixteen surface buckets
  (concrete, plaster, brick, stone, panel, metal, rust, glass, wood, tarp, asphalt,
  paving, dirt, foliage, matte, glow), plus seven weathered variants (worn concrete,
  plaster, brick, panel, metal, paving, asphalt) whose textures bake in stains, drip
  streaks, cracks, worn-through paint and rust bleeding from fixings
  (`textures::generate_wear`). Low-tier buildings, decayed ones and streets on cheap
  land are moved onto the worn materials wholesale (`Geometry::weather`), so grit costs
  no triangles. Each surface is a material with a procedurally
  generated, seamlessly tiling colour texture and normal map (`nd_procgen::textures`),
  roughness and metalness. Meshes carry world-scale planar UVs (horizontal along a
  wall, vertical up it), so brick courses line up across a facade and nothing has to
  be unwrapped. Vertex colours set the hue; the textures are grey. Glow stays unlit
  with mildly HDR vertex colours and low bloom.
- Parcels: the road corridors are subtracted from the land within reach of the roads
  with polygon booleans (`geo`), so a curved road removes exactly its own strip. Long
  straight frontages are first chunked where the coordinate along the road is a
  multiple of 96 m and every chunk seeds its own random stream from its own shape, so
  works elsewhere never move a parcel: the same road gives the same parcels. Road corridors are one
  mitred polygon per chain of road pieces, so a curve's edge is smooth. Rows one lot
  deep are peeled off every road frontage at once as a union of strips along the road
  edges (nothing can fold over on a bend), the first frontage claims the corners, and
  rows deepen while the leftover core would be a thin strip so facing rows meet in the
  middle. Each row is cut across at lot width perpendicular to the local road, cuts
  that would leave a sliver or a lot without frontage are moved along, thin strips
  squeezed between rows are halved and folded into the neighbouring lot that shares the
  longest boundary (while the result stays convex and at most three lots deep), and
  sharp inward corners get one straight cut. A parcel is usable
  only if an edge of at least six metres lies along a road and its shape is compact;
  anything else (block cores, slivers, land behind other lots) is kept as landlocked,
  unusable ground that nothing is ever built on. Parcels keep their exact outline, so
  lots on a diagonal road are trapezoids and corner lots are triangles, and buildings
  extrude the parcel polygon.
- Architecture: every (land use, tier) pair has its own family of styles, picked by
  weight from `assets/rules/buildings.ron`, assembled from a shared parts toolkit
  (`nd_procgen::buildings::parts`: facades, windows, storefronts, signs, LED screens,
  balconies, fire escapes, roof gear, fences, tanks, pipes, cranes, vehicles, trees,
  stalls, string lights) and a dense-city kit (`buildings::kit`: footprint cells,
  crammed windows with cages and AC boxes, signs hung out over the pavement, antenna
  forests, sky bridges, glowing crown outlines, laundry poles). Forty-seven styles:
  - Residential: the Stacks, Kowloon walled blocks, capsule hotels, walk-up tenements,
    brutalist blocks, balcony slabs, courtyard blocks, Hong Kong pencil towers, Shanghai
    twin slabs, stepped hillside terraces, crystal towers, glass castles (clusters of
    faceted crystals with cut crowns, replacing the old stone keep), garden towers,
    Ghost-in-the-Shell glass terraces, Akira megastructures, villa clusters.
  - Commercial: shophouse rows, market halls, night market arcades, retail blocks,
    podium offices, pencil towers, neon towers, corporate slabs, twist towers,
    cantilevered malls, glass terraces, megastructures.
  - Industry sub-zones have their own looks, chosen by the business: farms (fields in rows,
    a barn, silos, a greenhouse), oil fields (pumpjacks, a derrick, stained ground, a flare),
    refineries (columns, pipe racks, tanks, flares), container yards (stacks, a gantry, a
    loading lane), fish processing (sheds, an ice plant, crates, a boat on blocks) and truck and
    bus lots (marked bays filled to the sim's occupancy, a fuel island). Families `Farm` and
    `Logistics` join the adjacency table: farms cluster and shun heavy industry.
  - Industrial: junkyards, rusted warehouses and garage workshops (low), saw-tooth
    factories, tank farms and flatted factories (middle), datacentres, container depots
    and semiconductor fabs (high).
  - Entertainment: shack bars and night markets, clubs, cinemas and neon towers,
    casinos, arenas and megastructures.
  - Civic: precinct houses, ministries, libraries, courthouses, monuments, and the
    brutalist side of the state [pt-visual]: civil defence bunkers (battered concrete,
    slit windows, blast doors under a visor, searchlights, a barbed fence), ministry
    towers (an inverted ziggurat of finned concrete floors over a columned glass lobby,
    the emblem, a lidless eye in a ring, on the crown) and surveillance bureaus (a near
    windowless dark monolith with lit slits, a camera ring and a radar dome).
  - Monuments the player places (`buildings/monuments.rs`) [pt-visual]: by tier a menu of
    obelisks with glowing pyramidions and neon bands, colossal figures in long coats with
    a raised torch and a lit visor, triumphal arches facing the road with neon down the
    piers, memorial plazas (a dark wall of lit names, an eternal flame, a field of
    stelae) and an upright halo ring round a floating crystal; the seed picks one.
  - Courtyard blocks are a ring of wings, one quad per lot edge sharing its inner corners
    with the next (`residential::courtyard_ring`), so every outer and inner face has a
    wall behind it (wings used to be boxes shortened on a wedge while their windows ran
    the whole edge). A garden tower on a big lot with under seven floors is a garden
    courtyard instead: white plaster, tall glass, a pool court and planted wing roofs.
  - Roofs [pt-visual]: `parts::green_roof` (sedum, shrub beds, a gravel path) on high-end
    podium offices round the tower and on every setback of a high-end slab;
    `parts::solar_roof` (rows of panels with a walkway every fourth row) on high-end
    data centres (one row of chillers kept) and on high-end podium towers.
  - Park: scrub lots, gardens, plazas.
  Everything is attached to a surface: roof gear uses the roof footprint, columns hold
  up overhangs, wires run from poles to real units, jacks prop unsupported corners.
  The newer styles change form with their floors (`kit::Form`): a low site gets the
  walk-up or pavilion version, a middle one a podium and slab, a tall one a tower with
  setbacks and a crown. Rules files that predate a style fall back to its built-in
  weight.
- Style adjacency (`buildings::adjacency`): every style has a family (slum, walk-up,
  slab, spire, luxury, retail, office, night, heavy, tech) and a cluster score for its
  own kind; families like or dislike each other. The pick multiplies each option's
  weight by its fit to the site (towers need floors, pencil towers a narrow frontage)
  and by `exp` of the summed affinities of the buildings within 60 m, nearer ones
  counting more. The Stacks and Shanghai slabs cluster; spires, glass castles and
  arenas avoid repeating a neighbour. The renderer keeps a `StyleBook` that resolves
  styles in build order (a building sees only older neighbours), so the result does not
  depend on drawing order and a building keeps its look when a newer neighbour arrives.
- Earthworks that fit the land [pt-visual] (`landscape::earthwork_hold`, `earthwork_band`): a
  highway, slip road or railway eased 3 m or more below the natural land along its middle (a
  cutting) or above it (a fill) is held by retaining walls like a road the player sank: the ground
  is held at the way's height out to a band 5 m past the hard shoulder (pavement for a street) and
  is the natural land beyond, instead of 1-in-2.5 slopes graded up to 160 m out. The cutting walls
  (`terrain::works::tunnel_works`) fill exactly that band, face at the shoulder; a fill's wall
  stands from the land up to a parapet. Ways eased into a gentle slope are graded, never walled
  (walls used to stand free on the slopes 6 m off a highway). The Road tool warns where an
  at-grade leg's land climbs or falls 6 m more than the grade allows (a walled cutting or fill,
  or a tunnel or viaduct instead) and where a straight highway or railway turns more than 35°
  at a point. On a slope a junction pad hangs a 0.9 m asphalt skirt from its edge
  (`draw_pad`), closing the vertical slivers that opened between the pad's straight edge and the
  corners' aprons on their arms' profiles at steep junctions. A footpath crossing the track
  (no street) gets a dark rubber panel at the rails' height over the ballast and sleepers, the
  rails flush in it, ramps down to the paving and yellow tactile strips; the crossings' flashing lamps throw a pulsing red point light on the road at
  night (a pool beside the street lamps', a third its size, none at Low).
- Ramps (`roads::is_ramp_spec`: one-way streets narrower than a highway): an end dropped on a
  highway gets a lead point `RAMP_LEAD` (40 m) before a merge or after a diverge, beside the
  carriageway (`roads::ease_ramp_ends`), so the ramp joins nearly tangent; the join is drawn as a
  taper filled to the carriageway edge with edge lines and a chevron-hatched gore (0.6 m bright
  strokes every 4 m between solid 0.3 m outlines, readable from the top-down camera), no junction pad,
  crossings or stop lines (`street::ramp_gore`). Every one-way road (ramps, highways, one-way
  streets) has an arrow painted in each lane every 45 m (`street::travel_arrows`), in the
  holograms too. A ramp that has to stand 2.5 m or more above the land after easing (climbing to
  the highway's embankment or a deck) is made a viaduct, so it stands on columns, not earth.
  Slip roads proper are flagged `Segment::ramp` (`RoadNetwork::classify`, after every laying
  and on load): one-way, non-ring streets reached from a highway node through plain bends and
  ramp forks/merges (nodes of at most three one-way streets), up to the first two-way street;
  a one-way street in town is never one. Like the highway (`Segment::is_motorway`) they front
  no parcels, take no kerb parking, have hard shoulders instead of pavements and carry no walk
  graph edges, so nobody walks them. Their lanes ease their limit (`Lane::speed_limit` at the
  start to `Lane::limit_end`, `LaneNetwork::ramp_profiles`) from highway speed at the
  carriageway to street speed where they meet the town, a smoothstep over the ramp's whole
  length through its pieces and connectors, capped on bends at sqrt(3 r). Drivers read the
  limits on their route up to 220 m ahead and brake for a lower one at 1.5 m/s²
  (`Traffic::limit_ahead`); the car-following model's free-road term brakes at most 3 m/s²
  (`FREE_BRAKE`), where plain IDM stopped a 25 m/s car dead on a 6 m/s connector. A car
  covers as many short lanes (curve pieces, connectors) as a physics step takes it rather
  than one per step, which made it stand at each short piece's end and then spring ahead.
  Cars are drawn at the height of the road under their drawn position
  (`World::car_height_at`), not where they end the tick, so they no longer hop a step on
  every grade.
- Road holograms (`nd_procgen::preview::road_preview`, `render/holo.rs`): the road being drawn
  and every pending project's roads are drawn with the street generator itself (carriageway, lane
  lines, kerbs, pavements), at their planned heights with viaduct decks and columns and tunnel
  bores, a pad where each will make a junction, translucent in the preview colour; legs steeper
  than the grade turn red and so do the lots with buildings the road would take. The tool's
  hologram is rebuilt only when the points, the pointer (in quarter metres), the level or the plan
  change. Street meshes are set on their own run's height profile (`StreetInput::heights`), so a
  viaduct and the street under it are each drawn at their own height.
- Streets (`nd_procgen::street`): roads are drawn as runs, chains of segments joined
  at plain bends, with one continuous mitred carriageway and pavement per run so a
  curved road is one road rather than a string of pieces; asphalt with lane markings, kerbs and paved
  pavements, zebra crossings and stop lines at junctions, signal posts with an arm
  and one head per lane, lamps, bins, benches, bus shelters, vending kiosks, bollards
  at the corners, utility poles with sagging wires on cheap streets, trees in planters
  where the land is worth it.
- Cameras: an orbit camera for planning and a first-person walk mode (Tab) that
  steps onto the nearest pavement with mouse look and WASD, a wider field of view and
  closer fog. Tools are disabled while walking. In walk mode only, `nd_procgen::walk`
  adds street-level detail within 90 m of the eye (redrawn every 25 m): lit A-frames,
  lightboxes and blade signs at shopfronts, vending machines, food carts with steam,
  manholes and steam vents, cables, lanterns and banners strung across cheap streets,
  puddles holding a smear of sign colour, scooters in the gutter, bin bags, crates,
  bikes, cones, planters, and cracks, stains and gum on the slabs. It is laid out per
  road segment from its own seed, so props stay put as the walker moves, and set on that
  segment's height (a viaduct's pavement, a street on a hillside); highways, ramps, rings and
  tunnels get none.
- Performance: the sim gets a real-time budget per frame (12 ms) and drops its
  backlog instead of stalling the renderer; shadows use three cascades out to 650 m;
  the HUD shows fps, meshes and triangles.
- Look ("grounded neon", `render/look.rs`): realistic light under the neon. Surfaces carry a
  metallic-roughness map beside their colour and normal maps (`textures::roughness_map`: grime
  and cracks rougher, clean spots glossier, rust less metallic); expensive car paint has a
  clear coat. The dome's colours are painted, prefiltered on the CPU, into two small cubemaps
  (reflection levels and diffuse) for an environment light, repainted at most twice a second,
  so glass, paint and wet-looking asphalt reflect the sky and the city's glow and shadowed
  sides take the sky's colour; the flat ambient is cut to what that leaves. Screen-space
  ambient occlusion uses Bevy's shader with a radius that grows with view depth
  (`ssao_city.wgsl`, 0.73 to 5 m) so buildings darken the street at their feet from the
  orbit camera. TAA stops lamp posts and lane markings shimmering (window lights are in the
  depth prepass with the same discard, so dark windows leave no depth). Volumetric fog fills
  a 140 m slab of haze round the camera, lit by the sun or moon (light shafts) and by the
  horizon's colour, so it fades into the distance fog and the dome. Bevy's `Atmosphere` was
  not used: it needs physical sun units and a black night sky, and the dome's light-pollution
  band is the night's identity.
- Graphics tiers: Low (MSAA 4x, 1024 shadows), Medium (TAA, light SSAO, 2048 shadows), High
  (TAA, full SSAO, volumetric haze, 4096 shadows). The setting (View window, `graphics` API,
  `ND_QUALITY`) is Auto or a tier. Auto starts at High and steps down when the smoothed frame
  time stays over 18.8 ms for 2 s, up when it stays under 17.2 ms for 8 s; a step up taken
  back doubles the next wait (up to 4 min), changes are followed by 3 s of settling while
  pipelines compile, and it holds when the sim, not the renderer, fills the frame. One system
  derives the camera's components from the tier and the view (heat vision adds motion blur,
  drops the haze) and touches the camera only when that set changes, so nothing has to be
  put back. The Dev window shows the tier, the frame time and the effects.
- Citizens: small figures that walk the pavements, or ride a bike mesh when cycling; positions
  are interpolated between ticks from the previous pose so nothing jumps. Cars and trucks have wheels.
- Vehicles (`nd_procgen::vehicles`): one parametric generator draws every road vehicle from a
  `nd_sim::VehicleModel` (body, class, wear, tuned, livery, paint): hatchback, sedan, coupe, SUV,
  pickup, faceted cyber-truck, box van, delivery van, panel truck, semi with a container, garbage
  truck, a rounded driverless bubble bus (glass band all round, lidar puck, no cab) and an
  articulated one, motorcycle, scooter, a long low red Akira bike, and the six-wheeled pavement
  delivery robot. Bodies are side silhouettes extruded across with a narrower glass greenhouse;
  class bends them (cheap: taller, boxy, black bumpers, rust, a primer door, a dead headlamp;
  expensive: lower, raked, chrome, full-width light bars), tuning adds a spoiler, wide wheels, neon
  side strips and underglow, and a fleet livery (pizza, parcels, grocery, construction, utility,
  transit) repaints it with a stripe, a lit sign panel and a beacon. Each variant is under 600
  triangles and built once into three meshes (lit body, glow lamps, additive night fx) that the
  renderer caches per variant; underglow and the Akira bikes' ribbon light trails only show after
  dark. Parked vehicles in building dressing use the same generator at low detail.
- Mobility: one sim minute shows `traffic_seconds_per_tick` (1.5 s) of motion for everyone, so
  walking is 1.4 m/s, cycling 4.5 m/s and driving the lane speed limit on the same clock. Citizens
  own a car and/or a bike by class (poor: 12% car, 60% bike; middle: 60% / 35%; rich: 92% / 35%)
  and buy or sell them as savings allow. Walkers keep right of the pavement's middle, bikes the road
  shoulder, both the centre of a path; bike owners ride to parks for pleasure.
- Who drives what (`nd_sim::vehicle`): the model is chosen at purchase from income, age and the
  personality's hedonism. The poor run beat-up cheap cars, scooters and old motorcycles (they can
  buy one with less in the bank, and sell it when broke); the middle hatchbacks, saloons, SUVs and
  pickups, the young often a motorcycle; the rich coupes, SUVs and cyber-trucks, trading in an old
  wreck. Young rich hedonists tune theirs or ride an Akira bike. Motorcycles and scooters are
  driven in the car lanes like cars. Lorries follow their cargo (groceries and goods in delivery
  vans, chilled food and electronics in box vans, farm goods in panel trucks, imports and parts on
  semis, building materials in construction-liveried trucks) and wear the seller's livery; buses
  are bubble pods, a quarter articulated; through traffic mixes cars, semis, parcel vans and
  garbage trucks.
- Home deliveries (`nd_sim::bots`): every quarter hour from 08:00 to 22:00 a few grocers,
  bakeries and gadget shops sell an order to a household within 450 m that can pay (groceries
  only to a short pantry). Where the shop and the home both have land value 0.6 or more, a
  pavement robot carries it along the walk graph at 1.1 m/s, to the door and back (at most 40);
  elsewhere the shop's liveried van or the bakery's scooter drives it (at most 10 on the road).
  The sale is settled on dispatch. The report shows the robots under `bots`.
- Every part of a building stays on its lot. Lots are concave, triangular, slivered
  and skewed, so the toolkit is defensive: `offset_poly` limits mitres to 2.5 times
  the offset and drops inward-offset vertices that leave the polygon, `fit_inside`
  moves a shape onto the lot before shrinking it, `clamp_into` pulls a point back
  inside, `reach_out` measures the room a face has before the street, and anything
  laid out in the lot's bounding box (alleys, stairs, poles, masts, cranes, piles,
  turbine blades) checks the polygon first. A test generates every style on about
  six hundred lots cut by straight, diagonal and curved roads and fails if any vertex
  is more than 4 m outside its lot, below ground, or absurdly tall.
- Vacant land (`nd_procgen::vacant`) dresses by land value: below 0.38 it is dormant and
  trashy (dead grass, weeds, rubbish, tyres, drums, a broken fence, a tagged wall, sometimes a
  drum fire); in the middle dirt and gravel with wild grass, shrubs, rocks and a young tree or
  two; from 0.58 (the high tier) a parklet with mown lawn, a hedge with a gap, a path to a bench, bin, lamp and
  trees. Open ground far from roads gets rocks, scrub and dry trees scattered by noise
  (`terrain::scatter_nature`, capped at 3500 pieces, kept 40 m from any way).
- Grit: `parts::graffiti` paints tags, bubble letters and throw-ups on stacks, tenements,
  warehouses, junkyard hoardings, scrub-lot walls and any worn face; outdoor stairs, fire
  escapes and ladders rust when wear passes 0.4; streets below land value 0.35 get potholes,
  cracked slabs, faded markings and kerb litter.
- Inspecting: the selected building's lot is outlined, people and cars get a ring, the
  selected person's remaining route and recent trail are drawn with the destination lot, a
  hover card describes the parcel under the cursor, and a draggable person pane carries their
  story with a favourites list to jump back to. Zoning layer toggle with O.
- Track (`street.rs`): ballast, sleepers every 0.7 m, two rails; level crossings get planks
  over the carriageway, crossbuck signs, lights and raised barrier arms. Stations
  (`buildings/rail.rs`): brick hall with an arched glass front, canopy, clock and name
  board at the road; a 70 m platform along the track with a canopy on posts, a yellow
  line, benches and name boards; a ramp between them; a taxi rank and bike racks in the
  forecourt. Freight yards: warehouse with roller doors, a 90 m apron, a gantry crane
  spanning apron and track, container stacks, a forklift, lorries at the doors, fence,
  gate barrier, floodlights, weighbridge. Trains are drawn by the app as a locomotive and
  coaches or wagons placed along the route and blended between ticks.
- Bus depots (`buildings/transit.rs`): a shed with roller doors, bus bays with parked
  buses, a fuel island, a wash arch, an office and a barrier at the gate. Bus stops are
  drawn by the app from the lines (pole, sign, kerb mark, shelter), so the random street
  shelters became phone boxes. Buses are a long body with a window band and a destination
  blind, coloured by their line.
- Parks are drawn from the sim's plan (`buildings/park.rs`): the ground, path surface,
  bench condition, fence or hedge or railing and the tree mix follow the tier (dirt and
  weeds, dead trees, a broken bench and a drum fire on cheap land; lawn, hedges and
  broadleaf trees in the middle; forecourt, railings, gate piers, avenues of trees,
  conifers and palms on dear land). Trees are sampled on a shuffled jittered grid and
  kept off the paths, the features, the benches and the lot edge, so they fit any parcel
  shape. Each feature has its own drawer: court with hoops (one bent on cheap land),
  skate park with quarter pipes, playground with swings, slide, climbing frame, seesaw,
  spring riders and sandpit, pond or lake with reeds, lily pads, jetty, boat and island,
  fountain with jets, bandstand with string lights, striped carousel with horses, ferris
  wheel with gondolas and a ticket booth, kiosk with café tables and parasols, flower
  beds, statue, dog run.
- Power plants have their own styles: a coal station with a boiler hall, two stacks, a
  coal pile and conveyor and a transformer yard; a honeycomb of hexagonal sun trackers; a wind farm
  of helical vertical-axis turbines; a containment dome, hyperbolic cooling towers with steam
  and a double fence.
- Wind farms (`buildings/power.rs`): turbines of one size (18 m gunmetal mast, 11 m rotor of
  three graphite blades twisted a third of a turn, 3 m radius) in staggered rows 15 m apart,
  as many as fit with 2 m between every blade tip and the lot edge (`wind_sites`, tested for
  count growing with area and containment). The generator draws masts only
  (`Extras::animated_rotors`); the app puts a rotor entity on each mast (`wind_turbines` gives
  the same sites) and turns it about the vertical: still below wind 0.15, 0.6 to 4.2 rad/s
  above, easing up and down, stopped while paused. A red cap light and cyan tip lights trace
  the spin at night.
- Solar farms (`buildings/power.rs`, `render/solar.rs`): dark blue hexagonal panels (2.2 m to a
  corner, six cells with dark seams and a cyan rim light) on dual-axis trackers, a pedestal every
  4.2 m on a triangular grid, flat sides facing their neighbours, so stowed panels tile the yard as a
  honeycomb; touching parcels share one grid (`PlantField::solar_sites`). The generator draws the
  pedestals (`solar_trackers` gives the same sites); the app puts a panel entity on each and turns
  them all to face the sun the city is lit by (`nd_sim::power::sun_direction`), tilting at most 65
  degrees, waking from flat as the sun clears the horizon and lying flat (stowed) at night. The
  rotation is rewritten only when the facing moves a fifth of a degree. The zoning wash leaves plant
  and battery yards out (it flooded a solar farm on a residential lot green).
- [plant-groups] Solar and wind are sized to their lot (cost, upkeep and MW per m², capped at
  6× the reference lot on one lot); the Build window quotes them per m² and a card by the pointer
  gives the MW, cost and upkeep for the lot under it, and what it would join. Touching solar lots
  run as one "<Name> Sunfield", touching wind lots as one "<Name> Wind Park"
  (`world/plant_groups.rs`): derived from lot adjacency (an edge, or a lot line up to 2.5 m, never
  across a road or path), named by the oldest member, one row in the plant table, one inspector
  card, selected whole, +4% output per extra lot (up to +12%). A lot too small on its own may join
  a park of its kind next door when the joined lots reach the minimum.
- Hydro dam (`dam.rs`, drawn by `terrain::waterworks`): a concrete arch bowed upstream across the
  river beside the plant, stepped in three tiers, abutment blocks in the banks, a crest road with
  parapets and lamp posts, a spillway chute down the middle with radial gates, training walls and
  streaked white water into a stilling basin of foam that fades downstream, and two intake towers
  on the plant's side with a penstock to the turbine hall. Cyan light lines on the parapets, the
  ledges and the chute walls, magenta on the gates and the towers.
- Battery packs (`buildings/power.rs::battery`): container-sized white modules on plinths in rows
  (2 / 8 / up to 36 by size) with roof fans, door seams, an inverter skid at each row end, a control
  cabin and a fence; five status lights on each long face show fifths of charge, cyan charging,
  amber delivering, green holding, with a light line in the same colour. The sim raises
  `BuildingChanged` only when the lights would change (`Battery::status`).
- Coal smoke (`render/haze.rs`): no geometry in the plant. Each stack (`coal_stacks`) emits
  18 camera-facing puffs with a soft generated texture that rise, drift downwind (the wind
  direction veers slowly over days, the distance grows with wind), swell and fade over 36 s;
  14 wide, flat puffs of brown smog lie downwind at 7 to 30 m, which is what the street sees.
  Alpha comes from a palette of twelve steps per kind (plume at most 0.85, smog 0.38, both scaled by puff age and a soft texture), so the
  city reads through from above; the colour darkens at night; nothing casts or receives
  shadows. Thickness follows the plant's output: staffing times the grid's draw on supply.
- Window lights follow occupancy (`render/lights.rs`, `nd_procgen::lights`). [pt-visual] A jump
  in sim time of more than a stagger cycle (a long remote `step`) or back in it (a new city)
  snaps every building to its share, as a load does. In the app every
  window light is generated (`Extras::dynamic_lights`, about 92% of windows) through
  `parts::window_light`, which ranks it by a hash of its position and lays dark glass just
  behind it (`LIGHT_GLASS_BEHIND`, 13 mm: off the whole-centimetre grid the decals stand on, so
  it never shares a plane with a window frame; at 2 cm it lay in the frame's plane and the
  windows z-fought with the lights off; `buildings/coplanar_tests.rs` checks every style). When chunks are built, the rank and the building's slot go into the glow
  vertices' UVs, and chunk glow is drawn with a material extension that reads the building's
  lit share from a 256 by 256 one-byte texture and discards windows ranked above it. Four
  times a sim hour the app counts who is in every building (awake or asleep at home, staff)
  and writes `lit_share`: homes light a room per awake household in, fewer by day, a night
  light when asleep, dark when out; workplaces light with the staff in and customers while
  open, security lights when closed; buildings without homes or jobs stay lit; all of it
  scaled by the power share. No mesh is rebuilt. Nothing switches in step: citizens keep
  their own bedtimes (21:00 to midnight, up 05:00 to 07:00, `citizens::night_for` from the
  seed); the texture is refreshed every 3 sim minutes but each building takes its new share
  only on its own turn (one in five, by a hash of its slot) and eases toward it 12..44/255 a
  turn (a power cut is instant); and in the shader each window's threshold wanders by up to
  ±0.13 on its own clock (re-rolled every 40..180 sim minutes at its own phase, a sim-time
  uniform on the material), so blocks twinkle room by room.
- Map layers (`render/layers.rs`, ramps and heat grid in `nd_procgen::overlay`): land value
  (V), land density ([pt-growth] the pressure plus foot traffic per 64 m cell, a log heat map,
  `render/overlays.rs` `sync_overlay`), traffic load (a ribbon per lane from cars per metre and speed against the limit, green
  to magenta), traffic types (a disc under every vehicle: cars cyan, trucks orange, buses
  green; emergency red once the sim has them), people (a heat map of everyone where they
  are, indoors at their building), pollution and noise (plants by pollution radius,
  industry, nightlife, traffic), the pedestrian network (P), zoning outlines (O) and districts.
  **One layer at a time** (`map_layers.rs`): the top bar's Layer button (a dropdown with None)
  and the View window pick it, V, O and P toggle theirs, L steps through the heat layers, K
  through the land layers; the legend at the bottom right always shows the key of the layer on.
  Every layer is one `MapLayer` entry in `map_layers::MAP_LAYERS` (API key, label, picker group,
  hotkey, hint, the `Overlays` flag the renderer reads, a legend function); the picker, the
  legend, the `overlay` API and `status.overlays` follow the list, and `keep_exclusive` turns the
  older layers off whenever any code switches a flag on, so adding a layer is a flag on
  `Overlays` for its renderer plus one entry. The `overlay` API and `map {layer: "people"}` read
  the same data.
- Road condition (`render/nights.rs`, `overlay {layer: "road_condition"}`): a ribbon down every
  street piece the city maintains, green (new) through yellow to red (crumbling), rebuilt when the
  wear changes; the legend gives the mean wear on cheap, middle and dear land.
- Focus layers (`render/focus.rs`, the heat-vision material swap reused): with Traffic load on
  the city is dimmed and every vehicle on the road turns one light blue; vehicles in bored
  tunnels and the tunnels themselves are outlined through the ground (`layers::layer_gizmos`,
  a gizmo group with depth bias -1). Crime (`overlay {layer: "crime"}`): a heat map of the
  sim's hot spots plus open crime incidents (`crime_color`), the city dimmed a little less,
  police cars lit blue-white (light bars keep their colours), each police car's route ahead
  drawn over everything (flashing red and blue on a call, cyan on patrol) with a ring at its
  goal, and pulsing rings on crimes not yet answered. Heat vision wins over a focus view.
- Metro (`metro_ux.rs`, `overlay {layer: "metro"}`, M, Transit in the Layer menu): a focus view
  (`Focus::Metro`) that dims the whole city; over it every metro line along its tunnel in its
  colour, painted in screen space with egui (gizmo lines far off lost stretches under the
  translucent street overlays), and every station as a big marker: rings on the ground (a gizmo
  group with depth bias -1) and a round "M" badge with the station's name and a dot per line
  calling there. The metro line tool shows it (see Subway).
- Map view (`render/mapview.rs`, `overlay {layer: "map"}`): every mesh but water and the sky is
  hidden (after all visibility owners, in `PostUpdate`), and a flat unlit map is drawn: dark land
  (open over water), lots tinted by zone, roads as casing and fill ribbons by class (highway
  amber, avenue gold, street grey, path, rail), tunnels dashed, joints rounded. Street names
  (`nd_sim::streets`: the network cut into chains that carry straight on through junctions,
  named from the seed and a serial, kept across rebuilds by matching new chains to the saved
  outlines, saved with serde default) run along the roads, repeated along long streets and
  decluttered on screen, main roads first; pins with a glyph and the name mark services,
  plants, stations, ports, depots, civic buildings and big venues.
- Beach boardwalks [pt-visual] (`shore::boardwalks`, `render/waterside.rs`): a run of four or
  more public beach lots sharing their sides gets a wooden promenade along each lot's landward
  edges, meeting at the seams: deck on a skirt, a rail on the sand side open where each lot's
  path goes down, lamps with a neon band every 12 m, benches facing the sea, the odd kiosk.
- Marinas [pt-visual]: `Port::moored` (counted at midnight) is the berths the town's boat owners
  have let and the billionaires' superyachts lying at a large marina (each billionaire keeps one
  at the large marina nearest home, at most three a marina). The moored boats are drawn apart
  from the dock works (`terrain::moored_boats`): the berths let, in berth order, each in a look
  (`BoatLook`) that grows richer as the marina fills (speedboats among the sailing boats,
  catamarans among the yachts), and 60 m superyachts (four decks, a helipad, neon) side-to on an
  outer pontoon off the pier's tip.
- Parks merge visually: a park of several lots (`park::plan_parks` already plans one cluster)
  lays its lawn up to the seams (no inset on shared edges), keeps trees and lamps clear of the
  park's outside edges only, draws the zone outline only round the outside, and puts the
  forecourt, gate pillars and name sign on one anchor lot (`ParkLayout::anchor`, the largest
  lot on a street; serde default true for older saves). The plots stay separate lots.
- Night and sky (`render/sky.rs`, `update_daylight`): a dome of vertex colours 30 km out (the
  projection has no far plane), unlit and out of the fog, following the camera: the fog's
  colour at the horizon (so distant ground dissolves into it without a line), a light-pollution
  band above it at night (sodium orange under neon magenta, growing with the number of
  buildings), a dark blue zenith, and glare round the sun by day and dusk, faint round the moon.
  The fog itself is lit by the city at night and brightens toward the sun (`DistanceFog`
  directional light colour); night ambient is a warm violet bounce that grows with the city;
  moonlight is brighter; bloom rises at night (0.16) and falls by day (0.07).
- Land survey layers (`nd_sim::survey`, `world/survey.rs`, drawn by `render/survey.rs`): what
  the ground is good for, one wash at a time (K steps through them, the View window and the top
  bar's Layer button list them,
  the legend shows the key, what the layer means and the reading under the pointer). Resources:
  farmland (fertility less contamination), oil, geothermal hot spots (solid where a plant
  works, a faint halo around), rock, sand, water access, and deep water for shipping ports (the
  port's own width rule). Industry: for each kind the chance a new industrial lot there takes
  it, and "best industry" coloured by the favourite. These read the same `industry::weights`
  over an `IndustrySite` the sim draws from: `World::industry_site` and the survey both build
  the site through `SiteContext`, so the map cannot drift from the choice (the player's area
  bias is left out: the map shows what the ground wants). The static ground survey (16 m
  corners over the buildable square) is built once per landscape on a worker thread; the
  industry weights are recomputed when the day, the roads or the building count change.
  Read-only: no RNG, nothing in the world changes.
- Toolbar and sub-pane icons (`ui/icons.rs`) are painted with egui shapes: one per tool, a
  lane plan per road preset (lane arrows, centre line, rails, path paving, ramp curve), a
  glyph per land use in its zone colour.
- Building thumbnails (`render/thumbs.rs`): a few frames after startup every placeable's
  building is generated by the same grammar call the city uses (`generate_building` on a
  made-up lot of its size), spawned far off the map on render layer 7 with its own key and fill
  light, and filmed once by an orthographic camera into a 192 px image; the cameras and meshes
  go after 150 frames (time for new pipelines to compile), the images stay as egui textures
  (`Thumbs`) for the Build window. Save pictures: a straight-down orthographic camera over the
  built-up area renders into a 512 px image for 20 frames and a `Screenshot` of that image is
  written beside the save.
- Paths (`RoadKind::Path`): 3.2 m ways with no car lanes, drawn as paving with edge stones,
  low lamps, benches and bollards where they meet a street; they cut parcels but give no
  frontage, and a path crossing a street mid-block gets a zebra.
- Drone heat vision (`render/heat.rs`; H, the top bar's Heat button, API `heat_vision`): every
  mesh's material is swapped for a cached stand-in, cold surfaces dim and unlit, people,
  vehicles, drones and fires emitting flat at their temperature (lights and engines hottest,
  parked cars lukewarm); a `FullscreenMaterial` pass after motion blur and before bloom maps
  brightness through a teal-magenta-white thermal palette with scanlines, noise, a vignette
  and a reticle, and Bevy's `MotionBlur` smears what moves. Off puts every original back.
  The material swap is shared with the focus views (`render/swap.rs`): one original per
  entity whatever the view, stand-ins always derived from it, so heat vision toggled with
  the Traffic or Crime layer on can no longer leave heat's materials behind.
- Plant fields: touching solar or wind plants (`World::plant_cluster`, parcels sharing 4 m+
  of boundary) are drawn as one installation (`nd_procgen::PlantField`): each parcel lays its
  panel rows or turbine grid out in the anchor parcel's frame against the whole field's
  outline, no fence or margin on the seams; a plant coming or going redraws the chunks round it.
- Tower cranes: paint per site (orange, red, yellow, white), some with an LED billboard on the
  counter-jib; the head (`Geometry::cranes`) is its own entity that slews towards a new
  heading every lift (`crane_slew` of `SimClock::work_minutes`), so cranes work 8:00 to 17:00
  and stand still overnight and while paused, at a capped on-screen rate at any speed.

## The HUD: Build window, map layers, saves

- **Type** (`ui/theme.rs`): IBM Plex Sans for running text and figures, Chakra Petch (squared,
  cut-cornered, signage-like) for headings, window titles, buttons and street names on the map,
  IBM Plex Mono for monospace; Noto Sans Symbols 2 and egui's emoji fonts behind them for the
  symbols (★ ⏸ ⚡ ✔ ⚠ →). All SIL OFL, in `assets/fonts/` and embedded. One scale in points:
  Small 13 (the smallest text anywhere), Body 15, Button 15, Lead 17, Heading 19, Title 22,
  Mono 14; egui's text styles come from it and painters take its `FontId`s. The View window's
  Text size (Small 90%, Normal, Large 115%, Huge 130%) zooms the whole HUD, saved in
  `settings.ron` in the working directory (`ND_UI_SCALE` overrides it for a run). A test checks
  that every non-ASCII character in the app's and the sim's string literals has a glyph.
- **Bottom toolbar**: Inspect, Build, then the quick tools (Edit, Bulldoze, any tool that does
  not build), and the project controls. The strip above shows the active tool's options, or
  while the Build window is open just what is being built and how the last click went.
- **Build window** (`ui/build.rs`): one movable, resizable window for everything the player can
  put down. A tree of sections on the left (Roads, Zoning, Parks, Districts, Power, Services,
  Transit, Rail, Water, Attractions; the selected one lists its sub-sections, e.g. Services:
  Safety, Health and education, Sanitation; Transit: Buses, Metro, Air, Parking, Tolls,
  Freight); a grid of thumbnails in the middle (building renders, icons for roads, zones and
  lines); on the right the hovered or picked item's cost, upkeep, jobs, lot size, output or
  coverage and notes, then the active tool's options (lanes, level, brush, tier, the bus and
  metro line lists). Each thing is in one place: rail track and the stations and yards are all
  under Rail (no Rail preset under Roads, no rail pieces under Buildings), the bus depot only
  under Transit, the cargo hub under Transit, Freight. Picking an item picks the tool and preset
  that place it (`build::activate`); the tools are unchanged, so the digit and letter hotkeys and
  the API's `tool` command still work, and the window follows them: a building tool picked by
  key opens the window at its section. Items come from the sim's `BuildCategory::items()`, so a
  new placeable appears without UI changes (and gets a thumbnail).
- **Map layers**: see Rendering; the top bar's Layer button is the core control, the View window
  (once Land) also has lighting and the dim-projects and bus-line switches; land value and rents
  are City Data's City › Land and rent.
- **Saves and the title screen** (`ui/saves.rs`, `ui/title.rs`): Saves, Save as writes
  `saves/<name>.ron` (the name made file-safe), a sidecar `<name>.meta.json` (name as typed, day,
  population, treasury, buildings, landscape, city name, game id) and a top-down picture
  `<name>.png`; quick saves and autosaves get a sidecar and picture too. At startup a title
  screen lists the saved games newest first as cards with their picture, name, day and
  population, one row per city name: Continue loads the newest, New game opens the landscape
  picker, Play this map starts on the landscape already loaded under the name typed beside it.
  The city behind is paused while it is up; `ND_TITLE=0` skips it, and the API's `load`,
  `new_game` and `seed_demo` close it.
- **Perfect AI City** [perfect-city] (`nd_procgen::citygen::perfect` and `furnish`): a New game
  starting town ("Perfect AI City", `generate_city {preset: "perfect_city"}`, default seed 5)
  that is planned and built before day 1 and then left to grow. The site is a temperate coast:
  sea to the south, the river east of downtown, two lakes and the farms to the west, hot spots
  beyond the highway. The streets are the tensor-field tracer with one strong square grid (no
  radial plaza, no riverside streets), avenues about 300 m apart, collectors laid 2+2 as well,
  only the avenues crossing the railway (level crossings far apart, stations between them) and
  three diamond interchanges (`World::add_diamond`). The prevailing wind blows east, so the works
  (manufacturing, logistics by the highway, the coal plant, two nuclear plants, solar fields,
  batteries, the freight yard and the shipping port) are zoned whole round the river to the east,
  the farms and wind parks upwind to the west, homes between, a downtown of offices and shops by
  the sea with a park square, a public beach and parks along the shore. `furnish` then puts down,
  through the ordinary placement calls with every result checked and logged (`LaidCity::furnished`,
  the `generate_city` reply's `furnished`): straight 2+2 bridges where the river crossings are far
  apart, hydro on the river, geothermal on the hot spots (an access road out past the highway),
  police, fire, health and schools placed greedily until 99% of homes and shops are within reach,
  a university, 911 dispatch, jail, trash depots, a monument, a stadium, car parks round the core,
  parking wardens, a vertiport, a cargo hub, a central station and two stations, a ferry terminal,
  two metro lines, two bus depots and five bus lines, districts with policies (Downtown tech
  campus and Midtown neon strip with no height limit, Northside eco towers to 50 m, garden suburbs
  by the farms, Kowloon blocks and night markets by the railway, a tourist sea front, Eastworks and
  West Farms), footbridges over the avenues by the station, and a treasury of 50 million
  (`STARTING_TREASURY`; taxes stay standard). It takes 20-30 s to lay out. Load it from New game
  (Starting town: Perfect AI City), or regenerate the day-1 save with
  `cargo run --profile fast -p nd_procgen --example perfect_city -- 0` (writes
  `saves/perfect_ai_city.ron`; with a day count it also runs the city headless and prints a row per
  day). Tests: `crates/nd_procgen/tests/citygen_perfect_city.rs`.
- **City identity** (`nd_sim::identity`, `World::identity`): the city's name and a game id, saved
  with the world. A new game gets an id at once (`saves::new_game_id`: the time and 64 bits from a
  randomly keyed hasher, so the sim stays deterministic); the New game window and the title
  screen ask for the name (a suggestion from `names::city_name` is offered), and the City and
  Saves windows rename it. A save from before identities gets an id when it is next saved; a
  named one takes the name it was saved under as the city's (`saves::legacy_city_name`), quick
  saves and the autosave load as "Unnamed city". The Saves window and the title screen group saves by city name.
- **Game history** (`ui/history.rs`): every daily autosave is also written to
  `saves/history/<game id>/<unix ms>-day<N>.ron` (serialised once on the worker thread, written
  there and to `saves/autosave.ron`), with its sidecar and picture. The Game history window
  (Saves, Game history) lists the running game's entries newest first with their pictures; the
  player can name a moment (a bookmark, kept in the entry's sidecar), "Mark this moment" (save
  now, named), and Revert to an entry after a confirmation, which loads it like Load. Entries
  after the one reverted to stay, so a revert can be undone. The newest 24 unmarked entries of a
  game are kept; marked ones are never pruned.

## City Data: one full-screen view for the reports

Every report on the city lives in one place, the City Data view (`ui/city_data.rs`): the top
bar's City Data button or J opens it, Esc or its close button shuts it, and clicking the treasury
(Economy › Cash flow), the population (People › Population) or the power warning (Services ›
Power) in the top bar opens that page. It fills the map area between the top bar and the
toolbar, opaque, and does not move or resize; the game keeps running behind it, and the map's
labels, pins, bubbles, legend and hover cards stand down while it is up. A row of tabs, then the
tab's sub-tabs:

- **City**: Overview, Land and rent, Districts, Tourism.
- **Economy**: Overview (treasury, today so far, what the businesses want), Cash flow (flows by
  day, yesterday's books as signed bars), Ledger (the itemised drill-down), Taxes (the take,
  then the rates and tolls).
- **Market**: Goods and prices, Businesses (how each kind pays, demand for new buildings),
  Trade.
- **People**: Population, Income and inequality, Complaints and concerns, Social, Favourites.
- **Services**: Power, Transit, Emergencies.
- **Directory**: Businesses, People ([directory], `ui/directory.rs`, rows from `directory.rs`).

The Directory lists every business and every person in one table each, with a column per thing
the player asks of them. Businesses: name, kind (and industry), owner (chain, developer,
billionaire, founder, mega-corp, university; "independent" when nobody holds it), district, the street it fronts,
staff of jobs, takings today and yesterday, yesterday's net, cash, rating (stars) and today's
footfall, and its state (being built, out of business, broke, no staff, open, closed for
the night). People: name, age, class, income, workplace (or unemployed, child, retired), home and
its street, household size, mood, health (ill, or how well fed and days hungry), the way they
have travelled most lately, favourite place, friends, and what they are doing now (working, at
home, asleep, out, on the move, ill, homeless, arriving, away). A click on a heading sorts by it
(numbers biggest first), again reverses it, and a painted arrow marks it; values missing (no
rating yet, no job) sit at the bottom either way and ties go by name. A search box (every word
must appear in the row's name, kind, owner, street, district, workplace or home) and pickers
(kind, district and state; class, district and status) narrow it, with a count of the rows shown.
Cities run to ten thousand people, so the table body is a `ScrollArea::show_rows` that lays out
only the rows in view (each painted by hand: no widget per cell), under a header that stays put.
The rows are a snapshot read from the world once every five seconds while the page shows (and
at once on another city), and the sorted, filtered order is cached beside it, made again only
when the sort, the filter or the snapshot changes; both live in egui memory with the page's
settings. A click on a row selects it (the inspector opens over the page); "go" or a double
click flies there and shows the map. The API's `directory` returns the same rows in the same
order.

Tools stay floating windows above it, because they are used with the map: the inspector and
person pane, the Build window, the Lines editor, View (map layers, lighting, graphics, text size),
Saves, Help, Dev, New game. A name clicked on a page selects it (the inspector opens over the
page); a "go" or a follow closes the view so the player sees where the camera went. The page
showing and each tab's last sub-tab live in egui memory for the session; the API's `city_data`
and the old `window` names reach the same pages.

Every page has the same furniture (`ui/pane.rs`), top to bottom: a title and one line on what
the page is for, a row of headline figures as cards, then sections under one kind of heading
(the display face over a hairline), charts full width with the legend under them (hover a day
for every series' value), tables with the same striped rows and caption headings, and labelled
figures in a fixed label column. One range picker (72 h, 7, 30, 90 days, all) sits at the right
of the sub-tab row on pages that chart history, shared by all of them.

What the pages chart:

- **Economy**: treasury over time (hourly for 72 h); income and spending by flow as diverging
  stacked bars per day with the net as a line; the ledger line by line, each flow opening onto
  what made it, for yesterday or today so far (service upkeep by station, clinic, school and
  car park, help from the next town, less the hospital bills patients paid; transit upkeep by
  depot, port and the bus fleet; plant upkeep by plant; fares by line and station; construction
  by what was built), with a note of what each flow includes and buttons that fly to the
  building (Ledger); the tax take by class and land use, then the rates (Taxes).
- **People**: population, households, employed, unemployed, unhoused and waiting over time; the
  class mix stacked; Gini over time.
- **Power**: the last 72 hours' supply stacked by source above the axis (each kind of plant,
  batteries delivering, imports) and where a surplus went below it (into the batteries, exported),
  under a demand line; the batteries' stored MWh against capacity with charging and delivering MW;
  energy per day the same way; money for power per day (export income, import cost, upkeep);
  brownout hours and peak demand; a table of plants and batteries (output now and 24-hour mean,
  upkeep, cost per MWh, CO2 a day, smog, noise; batteries' charge and flow) and a 72-hour chart per
  plant (output against nameplate) and per battery (stored MWh and flow); [batteries] the
  batteries' rule this hour and the day's cheap and dear prices, and their savings per day. A
  battery's inspector shows its rule, its savings today and yesterday, and its state of charge
  against the price over the last hours.
  The page ends with the hour's balance, the batteries' fill, costs, the next 24 hours and the weather.
- **Transit**: riders per day by mode; riders per bus line; fares against upkeep; who is waiting
  and riding hour by hour; a table of lines with today, yesterday, the average and fares.
- **City**: homes and jobs (built and filled), buildings and sites, vehicles on the road by hour.
  **Market**: trade, businesses and deals, demand by land use, every good's price against its
  base, how each kind of business pays. **City › Land and rent**: land value and rents by tier over
  time, lots by land use. **Emergencies**: calls per day by kind, response time, unanswered and out-of-town calls.

The numbers come from the sim, not the app. `nd_sim::history` keeps a daily ring (365 days) and
an hourly ring (72 hours) inside `Economy`, so they are in the save and charts survive a load; a
day's record is read in two halves across midnight (`world/history.rs`: the day's counters before
midnight resets them, the closed ledger and levels after). The ledger is itemised: postings say
what they were for (`Economy::credit_item`: a building, a bus line, an income class, a land use
or a named item), one entry per item a day, so the lists are bounded by what stands in town.
Postings without an item stay in their row and show as "not itemised". All new fields are
`serde(default)`; the history's ledger rows are `Vec`s so a new flow does not break old records.
Recording reads the world and pushes; it draws no random numbers, so determinism is untouched.

## The inspector and the Lines window

The inspector reads top down in order of what the player asks first. A building opens with its
name, then coloured tags for what it is: the zone (in the zone's colour), the business kind and its
industry, or the service, plant, depot or "Homes"; the tier and the district. The style, floors and
build day follow in small print, then alarms (on fire, derelict, under construction with progress,
underpowered). Sections appear only when they apply: a business shows cash, today's takings and
spending, staff and a cash-flow sparkline (takings up in green, spending down in red, net as a gold
line, the last 30 closed days from `Business::books` plus today) and its stock folded away, and no
homes section; homes show only where people live and there is no business. Services, plants, rail,
metro, depots, parks and parking follow, then the people: who lives, works and is inside now, as a
grid of painted head-and-shoulders portraits (`ui/portrait.rs`, from the citizen's look: skin, hair
and greying, outfit in their two colours) with first names; a click selects the person. Land value,
rent, wage, condition, power and footfall sit in a folded "site" section at the end.

The style named is the style drawn. `nd_procgen::style_extras` collects every input that decides a
building's style (plants, car parks and taxi yards, depots, rail, docks, services, metro, the
business, a rooftop pad, the district look); the renderer builds its `Extras` on top of it, the
`StyleBook` (kept in `Sim::styles`) resolves styles from it against older neighbours, and
`nd_procgen::building_style` reads the book (or picks with the same inputs when the renderer has
not drawn the building yet), so the inspector and the remote API name exactly what is on screen.
Every farm business (grain, dairy, mill, vertical farm) is drawn as a farmstead.

A car shows its driver under the car without a second click: a portrait (click to select them),
name, age, class, mood, why and where they are driving, the drive so far against what it should
take, their anger, and a slow - normal - reckless slider from `DriverProfile::boldness` (speed,
overtaking, ambers and following distance against a by-the-book driver, -1..1; a ring marks their
calm self when anger has moved them). Everyone else aboard is a portrait grid: a bus's riders, and
anyone the sim says rides in the vehicle (`ui::occupants`, the one place a car's passenger list
plugs in).

The Lines window (top bar, or "Lines" in City Data's Services › Transit and on a bus) lists every bus line
with riders today and yesterday; picking one shows it on the map and opens its strip diagram: each
stop in running order, named after the nearest building, with boardings and alightings today
(yesterday on hover), and between stops the leg's length and minutes, timed by the buses where
they have run it (`StopStats::leg_minutes`, arrival to arrival, smoothed) and estimated from the
length otherwise. Buses sit on the strip where they are along the loop (`BusRoute::bus_progress`),
grey on their last lap, and select on a click. Per stop, "show" flies the camera there and marks
the stop with a tall post (`ToolState::stop_sel`), "delete" removes it. The line can be renamed,
recoloured (its buses too), given more or fewer buses, or deleted (with a confirming second
click). Stop counters live on the stops themselves (`BusStop::stats`, serde default), so they
follow a stop when one before it is removed; a moved or removed stop forgets the timing of the
legs it changed. Nothing here draws random numbers.

## Following, zooming and the people you care about

- **Follow** (inspector, person pane, favourites; API `follow`): `camera::CameraFollow` locks the
  orbit target on a person, vehicle, delivery robot or building. It is applied after the sim has
  stepped (`follow_and_fly`, after `SimSet::Sync`), so the camera sits on the thing where it is
  drawn this frame, with no lag. Pan keys, pan drags and the F-key jumps are locked; rotate and
  zoom stay free. A banner ("CAMERA LOCKED ... Stop following (Esc)") and a gold viewfinder frame
  round the map say so. Esc ends it and is used up, so a second Esc clears the selection. A
  followed car that parks hands the follow, and the selection, to its driver on foot.
- **Point of view**: Walk view while following looks through their eyes (a walker's head, a
  driver's seat, a bus window, a robot's camera), turning with them (smoothed) and right-drag
  looks around, springing back. The pointer stays free for the banner. The selection's outline
  box is not drawn from inside the vehicle.
- **Zoom to** (`camera::CameraFly`): a smooth flight of target, distance (in log space) and pitch,
  0.7 to 2.2 s by length, hopping out over long trips; a moving goal is tracked while the camera
  travels. Every "go" and "fly there" button in the HUD now flies the same way.
- **A person's places**: `World::holdings_of` is their home, work and every business they own
  (`Business::owner`; taxi founders own their firm). Each is a big clickable pin (HOME, WORK,
  OWNS, `ui/pins.rs`) over its building on a post (`marks.rs`), held at the screen edge when off
  screen, and a button in the person pane. Either selects the building. Second homes and
  businesses the well-off buy (`world/rich.rs`) are listed and pinned the same way; the person
  pane sums them up ("owns 3 properties · 4 cars: one driven, 3 in the garage at ...").
- **Favourites** wear gold in the world: a ring and a diamond over the head, and a gold box round
  the car they drive or ride in and their parked car (`marks.rs`). City Data's People › Favourites has
  go (a smooth flight), follow and a "$" gift.
- **Gift**: `World::gift` moves money (the button gives $1000) from the treasury to the
  household, or the savings of someone without one. It is booked as the `Gifts` flow, itemised
  by name. They remember it and cheer up. Refused when the treasury cannot pay.
- **Delivery robots** are selectable (`Selection::Bot`). The inspector shows their name ("Lucky
  Bodega bot #2": `DeliveryBot::serial`, the lowest free number in the shop's fleet), code, the
  owning shop, what they carry (`amount`, `household`), where they are going, the metres and
  minutes left, and the route on the map in the fleet colour. Each company's fleet is painted
  its own colour (`bots::fleet_colour`, a golden-ratio hue per shop).

## Roadmap

- v0 (done): roads, zones, lots, procedural buildings, immigration, schedules,
  rule-based minds, inspect panel, save/load, hot-reloaded palette and rules, day/night.
- v0.5 (done): projects with undo/redo and control-point editing, lanes, generated
  junctions with signals, car-following traffic, vehicles.
- v0.6 (done): land value, three building tiers, rents, wages, class, displacement,
  the worn 2040 look.
- v0.7 (done): resources, businesses with recipes, the market with trucks, imports
  and exports, hunger and shopping, business closures.
- v0.8 (done): polygonal parcels, 35 architectural styles per land use and tier from a
  parts toolkit, textured surfaces with normal maps, street furniture, walk mode.
- v0.9 (done): road tool in the spirit of Cities Skylines: ends snap to any point on a
  road and split it into a junction, curved roads (centripetal Catmull-Rom through the
  control points, built as 7 m pieces whose bends keep road speed), lane presets and
  one-way roads, ramps as yield merges (signals only where three two-way roads meet),
  and an upgrade panel in the Edit tool for lanes, direction and curvature.
- v1: household economy with resources and a market, districts and names, demand that
  reacts to prices, ambient through-traffic, lane changing, smooth arc roads.
- v2: LLM spotlight citizens with real backends, dialogue, gossip.
- v3: faction and corporate layer, events, story generation.

See `IDEAS.md` for everything that has not been scheduled.
- v0.10 (done): parking as a business with capacity, fees and shortage-driven
  construction; occupancy shown in the geometry; scale audit (metres everywhere).
- v0.11 (done): pedestrian network through blocks with footfall and walk-in trade.
- v0.12 (done): electricity: four plant kinds placed by the player, weather-dependent
  renewables, rationing by tier, brownouts that dim buildings and sour moods, a treasury.
- v0.13 (done): parks planned over connected parcels: paths lot to lot, benches people
  sit on, features by land value (courts to ferris wheels), adaptive trees, parks on paths.
- v0.14 (done): buses: lines drawn as loops of kerb-side stops, depots that dispatch and
  house them, riders who walk to a stop, wait, ride and pay.
- v0.15 (done): railway: trunk line and branches, level crossings, stations and freight
  yards beside the track, passenger and freight trains on headways, rail riders, rail freight.
- v0.16 (done): construction over time, eminent domain, redevelopment and density heights,
  vacant-land dressing and open-ground nature, graffiti and street grit, inspect UX.
- v0.17 (done): whole-project code review (docs/REVIEW.md) and the first batch of fixes
  from it: long frontages parcel, roads join junctions they pass through, no stale rail
  segment panic, no lost truck cargo, no NaN geometry, egui focus and right-drag input fixes.
- Roundabouts: a Road tool preset that stamps a give-way ring over roads and junctions,
  with a live preview of what it cuts, joins and takes.
