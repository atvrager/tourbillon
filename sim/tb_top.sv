// tb_top.sv — Simulation top-level wrapper for the Tourbillon CPU
//
// Provides clock/reset and instantiates the generated CPU module.
// The C++ testbench (Verilator) drives clk and rst_n and monitors tohost.

import rv32i_pkg::*;

module tb_top (
    input  wire         clk,
    input  wire         rst_n,
    output logic [31:0] tohost
);

    // -------------------------------------------------------------------------
    // CPU instance
    // -------------------------------------------------------------------------
    // The generated CPU module is self-contained: all memory FIFOs and process
    // logic are internal. Only clock and reset are external.
    CPU cpu_inst (
        .clk   (clk),
        .rst_n (rst_n)
    );

    // -------------------------------------------------------------------------
    // tohost register
    // -------------------------------------------------------------------------
    // TODO: Wire tohost to dmem writes targeting the HTIF tohost address
    // (typically 0x80001000). For now this is a placeholder that the C++
    // testbench can monitor. Once the CPU exposes a memory-mapped write port
    // or a dedicated tohost signal, connect it here.
    initial tohost = 32'h0;

endmodule
