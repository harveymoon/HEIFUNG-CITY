# Flags, 2026-10-11 evening session (H1–H4)
Game: the city saved as "Perfect AI City" (game_id 1a12bf3530f-5ae374dcaf07f480), started fresh at 10:12, day 20 / 17,627 people at 20:15. From target/release/flags/claude_flags.jsonl (session ids 1–4).

- **H1** day 7 19:23, Millrace 2 (dam), at (408, -72): dam and road overlap. Wants a two-step dam tool: place the building, then click two coasts (one near the building) for the wall; live size, cost and expected power while dragging the end. No road on a dam and no dam on a road; dams carry footpaths only.
- **H2** day 8 03:36, Cobalt Beach, at (399, 57): public beaches placed side by side should merge into one bigger beach, with a landscape that fits the combined shape. Umbrellas packed away at night.
- **H3** day 11 15:12, Tideway Yacht Club (large marina), at (-3, -687): the marina was allowed on top of a road; maybe it also made an intersection.
- **H4** day 11 21:36, Calloway Marina, at (-1073, -629): marina desire 5.6, two built, the demand list still says "held: the player builds marinas: Build > Water". Cause (nd_sim/src/pressure.rs:513): `held` shows while wanted > have, so with 5.6 wanted and 2 built it stays held. Working as written, but the wording reads as if nothing was built. Show "wants about 6, you have 2", and check whether 5.6 marinas is sensible for this city.
