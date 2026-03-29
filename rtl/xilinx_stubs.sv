// xilinx_stubs.sv — Minimal stubs for Verilator lint of Xilinx UltraScale+ primitives.
// Not for synthesis — Vivado provides the real unisim library.

/* verilator lint_off DECLFILENAME */
/* verilator lint_off UNUSEDSIGNAL */
/* verilator lint_off UNUSEDPARAM */
/* verilator lint_off PINCONNECTEMPTY */

module IBUFDS #(
    parameter IOSTANDARD = "LVDS"
)(
    input  wire I,
    input  wire IB,
    output wire O
);
    assign O = I;
endmodule

module BUFGCE (
    input  wire I,
    input  wire CE,
    output wire O
);
    assign O = I & CE;
endmodule

module MMCME4_ADV #(
    parameter real CLKIN1_PERIOD      = 10.0,
    parameter real CLKFBOUT_MULT_F    = 10.0,
    parameter real CLKFBOUT_PHASE     = 0.0,
    parameter      DIVCLK_DIVIDE     = 1,
    parameter real CLKOUT0_DIVIDE_F   = 10.0,
    parameter real CLKOUT0_PHASE      = 0.0,
    parameter real CLKOUT0_DUTY_CYCLE = 0.5,
    parameter      CLKOUT1_DIVIDE    = 10,
    parameter real CLKOUT1_PHASE      = 0.0,
    parameter real CLKOUT1_DUTY_CYCLE = 0.5,
    parameter      CLKOUT2_DIVIDE    = 10,
    parameter real CLKOUT2_PHASE      = 0.0,
    parameter real CLKOUT2_DUTY_CYCLE = 0.5
)(
    input  wire        CLKIN1,
    input  wire        CLKIN2,
    input  wire        CLKINSEL,
    input  wire        RST,
    input  wire        PWRDWN,
    input  wire        CLKFBIN,
    output wire        CLKFBOUT,
    output wire        CLKFBOUTB,
    output wire        CLKOUT0,
    output wire        CLKOUT0B,
    output wire        CLKOUT1,
    output wire        CLKOUT1B,
    output wire        CLKOUT2,
    output wire        CLKOUT2B,
    output wire        CLKOUT3,
    output wire        CLKOUT3B,
    output wire        CLKOUT4,
    output wire        CLKOUT5,
    output wire        CLKOUT6,
    output wire        LOCKED,
    output wire [15:0] DO,
    output wire        DRDY,
    output wire        PSDONE,
    output wire        CDDCREQ,
    output wire        CDDCDONE,
    input  wire [6:0]  DADDR,
    input  wire [15:0] DI,
    input  wire        DWE,
    input  wire        DEN,
    input  wire        DCLK,
    input  wire        PSCLK,
    input  wire        PSEN,
    input  wire        PSINCDEC
);
    // Stub: pass-through clock, always locked
    assign CLKFBOUT  = CLKIN1;  // stub: no real feedback loop
    assign CLKFBOUTB = ~CLKFBIN;
    assign CLKOUT0   = CLKIN1;
    assign CLKOUT0B  = ~CLKIN1;
    assign CLKOUT1   = CLKIN1;
    assign CLKOUT1B  = ~CLKIN1;
    assign CLKOUT2   = CLKIN1;
    assign CLKOUT2B  = ~CLKIN1;
    assign CLKOUT3   = 1'b0;
    assign CLKOUT3B  = 1'b0;
    assign CLKOUT4   = 1'b0;
    assign CLKOUT5   = 1'b0;
    assign CLKOUT6   = 1'b0;
    assign LOCKED    = ~RST;
    assign DO        = 16'h0;
    assign DRDY      = 1'b0;
    assign PSDONE    = 1'b0;
    assign CDDCREQ   = 1'b0;
    assign CDDCDONE  = 1'b0;
endmodule
