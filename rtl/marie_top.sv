// marie_top.sv — Xilinx Virtex UltraScale+ toplevel for the Marie Antoinette SoC
//
// Two modes controlled by the STANDALONE parameter:
//   STANDALONE = 0: Memory interfaces exposed as ports (external SRAM/controller)
//   STANDALONE = 1: Internal BRAM initialised from hex via $readmemh
//                   (imem = distributed RAM, dmem = block RAM)
//
// Board interface:
//   - 100 MHz LVDS differential clock pair (sys_clk_p/n)
//   - Active-low pushbutton reset (sys_rst_n)
//   - UART TX/RX/RTS/CTS pins
//
// Clock generation via MMCME4_ADV (VCO = 1200 MHz):
//   cpu_clk  = 100 MHz  (VCO / 12)
//   xbar_clk = 150 MHz  (VCO / 8)
//   dev_clk  =  50 MHz  (VCO / 24)
//
// Reset: async assert, sync deassert per domain, gated by MMCM locked.

import rv32i_pkg::*;
import cheri_pkg::*;

module marie_top #(
    parameter STANDALONE  = 0,
    parameter IMEM_FILE   = "hello.hex",
    parameter DMEM_FILE   = "hello.hex",
    parameter IMEM_DEPTH  = 4096,     // 16 KB  (STANDALONE only)
    parameter DMEM_DEPTH  = 16384,    // 64 KB  (STANDALONE only)
    parameter TMEM_DEPTH  = 8192      // 1 tag per 8 bytes of 64 KB (STANDALONE only)
)(
    // 100 MHz LVDS differential clock
    input  wire sys_clk_p,
    input  wire sys_clk_n,

    // Active-low system reset
    input  wire sys_rst_n,

    // UART physical pins
    output wire uart_tx,
    input  wire uart_rx,
    output wire uart_rts_n,
    input  wire uart_cts_n,

    // Memory interfaces (active when STANDALONE = 0; unused when 1)
    output wire        imem_rd_req_valid,
    input  wire        imem_rd_req_ready,
    output wire [31:0] imem_rd_req_data,
    input  wire        imem_rd_resp_valid,
    output wire        imem_rd_resp_ready,
    input  wire [31:0] imem_rd_resp_data,
    output wire        imem_wr_req_valid,
    input  wire        imem_wr_req_ready,
    output wire [63:0] imem_wr_req_data,

    output wire        dmem_rd_req_valid,
    input  wire        dmem_rd_req_ready,
    output wire [31:0] dmem_rd_req_data,
    input  wire        dmem_rd_resp_valid,
    output wire        dmem_rd_resp_ready,
    input  wire [31:0] dmem_rd_resp_data,
    output wire        dmem_wr_req_valid,
    input  wire        dmem_wr_req_ready,
    output wire [63:0] dmem_wr_req_data,

    // Tag memory interfaces (active when STANDALONE = 0; unused when 1)
    output wire        tmem_rd_req_valid,
    input  wire        tmem_rd_req_ready,
    output wire [31:0] tmem_rd_req_data,
    input  wire        tmem_rd_resp_valid,
    output wire        tmem_rd_resp_ready,
    input  wire        tmem_rd_resp_data,      // 1-bit tag
    output wire        tmem_wr_req_valid,
    input  wire        tmem_wr_req_ready,
    output wire [32:0] tmem_wr_req_data
);

    // =========================================================================
    // Clock generation
    // =========================================================================
    wire sys_clk_ibuf;

    IBUFDS #(
        .IOSTANDARD ("LVDS")
    ) u_ibufds (
        .I  (sys_clk_p),
        .IB (sys_clk_n),
        .O  (sys_clk_ibuf)
    );

    wire mmcm_clkfb;
    wire mmcm_locked;
    wire cpu_clk_mmcm;
    wire xbar_clk_mmcm;
    wire dev_clk_mmcm;

    MMCME4_ADV #(
        .CLKIN1_PERIOD      (10.000),   // 100 MHz input
        .CLKFBOUT_MULT_F    (12.000),   // VCO = 1200 MHz
        .CLKFBOUT_PHASE     (0.000),
        .DIVCLK_DIVIDE      (1),
        .CLKOUT0_DIVIDE_F   (12.000),   // 100 MHz — cpu
        .CLKOUT0_PHASE      (0.000),
        .CLKOUT0_DUTY_CYCLE (0.500),
        .CLKOUT1_DIVIDE     (8),        // 150 MHz — xbar
        .CLKOUT1_PHASE      (0.000),
        .CLKOUT1_DUTY_CYCLE (0.500),
        .CLKOUT2_DIVIDE     (24),       //  50 MHz — dev
        .CLKOUT2_PHASE      (0.000),
        .CLKOUT2_DUTY_CYCLE (0.500)
    ) u_mmcm (
        .CLKIN1     (sys_clk_ibuf),
        .CLKIN2     (1'b0),
        .CLKINSEL   (1'b1),
        .RST        (~sys_rst_n),
        .PWRDWN     (1'b0),
        .CLKFBIN    (mmcm_clkfb),
        .CLKFBOUT   (mmcm_clkfb),
        .CLKOUT0    (cpu_clk_mmcm),
        .CLKOUT1    (xbar_clk_mmcm),
        .CLKOUT2    (dev_clk_mmcm),
        .LOCKED     (mmcm_locked),
        .CLKOUT0B(), .CLKOUT1B(), .CLKOUT2B(),
        .CLKOUT3(), .CLKOUT3B(), .CLKOUT4(), .CLKOUT5(), .CLKOUT6(),
        .CLKFBOUTB(), .DO(), .DRDY(), .PSDONE(), .CDDCREQ(), .CDDCDONE(),
        .DADDR(7'h0), .DI(16'h0), .DWE(1'b0), .DEN(1'b0), .DCLK(1'b0),
        .PSCLK(1'b0), .PSEN(1'b0), .PSINCDEC(1'b0)
    );

    wire cpu_clk, xbar_clk, dev_clk;

    BUFGCE u_bufg_cpu  (.I(cpu_clk_mmcm),  .CE(1'b1), .O(cpu_clk));
    BUFGCE u_bufg_xbar (.I(xbar_clk_mmcm), .CE(1'b1), .O(xbar_clk));
    BUFGCE u_bufg_dev  (.I(dev_clk_mmcm),  .CE(1'b1), .O(dev_clk));

    // =========================================================================
    // Reset synchronisation
    // =========================================================================
    wire async_rst_n = sys_rst_n & mmcm_locked;
    wire cpu_rst_n, xbar_rst_n, dev_rst_n;

    rst_sync u_rst_cpu  (.clk(cpu_clk),  .async_rst_n(async_rst_n), .sync_rst_n(cpu_rst_n));
    rst_sync u_rst_xbar (.clk(xbar_clk), .async_rst_n(async_rst_n), .sync_rst_n(xbar_rst_n));
    rst_sync u_rst_dev  (.clk(dev_clk),  .async_rst_n(async_rst_n), .sync_rst_n(dev_rst_n));

    // =========================================================================
    // Memory interconnect
    // =========================================================================
    // Internal wires — driven by either external ports or internal BRAMs.
    wire        imem_req_v,  imem_req_r;
    wire [31:0] imem_req_d;
    wire        imem_resp_v, imem_resp_r;
    wire [31:0] imem_resp_d;
    wire        imem_wr_v,   imem_wr_r;
    wire [63:0] imem_wr_d;

    wire        dmem_req_v,  dmem_req_r;
    wire [31:0] dmem_req_d;
    wire        dmem_resp_v, dmem_resp_r;
    wire [31:0] dmem_resp_d;
    wire        dmem_wr_v,   dmem_wr_r;
    wire [63:0] dmem_wr_d;

    wire        tmem_req_v,  tmem_req_r;
    wire [31:0] tmem_req_d;
    wire        tmem_resp_v, tmem_resp_r;
    wire        tmem_resp_d;               // 1-bit tag
    wire        tmem_wr_v,   tmem_wr_r;
    wire [32:0] tmem_wr_d;

    generate
        if (STANDALONE) begin : gen_bram
            // ----- Internal BRAMs baked with hex -----
            fpga_mem #(
                .DEPTH(IMEM_DEPTH), .MEMFILE(IMEM_FILE), .USE_BRAM(0)
            ) u_imem (
                .clk(cpu_clk), .rst_n(cpu_rst_n),
                .rd_req_valid(imem_req_v),   .rd_req_ready(imem_req_r),
                .rd_req_data(imem_req_d),
                .rd_resp_valid(imem_resp_v), .rd_resp_ready(imem_resp_r),
                .rd_resp_data(imem_resp_d),
                .wr_req_valid(imem_wr_v),    .wr_req_ready(imem_wr_r),
                .wr_req_data(imem_wr_d)
            );

            fpga_mem #(
                .DEPTH(DMEM_DEPTH), .MEMFILE(DMEM_FILE), .USE_BRAM(1)
            ) u_dmem (
                .clk(dev_clk), .rst_n(dev_rst_n),
                .rd_req_valid(dmem_req_v),   .rd_req_ready(dmem_req_r),
                .rd_req_data(dmem_req_d),
                .rd_resp_valid(dmem_resp_v), .rd_resp_ready(dmem_resp_r),
                .rd_resp_data(dmem_resp_d),
                .wr_req_valid(dmem_wr_v),    .wr_req_ready(dmem_wr_r),
                .wr_req_data(dmem_wr_d)
            );

            fpga_tag_mem #(
                .DEPTH(TMEM_DEPTH)
            ) u_tmem (
                .clk(cpu_clk), .rst_n(cpu_rst_n),
                .rd_req_valid(tmem_req_v),   .rd_req_ready(tmem_req_r),
                .rd_req_data(tmem_req_d),
                .rd_resp_valid(tmem_resp_v), .rd_resp_ready(tmem_resp_r),
                .rd_resp_data(tmem_resp_d),
                .wr_req_valid(tmem_wr_v),    .wr_req_ready(tmem_wr_r),
                .wr_req_data(tmem_wr_d)
            );

            // External memory ports unused in standalone mode
            assign imem_rd_req_valid = 1'b0;
            assign imem_rd_req_data  = '0;
            assign imem_rd_resp_ready = 1'b0;
            assign imem_wr_req_valid = 1'b0;
            assign imem_wr_req_data  = '0;
            assign dmem_rd_req_valid = 1'b0;
            assign dmem_rd_req_data  = '0;
            assign dmem_rd_resp_ready = 1'b0;
            assign dmem_wr_req_valid = 1'b0;
            assign dmem_wr_req_data  = '0;
            assign tmem_rd_req_valid  = 1'b0;
            assign tmem_rd_req_data   = '0;
            assign tmem_rd_resp_ready = 1'b0;
            assign tmem_wr_req_valid  = 1'b0;
            assign tmem_wr_req_data   = '0;
        end else begin : gen_ext
            // ----- External memory ports -----
            assign imem_rd_req_valid = imem_req_v;
            assign imem_req_r        = imem_rd_req_ready;
            assign imem_rd_req_data  = imem_req_d;
            assign imem_resp_v       = imem_rd_resp_valid;
            assign imem_rd_resp_ready = imem_resp_r;
            assign imem_resp_d       = imem_rd_resp_data;
            assign imem_wr_req_valid = imem_wr_v;
            assign imem_wr_r         = imem_wr_req_ready;
            assign imem_wr_req_data  = imem_wr_d;

            assign dmem_rd_req_valid = dmem_req_v;
            assign dmem_req_r        = dmem_rd_req_ready;
            assign dmem_rd_req_data  = dmem_req_d;
            assign dmem_resp_v       = dmem_rd_resp_valid;
            assign dmem_rd_resp_ready = dmem_resp_r;
            assign dmem_resp_d       = dmem_rd_resp_data;
            assign dmem_wr_req_valid = dmem_wr_v;
            assign dmem_wr_r         = dmem_wr_req_ready;
            assign dmem_wr_req_data  = dmem_wr_d;

            assign tmem_rd_req_valid  = tmem_req_v;
            assign tmem_req_r         = tmem_rd_req_ready;
            assign tmem_rd_req_data   = tmem_req_d;
            assign tmem_resp_v        = tmem_rd_resp_valid;
            assign tmem_rd_resp_ready = tmem_resp_r;
            assign tmem_resp_d        = tmem_rd_resp_data;
            assign tmem_wr_req_valid  = tmem_wr_v;
            assign tmem_wr_r          = tmem_wr_req_ready;
            assign tmem_wr_req_data   = tmem_wr_d;
        end
    endgenerate

    // =========================================================================
    // UART pin wiring
    // =========================================================================
    wire uart_tx_enq_valid, uart_tx_enq_data;
    wire uart_rts_enq_valid, uart_rts_enq_data;

    assign uart_tx    = uart_tx_enq_valid ? uart_tx_enq_data : 1'b1;
    assign uart_rts_n = uart_rts_enq_valid ? ~uart_rts_enq_data : 1'b1;

    // =========================================================================
    // Marie SoC instance
    // =========================================================================
    Marie marie_inst (
        .cpu_clk(cpu_clk), .cpu_rst_n(cpu_rst_n),
        .xbar_clk(xbar_clk), .xbar_rst_n(xbar_rst_n),
        .dev_clk(dev_clk), .dev_rst_n(dev_rst_n),

        // Instruction memory
        .q_CPUCore_imem_read_req_enq_valid  (imem_req_v),
        .q_CPUCore_imem_read_req_enq_ready  (imem_req_r),
        .q_CPUCore_imem_read_req_enq_data   (imem_req_d),
        .q_CPUCore_imem_read_resp_deq_valid (imem_resp_v),
        .q_CPUCore_imem_read_resp_deq_ready (imem_resp_r),
        .q_CPUCore_imem_read_resp_deq_data  (imem_resp_d),
        .q_CPUCore_imem_write_req_enq_valid (imem_wr_v),
        .q_CPUCore_imem_write_req_enq_ready (imem_wr_r),
        .q_CPUCore_imem_write_req_enq_data  (imem_wr_d),

        // Data memory
        .q_dev_mem_read_req_enq_valid  (dmem_req_v),
        .q_dev_mem_read_req_enq_ready  (dmem_req_r),
        .q_dev_mem_read_req_enq_data   (dmem_req_d),
        .q_dev_mem_read_resp_deq_valid (dmem_resp_v),
        .q_dev_mem_read_resp_deq_ready (dmem_resp_r),
        .q_dev_mem_read_resp_deq_data  (dmem_resp_d),
        .q_dev_mem_write_req_enq_valid (dmem_wr_v),
        .q_dev_mem_write_req_enq_ready (dmem_wr_r),
        .q_dev_mem_write_req_enq_data  (dmem_wr_d),

        // Tag memory (cpu domain)
        .q_cpu_tmem_read_req_enq_valid  (tmem_req_v),
        .q_cpu_tmem_read_req_enq_ready  (tmem_req_r),
        .q_cpu_tmem_read_req_enq_data   (tmem_req_d),
        .q_cpu_tmem_read_resp_deq_valid (tmem_resp_v),
        .q_cpu_tmem_read_resp_deq_ready (tmem_resp_r),
        .q_cpu_tmem_read_resp_deq_data  (tmem_resp_d),
        .q_cpu_tmem_write_req_enq_valid (tmem_wr_v),
        .q_cpu_tmem_write_req_enq_ready (tmem_wr_r),
        .q_cpu_tmem_write_req_enq_data  (tmem_wr_d),

        // UART
        .q_UartPhy_tx_pin_enq_valid (uart_tx_enq_valid),
        .q_UartPhy_tx_pin_enq_ready (1'b1),
        .q_UartPhy_tx_pin_enq_data  (uart_tx_enq_data),
        .q_UartPhy_rx_pin_deq_valid (1'b1),
        .q_UartPhy_rx_pin_deq_ready (),
        .q_UartPhy_rx_pin_deq_data  (uart_rx),
        .q_UartPhy_rts_pin_enq_valid (uart_rts_enq_valid),
        .q_UartPhy_rts_pin_enq_ready (1'b1),
        .q_UartPhy_rts_pin_enq_data  (uart_rts_enq_data),
        .q_UartPhy_cts_pin_deq_valid (1'b1),
        .q_UartPhy_cts_pin_deq_ready (),
        .q_UartPhy_cts_pin_deq_data  (~uart_cts_n)
    );

endmodule

// =============================================================================
// Reset synchroniser — async assert, sync deassert
// =============================================================================

module rst_sync (
    input  wire clk,
    input  wire async_rst_n,
    output wire sync_rst_n
);

    (* ASYNC_REG = "TRUE" *) reg [1:0] sync_ff;

    always_ff @(posedge clk or negedge async_rst_n) begin
        if (!async_rst_n)
            sync_ff <= 2'b00;
        else
            sync_ff <= {sync_ff[0], 1'b1};
    end

    assign sync_rst_n = sync_ff[1];

endmodule
