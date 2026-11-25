# Examples

Each `.json` file is an Anvil project (`anvil-project-1`). The Studio can load
them through the **Examples…** selector; the headless API can load them with
`POST /api/project/load {"name": "..."}`.

| File | Project | What it demonstrates |
|---|---|---|
| `anvil-demo.json` | Anvil demo | pulse-driven RC divider, LED branch, transient and board flow |
| `voltage-divider.json` | Voltage divider | DC operating point |
| `rc-step-response.json` | RC step response | trapezoidal transient integration |
| `half-wave-rectifier.json` | Half-wave rectifier | diode nonlinear solve |
| `led-indicator.json` | LED indicator | DC diode + series resistor |
| `wheatstone-bridge.json` | Wheatstone bridge | unbalanced bridge node voltages |
| `rlc-ringing.json` | RLC ringing | inductor/capacitor transient |
| `logic-gates.json` | Logic gates | pulse-driven AND and NOT with delays |
| `full-wave-rectifier.json` | Full-wave rectifier | four-diode bridge and filter capacitor |

Regenerate the files after changing the project builders:

```sh
cargo run --release -- generate-examples
```

`gerbers/` contains a real export of the routed demo board (top/bottom copper,
outline, silkscreen and drill), regenerated with:

```sh
cargo run --release -- export examples/gerbers
```

The demo board autoroutes with zero DRC errors on the bundled rules. The example
files are generated deterministically; no timestamps or machine-specific data
are written.
