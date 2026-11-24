# The goods and business catalogue

Harvey (2026-11-20): "lets get specific and make sure this is in a single database place in our project."

Since round 13 (r13-catalogue) every fact about a **good** (`nd_sim::market::Resource`) and the
economic facts about every **kind of business** (`nd_sim::market::BusinessKind`) live in one file:
**`crates/nd_sim/src/catalogue.rs`**. It holds two tables with one row per enum variant, in the
enums' order, and the building sites' bill of materials:

- `GOODS: [GoodRow; Resource::COUNT]`: label, category, unit, base price, the container it ships
  in, the lorry body for a local delivery, hours of stock kept before export, road wear, retail or
  not, flown or not, and a note on flows outside the recipes.
- `BUSINESSES: [BusinessRow; BusinessKind::COUNT]`: label, recipe (inputs and outputs), opening
  hours, customers per job, max floors, stock hours, walk-in sale, home-delivery order, starting
  capital, flags (food chain, outing, lodging, waterfront, needs a site, student trade, truck lot,
  fouls the air), the leisure numbers (tab, booking share, family/date spot, loudness, trash, land
  value lift, shopfront sign) and the inspector's line about it.
- `INDUSTRIAL`, `COMMERCIAL`, `ENTERTAINMENT`, `CIVIC`: the kinds each land use can grow.
- `BUILD_MIXES`, `build_type`, `site_bill`: what each kind of building is made of, stage by stage (round 14, see below).
- `makers(r)` / `users(r)`: who makes and who uses a good, read off the recipes (never written twice).
- `site_materials()` [r14-builders]: every good a site bill can name (every `BuildingMaterial`): what
  the builders' merchant stocks for the trade and builders keep in their yards.

The old methods (`Resource::label`, `base_price`, `BusinessKind::recipe`, `hours`, `is_food`, ...)
still exist and keep their signatures; they are one-line reads of the catalogue, so no caller had to
change. Enum variants, their order and serde are unchanged: saves load as before.

## Every good and every kind of business

Generated from `catalogue.rs` and checked by the test
`catalogue::tests::catalogue_md_lists_every_good_and_kind` [r16-docs], so this list cannot drift:
after adding a good or a kind (or changing which goods a recipe names) run
`ND_BLESS_DOCS=1 cargo test -p nd_sim catalogue_md_lists_every_good_and_kind` to rewrite it.
"Made by" is `catalogue::makers` (recipe outputs and `local_from`); "Grows on" is the land-use
lists (`INDUSTRIAL`, `COMMERCIAL`, `ENTERTAINMENT` = leisure, `CIVIC`); "its own system" kinds are
opened by a pressure, placed by the player or started by their subsystem (ports, transit, food
trucks, builders).

<!-- doc-guard: catalogue lists start -->
| Good | Label | Category | Unit | Made by |
|---|---|---|---|---|
| `Produce` | produce | farm | crates | VertFarm |
| `Grain` | grain | farm | t | GrainFarm |
| `Flour` | flour | food | pallets | Mill |
| `BakedGoods` | baked goods | food | crates | Bakery, Cafe, Diner, NoodleBar, PizzaPlace, FastFood, FoodHall, StreetFood, BubbleTea, FoodTruck |
| `Meat` | meat | food | crates | ProteinVat, FishPlant, Slaughterhouse, FishStickFactory |
| `Dairy` | dairy | food | crates | Dairy |
| `Groceries` | groceries | consumer | pallets | Grocery, FishMarket |
| `Parts` | parts | manufactured | pallets | Fab |
| `Electronics` | electronics | manufactured | pallets | Assembly |
| `Goods` | goods | consumer | pallets | GadgetShop, Bookshop, BuildersMerchant |
| `Crude` | crude oil | petrochemical | kL | OilWell |
| `Fuel` | fuel | petrochemical | kL | Refinery |
| `Plastics` | plastics | petrochemical | t | Refinery, ChemicalWorks |
| `Fish` | fish | seafood | crates | Fishery, FishingPort |
| `Livestock` | livestock | farm | head | CowFarm, ChickenFarm, Feedlot |
| `Ore` | ore | minerals | t | StripMine |
| `Fillets` | fish fillets | seafood | crates | FishGutting |
| `Art` | art | culture | pieces | ArtStudio |
| `Metal` | metal | minerals | t | Smelter, ScrapYard |
| `Cement` | cement | building material | t | - |
| `Aggregate` | sand and gravel | building material | t | StripMine |
| `Bricks` | bricks and blocks | building material | t | - |
| `Glass` | glass | building material | t | - |
| `Steel` | structural steel | building material | t | Smelter, ScrapYard |
| `Rebar` | rebar | building material | t | Smelter, ScrapYard |
| `Timber` | timber | building material | t | - |
| `Roofing` | roofing | building material | pallets | - |
| `Insulation` | insulation | building material | pallets | Refinery, ChemicalWorks |
| `Pipe` | pipe and plumbing | building material | pallets | Refinery, ChemicalWorks |
| `Wiring` | wiring and electrical | building material | crates | Fab |
| `Fixtures` | fixtures and fittings | building material | crates | Fab |
| `Paint` | paint | building material | crates | Refinery, ChemicalWorks |

