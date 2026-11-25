# Simulation

Anvil implements a deliberately bounded circuit simulator. It is not a full
SPICE; it is a real MNA engine with analytic tests for every supported device
class.

## Supported devices

| Device | Model |
|---|---|
| Resistor | ideal `R > 0` |
| Capacitor | ideal, optional initial voltage |
| Inductor | ideal, optional initial current |
| Voltage source | ideal, DC / pulse / sine / PWL |
| Current source | ideal, DC / pulse / sine / PWL |
| Diode | Shockley exponential with emission coefficient and series resistance |
| Switch | ideal with `ron` / `roff` and static state |
| Logic | AND, OR, NAND, NOR, XOR, NOT, D flip-flop |
| Ground | node index 0, named `GND`, `0`, `VSS`, `AGND`, `DGND`, `GNDA`, `GNDD` |

Anything else is rejected at compile time with the component refdes. There is no
silent approximation of unsupported devices.

## Equations

Unknowns are node voltages (ground eliminated) plus branch currents for voltage
sources and inductors. The matrix is dense and solved by LU factorisation with
partial pivoting. A minimum conductance of 1e-15 S to ground keeps floating
nodes solvable without visibly perturbing textbook circuits.

### Diode companion

For junction voltage `vj`, `i = Is (exp(vj/nVt) - 1) + gmin vj` and
`g = Is exp(vj/nVt)/nVt + gmin`. With series resistance `Rs`, the terminal
companion is `geff = g/(1 + g Rs)` and `veq = vj - i/g`. Anvil stamps `geff`
between anode and cathode and an equivalent current of `geff * veq`.

Newton iteration limits junction updates to keep the exponential bounded and
converges on both node voltage and junction voltage changes. Reverse bias below
-20 V clamps the junction voltage; the resulting current error is far below the
0.1% acceptance budget for the documented model.

## Analyses

### DC operating point

Capacitors are open, inductors are shorts, sources use their DC value, and
combinational digital gates settle in a loop of at most 50 rounds. A circuit
whose digital feedback never settles returns `NotConverged`; it does not invent
a result.

### Transient

Backward Euler or trapezoidal integration with a fixed maximum time step. Each
reactive element carries a companion history term. The initial capacitor voltage
is the DC value unless an `ic` property is set; inductor current comes from the
DC branch current unless `ic` is set. Gate delays are honoured by an event queue:
the integrator clamps its step to the next scheduled digital event.

Digital outputs are Thevenin drivers (`vdd`, `rout`) to ground. Inputs are
sampled at `vdd/2`. The D flip-flop samples D on a rising clock edge and drives Q
and QN with complementary levels.

## Accuracy evidence

`cargo test --test acceptance` checks:

- 500 random resistive/current-source networks against an independently written
  Gaussian-elimination MNA reference solver.
- 12 voltage dividers, 8 parallel networks, 8 Millman/superposition networks,
  4 diodes against an independent bisection solve, and 8 RC transients against
  the closed-form exponential — 40 analytic circuits.
- DFF clocking, gate transients, RL, switch, reverse diode, Euler and waveform
  unit tests.

The acceptance tolerance for the 40 analytic circuits is 0.1%; most checks are
far tighter (1e-9 relative for linear DC, 1e-6 for diodes).

## Netlist compilation

Symbols map by pin name:

- `R/C/L`: pins `1`, `2`
- `V/I`: pins `+`, `-`
- `D/LED`: pins `A`, `K`
- `SW`: pins `1`, `2`
- gates: `A`, `B`, `Y`; NOT `A`, `Y`; DFF `D`, `CLK`, `Q`, `QN`

Values use engineering notation (`4.7k`, `100n`, `2.2u`). Source waveforms use
properties: `wave=pulse`, `v1`, `v2`, `delay`, `rise`, `width`, `period`;
`wave=sin`, `amp`, `freq`, `offset`, `delay`, `phase`; or `wave=pwl` with
`t:v,...` pairs. Digital properties: `delay`, `vdd`, `rout`, `initial`.

One ground net is required. Multiple ground nets are an error rather than a
guess. Power symbols (`GND`, `VCC`) are connective; they generate no device.

## Deliberate limits

- No MOSFETs, BJTs, op-amps, transformers, transmission lines, noise, AC sweep,
  Monte Carlo, temperature or self-heating.
- Ideal switches are static; there is no voltage-controlled switch model.
- Digital delays are real but quantised by the integrator's maximum time step
  when a delay is shorter than that step.
- No hierarchy: the model is one flat sheet.
- Recording is capped and decimates long runs (20k samples per series, 64 nodes).
