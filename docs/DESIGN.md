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
- **Planning**: roads and zones are *gestures*. The master plan is what is built; a
  project is a stack of deltas on top of it with undo and redo. Control points of any
  gesture can be dragged, inserted or deleted; bulldozing marks a gesture for removal.
  Implementing applies the deltas and rebuilds roads, zones, lots and lanes. Buildings
  whose lot survives the rebuild are kept, the rest are demolished.
- **Roads**: a node/segment graph with per-direction lane counts. New polylines snap to
  existing nodes and split segments they cross. Walking uses A* over nodes.
- **Lanes and traffic**: every segment becomes offset lanes per direction, shortened at
  junctions. Each junction gets connector lanes from every incoming lane to every
  outgoing lane (U-turns only at dead ends); junctions with three or more approaches
  run a fixed-cycle signal that gives one approach green at a time. Cars path over the
  lane graph with A*, follow the intelligent driver model, queue behind each other and
  stop at red lights. Traffic physics runs 1.5 seconds per sim minute so a car at 50 km/h
  moves at a believable pace on screen at 1x; trips therefore take sim-time tens of
  minutes, the usual city-builder compromise. Right-hand traffic; lane sets are centred
  on the road; junction curves stay inside the junction; cars refuse to enter a
  junction whose exit lane is full; stalled trips time out. Pedestrians walk on the
  right-hand pavement.
- **Zones**: polygons with a land use (Residential, Commercial, Industrial,
  Entertainment, Civic, Park). Lots are carved along every road segment that runs
  through a zone.
- **Buildings**: a vacant lot develops when demand for its land use is high. Floors
  and capacity (homes or jobs) depend on land use and a seed. Geometry is generated
  from the seed, so a save stays small.
- **Citizens**: immigrate when homes are free, take jobs when available, and move
  along roads between home, work, shops and nightlife. Each has a personality seed,
  a memory stream and a current thought.
- **Land value**: a 32 m grid summing road access, parks, civic and commercial
  amenities, industrial and nightlife nuisances, traffic noise, vacant lots and the
  tier of neighbouring homes, blurred once. Recomputed every two sim hours.
- **Tiers and rent**: buildings are low, middle or high tier. New construction takes
  the tier the land value supports (pulled down when people are waiting for something
  affordable). Full buildings on rising land upgrade; buildings on falling land decay.
  Cheap buildings wear out faster. Rent is tier base × land value × condition.
- **Class and migration**: a job pays a wage set by its building's land use and tier,
  times a personal skill factor. Income puts a citizen in a class. Newcomers take the
  best home they can afford (rent at most half their income) and are turned away if
  there is none. Three days of rent stress sends people looking for somewhere cheaper,
  or out of town once their savings are gone; the comfortable move up a tier.
- **Resources and businesses**: ten resources (produce, grain, flour, baked goods,
  meat, dairy, groceries, parts, electronics, goods). Every non-residential building
  hosts a business with a recipe: farms, protein vats, dairies, mills, fabs and
  assembly plants on industrial lots; grocers, bakeries, gadget shops and offices on
  commercial; venues on entertainment. Output per hour scales with filled jobs.
  Businesses hold stock and cash; prices rise when stock is scarce and fall when it
  piles up.
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
- **Parking**: every car trip ends in a space. Towers (high tier) bring their own
  spaces for about half their homes and jobs, middle-tier blocks a quarter, cheap
  buildings none. Drivers use the destination's spaces, else pay at the nearest
  parking business within 180 m with room (a fee by tier and land value that goes to
  the business), else leave the car on the street, lose a little mood, and are counted
  as a shortage. When the day's shortage outruns free spaces, development opens a
  parking lot on a vacant lot: a surface lot where land is cheap (about one space
  per 28 m², two jobs), a multi-storey car park on middle land (three to five decks).
  It is the cheapest business in town and lives or dies on fees. Parking lots render
  their occupancy: cars fill the painted bays of a surface lot and the edges and roof
  of a car park in proportion to the spaces in use.
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
  only, a brown haze and a heavy land value penalty for 300 m), solar (25 MW nameplate,
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
- **Economy stats**: demand per land use, Gini of incomes, class counts, rents per
  tier, displaced, emigrated and priced-out counters, plus market stats: businesses,
  deals, imports, exports, meals sold, hungry citizens, closures, and the average
  price and stock of every resource.

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
- Parcels: the road corridors are subtracted from a zone with polygon booleans
  (`geo`), so a curved road removes exactly its own strip. Road corridors are one
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
- Mobility: one sim minute shows `traffic_seconds_per_tick` (1.5 s) of motion for everyone, so
  walking is 1.4 m/s, cycling 4.5 m/s and driving the lane speed limit on the same clock. Citizens
  own a car and/or a bike by class (poor: 12% car, 60% bike; middle: 60% / 35%; rich: 92% / 35%)
  and buy or sell them as savings allow. Walkers use the middle of the pavement, bikes the road
  shoulder, both the centre of a path; bike owners ride to parks for pleasure.
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
- Power plants have their own styles: a coal station with a boiler hall, two stacks with
  plumes, a coal pile and conveyor and a transformer yard; rows of tilted panels on posts;
  white turbines with nacelles and three blades; a containment dome, hyperbolic cooling
  towers with steam and a double fence. Coal plants get a haze: stacked translucent brown
  discs on the overlay material that read as a dome from above and murk from the street.
  Underpowered buildings light fewer windows (`Ctx::lit_chance` scales with `powered`).
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
