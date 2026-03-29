# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Project Overview

Tourbillon (`tbn`) is a queue-centric hardware description language implemented in Rust. It compiles `.tbn` source files to synthesisable SystemVerilog. The full language specification lives in `TOURBILLON.md` — read it before making design decisions.

**Status:** Marie Antoinette SoC complete — Hello World prints via physical UART TX pin through 3-clock-domain bus fabric. Phase 0–3 complete. rv32ui compliance: 38/38 tests pass. Compiler features: `const` (localparam), `expr[hi:lo]` (bit slicing), `external fn` (DPI), `external Queue` (module port pins), pipe hierarchy with cross-pipe queue wiring, `tbn wave` (FST trace reader), width-aware SV emission. CDC: AsyncQueue gray-code FIFO, domain annotations, cross-domain validation. Split-phase processes for CDC-tolerant bus fabric. Conditional put/take guard analysis (can_fire only requires unconditional operations). UART: real bit-serial TX/RX/RTS/CTS in Tourbillon (UartTx, UartRx, UartDevice, UartPhy pipe). FPGA: Xilinx VU+ toplevel with MMCME4 clkgen (100/150/50 MHz), rst_sync, standalone BRAM mode. `TOURBILLON.md` is the authoritative specification.

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
tbn check <file.tbn>           # Type-check, deadlock analysis (no codegen)
tbn build <file.tbn> -o <dir>  # Compile to SystemVerilog (.sv files)
tbn graph <file.tbn>           # Emit process network as Graphviz DOT
tbn wave <file.fst>            # Read FST waveform trace (Verilator debug)
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
- **AsyncQueue(T, depth=N)** — clock domain crossing FIFO. Gray-code async FIFO in SV. Depth must be power of 2. Pipe-level `domain <name> : Clock` declarations, instances annotated with `[domain_name]`. Compiler enforces: sync Queue cannot cross domains, AsyncQueue must cross domains, Cell peek cannot cross domains.
- **Async sources** — `source = async` annotation for external inputs (interrupts, bus interfaces). Compiler generates synchroniser.
- **Constants** — `const NAME = value`. Named compile-time integer constants. Emitted as `localparam` in SV, inlined as literal values in expressions.
- **Bit slicing** — `expr[hi:lo]`. Extract bits from a `Bits N` value. Result type is `Bits(hi - lo + 1)`. Parser disambiguates from array index via `[int:int]` look-ahead.
- **External functions** — `external fn name(params) [-> RetTy]`. DPI function declarations. Emitted as `import "DPI-C"` in SV. Call-site type checking against registered signatures.
- **External queues** — `let pin = external Queue(Bits 1, depth = 1)`. Queue becomes a module port (no FIFO instantiated). Direction inferred from port bindings. Works through pipe hierarchy. Used for physical pin interfaces (UART TX/RX/RTS/CTS).
- **Pipe hierarchy** — Pipes can instantiate other pipes. Child pipe's process network is recursively elaborated and merged into parent. Cross-pipe queue wiring via bindings. Dangling queue endpoints become pipe ports.

### Compiler Pipeline (8 stages)

1. **Parse** → CST (chumsky)
2. **Desugar** → Cells to depth-1 Queues; pattern match to decision trees
3. **Type Check** → Hindley-Milner + linear-type discipline on Cells (peek exempt from linearity)
4. **Elaborate** → Resolve pipes, flatten hierarchy, build process network graph
5. **Schedule** → Rule priority assignment, conflict detection, shared-Cell arbitration
6. **Deadlock** → SCC-based token check, KPN capacity analysis, try_take relaxation (warnings only)
7. **Lower** → Process network → structural SystemVerilog (Queue→FIFO module, Cell→reg, rule→always_comb)
8. **Provenance** → BLAKE3 Merkle tree of sources, hash embedded in SV output

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
| FST traces | `fst-reader` 0.15 | debug |

### Module Layout

