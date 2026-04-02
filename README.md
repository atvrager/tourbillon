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
cargo run -- build examples/cpu_core.tbn examples/marie.tbn -o output/
# -> output/Marie.sv, output/tbn_fifo.sv, output/tbn_async_fifo.sv
```

## The Marie Antoinette SoC

Named after the legendary Breguet No. 1160 pocket watch — the most ambitious tourbillon timepiece ever built.

`examples/marie.tbn` is a multi-clock-domain SoC that exercises every language feature:

```
       cpu domain              xbar domain             dev domain
  ┌─────────────────┐     ┌────────────────┐     ┌──────────────────┐
  │   CPUCore pipe   │     │                │     │  MemDevice       │
  │  (RV32I 4-stage) │ CDC │   Xbar 1→4    │ CDC │  UartPhy pipe    │
  │                  ├─────┤  (addr decode) ├─────┤  ManifestDevice  │
  │  imem (local)    │     │                │     │  ClocksDevice    │
  └─────────────────┘     └────────────────┘     └───────┬──────────┘
                                                         │ external Queue pins
                                                    uart_tx  uart_rx
```

- **3 clock domains** (100/150/50 MHz in FPGA) with gray-code async FIFO CDC
- **Pipe hierarchy**: CPUCore pipe instantiated inside Marie with cross-pipe queue wiring
- **Address-decoded crossbar**: `addr[31:28]` routes to memory (0x8), UART (0x1), manifest (0x0), clocks (0x2)
- **Hardware device discovery**: ManifestDevice with pre-populated slots, CLAIM counter, SEAL lockout — firmware discovers peripherals via plain loads, no device tree needed
- **Clock frequency queries**: ClocksDevice returns `soc_pkg.sv` constants at runtime — enables software baud rate computation
- **Real UART**: UartTx/UartRx/UartDevice written entirely in Tourbillon — bit-serial shift registers at 921600 baud, with CTS flow control. No DPI — physical TX/RX/RTS/CTS pins via `external Queue`
- **Non-speculative pipeline**: no branch prediction — Execute sends correct next PC after full completion
- **Split-phase bus fabric**: all processes use `try_take` polling for CDC-tolerant multi-cycle operations

### Running Hello World (Verilator)

```bash
# Requires: verilator, riscv64-elf-as/ld/objcopy
make -C sim soc-hello   # "Hello, World!" via UART TX pin → DPI deserializer
```

Output: `Hello, World!` printed through 3-domain bus fabric + bit-serial UART.

### FPGA Export (Xilinx Virtex UltraScale+)

```bash
make -C sim rtl-export   # → marie_soc_rtl.zip (standalone, hello.hex baked in)
```

Unzip into Vivado, set `marie_top` as top (`STANDALONE=1`), add your board XDC, synthesize. UART TX prints "Hello, World!" on first boot at 921600 baud.

Two export modes:
- `rtl-export` / `rtl-export-standalone` — internal BRAM with hello.hex, just add constraints
- `rtl-export-ext` — external memory ports, wire your own SRAM controller

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
- **external Queue** — module port pins (no FIFO, for physical I/O)
- **const** — compile-time integer constants (→ `localparam`), arbitrary precision (BigUint)
- **const = external** — SV package identifiers (no localparam — single source of truth)
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
examples/      Tourbillon source files
  rv32i.tbn      RV32I reference core
  marie.tbn      Marie Antoinette SoC (3-domain, UART, bus fabric)
sim/           Verilator simulation infrastructure
  rv32i_pkg.sv   RV32I decode/ALU/branch support package
  soc_pkg.sv     Clock frequency constants (single source of truth)
  manifest_pkg.sv  Manifest slot access functions (synthesizable SV)
  mem_model.sv   Behavioral SRAM model
  soc_top.sv     SoC simulation wrapper (UART DPI bridge, multi-rate clocks)
  soc_tb.cpp     3-domain C++ testbench (100/150/50 MHz)
  Makefile       Build system (soc-hello, riscv-tests, soc-manifest-test, rtl-export)
  tests/         Assembly tests (smoke.hex, hello.S, manifest.S)
docs/          Design documents
rtl/           FPGA synthesis files
  marie_top.sv   Xilinx VU+ toplevel (MMCME4, rst_sync, STANDALONE param)
  fpga_mem.sv    Synthesisable memory (distributed RAM / block RAM)
tests/         Rust integration tests
```

## License

See LICENSE file.