| Kind | API key | Grows on | Uses | Makes |
|---|---|---|---|---|
| `VertFarm` | `vertical_farm` | industrial | - | Produce |
| `GrainFarm` | `grain_farm` | industrial | - | Grain |
| `ProteinVat` | `protein_vat` | industrial | Grain | Meat |
| `Dairy` | `dairy` | industrial | Grain | Dairy |
| `Mill` | `mill` | industrial | Grain | Flour |
| `Fab` | `parts_fab` | industrial | - | Parts |
| `Assembly` | `assembly_plant` | industrial | Parts | Electronics |
| `Grocery` | `grocery` | commercial | Produce, Flour, Meat, Dairy | Groceries |
| `Bakery` | `bakery` | commercial | Flour | BakedGoods |
| `GadgetShop` | `gadget_shop` | commercial | Electronics | Goods |
| `Venue` | `venue` | leisure | Groceries | - |
| `Office` | `office` | commercial, civic | - | - |
| `Parking` | `parking` | - (its own system) | - | - |
| `OilWell` | `oil_well` | industrial | - | Crude |
| `Refinery` | `refinery` | industrial | Crude | Fuel, Plastics |
| `ContainerYard` | `container_yard` | industrial | Fuel | - |
| `FleetLot` | `truck_and_bus_lot` | industrial | Fuel | - |
| `Fishery` | `fishery` | industrial | - | Fish |
| `FishPlant` | `fish_processing_plant` | industrial | Fish | Meat |
| `TowYard` | `tow_company` | industrial | - | - |
| `Hotel` | `hotel` | - (its own system) | Groceries | - |
| `TaxiCompany` | `taxi_company` | - (its own system) | - | - |
| `CargoHub` | `cargo_hub` | - (its own system) | - | - |
| `Mechanic` | `car_repair_shop` | - (its own system) | - | - |
| `CarDealer` | `car_dealership` | - (its own system) | - | - |
| `TrashCompany` | `trash_company` | industrial | - | - |
| `BikeShop` | `bike_shop` | - (its own system) | - | - |
| `CowFarm` | `cattle_farm` | industrial | Grain | Livestock |
| `ChickenFarm` | `chicken_farm` | industrial | Grain | Livestock |
| `Slaughterhouse` | `slaughterhouse` | industrial | Livestock | Meat |
| `StripMine` | `strip_mine` | industrial | - | Ore |
| `Marina` | `marina` | - (its own system) | - | - |
| `FishingPort` | `fishing_port` | - (its own system) | - | Fish |
| `FishGutting` | `fish_gutting_shed` | industrial | Fish | Fillets |
| `FishStickFactory` | `fishstick_factory` | industrial | Fillets | Meat |
| `FishMarket` | `fish_market` | - (its own system) | Fish | Groceries |
| `SeafoodRestaurant` | `seafood_restaurant` | - (its own system) | Fillets | - |
| `GolfRange` | `golf_driving_range` | - (its own system) | - | - |
| `ArtStudio` | `artists_studios` | - (its own system) | - | Art |
| `Gallery` | `gallery` | - (its own system) | Art | - |
| `Smelter` | `smelter` | industrial | Ore, Fuel | Metal |
| `Feedlot` | `factory_farm` | industrial | Grain | Livestock |
| `ChemicalWorks` | `chemical_works` | industrial | Crude | Plastics |
| `BeachResort` | `beach_resort` | - (its own system) | Groceries | - |
| `PierRestaurant` | `pier_restaurant` | - (its own system) | Groceries | - |
| `BeachBar` | `beach_bar` | - (its own system) | Groceries | - |
| `BoatRental` | `boat_rental` | - (its own system) | Fuel | - |
| `Cafe` | `cafe` | - (its own system) | Flour, Dairy | BakedGoods |
| `Bookshop` | `bookshop` | - (its own system) | Plastics | Goods |
| `ScrapYard` | `scrap_yard` | - (its own system) | - | Metal |
| `Diner` | `diner` | commercial | Meat, Dairy, Flour, Produce | BakedGoods |
| `NoodleBar` | `noodle_bar` | commercial, leisure | Flour, Meat, Produce | BakedGoods |
| `PizzaPlace` | `pizza_place` | commercial | Flour, Dairy, Meat, Produce | BakedGoods |
| `FastFood` | `fastfood_drivethrough` | commercial | Meat, Flour, Produce | BakedGoods |
| `FoodHall` | `food_hall` | commercial | Groceries, Meat, Produce | BakedGoods |
| `StreetFood` | `streetfood_stalls` | leisure | Meat, Produce, Flour | BakedGoods |
| `FineDining` | `finedining_restaurant` | leisure | Meat, Produce, Dairy | - |
| `BubbleTea` | `bubbletea_bar` | commercial | Dairy, Produce | BakedGoods |
| `Bar` | `bar` | leisure | Groceries | - |
| `KaraokeBox` | `karaoke_box` | leisure | Groceries | - |
| `Cinema` | `cinema` | leisure | Groceries | - |
| `Theatre` | `theatre` | leisure | Groceries | - |
| `ConcertHall` | `livemusic_hall` | leisure | Groceries | - |
| `BowlingAlley` | `bowling_alley` | leisure | Groceries | - |
| `Arcade` | `arcade` | leisure | Electronics | - |
| `Gym` | `gym` | commercial | - | - |
| `ComedyClub` | `comedy_club` | leisure | Groceries | - |
| `FreightForwarder` | `freight_forwarder` | - (its own system) | Fuel | - |
| `Contractor` | `building_contractor` | - (its own system) | - | - |
| `BuildersMerchant` | `builders_merchant` | - (its own system) | Plastics | Goods |
| `FoodTruck` | `food_truck` | - (its own system) | Meat, Produce, Flour, Dairy | BakedGoods |
<!-- doc-guard: catalogue lists end -->

