# neon-district MCP server

`nd_mcp.py` exposes the game's remote API (see [`docs/REMOTE.md`](../../docs/REMOTE.md)) as MCP
tools, so Claude Code (or any MCP client) can drive and inspect a running game. Python 3 standard
library only: no `pip install`.

## Register with Claude Code

```
claude mcp add --scope user neon-district -- python C:\CODE\Neon_District\tools\mcp\nd_mcp.py
```

Then start the game (`cargo run --release -p nd_app`); the tools fail with "game not reachable"
until it is up. If the game runs on another port, pass the same `ND_REMOTE_PORT` to both:

```
claude mcp add --scope user neon-district -e ND_REMOTE_PORT=4900 -- python C:\CODE\Neon_District\tools\mcp\nd_mcp.py
```

## Tools

| Tool | Wraps |
| --- | --- |
| `report` | `report`: the one-call city review (use first) |
| `map` | `map`: grid of cells; a single layer also comes back as an ASCII heat map text block |
| `history` | `history`: daily and hourly series with trend summaries |
| `list` | `list`: sortable, filterable tables |
| `export` | `export`: everything above to one JSON file |
| `window` | `window`: open/close/toggle the game's info windows |
| `status` | `status` (now also `alerts_top`, `windows_open`) |
| `control` | `pause` / `speed` / `step` (`action`: pause, resume, toggle, speed, step) |
| `build` | `add_road`, `add_zone`, `bulldoze`, `project` (`project_action`), `plan` |
| `place` | `place` (station, freight_yard, depot, parking, coal, solar, wind, nuclear) |
| `bus` | `bus` |
| `find` | `find` |
| `inspect` | `inspect` |
| `select` | `select` (+ `favourite`) |
| `camera` | `camera` |
| `view` | `overlay`, `lighting`, `tool` |
| `screenshot` | `screenshot`, then waits up to 5 s for the file and attaches the PNG as an image block (`include_image: false` to skip; files over `ND_MCP_MAX_IMAGE_BYTES`, default 5 MB, are not attached) |
| `events` | `events` (`since`), or `thoughts` with `what: "thoughts"` |
| `saves` | `save` / `load` / `list_saves` |
| `seed_demo` | `seed_demo` |
| `raw` | any command object, as is |

## The raw API, without MCP

```
curl http://127.0.0.1:4817/status
curl -X POST http://127.0.0.1:4817/cmd -H "Content-Type: application/json" -d "{\"cmd\":\"step\",\"ticks\":200}"
```

PowerShell:

```
Invoke-RestMethod http://127.0.0.1:4817/cmd -Method Post -ContentType application/json -Body '{"cmd":"find","what":"buildings","limit":5}'
```

## Testing the server by hand

Pipe JSON-RPC lines in while the game runs:

```
(echo {"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-06-18","capabilities":{},"clientInfo":{"name":"t","version":"0"}}}
 echo {"jsonrpc":"2.0","method":"notifications/initialized"}
 echo {"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"status","arguments":{}}}) | python tools\mcp\nd_mcp.py
```
