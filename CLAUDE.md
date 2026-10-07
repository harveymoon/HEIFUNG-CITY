# Neon District: notes for Claude Code agents

A Rust / Bevy 0.19 city builder. Design: `docs/DESIGN.md`. Remote API: `docs/REMOTE.md`.
Open work: `todo.md` (sorted by area; tick items with "→ DONE [branch]: what").

## Crates
- `crates/nd_sim`: the deterministic simulation (no Bevy). `World` in `src/world/mod.rs` with
  most systems in `src/world/*.rs`; roads, traffic, citizens, economy, pressure, estate, pillars.
  Unit tests sit next to the code (`world/*_tests.rs`); integration tests in `tests/`
  (`demo_city`, `society_demo`, `sim_checks`, `rebuild_regressions`); probes in `examples/`.
- `crates/nd_procgen`: meshes and generation: buildings (styles in `src/buildings/`), streets,
  kerbs, terrain, `citygen` (tensor-field example maps).
- `crates/nd_minds`: personalities and the async LLM "minds".
- `crates/nd_app`: the Bevy app: render (`src/render/`), UI (`src/ui/`), tools (`src/tools.rs`),
  the remote API (`src/remote.rs`, `src/remote/`), map layers (`src/map_layers.rs`).

## Rules
- **Never touch Harvey's running game:** remote API port 4817, and `target/release`
  (including `target/release/saves`). Read-only API calls on 4817 only when asked. Never kill
  a `neon_district.exe` you did not start.
- Never send scripted clicks or keys to other windows. Never use Ctrl+Shift+G (a system
  shortcut on this machine).
- Billionaires and other "named rich" people are fictional, from the game's name generator.
- New `World` fields need `#[serde(default)]` so old saves load. Keep the sim deterministic:
  no new world-RNG draws in hot paths; prefer hashes.
- Work in a git worktree under `.claude/worktrees/<name>` on its own branch; merge main into it
  before reporting; do not merge into main yourself.

## Building and testing
- Drive C: is tight. Build agent work to `CARGO_TARGET_DIR=O:/nd_targets/<branch>`.
- Speed: `.cargo/config.toml` turns on sccache (shared across worktrees; incremental off) and
  the rust-lld linker. Use `--profile fast` (release without LTO, 16 codegen units) for tests:
  `cargo test --profile fast --workspace`
- The playable exe is `cargo build -p nd_app --release` into `target/release` (main checkout
  only). Move a running `neon_district.exe` aside first (`mv` it to `neon_district.old-HHMM.exe`).
- `society_demo`'s 911 thresholds are borderline at a fixed seed; unrelated changes can flip it.

## Running a test instance
- `ND_REMOTE_PORT=48xx` (pick a free port, never 4817), `ND_TITLE=0` (skip the title screen),
  `ND_QUALITY=low|medium|high|auto`. Run it from your own working directory so its `saves/`
  and `flags/` stay yours.
- Drive it with `curl -H "Content-Type: application/json" -d '{"cmd":"..."}' http://127.0.0.1:<port>/cmd`:
  `status`, `step`, `generate_city {preset, seed}`, `camera`, `screenshot`, `select`,
  `inspect`, `report`, `tool`, `overlay`, `window`. Full list: `{"cmd":"help"}`.
- Harvey's in-game flags ("Flag for Claude") land in `flags/claude_flags.jsonl` under the
  game's working directory, and via the `flags` command.
