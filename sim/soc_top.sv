// soc_top.sv — Multi-clock simulation wrapper for the Marie Antoinette SoC
//
// Wires the generated Marie module to behavioral mem_model instances for
// instruction memory (imem, cpu domain) and data memory (dev_mem, dev domain).
// UART TX pin is deserialized back into bytes and emitted via DPI uart_sim_tx.
// UART RX pin is held idle (high). RTS is consumed and discarded.
//
// Three clock domains are driven independently by the C++ testbench:
// cpu_clk, xbar_clk, dev_clk.

/* verilator lint_off IMPORTSTAR */
import rv32i_pkg::*;
import cheri_pkg::*;
import soc_pkg::*;
import manifest_pkg::*;

/* verilator lint_off UNOPTFLAT */
/* verilator lint_off UNUSEDSIGNAL */

module soc_top (
    input  wire         cpu_clk,
    input  wire         cpu_rst_n,
    input  wire         xbar_clk,
    input  wire         xbar_rst_n,
    input  wire         dev_clk,
    input  wire         dev_rst_n,
    output logic [31:0] tohost,
    output wire         uart_tx_idle   // 1 when TX deserializer is quiescent
);

    // -------------------------------------------------------------------------
    // DPI function for TX output
    // -------------------------------------------------------------------------
    import "DPI-C" function void uart_sim_tx(input byte unsigned ch);

    // -------------------------------------------------------------------------
    // CPU instruction memory interconnect (cpu domain)
    // -------------------------------------------------------------------------
    wire         imem_rd_req_valid;
    wire         imem_rd_req_ready;
    wire [31:0]  imem_rd_req_data;
    wire         imem_rd_resp_valid;
    wire         imem_rd_resp_ready;
    wire [31:0]  imem_rd_resp_data;
    wire         imem_wr_req_valid;
    wire         imem_wr_req_ready;
    wire [63:0]  imem_wr_req_data;

    // -------------------------------------------------------------------------
    // Device data memory interconnect (dev domain)
    // -------------------------------------------------------------------------
    wire         dmem_rd_req_valid;
    wire         dmem_rd_req_ready;
    wire [31:0]  dmem_rd_req_data;
    wire         dmem_rd_resp_valid;
    wire         dmem_rd_resp_ready;
    wire [31:0]  dmem_rd_resp_data;
    wire         dmem_wr_req_valid;
    wire         dmem_wr_req_ready;
    wire [63:0]  dmem_wr_req_data;

    // -------------------------------------------------------------------------
    // UART pin interconnect (dev domain)
    // -------------------------------------------------------------------------
    wire         uart_tx_enq_valid;
    wire         uart_tx_enq_ready;
    wire         uart_tx_enq_data;    // TX bit from Marie
    wire         uart_rx_deq_valid;
    wire         uart_rx_deq_ready;
    wire         uart_rx_deq_data;    // RX bit to Marie
    wire         uart_rts_enq_valid;
    wire         uart_rts_enq_ready;
    wire         uart_rts_enq_data;   // RTS output from Marie
    wire         uart_cts_deq_valid;
    wire         uart_cts_deq_ready;
    wire         uart_cts_deq_data;   // CTS input to Marie

    // -------------------------------------------------------------------------
    // Tag memory interconnect (cpu domain)
    // -------------------------------------------------------------------------
    wire         tmem_rd_req_valid;
    wire         tmem_rd_req_ready;
    wire [31:0]  tmem_rd_req_data;
    wire         tmem_rd_resp_valid;
    wire         tmem_rd_resp_ready;
    wire         tmem_rd_resp_data;       // 1-bit to Marie
    wire [31:0]  tmem_rd_resp_data_wide;  // 32-bit from mem_model
    assign tmem_rd_resp_data = tmem_rd_resp_data_wide[0];
    wire         tmem_wr_req_valid;
    wire         tmem_wr_req_ready;
    wire [32:0]  tmem_wr_req_data;

    // -------------------------------------------------------------------------
    // Marie SoC instance
    // -------------------------------------------------------------------------
    Marie marie_inst (
        // CPU domain
        .cpu_clk   (cpu_clk),
        .cpu_rst_n (cpu_rst_n),

        // Crossbar domain
        .xbar_clk   (xbar_clk),
        .xbar_rst_n (xbar_rst_n),

        // Device domain
        .dev_clk   (dev_clk),
        .dev_rst_n (dev_rst_n),

        // CPU instruction memory (cpu domain)
        .q_CPUCore_imem_read_req_enq_valid  (imem_rd_req_valid),
        .q_CPUCore_imem_read_req_enq_ready  (imem_rd_req_ready),
        .q_CPUCore_imem_read_req_enq_data   (imem_rd_req_data),
        .q_CPUCore_imem_read_resp_deq_valid (imem_rd_resp_valid),
        .q_CPUCore_imem_read_resp_deq_ready (imem_rd_resp_ready),
        .q_CPUCore_imem_read_resp_deq_data  (imem_rd_resp_data),
        .q_CPUCore_imem_write_req_enq_valid (imem_wr_req_valid),
        .q_CPUCore_imem_write_req_enq_ready (imem_wr_req_ready),
        .q_CPUCore_imem_write_req_enq_data  (imem_wr_req_data),

        // Tag memory (cpu domain)
        .q_cpu_tmem_read_req_enq_valid  (tmem_rd_req_valid),
        .q_cpu_tmem_read_req_enq_ready  (tmem_rd_req_ready),
        .q_cpu_tmem_read_req_enq_data   (tmem_rd_req_data),
        .q_cpu_tmem_read_resp_deq_valid (tmem_rd_resp_valid),
        .q_cpu_tmem_read_resp_deq_ready (tmem_rd_resp_ready),
        .q_cpu_tmem_read_resp_deq_data  (tmem_rd_resp_data),
        .q_cpu_tmem_write_req_enq_valid (tmem_wr_req_valid),
        .q_cpu_tmem_write_req_enq_ready (tmem_wr_req_ready),
        .q_cpu_tmem_write_req_enq_data  (tmem_wr_req_data),

        // Device data memory (dev domain)
        .q_dev_mem_read_req_enq_valid  (dmem_rd_req_valid),
        .q_dev_mem_read_req_enq_ready  (dmem_rd_req_ready),
        .q_dev_mem_read_req_enq_data   (dmem_rd_req_data),
        .q_dev_mem_read_resp_deq_valid (dmem_rd_resp_valid),
        .q_dev_mem_read_resp_deq_ready (dmem_rd_resp_ready),
        .q_dev_mem_read_resp_deq_data  (dmem_rd_resp_data),
        .q_dev_mem_write_req_enq_valid (dmem_wr_req_valid),
        .q_dev_mem_write_req_enq_ready (dmem_wr_req_ready),
        .q_dev_mem_write_req_enq_data  (dmem_wr_req_data),

        // UART TX pin (Marie produces bits)
        .q_UartPhy_tx_pin_enq_valid (uart_tx_enq_valid),
        .q_UartPhy_tx_pin_enq_ready (uart_tx_enq_ready),
        .q_UartPhy_tx_pin_enq_data  (uart_tx_enq_data),

        // UART RX pin (Marie consumes bits)
        .q_UartPhy_rx_pin_deq_valid (uart_rx_deq_valid),
        .q_UartPhy_rx_pin_deq_ready (uart_rx_deq_ready),
        .q_UartPhy_rx_pin_deq_data  (uart_rx_deq_data),

        // UART RTS pin (Marie produces)
        .q_UartPhy_rts_pin_enq_valid (uart_rts_enq_valid),
        .q_UartPhy_rts_pin_enq_ready (uart_rts_enq_ready),
        .q_UartPhy_rts_pin_enq_data  (uart_rts_enq_data),

        // UART CTS pin (Marie consumes — flow control input)
        .q_UartPhy_cts_pin_deq_valid (uart_cts_deq_valid),
        .q_UartPhy_cts_pin_deq_ready (uart_cts_deq_ready),
        .q_UartPhy_cts_pin_deq_data  (uart_cts_deq_data)
    );

    // -------------------------------------------------------------------------
    // UART TX DPI bridge — deserialize bit stream back to bytes
    // -------------------------------------------------------------------------
    // The UartTx shift register outputs one bit per cycle on tx_pin.
    // Each bit is held for BAUD_DIV+1 cycles. We sample at the baud rate
    // midpoint and reconstruct bytes.
    localparam BAUD_DIV = DEV_FREQ_HZ / 921600;  // from soc_pkg

    // Always accept TX bits from Marie
    assign uart_tx_enq_ready = 1'b1;

    // TX deserializer state machine:
    // State 0: IDLE — wait for falling edge (start bit)
    // State 1: wait 1.5 bit periods to reach mid-first-data-bit, then sample
    // State 2-8: sample remaining data bits at baud intervals
    // State 9: stop bit — emit byte via DPI
    reg [7:0]  tx_shift;
    reg [3:0]  tx_bcnt;    // 0=idle, 1..8=sampling data, 9=stop
    reg [15:0] tx_baud;    // baud countdown
    reg        tx_prev;    // previous TX bit (for edge detection)

    always_ff @(posedge dev_clk or negedge dev_rst_n) begin
        if (!dev_rst_n) begin
            tx_shift <= 8'd0;
            tx_bcnt  <= 4'd0;
            tx_baud  <= 16'd0;
            tx_prev  <= 1'b1;
        end else begin
            tx_prev <= uart_tx_enq_valid ? uart_tx_enq_data : tx_prev;
            if (tx_bcnt == 0) begin
                // IDLE: detect start bit falling edge
                if (tx_prev && uart_tx_enq_valid && !uart_tx_enq_data) begin
                    tx_bcnt <= 4'd1;
                    // Skip start bit + half data bit: 1.5 bit periods
                    tx_baud <= BAUD_DIV + (BAUD_DIV >> 1);
                    tx_shift <= 8'd0;
                end
            end else begin
                if (tx_baud == 0) begin
                    if (tx_bcnt <= 8) begin
                        // Sample data bit (LSB first)
                        tx_shift <= {uart_tx_enq_data, tx_shift[7:1]};
                        tx_bcnt <= tx_bcnt + 1;
                        tx_baud <= BAUD_DIV;
                    end
                    if (tx_bcnt == 9) begin
                        // Stop bit reached — emit byte
                        uart_sim_tx(tx_shift);
                        tx_bcnt <= 4'd0;
                    end
                end else begin
                    tx_baud <= tx_baud - 1;
                end
            end
        end
    end

    // TX idle when deserializer is in state 0 and line is high (no start bit)
    assign uart_tx_idle = (tx_bcnt == 4'd0) && (!uart_tx_enq_valid || uart_tx_enq_data);

    // -------------------------------------------------------------------------
    // UART RX stub — idle line (always high, always valid)
    // -------------------------------------------------------------------------
    assign uart_rx_deq_valid = 1'b1;
    assign uart_rx_deq_data  = 1'b1;  // idle high

    // -------------------------------------------------------------------------
    // UART RTS — always accept
    // -------------------------------------------------------------------------
    assign uart_rts_enq_ready = 1'b1;

    // -------------------------------------------------------------------------
    // UART CTS — always clear to send (active-low: 0 = clear)
    // -------------------------------------------------------------------------
    assign uart_cts_deq_valid = 1'b1;
    assign uart_cts_deq_data  = 1'b0;  // CTS asserted (clear to send)

    // -------------------------------------------------------------------------
    // Instruction memory — cpu domain clock
    // -------------------------------------------------------------------------
    mem_model #(
        .DEPTH (16384)
    ) imem (
        .clk           (cpu_clk),
        .rst_n         (cpu_rst_n),
        .rd_req_valid  (imem_rd_req_valid),
        .rd_req_ready  (imem_rd_req_ready),
        .rd_req_data   (imem_rd_req_data),
        .rd_resp_valid (imem_rd_resp_valid),
        .rd_resp_ready (imem_rd_resp_ready),
        .rd_resp_data  (imem_rd_resp_data),
        .wr_req_valid  (imem_wr_req_valid),
        .wr_req_ready  (imem_wr_req_ready),
        .wr_req_data   (imem_wr_req_data)
    );

    // -------------------------------------------------------------------------
    // Device data memory — dev domain clock
    // The mem_model is combinational (rd_resp in same cycle as rd_req).
    // The SoC's MemDevice reads the response one cycle later via try_take,
    // so we register the response to hold data across cycles.
    // -------------------------------------------------------------------------
    wire        dmem_raw_resp_valid;
    wire [31:0] dmem_raw_resp_data;

    mem_model #(
        .DEPTH (16384)
    ) dmem (
        .clk           (dev_clk),
        .rst_n         (dev_rst_n),
        .rd_req_valid  (dmem_rd_req_valid),
        .rd_req_ready  (dmem_rd_req_ready),
        .rd_req_data   (dmem_rd_req_data),
        .rd_resp_valid (dmem_raw_resp_valid),
        .rd_resp_ready (1'b1),  // always consume from raw
        .rd_resp_data  (dmem_raw_resp_data),
        .wr_req_valid  (dmem_wr_req_valid),
        .wr_req_ready  (dmem_wr_req_ready),
        .wr_req_data   (dmem_wr_req_data)
    );

    // 1-cycle response queue: latch on valid request, hold until consumed
    reg        dmem_resp_valid_q;
    reg [31:0] dmem_resp_data_q;

    assign dmem_rd_resp_valid = dmem_resp_valid_q;
    assign dmem_rd_resp_data  = dmem_resp_data_q;

    always_ff @(posedge dev_clk or negedge dev_rst_n) begin
        if (!dev_rst_n) begin
            dmem_resp_valid_q <= 1'b0;
            dmem_resp_data_q  <= 32'd0;
        end else begin
            if (dmem_rd_req_valid) begin
                // New request: latch response (even if previous still valid)
                dmem_resp_valid_q <= 1'b1;
                dmem_resp_data_q  <= dmem_raw_resp_data;
            end else if (dmem_resp_valid_q && dmem_rd_resp_ready) begin
                // Consumed by Marie, no new request
                dmem_resp_valid_q <= 1'b0;
            end
        end
    end

    // -------------------------------------------------------------------------
    // Tag memory — cpu domain clock (1 bit per 8-byte region)
    // -------------------------------------------------------------------------
    wire [31:0] tmem_wr_addr = tmem_wr_req_data[32:1];
    wire [31:0] tmem_wr_data = {31'b0, tmem_wr_req_data[0]};

    mem_model_reg #(
        .DEPTH   (2048),
        .MEMFILE ("")
    ) tmem (
        .clk           (cpu_clk),
        .rst_n         (cpu_rst_n),
        .rd_req_valid  (tmem_rd_req_valid),
        .rd_req_ready  (tmem_rd_req_ready),
        .rd_req_data   (tmem_rd_req_data),
        .rd_resp_valid (tmem_rd_resp_valid),
        .rd_resp_ready (tmem_rd_resp_ready),
        .rd_resp_data  (tmem_rd_resp_data_wide),
        .wr_req_valid  (tmem_wr_req_valid),
        .wr_req_ready  (tmem_wr_req_ready),
        .wr_req_data   ({tmem_wr_addr, tmem_wr_data})
    );

    // -------------------------------------------------------------------------
    // Memory loading via +memfile= plusarg (hex files)
    // -------------------------------------------------------------------------
    string memfile;
    initial begin
        if ($value$plusargs("memfile=%s", memfile)) begin
            $readmemh(memfile, imem.storage);
            $readmemh(memfile, dmem.storage);
        end
    end

    // -------------------------------------------------------------------------
    // tohost monitor — watch dev_mem writes to 0x80001000
    // -------------------------------------------------------------------------
    localparam [31:0] TOHOST_ADDR = 32'h8000_1000;

    initial tohost = 32'h0;

    always_ff @(posedge dev_clk or negedge dev_rst_n) begin
        if (!dev_rst_n) begin
            tohost <= 32'h0;
        end else if (dmem_wr_req_valid) begin
            if (dmem_wr_req_data[63:32] == TOHOST_ADDR) begin
                tohost <= dmem_wr_req_data[31:0];
            end
        end
    end

endmodule
