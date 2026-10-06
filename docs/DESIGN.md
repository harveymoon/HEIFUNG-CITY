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
  are painted before the stop line. Junctions with three or more approaches run an
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
  change it cannot make, takes another way, which breaks gridlock rings. Traffic physics runs 1.5 seconds per sim minute so a car at 50 km/h
  moves at a believable pace on screen at 1x; trips therefore take sim-time tens of
  minutes, the usual city-builder compromise. Right-hand traffic; lane sets are centred
  on the road; junction curves stay inside the junction; cars refuse to enter a
  junction whose exit lane is full; a vehicle 30 traffic minutes on one route replans from where it stands, and nothing is ever taken off the road for being slow: jams are part of the city and drivers feel them. Pedestrians walk on the
  right-hand pavement.
- **Roundabouts** (`Gesture::Roundabout { center, radius, lanes }`, Road tool preset): a
  one-way ring of 1 or 2 lanes, 24 to 240 m across, traffic keeping the middle on its left
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
  is signalised, arms may only turn right onto the ring and ring lanes continue or leave
  from the outer lane. Entries give way with gap acceptance: a car about to reach the
  line goes unless a ring vehicle that will pass the entry (not leave by it) is in the
  junction or arrives less than 3 s after it, looking back round the ring as far as a ring
  car covers in 9 s (at least 45 m);
  ring traffic never yields and only follows a car merged ahead. Passing an exit in the
  outer lane of a two-lane ring costs routing a little, so the first and second exits
  keep outside and later ones go round on the inside. Hovering the tool shows the ring,
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
  household driving in (off a highway carriageway at the map edge, or the road at the city's edge),
  or when bought (at a dealer, the nearest goods or gadget shop, and delivered home by its driver; or
  out of town, driven in from the edge); it leaves only when sold (driven to the dealer), scrapped, or
  driven out of town by an emigrating owner. A trip by car starts at the car: the driver walks to it
  (within 320 m) and it pulls out of its kerb slot into a gap. At the far end it uses the
  destination's own spaces; else the driver weighs the nearest free kerb slots and parking lots by
  the drive there (by the route it would really take), the walk back and the fee; with nothing in
  sight it circles the streets around (counted as circling, the signal development opens lots on),
  and after four tries parks anywhere on the street. Kerbs: one slot per 6.5 m on both sides of
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
  then walks off the bench when the next decision comes.
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
  centre from the start; the player lays branches off it with the Road tool's Rail
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
  and the inspector marks it UNDERPOWERED. The treasury starts at 60,000, takes 12% of
  incomes and 5% of business revenue each day, and pays plant upkeep and imported power.
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
- **Air** (`nd_sim::air`, `world/air.rs`): the rich fly. Vertiports are placed under Transit (drone pad /
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
  ground: terrain blended into each road's profile within 22 m and flattened to a pad under every lot
  (the street's height at the frontage). The renderer draws the terrain from it, lifts flat-datum meshes
  onto it, raises buildings to their pad and movers to `surface_height` (decks over water). Water cuts
  parcels (`zoning::generate_lots_wet`); lots steeper than 0.2 are unusable. A one-way street of three
  or more lanes is a highway: 25 m/s, no frontage, grade-separated (crossings make no junction, only
  dropped ends join, so ramps merge with priority to the mainline). Embankments and cuttings
  slope no steeper than 1 in 2.5 (the blend to the terrain widens with the height, up to 160 m).
  Where any other way crosses the highway 4.5 m or more above or below it, the lower way runs
  through an underpass (`landscape::Underpass`): the ground is cut to its level for the width
  of its corridor plus 8 m along the whole embankment, the upper way crosses on a short deck
  (a registered deck, so its traffic stays up), and abutment and wing walls with coping
  follow the embankment down either side. Two carriageways cross the map at
  y = 420 from the start; through cars enter at the map edge and leave at the far one. Docks
  (`nd_sim::ports`) stand on the shore with a pier and a street stub, take over grown lots, and work
  once their street reaches the city: freighters route by water (river centrelines, tributaries
  joining, mouths opening on the sea) from off the map to the berth and back, landing imports for
  lorries; ferries run terminal to terminal or out to elsewhere with fares. Hydro (river current at
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
  road segment from its own seed, so props stay put as the walker moves.
- Performance: the sim gets a real-time budget per frame (12 ms) and drops its
  backlog instead of stalling the renderer; shadows use three cascades out to 650 m;
  the HUD shows fps, meshes and triangles.
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
  and buy or sell them as savings allow. Walkers use the middle of the pavement, bikes the road
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
  story with a favourites list to jump back to. Zoning outlines toggle with O.
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
  coal pile and conveyor and a transformer yard; rows of tilted panels on posts; a wind farm
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
  scaled by the power share. No mesh is rebuilt.
- Map layers (`render/layers.rs`, ramps and heat grid in `nd_procgen::overlay`): land value
  (V), traffic load (a ribbon per lane from cars per metre and speed against the limit, green
  to magenta), traffic types (a disc under every vehicle: cars cyan, trucks orange, buses
  green; emergency red once the sim has them), people (a heat map of everyone where they
  are, indoors at their building), pollution and noise (plants by pollution radius,
  industry, nightlife, traffic), the pedestrian network (P) and zoning outlines (O). L steps
  through the heat layers one at a time; the Land window's Layers section ticks any set; a
  legend at the bottom right shows the key of every layer that is on. The `overlay` API and
  `map {layer: "people"}` read the same data.
- Toolbar and sub-pane icons (`ui/icons.rs`) are painted with egui shapes: one per tool, a
  lane plan per road preset (lane arrows, centre line, rails, path paving, ramp curve), a
  glyph per land use in its zone colour.
- Paths (`RoadKind::Path`): 3.2 m ways with no car lanes, drawn as paving with edge stones,
  low lamps, benches and bollards where they meet a street; they cut parcels but give no
  frontage, and a path crossing a street mid-block gets a zebra.

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
