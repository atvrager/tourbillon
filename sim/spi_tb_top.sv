// spi_tb_top.sv — Verilator testbench wrapper for Spi2TLUL
//
// Two truly async clocks: sys_clk and spi_clk driven at different rates
// by the C++ testbench. CSB-as-reset for the SPI domain.
//
// The testbench acts as both:
//   - SPI master (driving MOSI, reading MISO)
//   - TileLink-UL device (accepting A-channel requests, sending D-channel responses)

/* verilator lint_off UNUSEDSIGNAL */
/* verilator lint_off UNOPTFLAT */

module spi_tb_top (
    input  wire sys_clk,
    input  wire sys_rst_n,
    input  wire spi_clk,
    input  wire spi_csb,        // active-low chip select → SPI reset

    // SPI master interface (directly from C++ testbench)
    input  wire spi_mosi,
    output wire spi_miso,

    // Test status outputs
    output logic [31:0] test_status,   // 0 = running, 1 = pass, 2 = fail

    // TL-A channel snoop (for C++ to read issued requests)
    output wire         tl_a_valid,
    output wire [189:0] tl_a_data,
    output wire [2:0]   tl_a_opcode,
    output wire [31:0]  tl_a_address
);

    // SPI domain reset: active when CSB is high (deselected)
    wire spi_rst_n = ~spi_csb;

    // Internal wires for TL channels
    wire         tl_a_enq_valid;
    logic        tl_a_enq_ready;
    wire [189:0] tl_a_enq_data;

    logic        tl_d_deq_valid;
    wire         tl_d_deq_ready;
    logic [142:0] tl_d_deq_data;

    // MISO wiring
    wire miso_enq_valid;
    wire miso_enq_data;
    assign spi_miso = miso_enq_valid ? miso_enq_data : 1'b0;

    // TL-A snoop outputs
    assign tl_a_valid   = tl_a_enq_valid;
    assign tl_a_data    = tl_a_enq_data;
    // TlA packed struct: {opcode[2:0], size[2:0], source[7:0], address[31:0], mask[15:0], data[127:0]}
    // opcode is the top 3 bits [189:187], address is [175:144]
    assign tl_a_opcode  = tl_a_enq_data[189:187];
    assign tl_a_address = tl_a_enq_data[175:144];

    // -------------------------------------------------------------------------
    // TileLink responder: auto-ack every A-channel request
    // Returns read data = address (for easy verification).
    // -------------------------------------------------------------------------
    // TlD packed struct: {opcode[2:0], size[2:0], source[7:0], data[127:0], error}
    // total = 3+3+8+128+1 = 143 bits

    localparam TL_GET = 3'd4;
    localparam TL_PUT_FULL = 3'd0;
    localparam TL_ACCESS_ACK = 3'd0;
    localparam TL_ACCESS_ACK_DATA = 3'd1;

    // Simple responder: when A is valid, latch the request and respond next cycle
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

    // Drive D channel response
    wire [2:0] req_opcode = resp_req[189:187];
    wire [2:0] req_size   = resp_req[186:184];
    wire [7:0] req_source = resp_req[183:176];
    wire [31:0] req_addr  = resp_req[175:144];

    // Response: echo address as data for reads, AccessAck for writes
    wire [2:0]   resp_opcode = (req_opcode == TL_GET) ? TL_ACCESS_ACK_DATA : TL_ACCESS_ACK;
    wire [127:0] resp_data   = {96'b0, req_addr};  // put address in low 32 bits

    always_comb begin
        tl_d_deq_valid = resp_pending;
        // TlD = {opcode[2:0], size[2:0], source[7:0], data[127:0], error}
        tl_d_deq_data = {resp_opcode, req_size, req_source, resp_data, 1'b0};
    end

    // -------------------------------------------------------------------------
    // DUT
    // -------------------------------------------------------------------------
    Spi2TLUL dut (
        .clk        (sys_clk),
        .rst_n      (sys_rst_n),
        .spi_clk    (spi_clk),
        .spi_rst_n  (spi_rst_n),

        // MOSI: always valid (pin is always presenting a bit)
        .q_mosi_pin_deq_valid (1'b1),
        .q_mosi_pin_deq_ready (),
        .q_mosi_pin_deq_data  (spi_mosi),

        // MISO
        .q_miso_pin_enq_valid (miso_enq_valid),
        .q_miso_pin_enq_ready (1'b1),
        .q_miso_pin_enq_data  (miso_enq_data),

        // TileLink
        .q_tl_a_enq_valid     (tl_a_enq_valid),
        .q_tl_a_enq_ready     (tl_a_enq_ready),
        .q_tl_a_enq_data      (tl_a_enq_data),
        .q_tl_d_deq_valid     (tl_d_deq_valid),
        .q_tl_d_deq_ready     (tl_d_deq_ready),
        .q_tl_d_deq_data      (tl_d_deq_data)
    );

    initial test_status = 0;

endmodule
