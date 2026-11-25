# Studio

The Studio is the product: a local browser workbench served from the Anvil
binary. It binds only to `127.0.0.1`, uses a per-launch token and embeds all HTML,
CSS and JavaScript. There is no CDN, no external font, no analytics and no
network access from the page.

## Launch

```sh
cargo run --release
```

The launcher starts the server (random port by default), opens the browser at
`http://127.0.0.1:<port>/?token=<128-bit-token>` and stays in the foreground.
`--port`, `--no-browser`, `--help` and `--version` are supported. There is no
terminal UI or interactive shell.

## Authentication and headers

- The API requires `X-Anvil-Token` with the launch token.
- SSE uses `?token=` because `EventSource` cannot set headers.
- Static assets are public but contain no project data.
- The page receives a strict CSP: same-origin scripts/styles/images, `blob:`
  for the generated preview, no frames, no forms.
- Responses are `Cache-Control: no-store` and `X-Content-Type-Options: nosniff`.

## API

| Method | Path | Purpose |
|---|---|---|
| GET | `/api/health` | version and liveness |
| GET | `/api/state` | project, schematic, netlist, board summary, DRC, route, job |
| GET | `/api/library` | parts, symbols and graphics for the canvas |
| GET | `/api/board` | board geometry, pads, traces, vias, ratsnest, DRC |
| GET | `/api/netlist` | extracted nets and diagnostics |
| GET | `/api/drc` | full DRC report |
| POST | `/api/simulate` | DC or transient analysis |
| POST | `/api/schematic/place` | place a library part |
| POST | `/api/schematic/move` | move a component |
| POST | `/api/schematic/rotate` | rotate 90° |
| POST | `/api/schematic/mirror` | mirror X |
| POST | `/api/schematic/delete` | delete component/wire/label |
| POST | `/api/schematic/wire` | add a wire polyline |
| POST | `/api/schematic/label` | add a local/global/power label |
| POST | `/api/schematic/junction` | add a junction |
| POST | `/api/schematic/value` | change a value |
| POST | `/api/schematic/property` | set a simulation property |
| POST | `/api/board/generate` | import the netlist onto a board |
| POST | `/api/board/clear-routes` | remove all traces and vias |
| POST | `/api/route/start` | start a background autoroute |
| POST | `/api/route/run-sync` | route synchronously (tests/batch) |
| GET | `/api/route/state` | job progress snapshot |
| GET | `/api/route/report` | last route report |
| GET | `/api/route/events` | SSE progress/done stream |
| GET | `/api/export/gerber?layer=...` | top/bottom/outline/silk Gerber |
| GET | `/api/export/drill` | Excellon drill |
| GET | `/api/export/preview.png?layer=...` | rasterized PNG preview |
| GET | `/api/export/manifest` | generated file names and sizes |
| GET | `/api/examples` | shipped examples |
| POST | `/api/project/load` | load an example by name |
| POST | `/api/project/demo` | reset to the demo |
| POST | `/api/project/reset` | new empty project |

All mutating schematic endpoints invalidate the current board, so layout cannot
silently drift from the schematic.

## UI

- **Schematic**: canvas with grid, pan/zoom, component placement from the
  library, drag to move, rotate/mirror/delete, wires, labels, selection value
  editing, netlist diagnostics and an example selector.
- **Simulate**: DC or transient with time step, stop time and integrator;
  multi-series canvas plot with legend; netlist summary.
- **Board**: generate from schematic, board canvas with top/bottom layers,
  pads, traces, vias, ratsnest and DRC markers; clearance/drill/annular checks;
  autoroute progress over SSE; clear routes.
- **Export**: top/bottom Gerber preview rendered by the internal rasterizer,
  download buttons for copper/outline/silk/drill and a file manifest.

The demo project loads at startup: a pulse-driven RC divider with an LED branch.
It routes with zero DRC errors.

## Concurrency

One project is held behind a mutex. Autoroute clones the board, routes in a
worker thread and publishes progress; the browser polls through SSE. A second
start while running returns HTTP 409. Panics in the worker are caught and
reported as a job error.

## Limits

- One in-memory project; no file dialogs, autosave, undo or multi-document tabs.
- No user accounts, TLS or remote access. Binding is loopback only.
- No mobile-specific layout.
- The schematic editor is intentionally small: no hierarchical sheets, bus
  drawing tool or symbol editor.
