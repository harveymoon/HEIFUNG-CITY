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
- `SITE_BILL`: what a building site orders (today one load of parts, scrap steel first).
- `makers(r)` / `users(r)`: who makes and who uses a good, read off the recipes (never written twice).
- `site_materials()` [r14-builders]: every good a site bill names (today `SITE_BILL`'s material and its
  scrap stand-in): what the builders' merchant stocks for the trade and builders keep in their yards.

The old methods (`Resource::label`, `base_price`, `BusinessKind::recipe`, `hours`, `is_food`, ...)
still exist and keep their signatures; they are one-line reads of the catalogue, so no caller had to
change. Enum variants, their order and serde are unchanged: saves load as before.

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
| Good: category, unit | nowhere | `GOODS[..].category`, `.unit` (new) |
| Good: who makes / uses it | implicit in recipes | `catalogue::makers` / `users` (from the recipes); other flows in `GOODS[..].note` |
| Building site materials (parts, scrap steel first, 4 + 2/floor) | `world/construction.rs` `order_materials` literals | `SITE_BILL`, read only by `World::site_load` (r14-builders) |
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

`SITE_BILL` is one bill for every site. Round 14 makes it a table of bills by building style and
size (`SiteBill` gains a list of `(Resource, units per floor)` and a stage), chosen by the site's
style.

[r14-builders] Sites are now built by builders (`world/builders.rs`, `docs/DESIGN.md` "Builders and the
builders' merchant"). The bill is read in ONE place: `World::site_load(bid) -> Vec<(Resource, f32)>`, the
lines of the delivery being ordered now (today `[(SITE_BILL.material, SITE_BILL.amount(floors))]`). The
builder buys each line (yard stock, scrap yard, merchant or maker, import), one lorry per line; the
delivery counts when the last lorry is in. A per-style, per-stage bill changes `site_load` (and lists its
goods in `site_materials`) and nothing else.

## The building trade's kinds [r14-builders]

- `Contractor` ("building contractor"): no recipe; lives on contracts (instalments from whoever is
  building), pays its crews, buys the sites' materials. `needs_site` (opened by the `Contractor` pressure
  when sites go to firms from out of town), max 2 floors, min lot 450 m², look `Style::BuildersYard`,
  `Livery::Construction` in its fleet colour, names from `names::trade_words` ("Okafor Construction").
  Out-of-town firms are not businesses: a name (`builders::out_of_town_name`) and a colour on the contract.
- `BuildersMerchant` ("builders' merchant"): recipe plastics into goods (the DIY floor, walk-in, residents
  shop there); its trade stock of `site_materials()` is bought and sold outside the recipe
  (`World::merchant_restock`, trade price `builders::trade_price` in `Business::sell_price`) and it is in
  the seller index for those goods. `needs_site` (the `BuildersMerchant` pressure: site material units a
  day and population), max 2 floors, min lot 900 m², look `Style::BuildersMerchant`, sign "HARDWARE".
- Numbers (crew size, reach, margins, merchant markups, pressure rates) are in `crates/nd_sim/src/builders.rs`.

## How to add a kind of business

1. Variant at the end of `BusinessKind`, add it to `BusinessKind::ALL`, bump `COUNT`.
2. Index in `catalogue::business_index` (compile error until you do).
3. A `BusinessRow` at the end of `BUSINESSES` (only what differs from `BASE`), and the land-use list
   it grows on, if any.
4. Walk the "still keyed on a kind elsewhere" list above: names, liveries, building looks, and the
   pressure that opens it.

## Behaviour changes in r13-catalogue

- Plastics ship in a dry box instead of a tank container (pellets in bags). Tanks now hold only crude
  and fuel. Plastics loads of two TEU or more can now go as forty-footers (tanks never did).
  Tank boxes already holding plastics in a save keep them until they are emptied.
