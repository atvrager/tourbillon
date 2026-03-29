# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Project Overview

Tourbillon (`tbn`) is a queue-centric hardware description language implemented in Rust. It compiles `.tbn` source files to synthesisable SystemVerilog. The full language specification lives in `TOURBILLON.md` — read it before making design decisions.

**Status:** Phase 0 complete. Phase 1 complete. Phase 2 in progress: lowerer completeness done (struct packed, enum, tuple destruct, variant match, array update, try_take wiring, Memory primitive); RV32I `.tbn` design compiles to Verilator-clean SV. `TOURBILLON.md` is the authoritative specification.

## Setup

After cloning, activate the git hooks:
```
git config core.hooksPath .githooks
```

## Build Commands

```
cargo build                    # Build the tbn compiler
cargo test                     # Run all tests
cargo test <test_name>         # Run a single test
cargo clippy                   # Lint
cargo fmt --check              # Check formatting
```

CLI usage:
```
tbn check <file.tbn>           # Type-check and deadlock analysis (no codegen)
tbn build <file.tbn> -o <dir>  # Compile to SystemVerilog (.sv files)
tbn status <file.tbn>          # Show provenance hash and cache status
tbn clean                      # Remove build cache (~/.tbn/store/)
```

## Git Hooks

Enforced via `.githooks/` + `core.hooksPath`:
- **pre-commit:** `cargo fmt --check` + `cargo clippy -- -D warnings`
- **pre-push:** `cargo test`

## Architecture

### Language Primitives

Tourbillon has exactly three constructs:

- **Queue(T, depth=N)** — the only sequential storage element. Bounded, typed, synchronous FIFO. Supports `take()`, `put()`, `try_take()`, and `peek()`.
- **Cell(T)** — syntactic sugar for depth-1 self-queue with linear-type discipline. Supports `peek()` for non-consuming concurrent reads. Compiles to a register.
- **Process** — guarded combinational logic with rules that consume/produce queue tokens. Rules fire atomically. Port kinds: `consumes:`, `produces:`, `state:`, `peeks:`.
- **Pipe** — structural composition (wiring only, no logic). Single-writer single-reader invariant at IR level.
- **Memory(K → V, depth, latency)** — addressable storage. Desugars to request/response queues. Compiler maps to vendor BRAM/SRAM.
- **Multi-producer arbitration** — `priority = [...]` or `arbitration = round_robin` on queues. Desugars to compiler-generated arbiter process.
- **Async sources** — `source = async` annotation for external inputs (interrupts, bus interfaces). Compiler generates synchroniser.

### Compiler Pipeline (7 stages)

1. **Parse** → CST (chumsky)
2. **Desugar** → Cells to depth-1 Queues; pattern match to decision trees
3. **Type Check** → Hindley-Milner + linear-type discipline on Cells (peek exempt from linearity)
4. **Elaborate** → Resolve pipes, flatten hierarchy, build process network graph
5. **Schedule** → Rule priority assignment, conflict detection, shared-Cell arbitration
6. **Lower** → Process network → structural SystemVerilog (Queue→FIFO module, Cell→reg, rule→always_comb)
7. **Provenance** → BLAKE3 Merkle tree of sources, hash embedded in SV output

### Core IR

The central IR is a **process network graph** (`petgraph::DiGraph`):
- Nodes = `ProcessNode` (instance name, rules, resolved ports with edge bindings)
- Edges = `QueueEdge` (element type, depth, `Queue` or `Cell` kind)
- Cells = self-loop edges; shared Cells carry `peeker_instances` listing cross-instance readers
- One `ProcessNetwork` per pipe declaration; standalone process defs produce no graph

This graph enables deadlock analysis (Petri net / KPN capacity checks), rule conflict detection, and scheduling.

### Key Rust Crates

| Concern | Crate | Phase |
|---|---|---|
| Parsing | `chumsky` 1.0-alpha | 0 |
| Diagnostics | `ariadne` 0.5 | 0 |
| CLI | `clap` 4 (derive) | 0 |
| IR graph | `petgraph` 0.7 | 1 |
| SV emission | `std::fmt::Write` (direct string building) | 1 |
| Hashing | `blake3` | 1+ |
| Build cache | `cacache` | 1+ |

