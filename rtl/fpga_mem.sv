// fpga_mem.sv — Synthesisable memory with ready/valid interface
//
// Two flavours controlled by USE_BRAM:
//   0 (default): Distributed RAM — combinational read, single-cycle response.
//                Vivado infers LUT RAM. Use for imem (Fetch needs same-cycle read).
//   1:           Block RAM — registered read with 1-cycle response latch.
//                Vivado infers BRAM. Use for dmem (MemDevice tolerates latency).
//
// Both: $readmemh(MEMFILE) at elaboration for bitstream initialisation.

/* verilator lint_off UNUSEDSIGNAL */
module fpga_mem #(
    parameter DEPTH    = 16384,
    parameter MEMFILE  = "",
    parameter USE_BRAM = 0        // 0 = distributed (combo read), 1 = block RAM
)(
    input  wire         clk,
    input  wire         rst_n,

    input  wire         rd_req_valid,
    output wire         rd_req_ready,
    input  wire [31:0]  rd_req_data,

    output wire         rd_resp_valid,
    input  wire         rd_resp_ready,
    output wire [31:0]  rd_resp_data,

    input  wire         wr_req_valid,
    output wire         wr_req_ready,
    input  wire [63:0]  wr_req_data
);

    localparam AW = $clog2(DEPTH);

    // Vivado infers distributed RAM or BRAM from the read style below.
    (* ram_style = USE_BRAM ? "block" : "distributed" *)
    reg [31:0] storage [0:DEPTH-1];

    // Always ready to accept requests
    assign rd_req_ready = 1'b1;
    assign wr_req_ready = 1'b1;

    // Write port — registered
    wire [31:0] wr_addr = wr_req_data[63:32];
    wire [31:0] wr_data = wr_req_data[31:0];

    always_ff @(posedge clk) begin
        if (wr_req_valid)
            storage[wr_addr[AW+1:2]] <= wr_data;
    end

    generate
        if (USE_BRAM == 0) begin : gen_distram
            // ---- Distributed RAM: combinational read, always valid ----
            assign rd_resp_valid = 1'b1;
            assign rd_resp_data  = storage[rd_req_data[AW+1:2]];
        end else begin : gen_bram
            // ---- Block RAM: registered read with response latch ----
            // Read fires on rd_req_valid; result latched until consumed.
            reg        resp_valid_q;
            reg [31:0] resp_data_q;

            assign rd_resp_valid = resp_valid_q;
            assign rd_resp_data  = resp_data_q;

            always_ff @(posedge clk or negedge rst_n) begin
                if (!rst_n) begin
                    resp_valid_q <= 1'b0;
                    resp_data_q  <= 32'd0;
                end else begin
                    if (rd_req_valid) begin
                        resp_valid_q <= 1'b1;
                        resp_data_q  <= storage[rd_req_data[AW+1:2]];
                    end else if (resp_valid_q && rd_resp_ready) begin
                        resp_valid_q <= 1'b0;
                    end
                end
            end
        end
    endgenerate

    // Initialise from hex file at elaboration time.
    // Vivado propagates this into BRAM/distributed RAM init values.
    initial begin
        $readmemh(MEMFILE, storage);
    end

endmodule
