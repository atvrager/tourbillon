// marie_fpga.sv — Standalone FPGA toplevel for Marie Antoinette SoC
//
// Self-contained: internal instruction and data memory initialised from
// hex files baked into the bitstream via $readmemh.  Just add clocks,
// reset, and UART — no external SRAM needed.
//
// Memory map:
//   imem: 4K × 32-bit (16 KB) distributed RAM — cpu_clk domain
//   dmem: 16K × 32-bit (64 KB) block RAM — dev_clk domain
//
// Hex files (set via parameters or Vivado generics):
//   IMEM_FILE = "hello.hex"   — loaded into both imem and dmem
//   DMEM_FILE = "hello.hex"

import rv32i_pkg::*;

module marie_fpga #(
    parameter IMEM_FILE = "hello.hex",
    parameter DMEM_FILE = "hello.hex",
    parameter IMEM_DEPTH = 4096,    // 16 KB
    parameter DMEM_DEPTH = 16384    // 64 KB
)(
    // 100 MHz LVDS differential clock
    input  wire sys_clk_p,
    input  wire sys_clk_n,

    // Active-low system reset
    input  wire sys_rst_n,

    // UART
    output wire uart_tx,
    input  wire uart_rx,
    output wire uart_rts_n,
    input  wire uart_cts_n
);

    // =========================================================================
    // Clock generation (same as marie_top.sv)
    // =========================================================================
    wire sys_clk_ibuf;

    IBUFDS #(.IOSTANDARD("LVDS")) u_ibufds (
        .I(sys_clk_p), .IB(sys_clk_n), .O(sys_clk_ibuf)
    );

    wire mmcm_clkfb, mmcm_locked;
    wire cpu_clk_mmcm, xbar_clk_mmcm, dev_clk_mmcm;

    MMCME4_ADV #(
        .CLKIN1_PERIOD      (10.000),
        .CLKFBOUT_MULT_F    (12.000),
        .CLKFBOUT_PHASE     (0.000),
        .DIVCLK_DIVIDE      (1),
        .CLKOUT0_DIVIDE_F   (12.000),  // 100 MHz — cpu
        .CLKOUT0_PHASE      (0.000),
        .CLKOUT0_DUTY_CYCLE (0.500),
        .CLKOUT1_DIVIDE     (8),       // 150 MHz — xbar
        .CLKOUT1_PHASE      (0.000),
        .CLKOUT1_DUTY_CYCLE (0.500),
        .CLKOUT2_DIVIDE     (24),      //  50 MHz — dev
        .CLKOUT2_PHASE      (0.000),
        .CLKOUT2_DUTY_CYCLE (0.500)
    ) u_mmcm (
        .CLKIN1(sys_clk_ibuf), .CLKIN2(1'b0), .CLKINSEL(1'b1),
        .RST(~sys_rst_n), .PWRDWN(1'b0),
        .CLKFBIN(mmcm_clkfb), .CLKFBOUT(mmcm_clkfb),
        .CLKOUT0(cpu_clk_mmcm), .CLKOUT1(xbar_clk_mmcm), .CLKOUT2(dev_clk_mmcm),
        .LOCKED(mmcm_locked),
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
    // Instruction memory — distributed RAM, cpu_clk domain
    // =========================================================================
    // Combinational read: Fetch does put(req) + take(resp) in one cycle.
    wire        imem_rd_req_valid,  imem_rd_req_ready;
    wire [31:0] imem_rd_req_data;
    wire        imem_rd_resp_valid, imem_rd_resp_ready;
    wire [31:0] imem_rd_resp_data;
    wire        imem_wr_req_valid,  imem_wr_req_ready;
    wire [63:0] imem_wr_req_data;

    fpga_mem #(
        .DEPTH    (IMEM_DEPTH),
        .MEMFILE  (IMEM_FILE),
        .USE_BRAM (0)               // distributed RAM — same-cycle read
    ) u_imem (
        .clk(cpu_clk), .rst_n(cpu_rst_n),
        .rd_req_valid(imem_rd_req_valid),   .rd_req_ready(imem_rd_req_ready),
        .rd_req_data(imem_rd_req_data),
        .rd_resp_valid(imem_rd_resp_valid), .rd_resp_ready(imem_rd_resp_ready),
        .rd_resp_data(imem_rd_resp_data),
        .wr_req_valid(imem_wr_req_valid),   .wr_req_ready(imem_wr_req_ready),
        .wr_req_data(imem_wr_req_data)
    );

    // =========================================================================
    // Data memory — block RAM, dev_clk domain
    // =========================================================================
    // Registered read with response latch. MemDevice polls via try_take.
    wire        dmem_rd_req_valid,  dmem_rd_req_ready;
    wire [31:0] dmem_rd_req_data;
    wire        dmem_rd_resp_valid, dmem_rd_resp_ready;
    wire [31:0] dmem_rd_resp_data;
    wire        dmem_wr_req_valid,  dmem_wr_req_ready;
    wire [63:0] dmem_wr_req_data;

    fpga_mem #(
        .DEPTH    (DMEM_DEPTH),
        .MEMFILE  (DMEM_FILE),
        .USE_BRAM (1)               // block RAM — 1-cycle registered read
    ) u_dmem (
        .clk(dev_clk), .rst_n(dev_rst_n),
        .rd_req_valid(dmem_rd_req_valid),   .rd_req_ready(dmem_rd_req_ready),
        .rd_req_data(dmem_rd_req_data),
        .rd_resp_valid(dmem_rd_resp_valid), .rd_resp_ready(dmem_rd_resp_ready),
        .rd_resp_data(dmem_rd_resp_data),
        .wr_req_valid(dmem_wr_req_valid),   .wr_req_ready(dmem_wr_req_ready),
        .wr_req_data(dmem_wr_req_data)
    );

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

        // Instruction memory (cpu_clk, distributed RAM)
        .q_CPUCore_imem_read_req_enq_valid  (imem_rd_req_valid),
        .q_CPUCore_imem_read_req_enq_ready  (imem_rd_req_ready),
        .q_CPUCore_imem_read_req_enq_data   (imem_rd_req_data),
        .q_CPUCore_imem_read_resp_deq_valid (imem_rd_resp_valid),
        .q_CPUCore_imem_read_resp_deq_ready (imem_rd_resp_ready),
        .q_CPUCore_imem_read_resp_deq_data  (imem_rd_resp_data),
        .q_CPUCore_imem_write_req_enq_valid (imem_wr_req_valid),
        .q_CPUCore_imem_write_req_enq_ready (imem_wr_req_ready),
        .q_CPUCore_imem_write_req_enq_data  (imem_wr_req_data),

        // Data memory (dev_clk, block RAM)
        .q_dev_mem_read_req_enq_valid  (dmem_rd_req_valid),
        .q_dev_mem_read_req_enq_ready  (dmem_rd_req_ready),
        .q_dev_mem_read_req_enq_data   (dmem_rd_req_data),
        .q_dev_mem_read_resp_deq_valid (dmem_rd_resp_valid),
        .q_dev_mem_read_resp_deq_ready (dmem_rd_resp_ready),
        .q_dev_mem_read_resp_deq_data  (dmem_rd_resp_data),
        .q_dev_mem_write_req_enq_valid (dmem_wr_req_valid),
        .q_dev_mem_write_req_enq_ready (dmem_wr_req_ready),
        .q_dev_mem_write_req_enq_data  (dmem_wr_req_data),

        // UART TX
        .q_UartPhy_tx_pin_enq_valid (uart_tx_enq_valid),
        .q_UartPhy_tx_pin_enq_ready (1'b1),
        .q_UartPhy_tx_pin_enq_data  (uart_tx_enq_data),

        // UART RX
        .q_UartPhy_rx_pin_deq_valid (1'b1),
        .q_UartPhy_rx_pin_deq_ready (),
        .q_UartPhy_rx_pin_deq_data  (uart_rx),

        // UART RTS
        .q_UartPhy_rts_pin_enq_valid (uart_rts_enq_valid),
        .q_UartPhy_rts_pin_enq_ready (1'b1),
        .q_UartPhy_rts_pin_enq_data  (uart_rts_enq_data),

        // UART CTS (active-low from pin, inverted to active-high for Marie)
        .q_UartPhy_cts_pin_deq_valid (1'b1),
        .q_UartPhy_cts_pin_deq_ready (),
        .q_UartPhy_cts_pin_deq_data  (~uart_cts_n)
    );

endmodule
