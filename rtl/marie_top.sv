// marie_top.sv — Xilinx Virtex UltraScale+ toplevel for the Marie Antoinette SoC
//
// Board interface:
//   - 100 MHz LVDS differential clock pair (sys_clk_p/n)
//   - Active-low pushbutton reset (sys_rst_n)
//   - UART TX/RX/RTS pins
//   - Memory interfaces (imem, dmem) to external SRAM/BRAM
//
// Clock generation via MMCME4_ADV (VCO = 1200 MHz):
//   cpu_clk  = 100 MHz  (VCO / 12)
//   xbar_clk = 150 MHz  (VCO / 8)
//   dev_clk  =  50 MHz  (VCO / 24)
//
// Reset: async assert, sync deassert per domain, gated by MMCM locked.

import rv32i_pkg::*;

module marie_top (
    // 100 MHz LVDS differential clock
    input  wire sys_clk_p,
    input  wire sys_clk_n,

    // Active-low system reset (directly from pad, no external debounce assumed)
    input  wire sys_rst_n,

    // UART physical pins
    output wire uart_tx,
    input  wire uart_rx,
    output wire uart_rts_n,
    input  wire uart_cts_n,

    // CPU instruction memory interface (cpu_clk domain)
    output wire        imem_rd_req_valid,
    input  wire        imem_rd_req_ready,
    output wire [31:0] imem_rd_req_data,
    input  wire        imem_rd_resp_valid,
    output wire        imem_rd_resp_ready,
    input  wire [31:0] imem_rd_resp_data,
    output wire        imem_wr_req_valid,
    input  wire        imem_wr_req_ready,
    output wire [63:0] imem_wr_req_data,

    // Device data memory interface (dev_clk domain)
    output wire        dmem_rd_req_valid,
    input  wire        dmem_rd_req_ready,
    output wire [31:0] dmem_rd_req_data,
    input  wire        dmem_rd_resp_valid,
    output wire        dmem_rd_resp_ready,
    input  wire [31:0] dmem_rd_resp_data,
    output wire        dmem_wr_req_valid,
    input  wire        dmem_wr_req_ready,
    output wire [63:0] dmem_wr_req_data
);

    // =========================================================================
    // Clock generation
    // =========================================================================

    // -------------------------------------------------------------------------
    // Differential input buffer → single-ended 100 MHz reference
    // -------------------------------------------------------------------------
    wire sys_clk_ibuf;

    IBUFDS #(
        .IOSTANDARD ("LVDS")
    ) u_ibufds (
        .I  (sys_clk_p),
        .IB (sys_clk_n),
        .O  (sys_clk_ibuf)
    );

    // -------------------------------------------------------------------------
    // MMCME4_ADV: 100 MHz → cpu (100), xbar (150), dev (50)
    // -------------------------------------------------------------------------
    // VCO = 100 MHz × 12.0 = 1200 MHz  (UltraScale+ range: 600–1440 MHz)
    // CLKOUT0 = 1200 / 12  = 100 MHz   (cpu)
    // CLKOUT1 = 1200 / 8   = 150 MHz   (xbar)
    // CLKOUT2 = 1200 / 24  =  50 MHz   (dev)
    // -------------------------------------------------------------------------
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
        .CLKINSEL   (1'b1),            // select CLKIN1
        .RST        (~sys_rst_n),       // MMCM reset (active-high)
        .PWRDWN     (1'b0),

        .CLKFBIN    (mmcm_clkfb),
        .CLKFBOUT   (mmcm_clkfb),      // internal feedback (zero-delay)

        .CLKOUT0    (cpu_clk_mmcm),
        .CLKOUT1    (xbar_clk_mmcm),
        .CLKOUT2    (dev_clk_mmcm),

        .LOCKED     (mmcm_locked),

        // Unused outputs / dynamic reconfig
        .CLKOUT0B   (),
        .CLKOUT1B   (),
        .CLKOUT2B   (),
        .CLKOUT3    (),
        .CLKOUT3B   (),
        .CLKOUT4    (),
        .CLKOUT5    (),
        .CLKOUT6    (),
        .CLKFBOUTB  (),
        .DO         (),
        .DRDY       (),
        .PSDONE     (),
        .CDDCREQ    (),
        .CDDCDONE   (),

        // Unused inputs
        .DADDR      (7'h0),
        .DI         (16'h0),
        .DWE        (1'b0),
        .DEN        (1'b0),
        .DCLK       (1'b0),
        .PSCLK      (1'b0),
        .PSEN       (1'b0),
        .PSINCDEC   (1'b0)
    );

    // -------------------------------------------------------------------------
    // Global clock buffers
    // -------------------------------------------------------------------------
    wire cpu_clk;
    wire xbar_clk;
    wire dev_clk;

    BUFGCE u_bufg_cpu  (.I(cpu_clk_mmcm),  .CE(1'b1), .O(cpu_clk));
    BUFGCE u_bufg_xbar (.I(xbar_clk_mmcm), .CE(1'b1), .O(xbar_clk));
    BUFGCE u_bufg_dev  (.I(dev_clk_mmcm),  .CE(1'b1), .O(dev_clk));

    // =========================================================================
    // Reset synchronisation
    // =========================================================================
    // Async assert (immediate on sys_rst_n or MMCM unlock), sync deassert
    // (clean release on the rising edge of each domain clock).
    // Two-FF synchroniser per domain — standard Xilinx reset methodology.

    wire async_rst_n = sys_rst_n & mmcm_locked;

    wire cpu_rst_n;
    wire xbar_rst_n;
    wire dev_rst_n;

    rst_sync u_rst_cpu  (.clk(cpu_clk),  .async_rst_n(async_rst_n), .sync_rst_n(cpu_rst_n));
    rst_sync u_rst_xbar (.clk(xbar_clk), .async_rst_n(async_rst_n), .sync_rst_n(xbar_rst_n));
    rst_sync u_rst_dev  (.clk(dev_clk),  .async_rst_n(async_rst_n), .sync_rst_n(dev_rst_n));

    // =========================================================================
    // UART pin wiring
    // =========================================================================
    wire uart_tx_enq_valid;
    wire uart_tx_enq_data;
    wire uart_rts_enq_valid;
    wire uart_rts_enq_data;

    // TX: output the bit when valid, else idle high
    assign uart_tx    = uart_tx_enq_valid ? uart_tx_enq_data : 1'b1;
    assign uart_rts_n = uart_rts_enq_valid ? ~uart_rts_enq_data : 1'b1;

    // =========================================================================
    // Marie SoC instance
    // =========================================================================
    Marie marie_inst (
        .cpu_clk    (cpu_clk),
        .cpu_rst_n  (cpu_rst_n),
        .xbar_clk   (xbar_clk),
        .xbar_rst_n (xbar_rst_n),
        .dev_clk    (dev_clk),
        .dev_rst_n  (dev_rst_n),

        // CPU instruction memory (cpu_clk domain)
        .q_CPUCore_imem_read_req_enq_valid  (imem_rd_req_valid),
        .q_CPUCore_imem_read_req_enq_ready  (imem_rd_req_ready),
        .q_CPUCore_imem_read_req_enq_data   (imem_rd_req_data),
        .q_CPUCore_imem_read_resp_deq_valid (imem_rd_resp_valid),
        .q_CPUCore_imem_read_resp_deq_ready (imem_rd_resp_ready),
        .q_CPUCore_imem_read_resp_deq_data  (imem_rd_resp_data),
        .q_CPUCore_imem_write_req_enq_valid (imem_wr_req_valid),
        .q_CPUCore_imem_write_req_enq_ready (imem_wr_req_ready),
        .q_CPUCore_imem_write_req_enq_data  (imem_wr_req_data),

        // Device data memory (dev_clk domain)
        .q_dev_mem_read_req_enq_valid  (dmem_rd_req_valid),
        .q_dev_mem_read_req_enq_ready  (dmem_rd_req_ready),
        .q_dev_mem_read_req_enq_data   (dmem_rd_req_data),
        .q_dev_mem_read_resp_deq_valid (dmem_rd_resp_valid),
        .q_dev_mem_read_resp_deq_ready (dmem_rd_resp_ready),
        .q_dev_mem_read_resp_deq_data  (dmem_rd_resp_data),
        .q_dev_mem_write_req_enq_valid (dmem_wr_req_valid),
        .q_dev_mem_write_req_enq_ready (dmem_wr_req_ready),
        .q_dev_mem_write_req_enq_data  (dmem_wr_req_data),

        // UART TX pin (Marie produces bits → physical TX)
        .q_UartPhy_tx_pin_enq_valid (uart_tx_enq_valid),
        .q_UartPhy_tx_pin_enq_ready (1'b1),
        .q_UartPhy_tx_pin_enq_data  (uart_tx_enq_data),

        // UART RX pin (physical RX → Marie consumes bits)
        .q_UartPhy_rx_pin_deq_valid (1'b1),
        .q_UartPhy_rx_pin_deq_ready (),
        .q_UartPhy_rx_pin_deq_data  (uart_rx),

        // UART RTS pin (Marie produces → physical RTS)
        .q_UartPhy_rts_pin_enq_valid (uart_rts_enq_valid),
        .q_UartPhy_rts_pin_enq_ready (1'b1),
        .q_UartPhy_rts_pin_enq_data  (uart_rts_enq_data),

        // UART CTS pin (physical CTS → Marie consumes, active-low inverted)
        .q_UartPhy_cts_pin_deq_valid (1'b1),
        .q_UartPhy_cts_pin_deq_ready (),
        .q_UartPhy_cts_pin_deq_data  (~uart_cts_n)
    );

endmodule

// =============================================================================
// Reset synchroniser — async assert, sync deassert
// =============================================================================
// Standard two-FF synchroniser with ASYNC_REG attribute for Xilinx place & route.
// Reset asserts immediately when async_rst_n drops; deasserts cleanly on the
// second rising clock edge after async_rst_n goes high.

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
