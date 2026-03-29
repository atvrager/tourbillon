# Tourbillon

**A queue-centric hardware description language with content-addressed provenance.**

*"Everything in its proper order."*

---

## 1. Thesis

The fundamental sequential primitive of digital hardware should be the **FIFO**, not the D flip-flop. The DFF is a degenerate queue — depth-1, no backpressure, no flow control — and the entire catalogue of pipeline hazards, stall logic, and valid/ready protocol bugs is the cost of choosing the wrong abstraction and then bolting the right one back on top.

Tourbillon is a domain-specific language, implemented in Rust, that enforces this discipline structurally. You cannot instantiate a register. You can only declare queues, write processes that consume and produce tokens, and compose them into pipelines. The compiler lowers to synthesisable SystemVerilog and carries a content-addressed provenance chain from source through synthesis to running silicon.

The name comes from Breguet's tourbillon escapement — a rotating cage mechanism in precision chronometers that cancels gravitational error by averaging it out over a full rotation. Like the escapement, Tourbillon meters the flow of data in discrete, controlled steps. CLI shorthand: **`tbn`**.

---

## 2. Core Language

### 2.1 The Three Primitives

Tourbillon programs consist of exactly three constructs:

#### Queue

The **only** sequential storage element.

```
let fetch_q = Queue(Addr × Word, depth = 2)
```

A `Queue(T, depth = N)` holds up to `N` tokens of type `T`. It exposes two operations:

| Operation     | Semantics                                | Blocks when      |
|---------------|------------------------------------------|-------------------|
| `q.put(val)`  | Enqueue `val` at tail                    | Queue full        |
| `q.take()`    | Dequeue and return head                  | Queue empty       |
| `q.try_take()`| Non-blocking variant, returns `Option(T)`| Never (returns `None`) |
| `q.peek()`    | Read head without dequeuing, returns `Option(T)` | Never (returns `None`) |

`peek()` is a non-consuming read — it observes `deq_data` and `deq_valid` without asserting `deq_ready`. It carries no linearity obligation and may execute concurrently with other operations on the same queue.

Queues are **typed**, **bounded**, and **synchronous** (one put and one take may occur per clock cycle per queue).

#### Cell

Syntactic sugar for persistent state with queue semantics:

```
state: pc : Cell(Addr, init = 0x8000_0000)
```

A `Cell(T)` desugars to a depth-1 self-queue. Within a single rule, you **must** `take()` before `put()`, and you **must** `put()` exactly once if you `take()`. The compiler enforces this via a linear-type discipline — a Cell is a borrowed resource.

A Cell also supports `peek()` — a non-consuming read that observes the current value without borrowing it. `peek()` carries no linearity obligation and may execute concurrently with a `take()`/`put()` pair from another rule. This enables the common pattern of reading state (e.g. a register file for operand fetch) while another process mutates it (e.g. writeback).

**Peek consistency:** when `peek()` and `take()`/`put()` occur on the same Cell in the same cycle, `peek()` always sees the **old value** (pre-write-back state). This matches hardware register file semantics — the read port returns the value from the previous clock edge. If forwarding is needed, the designer wires it explicitly through a queue.

At the SystemVerilog level, a Cell compiles to a register. But the programmer never sees the word "register" — they see a queue they borrow from and return to.

#### Process

A block of guarded combinational logic that consumes from input queues and produces to output queues.

```
process Fetch {
  consumes: redirect  : Queue(Addr, depth = 1)
  produces: fetch_out : Queue(Addr × Word)
  state:    pc        : Cell(Addr, init = 0x8000_0000)

  rule tick {
    -- body: sequential-looking code
    -- compiles to combinational logic + enable signals
  }
}
```

A process may contain one or more **rules**. A rule fires atomically when all its guard conditions are satisfied (all input queues it reads from are non-empty; all output queues it writes to are not full). Rules within a process are mutually exclusive per cycle unless the compiler can prove they don't conflict.