## Inventory: fact, where it was, where it is now

| Fact | Was | Now |
|---|---|---|
| Good: label (UI, remote API, reports) | `market/resource.rs` `Resource::label` match | `GOODS[..].label` |
| Good: base price | `market/resource.rs` `base_price` match | `GOODS[..].base_price` |
| Good: export keep hours | `market/resource.rs` `export_keep_hours` | `GOODS[..].export_keep_hours` |
| Good: road wear | `market/resource.rs` `road_wear_factor` | `GOODS[..].road_wear` |
| Good: retail | `market/resource.rs` `is_retail` | `GOODS[..].retail` |
| Good: container (dry/reefer/tank/open-top) | `rail/freight.rs` `BoxKind::for_resource` match | `GOODS[..].ships` |
| Good: lorry body | `vehicle.rs` `VehicleModel::for_cargo` match | `GOODS[..].truck` |
| Good: flown in | `airport.rs` `flies` | `GOODS[..].by_air` |
| Good: category, unit | nowhere | `GOODS[..].category`, `.unit` (r13; remote API since r14) |
| Good: made in town from another good's stock | nowhere | `GOODS[..].local_from` (r14) |
| Good: who makes / uses it | implicit in recipes | `catalogue::makers` / `users` (from the recipes); other flows in `GOODS[..].note` |
| Building site materials | `world/construction.rs` `order_materials` literals, then `SITE_BILL` (r13) | `BUILD_MIXES` + `site_bill` (r14); bought by the site's builder, `World::buy_site_load` (r14-builders) |
| Rubble lorry's stand-in cargo | `world/construction.rs` literal `Resource::Parts` | `RUBBLE` |
| Business: label (and API key) | `market/business_kind.rs` `label` | `BUSINESSES[..].label` |
| Business: recipe | `business_kind.rs` `recipe` | `.inputs`, `.outputs` |
| Business: land-use candidates | `business_kind.rs` `candidates` | `INDUSTRIAL`, `COMMERCIAL`, `ENTERTAINMENT`, `CIVIC` |
| Business: hours, customers per job, max floors, stock hours | `business_kind.rs` | `.hours`, `.customers_per_job`, `.max_floors`, `.stock_hours` |
| Business: food / outing / lodging / waterfront / needs site / student trade / truck lot | `business_kind.rs` (7 `matches!` lists) | flags on the row |
| Business: walk-in sale | `business_kind.rs` `walk_in_sale` | `.walk_in` |
| Business: tab factor | `business_kind.rs` + `leisure.rs` `tab_factor` (two tables) | `.tab` |
| Business: booking share, family/date spot, loudness, trash, land value lift | `leisure.rs` methods | `.booking_share`, `.family_friendly`, `.date_spot`, `.loudness`, `.trash`, `.land_value_lift` |
| Business: shopfront sign word | `leisure.rs` `sign_word` | `.sign` |
| Business: household delivery order | `bots.rs` `order_of` | `.home_order` |
| Business: starting capital (heavy industry x5) | `market/business.rs` `Business::new` literal | `.start_capital` |
| Business: inspector line | `nd_app/src/ui/inspector/about.rs` `business_note` match | `.about` |
| Business: fouls the air (inspector) | `nd_app/src/ui/inspector/about.rs` `dirty` | `.fouls_air` |
| UI colours / icons per good | none exist | (add a column when the UI wants one) |
| Map layers per good | none (layers are per land survey resource, not per good) | n/a |

