# AI committee and LLM citizens: assessment (October 2026)

Status: idea, not started. Written down so we can return to it later. Nothing here is built.

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