#### Pipe

Structural composition. No logic — just wiring.

```
pipe CPU {
  let fetch_q  = Queue(Addr × Word, depth = 2)
  let decode_q = Queue(Decoded, depth = 2)
  -- ...
  Fetch    { fetch_out = fetch_q, redirect = redir_q }
  Decode   { fetch_in  = fetch_q, decode_out = decode_q }
}
```

A pipe declares queues and binds them to process ports. The compiler checks that every queue has exactly one producer and one consumer (single-writer, single-reader), and that the element types match.

#### Multi-Producer Arbitration

The single-writer, single-reader invariant holds at the IR level. When multiple processes need to write to the same logical queue (e.g. a Common Data Bus in an out-of-order processor), the programmer annotates a multi-producer binding and the compiler desugars it into an arbiter process:

```
let cdb = Queue(Result, depth = 1, priority = [load_unit, alu, fp_unit])
```

Or with fair arbitration:

```
let cdb = Queue(Result, depth = 1, arbitration = round_robin)
```

The compiler generates an arbiter process node that consumes from N per-producer queues and produces to the single output queue. The IR remains single-writer, single-reader throughout — the sugar is structural, not semantic.

The compiler **requires** an explicit `priority` or `arbitration` annotation when multiple producers target one queue. There is no default policy — unannotated multi-producer queues are a compile error. Silent priority choices are a bug factory.

#### Memory

A built-in primitive for addressable storage with latency:

```
let imem = Memory(Addr → Word, depth = 4096, latency = 1)
```

`Memory(K → V, depth, latency)` desugars to separate read and write port queues, plus an internal process modeling read latency:

```
-- Given:
let imem = Memory(Addr → Word, depth = 4096, latency = 1)

-- Desugars to:
-- imem.read_req  : Queue(Addr)         -- send read address
-- imem.read_resp : Queue(Word)         -- receive read data (after `latency` cycles)
-- imem.write_req : Queue(Addr × Word)  -- send write address + data (fire-and-forget)
```

Separate read/write ports map directly to dual-port BRAM (port A reads, port B writes). Reads and writes may occur in the same cycle. Writes are fire-and-forget — no response token.

The `latency` parameter informs the deadlock analyser. During SV lowering, the compiler may map `Memory` nodes to vendor BRAM/SRAM primitives (Xilinx, Intel) instead of emitting FIFO-wrapped logic.

#### Asynchronous External Sources

Queues whose producer lives outside the Tourbillon process network (e.g. interrupt lines, external bus interfaces) are annotated with `source = async`:

```
external irq : Queue(IrqId, depth = 1, source = async)
```

The compiler generates a clock-domain synchroniser (two-FF by default) in the SV output. From the process network's perspective, an async queue is a non-deterministic token source — the deadlock analyser treats it as "tokens may or may not appear."

Priority encoding across multiple asynchronous sources uses the same arbiter desugaring:

```
let irq_mux = Queue(IrqId, depth = 1, priority = [nmi, timer, external])
```

### 2.2 Type System

#### Data Types

Standard synthesisable types, nothing exotic:

```
type Word   = Bits 32
type Addr   = Bits 32
type RegIdx = Bits 5

record Decoded {
  op      : AluOp
  rd      : RegIdx
  rs1_val : Word
  rs2_val : Word
  imm     : Word
  pc      : Addr
  mem     : MemOp
  wb      : Bool
}

enum MemOp = Load | Store | None
enum AluOp = Add | Sub | Sll | Slt | ...
```

Product types (records/tuples) and sum types (enums) are first-class. Pattern matching is exhaustive and checked at compile time.

#### Linearity on Cells

The type checker enforces that within any rule:

1. If `cell.take()` is called, `cell.put(val)` must be called exactly once on every control-flow path.
2. The taken value has an **affine** type — it may be used, but it must eventually be "returned" (put back or transformed and put into the same cell).

