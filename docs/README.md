# Docs index

Where to look. One line each; newest facts win over older notes.

## In the repo root
- `CLAUDE.md`: rules and how-tos for agents (build, test, run a test instance, never touch Harvey's game).
- `README.md`: the public front page: what the game is, how to build and play, the crates.
- `todo.md`: open work only, sorted by area; "Round 16 sweep" (what is really open from rounds 13-15), "Agents running" and "Follow-ups noted in agent reports (not started)" near the middle.

## Living docs (`docs/`)
- `DESIGN.md`: the design doc: every system, how it works and where its code is (long: search it, don't read it whole).
- `REMOTE.md`: the remote control API: transport, every command and its fields. Doc guard [r16-docs]: the test `remote_md_documents_every_command_and_alias` (`crates/nd_app/src/remote/dispatch.rs`) fails when a command or alias in `help` is missing here.
- `catalogue.md`: the goods and business catalogue (`crates/nd_sim/src/catalogue.rs`): every good and kind of business (a generated list, guarded by `catalogue_md_lists_every_good_and_kind`; `ND_BLESS_DOCS=1` rewrites it), where each fact lives, how to add a good or a kind, the building materials and bills.
- `NAVIGATION.md`: camera controls for mouse, laptop, keyboard and remote desktop, and the `nav` command.
- `AI_COMMITTEE.md`: the mayor's AI committee, model voices for citizens, Talk and Ask, and the LLM backends.
- `rendering_detail.md`: how close-up detail is drawn: the shared distance bands, the shared prop library, who owns washing and clutter, every detail pass.
- `STYLE_REFERENCE.md`: the building style guide drawn from Harvey's inspiration renders (Hong Kong and cyberpunk families).
- `playtest/photo_report_<date>.md`: the cleanup round's photographer report (broken / wrong / unrealistic / polish).
  - `playtest/photo_report_2026-11-20.md` (round 12, 17 findings, all worked in round 15) and `playtest/photo_report_2026-11-22.md` (round 13, 12 findings, all worked in round 14).
  - `playtest/photo_report_2026-11-24.md` (round 16, 14 findings for round 17; the tour PDF and the beautiful shots are in `O:/projects/Heifung_District/shots/r16-photographer/`).
- `playtest/leftovers_sweep_<date>.md`: the cleanup round's sweep of agents' LEFT notes and the shallow-systems audit (CLOSED / FIXED / FILE / DROP per note). Round 16's sweep is the "Round 16 sweep" section of `todo.md` itself.
- `playtest/trip_review_2026-11-21.md`: the trip review (following people and bikes; its open items are in `todo.md`).
- `playtest/flags_2026-10-15.md`: Harvey's flags K1-K52, still referenced by the open "J. Flags" section of `todo.md`.
- `review/save_determinism_wip.patch`: unfinished save-determinism work, referenced by `todo.md` and `crates/nd_sim/tests/determinism.rs`.
- `reference/` (not in git): Harvey's reference material; leave it where it is.

## Elsewhere
- `tools/mcp/README.md`: the MCP server that wraps the remote API.
- `tools/todo_split.py`: moves finished `todo.md` items to the archive (`python tools/todo_split.py`, see its docstring).

## Archive (`docs/archive/`): history, not instructions
- `todo_done_2026-11-21.md`: every finished `todo.md` item, verbatim, under its old heading (up to round 12 by r13-docs; rounds 13-15 appended by r16-docs).
- `IDEAS_2026-10.md`: the first ideas backlog (October 2026); new ideas go to `todo.md` "Extras".
- `reviews/`: the October code reviews (`REVIEW.md`, `REVIEW2.md`, per-crate notes); all findings fixed or filed.
- `playtest/`: October playtest lists, flag dumps and the transit ridership note; all folded into `todo.md` long ago.
- `qa/`: October road-structure QA reports with before/after screenshots.

Old names: the game was Neon District / District 2040, so archived notes use those names, `neon_district.exe`
and `O:/nd_targets/...` paths. Today: HEIFUNG://CITY, `heifung_city.exe`, work files under `O:/projects/Heifung_District`.
