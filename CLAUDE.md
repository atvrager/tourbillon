# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Project Overview

Tourbillon (`tbn`) is a queue-centric hardware description language implemented in Rust. It compiles `.tbn` source files to synthesisable SystemVerilog. The full language specification lives in `TOURBILLON.md` — read it before making design decisions.

**Status:** Pre-implementation. `TOURBILLON.md` is the authoritative specification. No compiler code exists yet.

## Planned Build Commands

Once the Rust project is scaffolded:

```
cargo build                    # Build the tbn compiler
cargo test                     # Run all tests
cargo test <test_name>         # Run a single test
cargo clippy                   # Lint
cargo fmt --check              # Check formatting
```

The compiler CLI (once built):
```
tbn build [--target <fpga>]    # Compile .tbn → SystemVerilog
tbn check                      # Type-check and deadlock analysis (no codegen)
tbn graph                      # Emit process network as DOT/Mermaid
```

## Architecture

### Language Primitives

Tourbillon has exactly three constructs:

- **Queue(T, depth=N)** — the only sequential storage element. Bounded, typed, synchronous FIFO.
- **Cell(T)** — syntactic sugar for depth-1 self-queue with linear-type discipline. Compiles to a register.
- **Process** — guarded combinational logic with rules that consume/produce queue tokens. Rules fire atomically.
- **Pipe** — structural composition (wiring only, no logic). Single-writer single-reader enforcement on queues.

### Compiler Pipeline (7 stages)

1. **Parse** → CST (winnow or chumsky)
2. **Desugar** → Cells to depth-1 Queues; pattern match to decision trees
3. **Type Check** → Hindley-Milner + linear-type discipline on Cells
4. **Elaborate** → Resolve pipes, flatten hierarchy, build process network graph
5. **Schedule** → Rule priority assignment, conflict detection, shared-Cell arbitration
6. **Lower** → Process network → structural SystemVerilog (Queue→FIFO module, Cell→reg, rule→always_comb)
7. **Provenance** → BLAKE3 Merkle tree of sources, hash embedded in SV output

### Core IR

The central IR is a **process network graph** (planned via `petgraph`):
- Nodes = Processes (containing rules as combinational expression trees)
- Edges = Queues (annotated with element type and depth)
- Cells = self-loop edges; shared Cells have multi-process access annotations

This graph enables deadlock analysis (Petri net / KPN capacity checks), rule conflict detection, and scheduling.

### Key Rust Crates (Planned)

| Concern | Crate |
|---|---|
| Parsing | `winnow` or `chumsky` |
| IR graph | `petgraph` |
| SV emission | `askama` or direct `Write` |
| Hashing | `blake3` |
| Build cache | `cacache` |
| CLI | `clap` |

### Provenance System

Content-addressed builds using BLAKE3 Merkle trees. The source root hash is embedded in generated SystemVerilog as both a comment and a synthesisable `localparam`. Build cache lives at `~/.tbn/store/<hash>/`.

## Design Principles

- The DFF is a degenerate queue. Users never see "register" — they see queues they borrow from and return to.
- Cell linearity: within a rule, if you `take()` a Cell you **must** `put()` exactly once on every control-flow path.
- Generated SystemVerilog should be flat, boring, and structurally regular — it's a compilation target.
- The compiler should aggressively collapse depth-1 queues with no contention into bare registers as an optimisation pass (semantic model stays queues; silicon is DFFs where safe).

## Open Design Questions

See `TOURBILLON.md` §8 for unresolved questions around shared Cell scheduling semantics, memory interface latency modeling, interrupt handling, and area overhead.