This prevents the two most common register-level bugs: forgetting to update state, and accidentally overwriting state with stale values.

```
rule crack {
  let regs = regfile.take()
  -- ... use regs ...
  -- COMPILE ERROR if you forget this line:
  regfile.put(regs)
}
```

#### Queue Protocol Types (Stretch Goal)

The aspiration is session types on queue interfaces:

```
-- "This queue carries at most one token per cycle"
redirect : Queue(Addr, depth = 1, protocol = Optional)

-- "This queue carries exactly one token per firing"
fetch_out : Queue(Addr × Word, protocol = Exactly(1))
```

The compiler would verify that `redirect.put()` is called at most once per rule firing, structurally. This connects to the session-type and process-algebra literature: each queue port is a channel with a typed protocol, and the compiler checks conformance.

#### Static Deadlock Analysis

Because all queues are bounded and the connectivity graph is known at compile time, the compiler can perform a static check for deadlock:

- Build the process network graph.
- For cyclic dependencies, verify that the sum of initial tokens in the cycle exceeds zero (ensuring at least one process can fire), or that queue depths are sufficient to prevent circular full-blocking.
- Conservative but sound: may reject some valid designs, never accept a deadlocking one.

This maps directly onto the capacity analysis from Kahn Process Network theory, and onto the marking-based liveness checks from Petri net analysis.

---

## 3. The RV32I Reference Design

A complete in-order RV32I core expressed in Tourbillon. This is both the language's test case and its tutorial.

```
-- ============================================
-- FETCH
-- ============================================
process Fetch {
  consumes: redirect   : Queue(Addr, depth = 1)
  produces: fetch_out  : Queue(Addr × Word)
  state:    pc         : Cell(Addr, init = 0x8000_0000)

  rule tick {
    let current_pc = pc.take()
    let instr = imem_read(current_pc)
    fetch_out.put((current_pc, instr))
    match redirect.try_take() {
      Some(target) => pc.put(target)
      None         => pc.put(current_pc + 4)
    }
  }
}

-- ============================================
-- DECODE
-- ============================================
process Decode {
  consumes: fetch_in   : Queue(Addr × Word)
  produces: decode_out : Queue(Decoded)
  peeks:    regfile    : Cell(Array(32, Word))

  rule crack {
    let (pc, raw) = fetch_in.take()
    let regs = regfile.peek()
    let d = decode_rv32i(raw)
    decode_out.put(Decoded {
      op      = d.op,
      rd      = d.rd,
      rs1_val = regs[d.rs1],
      rs2_val = regs[d.rs2],
      imm     = d.imm,
      pc      = pc,
      mem     = d.mem,
      wb      = d.wb,
    })
  }
}

-- ============================================
-- EXECUTE
-- ============================================
process Execute {
  consumes: exec_in   : Queue(Decoded)
  produces: redirect  : Queue(Addr, depth = 1)
            writeback : Queue(RegIdx × Word)

  rule go {
    let d = exec_in.take()
    let alu_out = alu(d.op, d.rs1_val, d.rs2_val, d.imm)

    match d.mem {
      Load  => {
        let val = dmem_read(alu_out)
        if d.wb { writeback.put((d.rd, val)) }
      }
      Store => {
        dmem_write(alu_out, d.rs2_val)
      }
      None  => {
        if d.wb { writeback.put((d.rd, alu_out)) }
      }
    }

    if is_branch(d.op) && branch_taken(d.op, d.rs1_val, d.rs2_val) {
      redirect.put(alu_out)
    }
  }
}

-- ============================================
-- WRITEBACK
-- ============================================
process Writeback {
  consumes: wb_in   : Queue(RegIdx × Word)
  state:    regfile : Cell(Array(32, Word), init = zeroes)

  rule commit {
    let (rd, val) = wb_in.take()
    let regs = regfile.take()
    let regs' = if rd != 0 { regs[rd := val] } else { regs }
    regfile.put(regs')
  }
}

-- ============================================
-- TOP-LEVEL
-- ============================================
pipe CPU {
  let fetch_q  = Queue(Addr × Word, depth = 2)
  let decode_q = Queue(Decoded, depth = 2)
  let redir_q  = Queue(Addr, depth = 1)
  let wb_q     = Queue(RegIdx × Word, depth = 2)

  Fetch     { fetch_out = fetch_q, redirect = redir_q }
  Decode    { fetch_in  = fetch_q, decode_out = decode_q, regfile = Writeback.regfile }
  Execute   { exec_in   = decode_q, redirect = redir_q, writeback = wb_q }
  Writeback { wb_in     = wb_q }
}
```

