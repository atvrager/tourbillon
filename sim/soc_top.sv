// soc_top.sv — Multi-clock simulation wrapper for the Marie Antoinette SoC
//
// Wires the generated Marie module to behavioral mem_model instances for
// instruction memory (imem, cpu domain) and data memory (dev_mem, dev domain).
// Monitors dev_mem writes to tohost address (0x80001000) for riscv-tests.
//
// Three clock domains are driven independently by the C++ testbench:
// cpu_clk, xbar_clk, dev_clk. The default clk/rst_n are unused but wired.

import rv32i_pkg::*;

/* verilator lint_off UNOPTFLAT */

module soc_top (
    input  wire         cpu_clk,
    input  wire         cpu_rst_n,
    input  wire         xbar_clk,
    input  wire         xbar_rst_n,
    input  wire         dev_clk,
    input  wire         dev_rst_n,
    output logic [31:0] tohost
);

    // -------------------------------------------------------------------------
    // CPU instruction memory interconnect (cpu domain)
    // -------------------------------------------------------------------------
    wire         imem_rd_req_valid;
    wire         imem_rd_req_ready;
    wire [31:0]  imem_rd_req_data;
    wire         imem_rd_resp_valid;
    wire         imem_rd_resp_ready;
    wire [31:0]  imem_rd_resp_data;
    wire         imem_wr_req_valid;
    wire         imem_wr_req_ready;
    wire [63:0]  imem_wr_req_data;

    // -------------------------------------------------------------------------
    // Device data memory interconnect (dev domain)
    // -------------------------------------------------------------------------
    wire         dmem_rd_req_valid;
    wire         dmem_rd_req_ready;
    wire [31:0]  dmem_rd_req_data;
    wire         dmem_rd_resp_valid;
    wire         dmem_rd_resp_ready;
    wire [31:0]  dmem_rd_resp_data;
    wire         dmem_wr_req_valid;
    wire         dmem_wr_req_ready;
    wire [63:0]  dmem_wr_req_data;

    // -------------------------------------------------------------------------
    // Marie SoC instance
    // -------------------------------------------------------------------------
    Marie marie_inst (
        // CPU domain
        .cpu_clk   (cpu_clk),
        .cpu_rst_n (cpu_rst_n),

        // Crossbar domain
        .xbar_clk   (xbar_clk),
        .xbar_rst_n (xbar_rst_n),

        // Device domain
        .dev_clk   (dev_clk),
        .dev_rst_n (dev_rst_n),

        // CPU instruction memory (cpu domain)
        .q_CPUCore_imem_read_req_enq_valid  (imem_rd_req_valid),
        .q_CPUCore_imem_read_req_enq_ready  (imem_rd_req_ready),
        .q_CPUCore_imem_read_req_enq_data   (imem_rd_req_data),
        .q_CPUCore_imem_read_resp_deq_valid (imem_rd_resp_valid),
        .q_CPUCore_imem_read_resp_deq_ready (imem_rd_resp_ready),
        .q_CPUCore_imem_read_resp_deq_data  (imem_rd_resp_data),
        .q_CPUCore_imem_write_req_enq_valid (imem_wr_req_valid),
        .q_CPUCore_imem_write_req_enq_ready (imem_wr_req_ready),
        .q_CPUCore_imem_write_req_enq_data  (imem_wr_req_data),

        // Device data memory (dev domain)
        .q_dev_mem_read_req_enq_valid  (dmem_rd_req_valid),
        .q_dev_mem_read_req_enq_ready  (dmem_rd_req_ready),
        .q_dev_mem_read_req_enq_data   (dmem_rd_req_data),
        .q_dev_mem_read_resp_deq_valid (dmem_rd_resp_valid),
        .q_dev_mem_read_resp_deq_ready (dmem_rd_resp_ready),
        .q_dev_mem_read_resp_deq_data  (dmem_rd_resp_data),
        .q_dev_mem_write_req_enq_valid (dmem_wr_req_valid),
        .q_dev_mem_write_req_enq_ready (dmem_wr_req_ready),
        .q_dev_mem_write_req_enq_data  (dmem_wr_req_data)
    );

    // -------------------------------------------------------------------------
    // Instruction memory — cpu domain clock
    // -------------------------------------------------------------------------
    mem_model #(
        .DEPTH (16384)
    ) imem (
        .clk           (cpu_clk),
        .rst_n         (cpu_rst_n),
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
    // Device data memory — dev domain clock
    // The mem_model is combinational (rd_resp in same cycle as rd_req).
    // The SoC's MemDevice reads the response one cycle later via try_take,
    // so we register the response to hold data across cycles.
    // -------------------------------------------------------------------------
    wire        dmem_raw_resp_valid;
    wire [31:0] dmem_raw_resp_data;

    mem_model #(
        .DEPTH (16384)
    ) dmem (
        .clk           (dev_clk),
        .rst_n         (dev_rst_n),
        .rd_req_valid  (dmem_rd_req_valid),
        .rd_req_ready  (dmem_rd_req_ready),
        .rd_req_data   (dmem_rd_req_data),
        .rd_resp_valid (dmem_raw_resp_valid),
        .rd_resp_ready (1'b1),  // always consume from raw
        .rd_resp_data  (dmem_raw_resp_data),
        .wr_req_valid  (dmem_wr_req_valid),
        .wr_req_ready  (dmem_wr_req_ready),
        .wr_req_data   (dmem_wr_req_data)
    );

    // 1-cycle response queue: latch on valid request, hold until consumed
    reg        dmem_resp_valid_q;
    reg [31:0] dmem_resp_data_q;

    assign dmem_rd_resp_valid = dmem_resp_valid_q;
    assign dmem_rd_resp_data  = dmem_resp_data_q;

    always_ff @(posedge dev_clk or negedge dev_rst_n) begin
        if (!dev_rst_n) begin
            dmem_resp_valid_q <= 1'b0;
            dmem_resp_data_q  <= 32'd0;
        end else begin
            if (dmem_rd_req_valid) begin
                // New request: latch response (even if previous still valid)
                dmem_resp_valid_q <= 1'b1;
                dmem_resp_data_q  <= dmem_raw_resp_data;
            end else if (dmem_resp_valid_q && dmem_rd_resp_ready) begin
                // Consumed by Marie, no new request
                dmem_resp_valid_q <= 1'b0;
            end
        end
    end

    // -------------------------------------------------------------------------
    // Memory loading via +memfile= plusarg (hex files)
    // -------------------------------------------------------------------------
    string memfile;
    initial begin
        if ($value$plusargs("memfile=%s", memfile)) begin
            $readmemh(memfile, imem.storage);
            $readmemh(memfile, dmem.storage);
        end
    end

    // -------------------------------------------------------------------------
    // next_pc_q is pre-loaded with 0x80000000 by FIFO INIT_VALUE on reset

    // -------------------------------------------------------------------------
    // tohost monitor — watch dev_mem writes to 0x80001000
    // -------------------------------------------------------------------------
    localparam [31:0] TOHOST_ADDR = 32'h8000_1000;

    initial tohost = 32'h0;

    always_ff @(posedge dev_clk or negedge dev_rst_n) begin
        if (!dev_rst_n) begin
            tohost <= 32'h0;
        end else if (dmem_wr_req_valid) begin
            if (dmem_wr_req_data[63:32] == TOHOST_ADDR) begin
                tohost <= dmem_wr_req_data[31:0];
            end
        end
    end

endmodule
