# AI committee and LLM citizens: assessment (October 2026)

Status: the committee is built (feat/committee, October 2026), and so are model-written conversation
lines and on-demand diaries for the citizens the player watches: see "What was built" and "Voices"
at the end.

## The developer's idea

- Citizens might write short diaries every day using an open-source LLM.
- Maybe every character gets limited decisions at junctions (the developer mentioned "the JEV thing").
- Instead, or as well: a committee of AIs the player appoints from a pool of politicians, six seats, each
  with a role and a personality lens. Between meetings they alert the player when thresholds in their
  scope are crossed. Every 30 sim days they review all game data and changes since the last meeting and
  suggest topics to vote on.
- The player is the mayor (or dictator): one vote on the committee, and can reappoint members at will.
- A secretary keeps the meeting on topic with a fixed agenda:
  1. Status report from every member, with optional responses from others.
  2. Optional topics of concern to vote on.
  3. Voting results and implementation plans.
  4. Implementation: the voted roads, zones and special buildings are placed in a proper, functional place
     on the map by the secretary, who can ask the player for help.
- Run the meeting as a simulated conversation between all members to see what happens to the city.

## Assessment

**Recommendation: build the committee first; give LLM diaries only to spotlit, favourite or inspected
citizens; keep the rule-based mind for the crowd.**

### Open-source models (as of mid-2026)

Main open-weight families: Meta's Llama, Alibaba's Qwen, Mistral, Google's Gemma, DeepSeek,
Microsoft's Phi, and OpenAI's gpt-oss. Common runtimes: Ollama, llama.cpp and LM Studio on one PC, or
vLLM for batched serving.

| Use | Model size | Why |
|---|---|---|
| Citizen diaries, one-liners | 1–8B | Fast and cheap; fine for short flavour text |
| Committee members, secretary | 14–32B, or a hosted model such as Claude | Must read a report, reason and keep to a role |

