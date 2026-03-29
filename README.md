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

# Compile the RV32I reference core to SystemVerilog
cargo run -- build examples/rv32i.tbn -o output/
# -> output/CPU.sv, output/tbn_fifo.sv
```

## RV32I Reference Core

The `examples/rv32i.tbn` file is a complete single-issue, 4-stage RV32I pipeline that passes **38/38 rv32ui compliance tests**.

### Viewing the Generated SystemVerilog

```bash
cargo run -- build examples/rv32i.tbn -o output/
cat output/CPU.sv
```

### Running the Simulation

Requires [Verilator](https://verilator.org) and a RISC-V cross-compiler (`riscv64-elf-gcc`):

```bash
# On Arch Linux:
sudo pacman -S verilator riscv64-elf-gcc riscv64-elf-binutils riscv64-elf-newlib

# Initialize riscv-tests submodule
git submodule update --init

# Build the rv32ui test suite (custom env, no CSR support needed)
cd sim && bash build_tests.sh

# Run the smoke test (no toolchain needed)
make test-hex

# Run the full rv32ui compliance suite (38 tests)
make riscv-tests
```

Or run everything through cargo:

```bash
cargo test rv32ui_compliance -- --nocapture
```

### Architecture

```
Fetch ──→ Decode ──→ Execute ──→ Writeback
  ↑                    │              │
  │    redirect ←──────┘              │
  └──────────── done_q ←─────────────┘
```

- **Single-issue** via done_q credit token (~4 CPI)
- **Monolithic register file** as `Cell(Array(32, Word))`
- **Sub-word load/store** via read-modify-write
- **Memory** via combinational SRAM models (behavioral)

## Language Features

```
type Word = Bits 32

process Counter {
    state: count : Cell(Word, init = 0)

    rule tick {
        let c = count.take()
        count.put(c + 1)
    }
}

pipe Top {
    Counter {}
}
```

- **Queue(T, depth=N)** — bounded typed FIFO, the only sequential storage
- **Cell(T)** — depth-1 self-queue with linear-type discipline (must take then put)
- **Process** — guarded combinational logic with atomic rules
- **Pipe** — structural composition (wiring only)
- **Memory(K -> V)** — addressable storage, desugars to req/resp queues
- **Indexed cell ports** — `regs[32] : Cell(Word)` expands to 32 individual cells

## Project Structure

```
src/           Rust compiler (parse → desugar → typecheck → elaborate → schedule → lower)
examples/      Tourbillon source files (rv32i.tbn, counter.tbn, ...)
sim/           Verilator simulation infrastructure
  rv32i_pkg.sv   RV32I decode/ALU/branch support package
  mem_model.sv   Behavioral SRAM model
  tb_top.sv      Simulation top-level
  tb_cpu.cpp     C++ testbench with ELF loader
tests/         Rust integration tests
riscv-tests/   RISC-V compliance test suite (git submodule)
```

## License

See LICENSE file.