### Still keyed on a kind elsewhere (on purpose)

These are rules of one subsystem that happen to switch on the kind, not facts about the kind, or
they are curves rather than values. They stay with their subsystem; most end in `_ =>` so a new
kind gets the default. Check them when adding a kind:

- `industry.rs` `IndustryKind::businesses` (industry sub-zones; nuisance, dirt, spill by industry);
  `BusinessKind::dirtiness` / `spill` (chemical works override) in `business_kind.rs`.
- `leisure.rs` `appeal`, `tourist_pull` (who likes it at what hour), `EATERIES` / `NIGHTS_OUT` lists.
- `pressure.rs` `PressureKind::business` (which pressure opens it), `commercial.rs` `CommercialSub`
  menus, `tax.rs` `TaxCategory::of_business`, `districts.rs` `favours`.
- `companies.rs` (fleets, yards, min area, shop chains, specialty), `estate.rs` (expansion, site
  cap, branches), `world/relocate.rs` `rooted`, `world/tenants.rs` `fits_shopfront`,
  `world/stories.rs` `is_nightlife`, `world/shore.rs` `jetty_kind_for`, `world/dirt.rs`,
  `yards.rs` (yard clutter by trade), `names.rs` `trade_words` (brand names).
- `vehicle.rs` `Livery::for_business` (van liveries).
- Looks: `nd_procgen/src/buildings/choose.rs` `industry_styles` and the styles' own matches,
  `nd_app/src/audio/spots.rs`, `nd_app/src/ui/inspector/building.rs`.
- Goods: buyers' stock caps in `waste.rs` `sell_recovered` (scrap yards 80, others 30), the
  household shopping rules in `world/market.rs` and `bots.rs` (groceries, meals, goods).

## How to add a good (round 14: building materials)

1. Add the variant **at the end** of `Resource` in `market/resource.rs` (saves key maps by name, but
   keep the order), add it to `Resource::ALL` and bump `Resource::COUNT`.
2. Add its index to `catalogue::good_index` (the compiler stops you until you do).
3. Add a `GoodRow` at the end of `GOODS`: label, category (add `GoodCategory::BuildingMaterial` for
   concrete, brick, glass, steel, timber, rebar, ...), unit, base price, `ships` (the box), `truck`,
   and a `note` if it comes from outside the recipes.
4. Give it a maker and users through recipes in `BUSINESSES` (a brickworks, a cement plant, a
   hardware store), or say in the note where it comes from (imports).
5. Run `cargo test -p nd_sim catalogue`: the completeness tests check the row is in place, named,
   priced, and that anything used has a source.

