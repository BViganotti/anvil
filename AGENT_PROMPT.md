# Anvil — One-Shot Agent Prompt

You are the sole engineer for this repository, `anvil`. Build it in ONE
continuous run. Do not ask questions. Do not stop after planning, after the first
milestone, or after the first working demo. Continue until the Definition of Done
in `REQUIREMENTS.md` is true.

Before coding, read `PLAN.md` and `REQUIREMENTS.md` completely. Then start
implementing immediately. Do not stop after writing files.

Mission: Anvil is a local browser EDA workbench: draw a schematic, simulate it,
lay out a board, run DRC, autoroute every trace, and export Gerber/drill files.
It is not a database, not a CRDT, not crash analysis, not observability, and not
a CLI.

Hard rules:
- Rust stable, Cargo workspace. No external crates, no internet at runtime, no
  CDN, no external fonts/assets, no vendored code.
- Rust standard library only.
- UI: embedded local HTTP/SSE server on 127.0.0.1 with original HTML/CSS/JS
  compiled into the binary; auto-open the browser. No terminal interaction.
- Soundness over speed. Bounded memory, no panics on malformed input.
- Target 15–18k LOC. No filler.
- At least 90 commits, target 100+. Every commit builds and passes tests.
- Commit every vertical slice. Push every 3–5 commits if a remote exists. If
  `gh` is authenticated, create PUBLIC `BViganotti/anvil` (or `anvil-eda` if
  taken) and push; otherwise write PUSH.md.
- NO CI/CD. Do not create `.github/workflows`.
- README, MIT LICENSE, CHANGELOG, examples, tests, fuzz, benchmark, docs.
- If blocked, ship the smallest sound fallback, document it in LIMITATIONS.md,
  and continue. Never fake DRC or simulation results.

Follow the milestones in `PLAN.md` exactly in order. Every milestone is a set of
vertical slices, not a design phase. Write tests alongside code. Keep the build
green. At the end write FINAL_REPORT.md with LOC, commits, tests, coverage,
benchmarks and known gaps.

Continue until every item in the Definition of Done is true. Do not stop early.