### 3.1 What the Compiler Produces

Each construct lowers predictably:

| Tourbillon Construct | SystemVerilog Output |
|---|---|
| `Queue(T, depth=N)` | Parameterised FIFO module: `enq_valid`, `enq_ready`, `enq_data`, `deq_valid`, `deq_ready`, `deq_data` |
| `Cell(T)` | `reg [W-1:0] name` with read-before-write semantics |
| `rule` body | `always_comb` block computing next-state values, guarded by an `enable` signal that is the conjunction of all input-nonempty and output-not-full conditions |
| `pipe` wiring | Structural `module` instantiation with port connections |

The generated SystemVerilog is intentionally flat, boring, and structurally regular. It is a compilation target, not something a human should edit.

---

## 4. Content-Addressed Provenance

### 4.1 The Problem

"Is the bitstream on this FPGA built from the source code I'm looking at?"

This question is surprisingly hard to answer with timestamp-based build systems. Tourbillon answers it with **content-addressing**: every artifact is identified by a cryptographic hash of its inputs, and that hash propagates all the way to running silicon.

### 4.2 The Merkle Source Tree

On `tbn build`, the compiler:

1. Hashes every `.tbn` source file (BLAKE3, for speed).
2. Constructs a Merkle tree over the sorted set of source hashes.
3. The **root hash** is the canonical identity of this build.

```
source_root = BLAKE3(
  BLAKE3(cpu.tbn) ||
  BLAKE3(decode.tbn) ||
  BLAKE3(execute.tbn) ||
  BLAKE3(fetch.tbn) ||
  BLAKE3(writeback.tbn)
)
```

This root hash is embedded in the generated SystemVerilog in two forms:

1. **As a comment** (human-readable):
   ```systemverilog
   // Tourbillon provenance: 7a3fc012...
   ```

2. **As a synthesisable constant** (machine-readable):
   ```systemverilog
   localparam logic [255:0] TBN_PROVENANCE = 256'h7a3fc012...;
   ```

If the design includes a JTAG TAP or UART debug port, the provenance hash is exposed as a memory-mapped register. The compiler can optionally generate this debug shim.

### 4.3 The Build Cache

The build system is a content-addressed store, following Nix-store semantics:

```
~/.tbn/store/
  7a3fc012.../
    source_manifest.json   -- which files, their individual hashes
    cpu.sv                 -- generated SystemVerilog
    cpu.synth.json         -- synthesis metadata
```

If nothing changed, the hash is the same, and nothing rebuilds. If a single character in one source file changes, the root hash changes, and every downstream artifact is invalidated. There is no "I forgot to rebuild" — the hash either matches or it doesn't.

### 4.4 The Verify Command

```bash
$ tbn status

  source root          : 7a3f…c012
  last build           : 7a3f…c012  ✓ current
  fpga@/dev/ttyUSB0    : 2e91…bb08  ✗ STALE (3 sources changed)
```