### Module Layout

```
src/
  main.rs            -- CLI (clap): tbn check / tbn build
  lib.rs             -- Pipeline: parse → desugar → type-check → elaborate → schedule → lower
  ast.rs             -- AST types (Spanned nodes, all language constructs)
  ir.rs              -- IR types: ProcessNetwork, ProcessNode, QueueEdge, ResolvedPort
  elaborate.rs       -- Elaboration pass: AST pipes → petgraph process networks
  schedule.rs        -- Rule priority assignment, conflict detection
  lower.rs           -- SV emitter: process network → SystemVerilog
  provenance.rs      -- BLAKE3 hashing, source manifest, cache helpers
  parse/
    mod.rs           -- Orchestrates lexer → parser, converts errors
    token.rs         -- Token enum (keywords, operators, punctuation)
    lexer.rs         -- Chumsky character-level lexer → token stream
    parser.rs        -- Chumsky token-level parser → AST
  desugar.rs         -- MethodCall → Take/TryTake/Peek/Put resolution
  types/
    mod.rs           -- Orchestration: collect type defs, check processes/pipes
    ty.rs            -- Internal type representation (Bits, Bool, Tuple, Record, ...)
    env.rs           -- Type environment: scoped name → type mapping
    check.rs         -- Expression/statement type inference
    linearity.rs     -- Cell take/put discipline per rule
  diagnostics.rs     -- Error/warning types + ariadne rendering
tests/
  smoke.rs           -- Basic end-to-end tests
  process.rs         -- Process + rule integration tests
  linearity.rs       -- Cell linearity error tests
  elaborate.rs       -- Elaboration integration tests
  schedule.rs        -- Schedule integration tests
  lower.rs           -- Lowering / SV codegen integration tests
  provenance.rs      -- Provenance hashing and embedding tests
examples/
  rv32i.tbn          -- RV32I reference core (4-stage pipeline)
  counter.tbn        -- Simple counter (Cell + take/put)
  producer_consumer.tbn -- Queue producer/consumer
  branch.tbn         -- Conditional routing
  peek.tbn           -- Cross-instance Cell peek
  priority.tbn       -- Multi-rule priority suppression
sim/
  rv32i_pkg.sv       -- Hand-written RV32I decode/ALU/branch SV package
  tb_top.sv          -- Verilator simulation top-level wrapper
  tb_cpu.cpp         -- Verilator C++ testbench driver
  Makefile           -- Simulation build system
  tests/smoke.S      -- Minimal RV32I smoke test assembly
```

### Provenance System

Content-addressed builds using BLAKE3 Merkle trees. The source root hash is embedded in generated SystemVerilog as both a comment and a synthesisable `localparam`. Build cache lives at `~/.tbn/store/<hash>/`.

## Design Principles

- The DFF is a degenerate queue. Users never see "register" — they see queues they borrow from and return to.
- Cell linearity: within a rule, if you `take()` a Cell you **must** `put()` exactly once on every control-flow path.
- Generated SystemVerilog should be flat, boring, and structurally regular — it's a compilation target.
- Full FIFO emission for now — register collapse is a future optimisation pass. Do not implement collapse until explicitly requested.
- Multi-producer syntax is sugar; the IR always has single-writer single-reader queues with arbiter process nodes inserted by the compiler.
- `peek()` sees the old value when concurrent with `take()`/`put()` in the same cycle (read port sees pre-write-back state).

## Key Semantic Rules

- Multi-producer queues **require** explicit `priority` or `arbitration` annotation. No defaults — unannotated multi-producer is a compile error.
- `peek()` always sees the **old value** (pre-write-back state) when concurrent with `take()`/`put()` in the same cycle.
- `Memory` has separate read/write port queues (`read_req`, `read_resp`, `write_req`). Writes are fire-and-forget. Maps to dual-port BRAM.
