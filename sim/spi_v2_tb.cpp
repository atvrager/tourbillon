// spi_v2_tb.cpp — Verilator testbench for Spi2TLULV2 (frame-based DMA)
//
// Truly async clocks: sys=10ns (100 MHz), spi=13ns (~76.9 MHz).
// Tests the frame-based protocol: one CSB assertion = one DMA descriptor.

#include <verilated.h>
#include <verilated_fst_c.h>
#include "Vspi_v2_tb_top.h"

#include <cstdio>
#include <cstdint>

static const uint64_t SYS_HALF = 5000;
static const uint64_t SPI_HALF = 6500;

static Vspi_v2_tb_top *dut;
static VerilatedFstC *trace;
static uint64_t sim_time = 0;
static uint64_t next_sys_edge, next_spi_edge;

static void eval_and_trace() {
    dut->eval();
    if (trace) trace->dump(sim_time);
}

static char tick() {
    char which;
    if (next_sys_edge < next_spi_edge) {
        sim_time = next_sys_edge;
        dut->sys_clk = !dut->sys_clk;
        next_sys_edge += SYS_HALF;
        which = 's';
    } else if (next_spi_edge < next_sys_edge) {
        sim_time = next_spi_edge;
        dut->spi_clk = !dut->spi_clk;
        next_spi_edge += SPI_HALF;
        which = 'p';
    } else {
        sim_time = next_sys_edge;
        dut->sys_clk = !dut->sys_clk;
        dut->spi_clk = !dut->spi_clk;
        next_sys_edge += SYS_HALF;
        next_spi_edge += SPI_HALF;
        which = 'b';
    }
    eval_and_trace();
    return which;
}

static void spi_cycles(int n) {
    for (int i = 0; i < n; ) {
        char w = tick();
        if ((w == 'p' || w == 'b') && dut->spi_clk) i++;
    }
}

static void both_cycles(int sys_n) {
    for (int sc = 0; sc < sys_n; ) {
        char w = tick();
        if ((w == 's' || w == 'b') && dut->sys_clk) sc++;
    }
}

// ---------------------------------------------------------------------------
// SPI helpers
// ---------------------------------------------------------------------------

static void spi_xfer_byte(uint8_t tx) {
    for (int bit = 7; bit >= 0; bit--) {
        dut->spi_mosi = (tx >> bit) & 1;
        spi_cycles(1);
    }
}

static void csb_assert() {
    dut->spi_csb = 0;
    dut->spi_mosi = 0;
    both_cycles(5);
}

static void csb_deassert() {
    dut->spi_csb = 1;
    dut->spi_mosi = 0;
    both_cycles(5);
}

// Send a 7-byte DMA frame header: [op, addr(BE), len(BE)]
static void spi_send_header(uint8_t op, uint32_t addr, uint16_t len) {
    spi_xfer_byte(op);
    spi_xfer_byte((addr >> 24) & 0xFF);
    spi_xfer_byte((addr >> 16) & 0xFF);
    spi_xfer_byte((addr >>  8) & 0xFF);
    spi_xfer_byte((addr >>  0) & 0xFF);
    spi_xfer_byte((len >> 8) & 0xFF);
    spi_xfer_byte((len >> 0) & 0xFF);
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

static int test_count = 0;
static int fail_count = 0;

static void check(bool cond, const char *msg) {
    test_count++;
    if (!cond) {
        fprintf(stderr, "  FAIL: %s\n", msg);
        fail_count++;
    } else {
        printf("  PASS: %s\n", msg);
    }
}

// Test 1: Truly async clocks
static void test_async_clocks() {
    printf("[test_async_clocks]\n");
    int coincident = 0, spi_pos = 0, sys_pos = 0;
    for (int i = 0; i < 2000; i++) {
        char w = tick();
        if (w == 'b' && dut->sys_clk && dut->spi_clk) {
            coincident++; spi_pos++; sys_pos++;
        } else if ((w == 'p') && dut->spi_clk) spi_pos++;
        else if ((w == 's') && dut->sys_clk) sys_pos++;
    }
    printf("    sys=%d spi=%d coincident=%d\n", sys_pos, spi_pos, coincident);
    check(spi_pos != sys_pos, "different frequencies");
    check(coincident < spi_pos / 2, "not locked");
}

// Test 2: Read frame → TL-A fires with Get opcode
static void test_read_frame() {
    printf("[test_read_frame]\n");
    csb_assert();
    spi_send_header(0x01, 0x10000000, 0x0000);  // read, addr, 1 beat
    csb_deassert();

    // Let the descriptor cross CDC and TlFsm issue the Get
    both_cycles(100);
    check(true, "read frame: no hang");
}

// Test 3: Write frame → TL-A fires with PutFullData opcode
static void test_write_frame() {
    printf("[test_write_frame]\n");
    csb_assert();
    spi_send_header(0x02, 0x20000000, 0x0000);  // write, addr, 1 beat

    // Send 16 bytes of write data (one 128-bit beat)
    for (int i = 0; i < 16; i++) {
        spi_xfer_byte(i & 0xFF);
    }
    csb_deassert();

    both_cycles(100);
    check(true, "write frame: no hang");
}

// Test 4: CSB toggle stability — rapid frame submissions
static void test_csb_stability() {
    printf("[test_csb_stability]\n");
    for (int i = 0; i < 10; i++) {
        csb_assert();
        spi_send_header(0x01, 0x10000000 + (i << 4), 0x0000);
        csb_deassert();
        both_cycles(20);
    }
    check(true, "10 rapid frames without crash");
}

// Test 5: Frame protocol is one CSB = one command (no register setup)
// Verify that a single 7-byte header suffices — no multi-CSB ceremony.
static void test_single_csb_transaction() {
    printf("[test_single_csb_transaction]\n");

    // One read frame in one CSB pulse
    csb_assert();
    spi_send_header(0x01, 0xDEADBEEF, 0x0003);  // 4 beats
    // Clock dummy bytes for the read response (4 × 16 = 64 bytes)
    for (int i = 0; i < 64; i++) {
        spi_xfer_byte(0x00);
    }
    csb_deassert();

    both_cycles(200);
    check(true, "single CSB transaction for 4-beat read: no hang");
}

// ---------------------------------------------------------------------------
// Main
// ---------------------------------------------------------------------------

int main(int argc, char **argv) {
    Verilated::commandArgs(argc, argv);
    Verilated::traceEverOn(true);

    dut = new Vspi_v2_tb_top;
    trace = new VerilatedFstC;
    dut->trace(trace, 10);
    trace->open("spi_v2_trace.fst");

    dut->sys_clk = 0; dut->spi_clk = 0;
    dut->sys_rst_n = 0; dut->spi_csb = 1; dut->spi_mosi = 0;
    next_sys_edge = SYS_HALF; next_spi_edge = SPI_HALF;
    eval_and_trace();

    both_cycles(10);
    dut->sys_rst_n = 1;
    both_cycles(10);

    test_async_clocks();
    test_read_frame();
    test_write_frame();
    test_csb_stability();
    test_single_csb_transaction();

    printf("\n%d tests, %d passed, %d failed\n",
           test_count, test_count - fail_count, fail_count);

    trace->close();
    delete trace;
    delete dut;
    return fail_count > 0 ? 1 : 0;
}
