// mem_model.sv — Behavioral SRAM with ready/valid interface
//
// Acts as a combinational lookup table: always ready, always valid.
// Read: address in rd_req_data → data out rd_resp_data (same cycle).
// Write: registered, stores on posedge clk when wr_req_valid.
//
// The "always valid" response is critical for pipeline bootstrap:
// the CPU's Fetch rule needs a valid response before it can issue
// its first read request, so the memory must not gate resp_valid
// on req_valid (that would create a deadlock).

/* verilator lint_off UNUSEDSIGNAL */
module mem_model #(
    parameter DEPTH   = 16384,   // 64 KB default (16K × 32-bit words)
    parameter MEMFILE = ""
)(
    input  wire         clk,
    input  wire         rst_n,      // unused but kept for interface regularity

    // Read request (address in)
    input  wire         rd_req_valid,
    output wire         rd_req_ready,
    input  wire [31:0]  rd_req_data,     // byte address

    // Read response (data out)
    output wire         rd_resp_valid,
    input  wire         rd_resp_ready,
    output wire [31:0]  rd_resp_data,

    // Write request ({addr[63:32], data[31:0]})
    input  wire         wr_req_valid,
    output wire         wr_req_ready,
    input  wire [63:0]  wr_req_data
);

    localparam AW = $clog2(DEPTH);

    reg [31:0] storage [0:DEPTH-1];

    // Always ready, always valid — combinational lookup table
    assign rd_req_ready  = 1'b1;
    assign wr_req_ready  = 1'b1;

    /* verilator lint_off UNOPTFLAT */
    assign rd_resp_valid = 1'b1;
    assign rd_resp_data  = storage[rd_req_data[AW+1:2]];
    /* verilator lint_on UNOPTFLAT */

    // Registered writes
    wire [31:0] wr_addr = wr_req_data[63:32];
    wire [31:0] wr_data = wr_req_data[31:0];

    always_ff @(posedge clk) begin
        if (wr_req_valid) begin
            storage[wr_addr[AW+1:2]] <= wr_data;
        end
    end

    // Load initial contents
    initial begin
        if (MEMFILE != "") begin
            $readmemh(MEMFILE, storage);
        end
    end

endmodule