"The JEV thing" was not identified. If it means JEPA (Meta's Joint Embedding Predictive Architecture),
that is a world-model approach that predicts in an abstract representation, mostly from video. It is not
a text model that makes choices, so it is not a natural fit for citizens' decisions. Revisit if it was
something else.

### Diaries for every citizen: too much data

- The demo city reaches about 3,000 people by day 8. A 150-word diary each per day is roughly 450,000
  tokens per sim day.
- At 256× a sim day passes in about 6 real seconds. No local setup keeps up, and a hosted model would be
  expensive.
- The game is already built for the workable version:
  - **Most citizens:** the rule-based mind (cheap, deterministic).
  - **Spotlit and favourite citizens:** LLM diaries. `nd_minds` already has the hybrid mind and a
    Spotlight toggle wired to an LLM backend (a mock today).
  - **Everyone else:** diaries on demand, written from the memories the sim already records ("Shelves
    were empty at…", "Rent got too steep…") when the player opens that person, then cached.

### The committee: a good fit

- **Cheap.** Six members plus a secretary every 30 sim days is a few calls each over a compact report of
  about 5–10k tokens.
- **Grounded.** Members read `report`, `map` and `history` from the remote API (docs/REMOTE.md), so they
  argue about real numbers.
- **Free between meetings.** Threshold alerts per member scope can be plain rules (e.g. "transit riders
  down 20%", "unpowered buildings > 0"); the LLM only phrases the warning.

### Proposed design

1. **Pool and appointments.** Candidate politicians with a persona, a role (transit, housing, finance,
   safety, environment, business, …) and a bias. The mayor appoints and dismisses at any time; six seats.
2. **Meeting.** The secretary runs the fixed agenda above as a structured conversation with turn limits,
   so it stays on topic and costs a predictable amount. Pause the sim during meetings.
3. **Votes.** The mayor is one vote; optionally a mayoral veto. Proposals and votes are structured JSON,
   e.g. `{"action": "rezone", "area": ..., "to": "residential"}`, validated against what the API can
   actually do before they are tabled.
4. **Implementation.** The LLM does not pick coordinates (it would put a stadium in a river):
   - Code generates valid candidate sites (e.g. lots over 3,000 m² with road access and police coverage
     under 40%).
   - The secretary chooses among those options and gives a reason.
   - The result goes into a named project for the mayor to review (uses the multiple-projects feature from
     feat/planning), then is implemented.
   - If no valid site exists, the secretary asks the mayor.
5. **Memory.** Each member keeps a short record of past votes and outcomes so positions stay consistent.

### Risks

- **Repetitive or agreeable personas.** Give members real conflicting goals and a memory of past votes.
- **Errors compound.** Each meeting changes the city the next meeting reviews. Keep actions reversible
  through projects.
- **Model speed.** A local model's speed decides whether a meeting takes seconds or minutes; pausing the
  sim avoids the problem.

### Suggested order

1. Committee and secretary with alerts and meetings: real votes, read-only actions.
2. Implementation through projects (candidate sites from code, secretary chooses, mayor reviews).
3. On-demand diaries for inspected and spotlit citizens.

## What was built (feat/committee)

The committee, with a pluggable text backend: it works fully offline with a template stand-in, and
switches to a local model or the Claude API in its settings. The two halves:

- `nd_sim::committee` (deterministic, no model): the pool, seats, the city report, alert rules,
  proposal actions and their validation, votes by the rules, candidate sites, projects and
  memory. `World::committee` is `serde(default)`: old saves load and set a committee up when it
  is first used. The sim never reads it.
- `nd_minds::committee`: the `CommitteeBackend` trait (submit, poll), the stand-in
  (`MockCommittee`), the HTTP connectors (`http.rs`), the `Courier` that falls back to the
  stand-in for any failed turn, the settings, and the meeting `Driver`.
- In the game: `nd_app/src/committee.rs` (the runtime, pumped once a frame), the Committee window
  (`ui/committee.rs`, top bar: Committee), the remote API (`remote/committee.rs`, docs/REMOTE.md).

### Pool and appointments

Fourteen candidates from the game's name generator (fictional, distinct), seeded from the world
and landscape seeds by a hash: no world-RNG draws. Each has a scope (transit, housing, finance,
public safety, environment, business, power and roads, health and schools), a temper (firebrand,
technocrat, populist, pragmatist, idealist, old guard, contrarian, dealmaker), leanings
(spending, growth, order, loyalty, each -1..1, nudged by the job but never fixed by it, so a
pro-growth environment commissioner can happen), a fictional party, a background, and goals: the
job's goal plus one from their strongest leaning, which often pulls against it. A new committee
seats one member for each of transit, housing, finance, safety, environment and business; the
mayor appoints and dismisses at will (Members tab, or `committee {action: appoint|dismiss}`). A
dismissed member keeps their memory and record.

### Alerts between meetings

Once a sim day the rules in `committee/rules.rs` run on the city report: transit riders down
20% week on week, a town of 400 with no bus line, rent burden over 35% (or up five points in a
week), people unhoused, no free homes, a deficit, a low treasury, crime up 25%, police or fire
cover under half the homes, unanswered calls, contamination spreading or a hotspot over 60%,
unemployment over 10%, commercial demand unmet, any building without power, brownouts, worn
roads, clinic or school cover low. A rule fires only if a seated member holds its scope (the
first in seat order raises it), then sleeps for its cooldown. The alert is a toast and an entry
in the window; the backend only phrases it (the rule's own sentence stands until it answers).

### Meetings

Every 30 sim days, or when the mayor calls one; the sim pauses until it closes. The report
(`CityReport`, about 600 tokens) is built from the same data as the remote `report`, `history` and
`map`: the history ring's last seven days against the seven before, the economy, coverage, the
grid, the ground, and Downtown plus four compass sides (and any painted districts) with homes,
jobs, cover, contamination and free parcels each. The secretary's agenda (`driver.rs`):

1. Opening; a status report from each member on their scope's figures against the last
   meeting's; up to two replies, from the members whose leanings differ most.
2. Topics of concern: up to three members, the most pressing first, table a structured proposal
   (`Action`, JSON): `build_service`, `build_power`, `build_depot`, `rezone`, `add_bus_line`,
   `change_tax`, `set_toll`, `road_repair`, `remediate`. Each is validated (known kind, real
   area, sane amounts, affordable, a depot before a bus line...) before it is tabled; an invalid
   one is ruled out of order in the transcript.
3. Votes: each member votes on every proposal in one turn; then the mayor votes yes, no, abstain
   or vetoes (the veto can be switched off). Simple majority of votes cast, ties fail.
4. Implementation: for each passed proposal the code finds valid options (`site_options`):
   for a building, free unzoned parcels and vacant lots in the area that `placement_problem_at`
   accepts, ranked by homes nearby without that service's cover; for zoning, groups of free
   parcels by a street that accept the use; for a bus line, stops by the busiest homes and
   workplaces at each end; for a clean-up, the worst contaminated cells. The secretary picks one
   with a reason, and the result becomes a named project ("Committee: Eastside police post").
   Zoning also goes into a parked world project of the same name, previewed dimmed like the
   mayor's own. With no valid option the project waits as "needs you" and the secretary asks
   the mayor.

Then the secretary closes. Turn limits keep cost predictable: a meeting of six makes at most 22
backend calls. The transcript is typed out in the window as it is produced; Skip shows it all.
A meeting in session is saved with the game and resumes after a load.

The model never picks coordinates and never changes the city: the mayor implements a project
(Projects tab or `committee {action: implement}`), picks another option, looks for sites again,
or dismisses it.

### Memory

After the votes every member remembers how they voted, whether it passed and their scope's
headline figure at the time. At the next meeting each memory is judged by how that figure moved
("crime per day down 18% since"), and an implemented or dismissed project is noted. The rules
lean toward a member's past vote on the same kind of action unless it went badly; the models are
shown the last four memories. Saved with the game.

### The stand-in

Without any model the committee still talks: templates per temper (openers, verdicts on good or
bad figures, agreeing and disagreeing replies, motions), filled with the real figures and trends,
the member's goals and memories; proposals from rules per scope (`proposals::suggest`); votes from
the rules (`proposals::lean`: scope, leanings against the action's traits, the money against the
treasury, temper, party, memory, a little seeded noise). Every choice comes from the meeting's
seed, so a meeting replays word for word.

### Switching backends

In the window's Settings tab (or `committee {action: settings}`), saved to
`committee_settings.ron` in the game's working directory:

- **Stand-in (offline)**: the default.
- **Local model**: any OpenAI-compatible server (`POST {url}/chat/completions`). Ollama:
  `http://127.0.0.1:11434/v1` with a model name such as `qwen2.5:14b`; LM Studio:
  `http://127.0.0.1:1234/v1`. A 14-32B model keeps to its role best. Think blocks are stripped.
- **Claude API**: the Messages API (`https://api.anthropic.com/v1/messages`, `anthropic-version:
  2023-06-01`), model `claude-sonnet-5-5` by default (any model id can be typed), effort `low`
  (`output_config.effort`), no sampling parameters, and by default `fallbacks: "default"` (beta
  `server-side-fallback-2026-07-01`) so a declined turn is re-run on a fallback model; switch that
  off if a model rejects it. The key comes from `ANTHROPIC_API_KEY`, else from the settings file
  (paste it in the window); it is never shown, logged or put in an error message.

"Test connection" sends one tiny request on a thread of its own. Requests run on a worker thread;
the game never waits on them. A turn that fails, returns no usable JSON or takes longer than the
timeout is answered by the stand-in, with a note in the transcript and the error in Settings.

### Voices: conversation lines and diaries

The same backend (and the same settings) gives words to the people the player cares about
(`nd_minds::committee::voices`, `ui/diary.rs`):

- **Conversation lines.** The social layer writes a template line into every conversation
  (`world/social.rs` `talk_line`), as before. When a spotlit or favourite citizen takes part and a
  model is switched on, the line is asked of the model (at most three at a time) with both
  people's personality, class, mood, bond, the topic, how it went, where, and the template line;
  the answer replaces the template in the speech bubble when it arrives. With the stand-in nothing
  is asked: the template is the line.
- **Diaries.** The person pane's Diary button (or `diary {citizen}` in the API) opens their diary
  of today, written from what the sim already remembers for them (today's memories and
  conversations), in their voice: the stand-in composes it from templates by personality and mood,
  a model writes 60 to 120 words. Cached per citizen and day; "Write it again" asks anew.

Both are display only: kept in the app (`Voices`, not the save), never read by the sim, and a
failed or slow request leaves the template (the courier's fallback). Spotlit citizens' thoughts
still go through the separate `HybridMind` path.

### Not done yet

- Streaming tokens from the model (the window types out each finished turn instead).
- The mayor pointing at a site on the map for a "needs you" project (they can retry after the
  city changes, or dismiss it).