If it needs a box or a lorry that does not exist yet (flat rack for steel and timber, bulk tanker for
cement, curtainsider for pallets), add the `BoxKind` / `BodyType` variant at the end of its enum, its
look in `nd_procgen/src/buildings/parts/boxes.rs` and `nd_app/src/render/freight.rs`, then use it in
the row.

### Bills of materials

See "Building materials and bills of materials" below (round 14).

[r14-builders] Sites are built by builders (`world/builders.rs`, `docs/DESIGN.md` "Builders and the
builders' merchant"). The builder buys the lines of the stage being supplied (`World::site_load(bid)`, the
same list as `World::site_lines_to_order`: (good, qty)): from its own yard stock, a scrap yard (steel),
the builders' merchant (which stocks every `BuildingMaterial` good, `catalogue::site_materials()`) or a
maker of the good's `local_from` in town, whichever is cheapest, else all of the stage's imports on one
lorry. Each load is booked on the site's `due` list before its lorry leaves, as r14-materials does.

## The building trade's kinds [r14-builders]

- `Contractor` ("building contractor"): no recipe; lives on contracts (instalments from whoever is
  building), pays its crews, buys the sites' materials. `needs_site` (opened by the `Contractor` pressure
  when sites go to firms from out of town), max 2 floors, min lot 450 m², look `Style::BuildersYard`,
  `Livery::Construction` in its fleet colour, names from `names::trade_words` ("Okafor Construction").
  Out-of-town firms are not businesses: a name (`builders::out_of_town_name`) and a colour on the contract.
- `BuildersMerchant` ("builders' merchant"): recipe plastics into goods (the DIY floor, walk-in, residents
  shop there); its trade stock of `site_materials()` (every building material) is bought and sold outside
  the recipe (`World::merchant_restock`: from makers of the good's `local_from`, else a bulk import;
  trade price `builders::trade_price` in `Business::sell_price`) and it is in the seller index for those
  goods. `needs_site` (the `BuildersMerchant` pressure: site material units a day and population), max 2
  floors, min lot 900 m², look `Style::BuildersMerchant`, sign "HARDWARE".
- Numbers (crew size, reach, margins, merchant markups, pressure rates) are in `crates/nd_sim/src/builders.rs`.

## How to add a kind of business

1. Variant at the end of `BusinessKind`, add it to `BusinessKind::ALL`, bump `COUNT`.
2. Index in `catalogue::business_index` (compile error until you do).
3. A `BusinessRow` at the end of `BUSINESSES` (only what differs from `BASE`), and the land-use list
   it grows on, if any.
4. Walk the "still keyed on a kind elsewhere" list above: names, liveries, building looks, and the
   pressure that opens it.

## Food trucks (r14-foodtrucks)