```
src/
  main.rs            -- CLI (clap): tbn check / tbn build / tbn graph / tbn wave
  lib.rs             -- Pipeline: parse → desugar → type-check → elaborate → schedule → lower
  ast.rs             -- AST types (Spanned nodes, all language constructs)
  ir.rs              -- IR types: ProcessNetwork, ProcessNode (is_memory_stub), QueueEdge, ResolvedPort
  elaborate.rs       -- Elaboration pass: AST pipes → petgraph process networks
  schedule.rs        -- Rule priority assignment, conflict detection, try_take classification
  deadlock.rs        -- Static deadlock analysis: SCC token check, KPN capacity, try_take relaxation
  graph.rs           -- DOT graph export for process networks
  lower.rs           -- SV emitter: process network → SystemVerilog
  provenance.rs      -- BLAKE3 hashing, source manifest, cache helpers
  parse/
    mod.rs           -- Orchestrates lexer → parser, converts errors
    token.rs         -- Token enum (keywords, operators, punctuation)
    lexer.rs         -- Chumsky character-level lexer → token stream
    parser.rs        -- Chumsky token-level parser → AST
  desugar.rs         -- MethodCall → Take/TryTake/Peek/Put; indexed cell port expansion
  types/
    mod.rs           -- Orchestration: collect type defs, check processes/pipes
    ty.rs            -- Internal type representation (Bits, Bool, Tuple, Record, ...)
    env.rs           -- Type environment: scoped name → type mapping
    check.rs         -- Expression/statement type inference
    linearity.rs     -- Cell take/put discipline per rule
  diagnostics.rs     -- Error/warning types + ariadne rendering
  wave.rs            -- FST waveform trace reader (fst-reader crate)
tests/
  smoke.rs           -- Basic end-to-end tests
  process.rs         -- Process + rule integration tests
  linearity.rs       -- Cell linearity error tests
  elaborate.rs       -- Elaboration integration tests
  schedule.rs        -- Schedule integration tests
  lower.rs           -- Lowering / SV codegen integration tests
  provenance.rs      -- Provenance hashing and embedding tests
  deadlock.rs        -- Deadlock analysis + DOT graph integration tests
  async_queue.rs     -- AsyncQueue CDC: parsing, elaboration, lowering, DOT tests
  constants.rs       -- Const declaration tests
  bit_slice.rs       -- Bit slice expression tests
  external_fn.rs     -- External function / DPI tests
  external_queue.rs  -- External queue / module port pin tests
  pipe_hierarchy.rs  -- Pipe-in-pipe instantiation + cross-pipe wiring tests
  riscv_tests.rs     -- rv32ui compliance: build sim + run 38 tests via Verilator
examples/
  rv32i.tbn          -- RV32I reference core (4-stage pipeline)
  async_example.tbn  -- Two-domain CDC producer-consumer (AsyncQueue)
  counter.tbn        -- Simple counter (Cell + take/put)
  producer_consumer.tbn -- Queue producer/consumer
  branch.tbn         -- Conditional routing
  peek.tbn           -- Cross-instance Cell peek
  priority.tbn       -- Multi-rule priority suppression
  marie.tbn          -- Marie Antoinette SoC: 3-domain CPU + bus fabric + UART
sim/
  rv32i_pkg.sv       -- Hand-written RV32I decode/ALU/branch SV package (compute_result, load_extend, store_merge)
  mem_model.sv       -- Behavioral SRAM with ready/valid interface (combinational read, verilator public)
  tb_top.sv          -- Verilator simulation top-level: CPU + memory models + tohost monitor
  tb_cpu.cpp         -- Verilator C++ testbench driver with ELF loader and FST trace
  soc_top.sv         -- Multi-clock SoC testbench wrapper: Marie + memory models + UART DPI bridge
  soc_tb.cpp         -- 3-domain Verilator driver with ELF loader, multi-rate clocks (100/150/50 MHz)
  Makefile           -- Simulation build system (sv, build, test-hex, riscv-tests, soc-*, rtl-export)
  verilog2hex.py     -- Convert objcopy Verilog hex to $readmemh format (rebase addresses)
  env/p/riscv_test.h -- Custom no-CSR riscv-tests environment for Tourbillon CPU
  build_tests.sh     -- Builds rv32ui tests from riscv-tests submodule with custom env
  tests/smoke.S      -- Minimal RV32I smoke test assembly
  tests/smoke.hex    -- Hand-encoded smoke test (hex, no toolchain needed)
  tests/hello.S      -- "Hello, World!" SoC test (UART TX at 3 MBaud)
rtl/
  marie_top.sv       -- Xilinx VU+ FPGA toplevel: MMCME4 clkgen, rst_sync, STANDALONE param
  fpga_mem.sv        -- Synthesisable memory: distributed RAM (imem) or block RAM (dmem)
  xilinx_stubs.sv    -- IBUFDS/MMCME4_ADV/BUFGCE stubs for Verilator lint
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