```bash
$ tbn verify /dev/ttyUSB0

  Reading provenance register at 0xFFFF_FF00...
  FPGA reports    : 2e91…bb08
  Current source  : 7a3f…c012
  Status          : MISMATCH

  Changed since FPGA build:
    fetch.tbn      (modified)
    execute.tbn    (modified)
    decode.tbn     (modified)
```

`tbn verify` reads the provenance hash from the FPGA over JTAG or UART and compares it against the current source tree. It can also diff against the build cache to show exactly which files changed.

### 4.5 Extended Provenance Chain

The Merkle tree can be extended beyond source:

```
artifact_hash = BLAKE3(
  source_root          ||
  compiler_version     ||
  target_config_hash   ||
  synthesis_tool_hash  ||
  constraint_file_hash
)
```

This means: "this bitstream was produced from *this* source, by *this* compiler version, targeting *this* FPGA with *these* constraints." Any change anywhere in the chain invalidates the match. Full reproducibility, cryptographically enforced.

---

## 5. Compiler Architecture

The compiler is implemented in Rust. The pipeline is:

```
  .tbn source
       │
       ▼
  ┌─────────┐
  │  Parse   │   chumsky grammar → CST
  └────┬─────┘
       │
       ▼
  ┌──────────┐
  │  Desugar  │   Cell → depth-1 Queue; pattern match → decision tree
  └────┬──────┘
       │
       ▼
  ┌────────────┐
  │  Type Check │   Hindley-Milner + linearity check on Cells
  └────┬───────┘   + queue protocol checking (stretch)
       │
       ▼
  ┌────────────┐
  │  Elaboration│   Resolve pipes, flatten hierarchy,
  └────┬───────┘   build process network graph
       │
       ▼
  ┌────────────┐
  │  Schedule   │   Assign rule priorities, detect conflicts,
  └────┬───────┘   insert arbitration for shared Cells
       │
       ▼
  ┌────────────┐
  │  Lower      │   Process network → structural SystemVerilog
  └────┬───────┘   Queue → FIFO module, Cell → reg, rule → always_comb
       │
       ▼
  ┌────────────┐
  │  Provenance │   Hash source tree, embed in SV output,
  └────┬───────┘   populate build cache
       │
       ▼
  .sv output + build manifest
```

### 5.1 Rust Crates

| Concern | Crate | Status |
|---|---|---|
| Parsing | `chumsky` 1.0-alpha (error recovery) | In use |
| Diagnostics | `ariadne` 0.5 (span-highlighted errors) | In use |
| CLI | `clap` 4 (derive) | In use |
| Type checking | Custom (Hindley-Milner + linearity) | In use |
| IR | `petgraph` 0.7 for the process network graph | In use |
| SV emission | `std::fmt::Write` (direct string building) | In use |
| Hashing | `blake3` | Planned |
| Build cache | `cacache` (content-addressable, Nix-like) | Planned |
| JTAG/UART | `serialport` for verify; `probe-rs` for JTAG | Planned |

### 5.2 Intermediate Representation

The core IR is a **process network graph**:

- **Nodes** are processes (containing one or more rules, each a combinational expression tree).
- **Edges** are queues (annotated with element type and depth).
- **Cell nodes** are special self-loop edges with a distinguished "shared access" annotation when multiple processes touch the same Cell.

This representation is directly amenable to:

- Deadlock analysis (Petri net marking / KPN capacity check).
- Rule conflict detection (two rules touching the same Cell → need arbitration).
- Scheduling (topological sort for pipelines; priority encoding for conflicting rules).

---

## 6. Formal Verification Angle

The process network IR maps naturally onto several verification frameworks:

### 6.1 Process Algebra (mCRL2 / SPIN)

Each Tourbillon process is a CSP process. Each queue is a channel. `tbn` could export the process network as an mCRL2 specification automatically, giving you model-checked deadlock freedom for free.

### 6.2 Session Types

If queue protocols are formalised, the type system itself becomes a proof of protocol conformance. The compiler is the proof checker. No external tool needed — correctness by construction.

