# Ideas backlog

Add freely. Move items into `DESIGN.md` when they get scheduled.

## World and procgen
- Vertical city: elevated roads, skybridges, buildings that stack on top of each other.
- Districts that emerge from land use clusters, each with a generated name and vibe.
- Weather: rain with wet-asphalt reflections, fog that eats the bloom, acid storms.
- Procedural holographic ads on building faces, generated from the shops inside.
- Underground: subway lines as a second road graph.
- Ruins and squats in zones that lose demand.

## Simulation
- Ambient through-traffic from the city edges so streets feel busy even in a small town.
- Lane changing and turn-lane choice; smarter signal timing (actuated, green waves).
- Elevated roads and bridges for proper grade-separated ramps; angle and grid snapping guides while drawing.
- Turn lanes and priority rules (main road has right of way over a ramp).
- Power and data grids as utilities; brownouts dim the neon.
- Crime and police presence as a per-district pressure.
- Price-sensitive demand: citizens pick cheaper shops, businesses undercut each other.
- Supply contracts and loyalty between businesses; delivery schedules instead of hourly checks.
- Landlords and property: who owns the towers, speculation, rent control as a policy lever.
- Districts with names computed from land value and land use clusters; district-level stats.
- Services (clinics, schools, transit stops) as land value amenities you place.
- Corporations as actors that build their own towers and lobby you.
- Health system: implants, clinics, black-market ripperdocs.

## Citizens and minds
- LLM backends: Anthropic, OpenAI-compatible, local models through an HTTP server.
- Dialogue window when you click a spotlight citizen.
- Gossip: memories propagate between citizens who share a venue.
- Reputation of places and of the player, remembered by citizens.
- Citizen-written reviews of shops and districts.
- Factions and gangs with territories.

## Player tools
- Zoning overlays showing demand, happiness, crime.
- Time-lapse recording of a city's growth.
- Photo mode with filters.

## Architecture
- More Stacks variety: buses and RVs as units, external stair towers, graffiti.
- Megastructure blocks spanning two lots, elevated walkways between them.
- Holographic crests for corporations; signage that names the actual business inside.
- Buildings that react to their condition over time: boarded windows, squats in derelicts, scaffolding during upgrades.
- Interiors visible through shopfronts in walk mode; doors you can walk through.
- Parcel merging for big styles (arenas, depots) so they take two or three lots.
- Non-convex zones: proper polygon clipping instead of half-plane cuts.

## Rendering
- Instanced rendering for windows and citizens once counts get large.
- Decals: posters, graffiti and puddles on the generated textures.
- Collision in walk mode (kerbs, walls) and a photo-mode free camera.
- Citizens with walk cycles and varied clothing; cars as real models.
- Screen-space reflections on wet ground.
- Volumetric fog shafts under flying billboards.
- LOD for distant buildings (merged district meshes).