`BusinessKind::FoodTruck` (appended) is the row for the trade as a whole: `needs_site`, so the
market never grows one on a lot; its inspector line. The types of truck have their own table,
**`FOOD_TRUCKS: [TruckRow; TruckKind::COUNT]`**, in `TruckKind` order (`nd_sim::food_trucks`):
label and API key, menu, the price and the satiety of one serving, its `ingredients` per serving
(bought each morning from the town's businesses holding them, else wholesale), trading `hours`
(past midnight as 24..), the longest stint, `affinity` per kind of crowd (park, beach, stadium,
nightlife, offices, event), `warm_only` (ice cream), `cold_lift` (coffee), servings loaded a day,
`cart` (a three-wheeler with an umbrella), the sign word and `jingle`. Their paint jobs are
`Livery::FoodTruck(type)` (`nd_procgen::vehicles::food_truck_colors`). To add a type: a variant at
the end of `TruckKind` (and `ALL`, `COUNT`), a row at the end of `FOOD_TRUCKS`, its colours and
names (`food_trucks::truck_name`), and `cargo test -p nd_sim catalogue` checks the row.

## Behaviour changes in r13-catalogue

- Plastics ship in a dry box instead of a tank container (pellets in bags). Tanks now hold only crude
  and fuel. Plastics loads of two TEU or more can now go as forty-footers (tanks never did).
  Tank boxes already holding plastics in a save keep them until they are emptied.

## Building materials and bills of materials (round 14, r14-materials)

Harvey (2026-11-20): "every building should have different requirements for materials. glass, brick,
concrete, steel, wood, ect for each size/type of architecture ... Then containers can display what and
how much of different supplies it contains. keep it medium granularity."

### The materials (13 goods, category `BuildingMaterial`, appended after `Metal`)

| Good | Unit | Price | Box | Site lorry | Made in town by (`local_from`) |
|---|---|---|---|---|---|
| cement | t | 6 | bulk box (new) | concrete mixer (new) | imported for now |
| sand and gravel | t | 4 | open-top | tipper (new) | strip mines (their rock stock) |
| bricks and blocks | t | 5 | dry | flatbed, brick pallets | imported for now |
| glass | t | 20 | dry | flatbed, glass on an A-frame | imported for now |
| structural steel | t | 80 | flat rack (new) | flatbed, I-beams | smelters, scrap yards (scrap first) |
| rebar | t | 72 | flat rack | flatbed, bar bundles | smelters, scrap yards |
| timber | t | 8 | flat rack | flatbed, timber packs | imported for now (forestry later) |
| roofing | pallets | 10 | dry | curtain-sider (new) | imported for now |
| insulation | pallets | 12 | dry | curtain-sider | refinery / chemical works (plastics) |
| pipe and plumbing | pallets | 14 | flat rack | flatbed, pipe | refinery / chemical works (plastics) |
| wiring and electrical | crates | 14 | dry | box van | parts fabs |
| fixtures and fittings | crates | 12 | dry | panel truck | parts fabs |
| paint | crates | 14 | dry | box van | refinery / chemical works (plastics) |

`GoodRow::local_from` is how a material is made in town without a recipe of its own: a site buys the
maker's stock of that good (the smelter's billets, the mine's rock, the refinery's plastics, the fab's
parts) and it is shipped as the material. `catalogue::makers` counts those makers, so "who makes it"
still reads off one place. No existing recipe changed. Concrete is cement plus sand and gravel (mixed on
site, delivered by the mixer); a cement works, brickworks, glassworks and sawmill would be new business
kinds (left for later, or for whoever adds them: give them the good as a recipe output and drop the
"imported for now" from the note).

New `BoxKind`s: `FlatRack` (floor, two end walls, a strapped pack; no number stencils) and `Bulk` (a dry
box with roof hatches). Looks in `nd_procgen/src/buildings/parts/plant.rs` (`BoxLook::of` maps a kind to
its look; `boxes.rs` and `nd_app/src/render/freight.rs` call it). New `BodyType`s: `Flatbed` (loader
crane, its deck load drawn from `VehicleModel::boxed` = a `vehicle::Load` code from 150, clear of the
container chassis codes), `Mixer`, `Tipper`, `Curtainsider` (`nd_procgen/src/vehicles.rs`).

### Bills of materials

- `catalogue::BuildType`: how a building is built (glass tower, concrete tower, concrete block, brick
  row, timber house, shack, steel shed, heavy plant, infrastructure, landscape, box stack).
- `catalogue::BUILD_MIXES`: **the one table** of what each build type is made of: per line a good, a
  stage (foundations, frame, envelope, fit-out) and `base + per_floor * floors` units.
- `catalogue::build_type(&SiteFacts)`: the build type from the inputs that also choose the procgen style
  (district look, land use, tier, floors, trade, plant, rail/port/depot/service, box home, megablock).
  The sim cannot see procgen's styles (procgen depends on the sim, and the sim runs headless), so this is
  a sim-side reading of the same inputs: Glass Campus / Skyline / Tech Campus at 3+ floors are glass
  towers, Brick & Steel / Historic / Lantern / Night Market up to 5 floors brick rows, Garden and
  Suburban houses timber, the Shantytown self-built, industry steel sheds, plants heavy, and so on.
- `catalogue::site_bill(build, floors, lot_area, tier) -> Vec<BillLine>`: the bill, scaled by the lot
  (`lot_area / 500`, clamped 0.6..2.5) and the tier (0.85 / 1.0 / 1.2). `BillLine { good, qty, stage }`
  is a plain list, for the site's own orders and for anyone who buys a bill (builder companies, the
  hardware store): `World::site_bill(bid)`.

Examples at 500 m², mid tier (`cargo test -p nd_sim print_example_bills -- --nocapture`):

