// fpga_tag_mem.sv — Synthesisable 1-bit tag memory with ready/valid interface
//
// One tag bit per 8-byte capability-aligned region.
// Distributed RAM (combinational read, registered write) — same pattern
// as fpga_mem.sv USE_BRAM=0 but with 1-bit data width.
//
// CPU-domain clock (no CDC — tag memory lives in the CPU clock domain).
//
// Write request format: {addr[32:1], tag[0]} — 33 bits total.
// Read request: 32-bit address (converted via cheri_pkg::tag_addr).
// Read response: 1-bit tag value.

/* verilator lint_off UNUSEDSIGNAL */
module fpga_tag_mem #(
    parameter DEPTH = 8192       // one tag per 8 bytes of 64 KB data space
)(
    input  wire         clk,
    input  wire         rst_n,

    input  wire         rd_req_valid,
    output wire         rd_req_ready,
    input  wire [31:0]  rd_req_data,

    output wire         rd_resp_valid,
    input  wire         rd_resp_ready,
    output wire         rd_resp_data,      // 1-bit tag

    input  wire         wr_req_valid,
    output wire         wr_req_ready,
    input  wire [32:0]  wr_req_data        // {addr[32:1], tag[0]}
);

    localparam AW = $clog2(DEPTH);

    // Vivado infers distributed RAM (LUT RAM) from combinational read.
    (* ram_style = "distributed" *)
    reg storage [0:DEPTH-1];

    // Always ready to accept requests
    assign rd_req_ready = 1'b1;
    assign wr_req_ready = 1'b1;

    // Write port — registered
    wire [31:0] wr_addr = wr_req_data[32:1];
    wire        wr_tag  = wr_req_data[0];

    always_ff @(posedge clk) begin
        if (wr_req_valid)
            storage[wr_addr[AW+1:2]] <= wr_tag;
    end

    // Read port — combinational (distributed RAM)
    assign rd_resp_valid = 1'b1;
    assign rd_resp_data  = storage[rd_req_data[AW+1:2]];

    // Initialise all tags to 0
    integer i;
    initial begin
        for (i = 0; i < DEPTH; i = i + 1)
            storage[i] = 1'b0;
    end

endmodule
