# Tourbillon

A queue-centric hardware description language with content-addressed provenance.

Tourbillon compiles `.tbn` source files to synthesisable SystemVerilog. The fundamental sequential primitive is the **FIFO**, not the D flip-flop. Pipeline hazards, stall logic, and valid/ready protocol bugs are eliminated structurally — you declare queues, write processes that consume and produce tokens, and compose them into pipelines.

See [TOURBILLON.md](TOURBILLON.md) for the full language specification.

## Quick Start

```bash
# Build the compiler
cargo build

# Run the full test suite (including rv32ui compliance)
cargo test

# Compile the Marie Antoinette SoC to SystemVerilog
cargo run -- build examples/marie.tbn -o output/
# -> output/Marie.sv, output/tbn_fifo.sv, output/tbn_async_fifo.sv
```

## The Marie Antoinette SoC

Named after the legendary Breguet No. 1160 pocket watch — the most ambitious tourbillon timepiece ever built.

`examples/marie.tbn` is a multi-clock-domain SoC that exercises every language feature:

```
       cpu domain              xbar domain             dev domain
  ┌─────────────────┐     ┌────────────────┐     ┌────────────────┐
  │   CPUCore pipe   │     │                │     │                │
  │  (RV32I 4-stage) │ CDC │   Xbar 1→2    │ CDC │  MemDevice     │
  │                  ├─────┤  (addr decode) ├─────┤  UartDevice    │
  │  imem (local)    │     │                │     │  (DPI uart_tx) │
  └─────────────────┘     └────────────────┘     └────────────────┘
```

- **3 clock domains** with gray-code async FIFO CDC
- **Pipe hierarchy**: CPUCore pipe instantiated inside Marie with cross-pipe queue wiring
- **Address-decoded crossbar**: `addr[31:28]` routes to memory (0x8xxx) or UART (0x1xxx)
- **DPI UART**: `external fn uart_tx(ch : Bits 8)` prints via Verilator DPI-C
- **Non-speculative pipeline**: no branch prediction — Execute sends correct next PC after full completion
- **Split-phase bus fabric**: all processes use `try_take` polling for CDC-tolerant multi-cycle operations

### Running Hello World

```bash
# Requires: verilator, riscv64-elf-gcc (or riscv64-unknown-elf-gcc)
make -C sim soc-build                         # Compile SoC → Verilator sim (~8s)
make -C sim RISCV_PREFIX=riscv64-elf- soc-hello   # "Hello, World!" via UART
```

Output: `Hello, World!` printed through the full 3-domain bus fabric, PASS after 736 cycles.

## RV32I Reference Core

`examples/rv32i.tbn` — a complete single-issue, 4-stage RV32I pipeline that passes **38/38 rv32ui compliance tests**.

```
Fetch ──→ Decode ──→ Execute ──→ Writeback
  ↑                    │              │
  │    redirect ←──────┘              │
  └──────────── done_q ←─────────────┘
```

```bash
# Compile to SystemVerilog
cargo run -- build examples/rv32i.tbn -o output/

# Run rv32ui compliance (requires verilator + riscv toolchain)
make -C sim riscv-tests
```

## Language Features

```tourbillon
const DEPTH = 4
type Word = Bits 32

external fn debug_print(val : Bits 32)

process Counter {
    state: count : Cell(Word, init = 0)
    produces: out : Queue(Word)

    rule tick {
        let c = count.take()
        let hi = c[31:28]        -- bit slicing
        debug_print(c)            -- DPI call
        out.put(c + DEPTH)        -- const reference
        count.put(c + 1)
    }
}

pipe Inner {
    let q = Queue(Word, depth = 2)
    Counter { out = q }
}

pipe Top {
    let bridge = Queue(Word, depth = 4)
    Inner { q = bridge }          -- pipe hierarchy
}
```

**Primitives:**
- **Queue(T, depth=N)** — bounded typed FIFO, the only sequential storage
- **Cell(T)** — depth-1 self-queue with linear-type discipline
- **Process** — guarded combinational logic with atomic rules
- **Pipe** — structural composition with hierarchy and cross-pipe wiring
- **Memory(K → V)** — addressable storage, desugars to req/resp queues
- **AsyncQueue(T, depth=N)** — clock domain crossing FIFO (gray-code)
- **const** — compile-time integer constants (→ `localparam`)
- **expr[hi:lo]** — bit slicing with inferred result width
- **external fn** — DPI function declarations (→ `import "DPI-C"`)

## Tooling

```bash
tbn check <file.tbn>            # Type-check + deadlock analysis
tbn build <file.tbn> -o <dir>   # Compile to SystemVerilog
tbn graph <file.tbn>            # Emit process network as Graphviz DOT
tbn wave <file.fst> [-f pat]    # Read Verilator FST trace for debugging
tbn status <file.tbn>           # Show provenance hash
tbn clean                       # Remove build cache
```

## Project Structure

```
src/           Rust compiler (parse → desugar → typecheck → elaborate → schedule → lower)
  wave.rs        FST waveform trace reader
examples/      Tourbillon source files
  rv32i.tbn      RV32I reference core (direct-wired, speculative)
  marie.tbn      Marie Antoinette SoC (3-domain, non-speculative, bus fabric)
sim/           Verilator simulation infrastructure
  rv32i_pkg.sv   RV32I decode/ALU/branch support package
  mem_model.sv   Behavioral SRAM model
  tb_top.sv      Direct-wired CPU simulation top-level
  tb_cpu.cpp     Single-clock C++ testbench with ELF loader
  soc_top.sv     Marie SoC multi-clock simulation wrapper
  soc_tb.cpp     3-domain C++ testbench with UART DPI
  tests/         Assembly tests (smoke.hex, hello.S)
tests/         Rust integration tests
riscv-tests/   RISC-V compliance test suite (git submodule)
```

## License

See LICENSE file.
