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
  once the scene clears a tow truck hauls the wreck to the nearest industrial yard. Street
  takeovers: on ~30 % of nights (with a crew) at 22:00 a meet is planned; rebellious young locals
  drive over, outsiders roll in off the highway as a pack ignoring lights, the junction is closed,
  cars ring it and the flashiest spin donuts in tyre smoke, a crowd walks over, a neighbour calls
  the police; police on scene break it up (some arrests, cars impounded), else it ends after
  45-80 minutes and everyone leaves in convoy.
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
  Every zoned parcel becomes a lot; uses with lots bigger than a home's join small
  neighbouring parcels (works yards and civic sites two or three, a shop a small
  neighbour) and industry and civic grounds fold in the backland behind them
  (`zoning::join_parcels`). Industrial strokes can carry an industry sub-zone that is
  set on implement.
- **Buildings**: a vacant lot develops when demand for its land use is high. Floors
  and capacity (homes or jobs) depend on land use and a seed. Geometry is generated
  from the seed, so a save stays small.
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
  tier of neighbouring homes, blurred once. Recomputed every two sim hours. Power plants
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
  and water proximity. Contamination is a saved 32 m raster (0..1): oil wells pour 0.01 per
  working hour within 60 m, refineries 0.004 within 80 m, coal plants 0.003 within 120 m;
  each day an eighth of every cell seeps into its neighbours and 3.5% fades, so a closed field
  takes months to recover. Grain farms yield 0.6 + 0.8 x fertility and lose up to 90% on fouled
  ground; container yards and truck lots burn fuel and earn handling fees (2 per filled job
  hour, more near a port or yard), truck lots a bay fee per parked lorry.
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
  parking is banned, the tow truck is called. A company in town sends a truck only when one is free; the company from out of town has four on the city's roads at most, the other jobs wait their turn (uncapped, a city short of parking had 170 tow trucks in its jam). Tow companies are businesses on industrial land
  (logistics); the nearest with a truck free sends one, which hooks the car where it stands and
  carries it to its pound (drawn on the truck as wrecks are). With none in town a company from out
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
- **Parking businesses**: when the day's circling outruns free spaces, development opens a
  parking lot on a vacant lot near the last driver who could not find a space: a surface lot where
  land is cheap (about one space per 28 m², two jobs), a multi-storey car park on middle land
  (three to five decks). It is the cheapest business in town and lives or dies on fees (a fee by
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
- **Construction** (`buildings::Construction`): nothing pops into existence. `build_on` opens
  a site with a build time of (1 + 0.4 × floors) days scaled by tier, clamped to 1–12 days;
  it offers no homes and a temporary crew of jobs that the unemployed commute to. Progress
  advances hourly and needs four loads of parts, one per quarter, each ordered a quarter
  ahead from a local seller or by import; a late truck stalls the site at the next quarter.
  Every 10% the renderer redraws the site; on completion the crew is released, the real
  homes and jobs open, and tenants may move in. Player-placed specials and parking lots
  complete at once. Housing and job pressure are damped by what is already under
  construction. Sites are drawn as hoarding, raft, shell to the last full floor, scaffold,
  a tower crane from six floors, pallets, cabin, mixer and hi-vis workers.
- **Demolition** (`buildings::Demolition`): nothing vanishes either. Whatever pulls a
  building down (a bulldozed zone, road works, a shop shut for a week, a take-over, a
  redevelopment) turns it into a demolition site: residents, households and staff leave
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
  never had; showing the clearing first gives the same picture without that.) Sites are
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
  New buildings are capped at two floors plus one per ten neighbouring floors within 150 m,
  so high-rises appear only where the block is already dense.
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
  side is missing. Track drawn by the player is eased when built: ends joining existing
  track leave or arrive along it through a 60 m turnout, corners are rounded to a 120 m
  radius, long legs stay straight, so there are no right angles or square forks.
  Trains run west to east from one end of the line to the other, calling at every
  facility in order; a dead-end spur is run to the last platform and back. Branches
  have no passing loops, so one train holds the branch at a time: others wait at the
  switch, and no new train is dispatched while one is waiting. Passenger trains come
  every two hours of clock from 05:00 to 23:00 when a station exists; freight trains
  every three hours when a yard has goods waiting, or six regardless. Trains are not
  saved; they are back on the next headway. People take the train for trips over 500 m
  between two stations within 240 m of each end (`TransitLeg::Rail`), pay a fare, and
  newcomers mostly arrive by train when a station exists. With a freight yard, imports
  come by rail at 80% of the price and wait at the yard for the next train, which hands
  them to lorries; exports earn 20% more. Stations lift land value, yards lower it.
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
  under the lot's middle. The Buses tool's Metro mode (or the `subway` API) lays a line through
  stations in order: one polyline of twin bores that ignores the street grid, laid with the
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
  (`nd_procgen::metro`, `render/subway.rs`); the Transit window and the inspector show lines,
  timetables and boardings per station.
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
  least, high tiers more); plants draw none. Four kinds of plant go on vacant lots with
  the Power tool, paid from the treasury: coal (120 MW, cheap, steady, industrial land
  only, a brown haze, ash on the ground and a heavy land value penalty for 360 m), solar (25 MW nameplate,
  any land, output follows the sun and is dimmed by cloud), wind (40 MW nameplate, any
  land, output follows the wind) and nuclear (400 MW, steady, very expensive to build and
  run, industrial land only, a smaller penalty for the nervous neighbours). Weather is a
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
  lot is clear and run in slot order, so the dispatch is deterministic. Placing a plant or battery
  refreshes who is powered but books no hour (no money, no battery energy, no dark hour); the hour is
  booked once, on the hour. A plant does not feed the grid while the old building on its lot is
  still being cleared; solar, wind and battery yards clear it at once. Each plant kind has an air
  and noise model beside its land-value cone: CO2 per MWh (coal 0.95 t, geothermal 0.08, the rest
  0), smog at the fence (coal 1, geothermal 0.15) and noise while running (coal 68 dB(A) down to
  solar 38; batteries 40-48). `World::plant_rows` puts each plant's and battery's output, 24-hour
  mean, upkeep, cost per MWh (upkeep over the last 24 hours' energy), CO2, smog and noise in one row
  for the Power window and the API; `nd_sim::history` keeps a per-plant record (`PlantLog`: 72
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
  scene, works (engines knock fire down 0.012 a minute; crimes 25 min, patients 12), resolves (fire
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
  resolved ones on file, shown live in the Emergencies window and by the `incidents` API command.
  Services draw from their own random stream so the rest of the simulation is not perturbed.
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
  counted in `ServiceStats::chases_*`, shown in the Emergencies window and by `incidents`/`report`.
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
  ran 11 cabs, about 120 rides a day, 2 to 3% of trips, most of its trade in the evening and night. Transit window and `report.transit.taxis`.
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
  holds on to the buyers, and readdresses loads still at yards, ports, on trains or ships to them:
  nothing is lost. Transit window and `report.transit.cargo_hubs`.
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
  and frighten the neighbours (mood; rich households near heat 4+ leave). *Homelessness*: a
  household evicted with nowhere cheaper and no money stays on the street 70% of the time if any
  land is cheap (value < 0.36): camps of up to six households on cheap pavements, vacant lots and
  unzoned parcels, or (with a car) an RV or van at the kerb; they eat charity meals, walk back to
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
  Districts window lists every district's figures (area, buildings by tier, homes and residents,
  jobs, land value, rent, hotels and beds, visitors staying, pull) with the policy editor.
- **Tourism** (`nd_sim::tourism`, `world/tourism.rs`): attractions are re-read every two hours:
  monuments and stadiums by tier, mid and high tier parks by area, open venues, landmark districts
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
  its own seeded streams. The Districts window and the `tourism` API show attractiveness, demand,
  arrivals by mode, beds, the turned away, spending by hotels / shops / venues and the tax.
- **Taxes** (`nd_sim::tax`, `world/taxes.rs`, the Taxes window): a rate per line, collected at
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
- **Toll booths** (`nd_sim::tolls`, `world/tolls.rs`, Buildings > Transit): a booth (4,000, 60 a
  day) or a plaza (15,000, 200 a day) stands across a straight stretch of street or highway, both
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
  parcels (`zoning::generate_lots_wet`); lots steeper than 0.2 are unusable. A one-way street of three
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
- **Depth view** (Underground / Ground / Sky, U cycles, top bar): Underground hides the terrain,
  water, buildings, lots, surface roads and walkers, draws surface roads as faint outlines and
  shows the tunnels (drawn as their own street layer, `StreetLayer::Underground`, with bore walls
  and a lit strip; each layer draws only its own roads' crossings, beacons and signal posts) and
  the cars in them. The Tunnel preset switches to it, other presets back to
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
  once their street reaches the city: freighters route by water (river centrelines, tributaries
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
  - Civic: precinct houses, ministries, libraries, courthouses, monuments.
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
  two; from 0.66 a parklet with mown lawn, a hedge with a gap, a path to a bench, bin, lamp and
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
- Window lights follow occupancy (`render/lights.rs`, `nd_procgen::lights`). In the app every
  window light is generated (`Extras::dynamic_lights`, about 92% of windows) through
  `parts::window_light`, which ranks it by a hash of its position and lays dark glass just
  behind it. When chunks are built, the rank and the building's slot go into the glow
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
  (V), traffic load (a ribbon per lane from cars per metre and speed against the limit, green
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
- Focus layers (`render/focus.rs`, the heat-vision material swap reused): with Traffic load on
  the city is dimmed and every vehicle on the road turns one light blue; vehicles in bored
  tunnels and the tunnels themselves are outlined through the ground (`layers::layer_gizmos`,
  a gizmo group with depth bias -1). Crime (`overlay {layer: "crime"}`): a heat map of the
  sim's hot spots plus open crime incidents (`crime_color`), the city dimmed a little less,
  police cars lit blue-white (light bars keep their colours), each police car's route ahead
  drawn over everything (flashing red and blue on a call, cyan on patrol) with a ring at its
  goal, and pulsing rings on crimes not yet answered. Heat vision wins over a focus view.
- Map view (`render/mapview.rs`, `overlay {layer: "map"}`): every mesh but water and the sky is
  hidden (after all visibility owners, in `PostUpdate`), and a flat unlit map is drawn: dark land
  (open over water), lots tinted by zone, roads as casing and fill ribbons by class (highway
  amber, avenue gold, street grey, path, rail), tunnels dashed, joints rounded. Street names
  (`nd_sim::streets`: the network cut into chains that carry straight on through junctions,
  named from the seed and a serial, kept across rebuilds by matching new chains to the saved
  outlines, saved with serde default) run along the roads, repeated along long streets and
  decluttered on screen, main roads first; pins with a glyph and the name mark services,
  plants, stations, ports, depots, civic buildings and big venues.
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
  (once Land) also has lighting, land value and the dim-projects and bus-line switches.
- **Saves and the title screen** (`ui/saves.rs`, `ui/title.rs`): Saves, Save as writes
  `saves/<name>.ron` (the name made file-safe), a sidecar `<name>.meta.json` (name as typed, day,
  population, treasury, buildings, landscape) and a top-down picture `<name>.png`; quick saves
  and autosaves get a sidecar and picture too. At startup a title screen lists the saved games
  newest first as cards with their picture, name, day and population: Continue loads the newest,
  New game opens the landscape picker, Play this map starts on the landscape already loaded. The
  city behind is paused while it is up; `ND_TITLE=0` skips it, and the API's `load`, `new_game`
  and `seed_demo` close it.

## Info windows: compact and expanded

Every info window (City, Economy, Market, People, Power, Transit, Emergencies, View) has two
views behind one control: the corner-marks button at the right of the window's own header,
beside close, the same widget in the same place on every window (`ui/pane.rs`). The *compact*
view is the summary that sits beside the map; the *expanded* view is a large window (820 by 680
to start, resizable) with charts over time and the detail behind each figure. Only one window is
expanded at a time: expanding another puts the first back, so the map is never buried under two
big panels. Saves, Help and Dev are tools, not reports, and stay compact.

Every expanded view follows one pattern, top to bottom: a range picker (72 h, 7, 30, 90 days,
all), a row of headline figures, charts over the range (hover a day for every series' value),
then a table with a row per thing (plant, bus line, good, business kind, ledger item), then the
compact view's figures so nothing goes missing.

- **Economy**: treasury over time (hourly for 72 h); income and spending by flow as diverging
  stacked bars per day with the net as a line; the ledger line by line, each flow opening onto
  what made it, for yesterday or today so far (service upkeep by station, clinic, school and
  car park, help from the next town, less the hospital bills patients paid; transit upkeep by
  depot, port and the bus fleet; plant upkeep by plant; fares by line and station; construction
  by what was built), with a note of what each flow includes and buttons that fly to the
  building; the tax take by class and land use; trade. A taxes section (rates per sub-type) slots
  in after the tax take as one more section function.
- **People**: population, households, employed, unemployed, unhoused and waiting over time; the
  class mix stacked; Gini over time. The compact view keeps a 30-day population sparkline.
- **Power**: the last 72 hours' supply stacked by source above the axis (each kind of plant,
  batteries delivering, imports) and where a surplus went below it (into the batteries, exported),
  under a demand line; the batteries' stored MWh against capacity with charging and delivering MW;
  energy per day the same way; money for power per day (export income, import cost, upkeep);
  brownout hours and peak demand; a table of plants and batteries (output now and 24-hour mean,
  upkeep, cost per MWh, CO2 a day, smog, noise; batteries' charge and flow) and a 72-hour chart per
  plant (output against nameplate) and per battery (stored MWh and flow). The compact view has the
  hour's balance, the batteries' fill and the same table, narrower.
- **Transit**: riders per day by mode; riders per bus line; fares against upkeep; who is waiting
  and riding hour by hour; a table of lines with today, yesterday, the average and fares.
- **City**: homes and jobs (built and filled), buildings and sites, vehicles on the road by hour.
  **Market**: trade, businesses and deals, demand by land use, every good's price against its
  base, how each kind of business pays. **View**: land value and rents by tier over time, lots by
  land use. **Emergencies**: calls per day by kind, response time, unanswered and out-of-town calls.

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

The Lines window (top bar, or "Lines" in the Transit window and on a bus) lists every bus line
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
