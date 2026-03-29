// marie_top.sv — FPGA/ASIC toplevel for the Marie Antoinette SoC
//
// Routes UART TX/RX/RTS pins directly to top-level ports.
// Memory interfaces connect to external SRAM/BRAM controllers.
// This is the synthesis target — no DPI, no behavioral models.

import rv32i_pkg::*;

module marie_top (
    // Clock domains
    input  wire cpu_clk,
    input  wire cpu_rst_n,
    input  wire xbar_clk,
    input  wire xbar_rst_n,
    input  wire dev_clk,
    input  wire dev_rst_n,

    // UART physical pins
    output wire uart_tx,
    input  wire uart_rx,
    output wire uart_rts_n,

    // CPU instruction memory interface (cpu domain)
    output wire        imem_rd_req_valid,
    input  wire        imem_rd_req_ready,
    output wire [31:0] imem_rd_req_data,
    input  wire        imem_rd_resp_valid,
    output wire        imem_rd_resp_ready,
    input  wire [31:0] imem_rd_resp_data,
    output wire        imem_wr_req_valid,
    input  wire        imem_wr_req_ready,
    output wire [63:0] imem_wr_req_data,

    // Device data memory interface (dev domain)
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

    // -------------------------------------------------------------------------
    // UART pin wiring
    // -------------------------------------------------------------------------
    // Marie's tx_pin is a queue interface (enq side = output):
    //   enq_valid = Marie has a bit to send
    //   enq_ready = always accept (TX pin is a wire)
    //   enq_data  = the TX bit
    //
    // In a real FPGA, these are directly the TX pin level each cycle.
    // The ready/valid handshake is trivially satisfied.

    wire uart_tx_enq_valid;
    wire uart_tx_enq_data;
    wire uart_rts_enq_valid;
    wire uart_rts_enq_data;

    // TX: output the bit when valid, else idle high
    assign uart_tx    = uart_tx_enq_valid ? uart_tx_enq_data : 1'b1;
    assign uart_rts_n = uart_rts_enq_valid ? ~uart_rts_enq_data : 1'b1;

    // -------------------------------------------------------------------------
    // Marie SoC instance
    // -------------------------------------------------------------------------
    Marie marie_inst (
        .cpu_clk   (cpu_clk),
        .cpu_rst_n (cpu_rst_n),
        .xbar_clk   (xbar_clk),
        .xbar_rst_n (xbar_rst_n),
        .dev_clk   (dev_clk),
        .dev_rst_n (dev_rst_n),

        // CPU instruction memory
        .q_CPUCore_imem_read_req_enq_valid  (imem_rd_req_valid),
        .q_CPUCore_imem_read_req_enq_ready  (imem_rd_req_ready),
        .q_CPUCore_imem_read_req_enq_data   (imem_rd_req_data),
        .q_CPUCore_imem_read_resp_deq_valid (imem_rd_resp_valid),
        .q_CPUCore_imem_read_resp_deq_ready (imem_rd_resp_ready),
        .q_CPUCore_imem_read_resp_deq_data  (imem_rd_resp_data),
        .q_CPUCore_imem_write_req_enq_valid (imem_wr_req_valid),
        .q_CPUCore_imem_write_req_enq_ready (imem_wr_req_ready),
        .q_CPUCore_imem_write_req_enq_data  (imem_wr_req_data),

        // Device data memory
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
        .q_UartPhy_rx_pin_deq_ready (),       // Marie controls when to read
        .q_UartPhy_rx_pin_deq_data  (uart_rx),

        // UART RTS pin (Marie produces → physical RTS)
        .q_UartPhy_rts_pin_enq_valid (uart_rts_enq_valid),
        .q_UartPhy_rts_pin_enq_ready (1'b1),
        .q_UartPhy_rts_pin_enq_data  (uart_rts_enq_data)
    );

endmodule
