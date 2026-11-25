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
| `resistor-ladder.json` | Resistor ladder | three-stage attenuator taps |
| `current-bias.json` | Current bias | current source into parallel resistors |
| `parallel-rc.json` | Parallel RC | two-resistor charging network |
| `diode-clamp.json` | Diode clamp | diode limiting a sine to a 3 V rail |
| `half-adder.json` | Half adder | XOR sum and AND carry from pulse inputs |
| `dff-divider.json` | DFF divider | flip-flop with QN feedback divides by two |
| `led-series.json` | LED series | two diodes and a shared resistor |
| `inductor-filter.json` | Inductor filter | RLC low-pass response |
| `rc-differentiator.json` | RC differentiator | capacitive edge response |
| `diode-peak-detector.json` | Diode peak detector | diode and capacitor hold the peak |
| `rl-step.json` | RL step | series inductor current step |
| `switch-divider.json` | Switch divider | ideal switch changes the divider |
| `three-source-superposition.json` | Three-source node | Millman/superposition with three sources |
| `rc-decay.json` | RC decay | capacitor initial-condition discharge |
| `parallel-inductors.json` | Parallel inductors | coupled load transient with two inductors |

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
