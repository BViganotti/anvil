# Security

Anvil is a local tool that executes no user-supplied code and has no network
access at runtime. Its threat model is a malicious web page in the same browser
or a malformed project/API payload, not a remote attacker.

## Network

- The server binds explicitly to `127.0.0.1` (IPv4 loopback). There is no option
  to bind a public interface.
- A random 128-bit token is generated per launch from OS entropy mixed with time,
  process id and a stack address. Hex tokens are compared in constant time.
- Every `/api` request requires `X-Anvil-Token`. SSE uses a `token` query
  parameter because the EventSource API cannot set headers.
- Static assets are public but contain no project data.
- There is no TLS; the loopback interface is the trust boundary.
- `open_browser` invokes the platform opener with the tokenized loopback URL.

## Browser

- The page has a strict Content Security Policy: same-origin scripts, styles and
  images, plus `blob:` for generated previews. No frames, forms or remote origins.
- All assets are embedded in the binary; there are no external fonts, scripts,
  images or CDN references.
- Hostile strings from project data are rendered with HTML escaping in list and
  diagnostic panels, and Canvas text APIs for the drawing.
- No `eval`, no dynamic script injection, no `innerHTML` of unescaped user text.

## Parsing and limits

- JSON: 4 MB input, depth 64, duplicate keys rejected, non-finite numbers
  rejected, 1 MB strings, bounded arrays/objects.
- HTTP: 32 KB headers, 4 MB body, 100 headers, no chunked encoding, path/query
  length limits, percent-decoding errors rejected.
- Gerber: 16 MB input, 1,000,000 operations, 10,000 apertures, arcs/regions/
  macros/step-and-repeat/clear polarity rejected.
- Drill: 16 MB, 10,000 tools, 500,000 hits, ambiguous fixed-format integers
  rejected.
- Schematic/PCB: bounded components, wires, points, labels, buses, pads, traces,
  vias and coordinate range.
- Simulation: at most 512 nodes, 4,000 elements, 2,000,000 steps; matrices capped.
- Raster: maximum 3000 px per dimension; cell/operation limits in DRC grid.

## Panic containment

- All parsers and editors return `Result`; no `unwrap` on untrusted data.
- DRC/router use bounded loops and integer arithmetic with widening.
- The autoroute worker is wrapped in `catch_unwind`; a panic is reported as a job
  error and the server remains alive.
- Mutex poisoning is recovered rather than panicking.
- The fuzz harness (2,000 cases) and the 1,000-case acceptance fuzz require zero
  panics.

## What Anvil does not secure

- Project files are trusted once loaded; they are not sandboxed.
- The local user can read the token from the process arguments/URL.
- Anyone who can connect to loopback and obtain the token can edit the in-memory
  project. There are no accounts or authorization levels.
- Browser extensions and local malware are outside the threat model.