| Build type (floors) | Foundations | Frame | Envelope | Fit-out | Worth (old parts bill) |
|---|---|---|---|---|---|
| glass tower (20) | 14 t cement, 2 t rebar, 18 t sand/gravel | 8 t steel, 8 t cement | 14 t glass, 4 pallets insulation | 6 crates wiring, 7 crates fixtures, 4 pallets pipe | 1,533 (1,584) |
| concrete tower (12) | 10 t cement, 1 t rebar, 13 t sand/gravel | 10 t cement, 2 t rebar, 13 t sand/gravel | 15 t bricks/blocks, 4 t glass | 3 wiring, 5 fixtures, 2 pipe, 2 paint | 813 (1,008) |
| brick row (3) | 3 t cement, 4 t sand/gravel | 15 t bricks, 4 t timber | 2 pallets roofing, 1 t glass, 1 insulation | 1 pipe, 1 wiring, 1 fixtures, 1 paint | 252 (360) |
| timber house (2) | 2 t cement, 2 t sand/gravel | 8 t timber | 2 roofing, 1 t glass, 1 insulation | small lines | 182 (288) |
| self-built shack (1) | 1 t sand/gravel | 2 t timber, 0.4 t scrap steel | 1 roofing | a crate of fittings | 68 (216) |
| steel shed (1) | 4.5 t cement, 6.5 t sand/gravel, 0.5 t rebar | 2 t steel | 4 roofing, 1 insulation | 1 wiring, 1 pipe | 313 (216) |

(The worths are at base prices; the test `bills_cost_about_what_parts_did` keeps the ordinary types
within a factor of two of the old parts bill so the economy keeps its scale.)

### Deliveries by stage

A site still has four deliveries, one per quarter (`Construction::stage`), and still orders one stage
ahead. Each order now carries the whole stage's lines (`World::order_materials`):

1. each line with a local maker (`local_from`) comes from that maker in its own lorry (steel and rebar
   from a scrap yard first, K34), paid to the maker;
2. what no local maker can supply is imported, **all of a stage's imports on one lorry** from out of
   town (booked as its first good, the lead; the other lines ride with it), paid to the outside;
3. `Construction::due` lists every line on the road with the load it rides in (`SiteLoad::via`);
   `World::site_received` moves arrived lines to `Construction::delivered` (the stockpile), and the
   stage counts as supplied (`deliveries += 1`) only when every line of its bill is on site. Its work
   waits until then (`waiting_for_materials`, unchanged).
4. A load lost on the way (`site_unsupplied`: no road) drops its lines from `due`; the site then orders
   only what is missing (`World::site_lines_to_order`).
5. Older saves: a load the site was not waiting for (a parts lorry from before bills) counts as the
   stage's delivery when nothing else is due, as loads always did; the new fields default empty.

`Construction::on_site()` is the pile: each stage's deliveries less what its quarter's work has used.
`World::site_materials(bid)` gives per stage the bill, delivered, on the road, on site and the state (to
order, on the way, on site, being built, done): the building inspector's MATERIALS section and the remote
API's `construction.stages` (plus `build_type`, `bill_value`). The site's drawn stacks are its real
pile (`Extras::site_pile`, `World::site_pile_goods`): brick pallets, cement bags, a sand heap, steel and
rebar on bearers, timber packs, glass on an A-frame, pipe, roofing and insulation pallets.

### Showing what and how much

- Boxes: `containers::Goods::amount` (serde default) and `Goods::describe()`: the box inspector says
  "Full of 25 t sand and gravel" under "20 ft open-top box"; box rows say "20ft flat rack · ... · 50 t
  structural steel". Boxes from older saves have no amount and say just the good.
- Lorries: the inspector says "Site lorry · flatbed" and lists the whole load
  (`World::load_manifest`: "8 t structural steel" or "14 t glass, 4 pallets insulation").
- Remote API: `report economy` resources carry `category`, `unit`, `box`, `truck`, `local_from`.

### For the builder companies (r14-builders) and the hardware store

The bill is `World::site_bill(bid) -> Vec<BillLine { good, qty, stage }>` and the open lines of the next
stage are `World::site_lines_to_order(bid)`. A builder that buys a stage itself should push its loads to
`Construction::due` (with `via` = the cargo's resource) and ship them to the site; `deliver` hands them
to `site_received` and the stage gating works unchanged. A hardware store would stock materials like any
retailer (`Business::add`) and be found by `best_seller` once a material lists it as a maker (a recipe
output, or a `local_from`-style rule).
