// Verilator testbench for Tourbillon RV32I core
// Drives clock/reset and monitors the tohost register for pass/fail.
//
// NOTE: Memory loading is handled in tb_top.sv via $readmemh.
// Pass the hex file path as a plusarg: +memfile=path/to/mem.hex

#include <verilated.h>
#include <verilated_vcd_c.h>
#include "Vtb_top.h"

#include <cstdio>
#include <cstdlib>
#include <cstring>

static void usage(const char *prog) {
    fprintf(stderr, "Usage: %s [--cycles N] [--trace] [+memfile=<hex>]\n", prog);
    exit(1);
}

int main(int argc, char **argv) {
    Verilated::commandArgs(argc, argv);

    // --- Parse CLI arguments ---
    uint64_t max_cycles = 100000;
    bool trace_en = false;

    for (int i = 1; i < argc; i++) {
        if (strcmp(argv[i], "--cycles") == 0) {
            if (i + 1 >= argc) usage(argv[0]);
            max_cycles = strtoull(argv[++i], nullptr, 0);
        } else if (strcmp(argv[i], "--trace") == 0) {
            trace_en = true;
        }
    }

    // --- Instantiate DUT ---
    Vtb_top *dut = new Vtb_top;

    // --- VCD trace setup ---
    VerilatedVcdC *tfp = nullptr;
    if (trace_en) {
        Verilated::traceEverOn(true);
        tfp = new VerilatedVcdC;
        dut->trace(tfp, 99);
        tfp->open("sim_trace.vcd");
        printf("[tb] VCD tracing enabled -> sim_trace.vcd\n");
    }

    // --- Reset sequence ---
    dut->clk = 0;
    dut->rst_n = 0;

    // Hold reset for 5 cycles
    for (int i = 0; i < 10; i++) {
        dut->clk = !dut->clk;
        dut->eval();
        if (tfp) tfp->dump(i);
    }
    dut->rst_n = 1;

    // --- Main simulation loop ---
    uint64_t cycle = 0;
    uint64_t tick = 10;
    int result = -1;

    while (cycle < max_cycles && result < 0) {
        // Rising edge
        dut->clk = 1;
        dut->eval();
        if (tfp) tfp->dump(tick++);

        // Check tohost
        uint32_t tohost = dut->tohost;
        if (tohost != 0) {
            if (tohost == 1) {
                result = 0;  // PASS
            } else {
                result = 1;  // FAIL
                fprintf(stderr, "[tb] FAIL: tohost = %u (test %u failed)\n",
                        tohost, tohost >> 1);
            }
        }

        // Falling edge
        dut->clk = 0;
        dut->eval();
        if (tfp) tfp->dump(tick++);

        cycle++;
    }

    // --- Report result ---
    if (result == 0) {
        printf("[tb] PASS after %lu cycles\n", cycle);
    } else if (result < 0) {
        fprintf(stderr, "[tb] TIMEOUT after %lu cycles\n", cycle);
        result = 1;
    }

    // --- Cleanup ---
    if (tfp) {
        tfp->close();
        delete tfp;
    }
    dut->final();
    delete dut;

    return result;
}