### 6.3 SMT-backed Property Checking

For datapath properties ("the ALU never receives an illegal opcode," "register x0 is always zero"), the compiler could generate SVA assertions in the SystemVerilog output, or export to a SymbiYosys-compatible format for bounded model checking.

### 6.4 Equivalence Checking

Because the lowering from Tourbillon IR to SystemVerilog is mechanical and structural, formal equivalence checking between the IR and the generated SV is tractable. This closes the "do you trust the compiler?" gap.

---

## 7. CLI Reference

```
tbn build [--target <fpga>]     Compile .tbn → SystemVerilog, populate cache
tbn check                       Type-check and deadlock-analyse without codegen
tbn status                      Show hash status: source vs build vs FPGA
tbn verify <port>               Read provenance from FPGA, compare to source
tbn graph                       Emit process network as DOT / Mermaid
tbn export-mcrl2                Export process network for mCRL2 model checking
tbn clean                       Purge build cache
tbn init <name>                 Scaffold a new Tourbillon project
```

---

## 8. Design Decisions (Resolved)

1. **Shared Cell semantics.** Resolved via `peek()` — a non-consuming read with no linearity obligation. Processes that only need to read a Cell (e.g. Decode reading the register file) use `peek()` and declare the port as `peeks:`. Processes that mutate a Cell use `take()`/`put()` with exclusive linear access. `peek()` and `take()`/`put()` may execute concurrently on the same Cell in the same cycle. No priority annotations needed for the read-alongside-write case.

2. **Memory interfaces.** Resolved via the `Memory(K → V, depth, latency)` primitive. Desugars to request/response queues with an internal latency-modeling process. The compiler maps Memory nodes to vendor BRAM/SRAM during SV lowering. The `latency` parameter informs deadlock analysis. The RV32I reference design should be updated to use split-transaction memory access as a Phase 2 deliverable.

3. **Interrupt handling.** Resolved via `source = async` queue annotation. Asynchronous external sources are modeled as queues whose producer is outside the process network. The compiler generates a clock-domain synchroniser in SV. Priority across multiple interrupt sources uses the same multi-producer arbiter desugaring as any other N-to-1 contention. Inevitable synchronisation latency is accepted — Tourbillon is not targeting hard-real-time control systems out of the box.

4. **Area overhead.** Full FIFO emission (Level 3) for the foreseeable future. Register collapse is a future optimisation pass. The semantic model is queues everywhere; the silicon will initially also be FIFOs everywhere. When the optimiser is eventually implemented, it will operate at multiple levels of aggression (bare register for depth-1 no-contention, register + valid bit for provably-single-occupancy, full FIFO otherwise), controlled by a compiler flag.

5. **Provenance in ASIC flows.** Deferred. FPGA verification over JTAG/UART is the near-term story. ASIC tape-out provenance (ROM, fuse block, eFuse) will be designed when an ASIC target is in scope.

---

## 9. Roadmap

| Phase | Deliverable | Scope | Status |
|---|---|---|---|
| **0 — Bootstrap** | Parser + type checker + Cell linearity | Core language compiles, no SV output | **Complete** — lexer, parser, desugaring, type checker, linearity checker |
| **1 — Codegen** | SV emitter + FIFO library + provenance embedding | End-to-end flow: `.tbn` → `.sv` | **Complete** — All 7 stages implemented; `tbn build` produces provenance-tagged SV |
| **2 — RV32I** | Reference core passes simulation (verilator) | Proves the language works for real hardware | Planned |
| **3 — Verify** | `tbn status` / `tbn verify` over UART/JTAG | Provenance chain to running FPGA | Planned |
| **4 — Formal** | mCRL2 export + deadlock checker | Verification story | Planned |
| **5 — Session** | Protocol types on queue interfaces | Advanced type system | Planned |

---

*Tourbillon: queues all the way down, hashes all the way up.*
