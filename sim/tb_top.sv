// tb_top.sv — Simulation top-level wrapper for the Tourbillon CPU
//
// Wires the generated CPU module (with memory ports) to behavioral
// mem_model instances for instruction and data memory. Monitors dmem
// writes to the tohost address (0x80001000) for riscv-tests pass/fail.
//
// CPUCore sub-pipe ports are prefixed with CPUCore_ in the generated SV.

import rv32i_pkg::*;
import cheri_pkg::*;

/* verilator lint_off UNOPTFLAT */
// Known combinational loops through CPU↔memory ready/valid handshakes.
// These are inherent to the zero-latency memory model and resolve correctly
// via Verilator's iterative evaluation.

module tb_top (
    input  wire         clk,
    input  wire         rst_n,
    output logic [31:0] tohost
);

    // -------------------------------------------------------------------------
    // Memory file path (set via Verilator +memfile= plusarg)
    // -------------------------------------------------------------------------
    string memfile;

    // -------------------------------------------------------------------------
    // CPU ↔ memory interconnect signals
    // -------------------------------------------------------------------------

    // imem read request (CPU → imem)
    wire         imem_rd_req_valid;
    wire         imem_rd_req_ready;
    wire [31:0]  imem_rd_req_data;

    // imem read response (imem → CPU)
    wire         imem_rd_resp_valid;
    wire         imem_rd_resp_ready;
    wire [31:0]  imem_rd_resp_data;

    // imem write request (CPU → imem, unused in practice)
    wire         imem_wr_req_valid;
    wire         imem_wr_req_ready;
    wire [63:0]  imem_wr_req_data;

    // dmem read request (CPU → dmem)
    wire         dmem_rd_req_valid;
    wire         dmem_rd_req_ready;
    wire [31:0]  dmem_rd_req_data;

    // dmem read response (dmem → CPU)
    wire         dmem_rd_resp_valid;
    wire         dmem_rd_resp_ready;
    wire [31:0]  dmem_rd_resp_data;

    // dmem write request (CPU → dmem)
    wire         dmem_wr_req_valid;
    wire         dmem_wr_req_ready;
    wire [63:0]  dmem_wr_req_data;

    // -------------------------------------------------------------------------
    // CPU instance
    // -------------------------------------------------------------------------
    CPU cpu_inst (
        .clk   (clk),
        .rst_n (rst_n),

        // imem read (CPUCore sub-pipe prefix)
        .q_CPUCore_imem_read_req_enq_valid  (imem_rd_req_valid),
        .q_CPUCore_imem_read_req_enq_ready  (imem_rd_req_ready),
        .q_CPUCore_imem_read_req_enq_data   (imem_rd_req_data),
        .q_CPUCore_imem_read_resp_deq_valid (imem_rd_resp_valid),
        .q_CPUCore_imem_read_resp_deq_ready (imem_rd_resp_ready),
        .q_CPUCore_imem_read_resp_deq_data  (imem_rd_resp_data),

        // imem write (CPUCore sub-pipe prefix)
        .q_CPUCore_imem_write_req_enq_valid (imem_wr_req_valid),
        .q_CPUCore_imem_write_req_enq_ready (imem_wr_req_ready),
        .q_CPUCore_imem_write_req_enq_data  (imem_wr_req_data),

        // dmem read
        .q_dmem_read_req_enq_valid  (dmem_rd_req_valid),
        .q_dmem_read_req_enq_ready  (dmem_rd_req_ready),
        .q_dmem_read_req_enq_data   (dmem_rd_req_data),
        .q_dmem_read_resp_deq_valid (dmem_rd_resp_valid),
        .q_dmem_read_resp_deq_ready (dmem_rd_resp_ready),
        .q_dmem_read_resp_deq_data  (dmem_rd_resp_data),

        // dmem write
        .q_dmem_write_req_enq_valid (dmem_wr_req_valid),
        .q_dmem_write_req_enq_ready (dmem_wr_req_ready),
        .q_dmem_write_req_enq_data  (dmem_wr_req_data)
    );

    // -------------------------------------------------------------------------
    // Instruction memory (read-only in practice)
    // -------------------------------------------------------------------------
    mem_model #(
        .DEPTH   (16384),
        .MEMFILE ("")           // loaded dynamically below
    ) imem (
        .clk           (clk),
        .rst_n         (rst_n),
        .rd_req_valid  (imem_rd_req_valid),
        .rd_req_ready  (imem_rd_req_ready),
        .rd_req_data   (imem_rd_req_data),
        .rd_resp_valid (imem_rd_resp_valid),
        .rd_resp_ready (imem_rd_resp_ready),
        .rd_resp_data  (imem_rd_resp_data),
        .wr_req_valid  (imem_wr_req_valid),
        .wr_req_ready  (imem_wr_req_ready),
        .wr_req_data   (imem_wr_req_data)
    );

    // -------------------------------------------------------------------------
    // Data memory (registered response for split-phase Execute)
    // -------------------------------------------------------------------------
    mem_model_reg #(
        .DEPTH   (16384),
        .MEMFILE ("")           // loaded dynamically below
    ) dmem (
        .clk           (clk),
        .rst_n         (rst_n),
        .rd_req_valid  (dmem_rd_req_valid),
        .rd_req_ready  (dmem_rd_req_ready),
        .rd_req_data   (dmem_rd_req_data),
        .rd_resp_valid (dmem_rd_resp_valid),
        .rd_resp_ready (dmem_rd_resp_ready),
        .rd_resp_data  (dmem_rd_resp_data),
        .wr_req_valid  (dmem_wr_req_valid),
        .wr_req_ready  (dmem_wr_req_ready),
        .wr_req_data   (dmem_wr_req_data)
    );

    // Load memory contents from hex file (both imem and dmem get same image)
    initial begin
        if ($value$plusargs("memfile=%s", memfile)) begin
            $readmemh(memfile, imem.storage);
            $readmemh(memfile, dmem.storage);
        end
    end

    // next_pcc_q is pre-loaded with root PCC capability at 0x80000000 by FIFO INIT_VALUE
    // (tag bit is 0 in Phase 1 — no tag checking yet)

    // -------------------------------------------------------------------------
    // tohost monitor — watch for dmem writes to 0x80001000
    // -------------------------------------------------------------------------
    localparam [31:0] TOHOST_ADDR = 32'h8000_1000;

    initial tohost = 32'h0;

    always_ff @(posedge clk or negedge rst_n) begin
        if (!rst_n) begin
            tohost <= 32'h0;
        end else if (dmem_wr_req_valid) begin
            if (dmem_wr_req_data[63:32] == TOHOST_ADDR) begin
                tohost <= dmem_wr_req_data[31:0];
            end
        end
    end

endmodule
