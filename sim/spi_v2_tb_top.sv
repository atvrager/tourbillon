// spi_v2_tb_top.sv — Verilator testbench wrapper for Spi2TLULV2
//
// Same structure as V1 wrapper: two async clocks, CSB-as-reset,
// auto-responding TileLink device. Adapted for the V2 frame-based module.

/* verilator lint_off UNUSEDSIGNAL */
/* verilator lint_off UNOPTFLAT */

module spi_v2_tb_top (
    input  wire sys_clk,
    input  wire sys_rst_n,
    input  wire spi_clk,
    input  wire spi_csb,

    input  wire spi_mosi,
    output wire spi_miso,

    output wire         tl_a_valid,
    output wire [189:0] tl_a_data,
    output wire [2:0]   tl_a_opcode,
    output wire [31:0]  tl_a_address
);

    wire spi_rst_n = ~spi_csb;

    // TL channel wires
    wire         tl_a_enq_valid;
    logic        tl_a_enq_ready;
    wire [189:0] tl_a_enq_data;
    logic        tl_d_deq_valid;
    wire         tl_d_deq_ready;
    logic [142:0] tl_d_deq_data;

    // MISO
    wire miso_enq_valid, miso_enq_data;
    assign spi_miso = miso_enq_valid ? miso_enq_data : 1'b0;

    // TL-A snoop
    assign tl_a_valid   = tl_a_enq_valid;
    assign tl_a_data    = tl_a_enq_data;
    assign tl_a_opcode  = tl_a_enq_data[189:187];
    assign tl_a_address = tl_a_enq_data[175:144];

    // -------------------------------------------------------------------------
    // TileLink auto-responder (same as V1)
    // -------------------------------------------------------------------------
    localparam TL_GET = 3'd4;
    localparam TL_ACCESS_ACK = 3'd0;
    localparam TL_ACCESS_ACK_DATA = 3'd1;

    logic resp_pending;
    logic [189:0] resp_req;

    always_ff @(posedge sys_clk or negedge sys_rst_n) begin
        if (!sys_rst_n) begin
            resp_pending <= 1'b0;
            resp_req <= '0;
            tl_a_enq_ready <= 1'b1;
        end else begin
            if (tl_a_enq_valid && tl_a_enq_ready && !resp_pending) begin
                resp_pending <= 1'b1;
                resp_req <= tl_a_enq_data;
                tl_a_enq_ready <= 1'b0;
            end else if (resp_pending && tl_d_deq_ready) begin
                resp_pending <= 1'b0;
                tl_a_enq_ready <= 1'b1;
            end
        end
    end

    wire [2:0] req_opcode = resp_req[189:187];
    wire [2:0] req_size   = resp_req[186:184];
    wire [7:0] req_source = resp_req[183:176];
    wire [31:0] req_addr  = resp_req[175:144];

    wire [2:0]   resp_opcode = (req_opcode == TL_GET) ? TL_ACCESS_ACK_DATA : TL_ACCESS_ACK;
    wire [127:0] resp_data   = {96'b0, req_addr};

    always_comb begin
        tl_d_deq_valid = resp_pending;
        tl_d_deq_data = {resp_opcode, req_size, req_source, resp_data, 1'b0};
    end

    // -------------------------------------------------------------------------
    // DUT: Spi2TLULV2
    // -------------------------------------------------------------------------
    Spi2TLULV2 dut (
        .clk        (sys_clk),
        .rst_n      (sys_rst_n),
        .spi_clk    (spi_clk),
        .spi_rst_n  (spi_rst_n),

        .q_mosi_pin_deq_valid (1'b1),
        .q_mosi_pin_deq_ready (),
        .q_mosi_pin_deq_data  (spi_mosi),

        .q_miso_pin_enq_valid (miso_enq_valid),
        .q_miso_pin_enq_ready (1'b1),
        .q_miso_pin_enq_data  (miso_enq_data),

        .q_tl_a_enq_valid     (tl_a_enq_valid),
        .q_tl_a_enq_ready     (tl_a_enq_ready),
        .q_tl_a_enq_data      (tl_a_enq_data),
        .q_tl_d_deq_valid     (tl_d_deq_valid),
        .q_tl_d_deq_ready     (tl_d_deq_ready),
        .q_tl_d_deq_data      (tl_d_deq_data)
    );

endmodule
