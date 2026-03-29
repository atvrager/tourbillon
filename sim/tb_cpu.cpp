// Verilator testbench for Tourbillon RV32I core
// Drives clock/reset, loads a flat binary into instruction memory,
// and monitors the tohost memory-mapped register for pass/fail.

#include <verilated.h>
#include <verilated_vcd_c.h>
#include "Vtb_top.h"

#include <cstdio>
#include <cstdlib>
#include <cstring>
#include <fstream>
#include <vector>
#include <string>

static const uint32_t TOHOST_ADDR = 0x80001000;

static void usage(const char *prog) {
    fprintf(stderr, "Usage: %s [--cycles N] [--trace] <binary>\n", prog);
    exit(1);
}

int main(int argc, char **argv) {
    Verilated::commandArgs(argc, argv);

    // --- Parse CLI arguments ---
    uint64_t max_cycles = 100000;
    bool trace_en = false;
    const char *bin_path = nullptr;

    for (int i = 1; i < argc; i++) {
        if (strcmp(argv[i], "--cycles") == 0) {
            if (i + 1 >= argc) usage(argv[0]);
            max_cycles = strtoull(argv[++i], nullptr, 0);
        } else if (strcmp(argv[i], "--trace") == 0) {
            trace_en = true;
        } else if (argv[i][0] == '-' && argv[i][1] == '-') {
            // Skip unknown Verilator plusargs
        } else {
            bin_path = argv[i];
        }
    }

    if (!bin_path) {
        fprintf(stderr, "Error: no binary file specified\n");
        usage(argv[0]);
    }

    // --- Load binary file ---
    std::ifstream f(bin_path, std::ios::binary | std::ios::ate);
    if (!f.is_open()) {
        fprintf(stderr, "Error: cannot open '%s'\n", bin_path);
        return 1;
    }
    size_t bin_size = f.tellg();
    f.seekg(0);
    std::vector<uint8_t> bin_data(bin_size);
    f.read(reinterpret_cast<char *>(bin_data.data()), bin_size);
    f.close();

    printf("[tb] Loaded %zu bytes from %s\n", bin_size, bin_path);

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

    // --- Backdoor load: write binary into instruction memory ---
    // The tb_top module exposes imem as a flat array.
    // We write 32-bit words assuming little-endian binary layout.
    size_t num_words = (bin_size + 3) / 4;
    for (size_t i = 0; i < num_words; i++) {
        uint32_t word = 0;
        for (int b = 0; b < 4; b++) {
            size_t idx = i * 4 + b;
            if (idx < bin_size) {
                word |= static_cast<uint32_t>(bin_data[idx]) << (b * 8);
            }
        }
        // Backdoor write into the memory array via DPI or direct access.
        // The tb_top wrapper should expose: dut->tb_top->imem[i] or similar.
        // For Verilator, we access the internal memory array directly:
        if (i < 16384) {  // 64KB / 4 = 16K words
            dut->rootp->tb_top__DOT__imem[i] = word;
        }
    }
    printf("[tb] Loaded %zu words into instruction memory\n", num_words);

    // Also copy into data memory for load/store tests
    for (size_t i = 0; i < num_words && i < 16384; i++) {
        dut->rootp->tb_top__DOT__dmem[i] = dut->rootp->tb_top__DOT__imem[i];
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
    uint64_t tick = 10;  // Continue from reset ticks
    int result = -1;     // -1 = running, 0 = pass, 1 = fail

    while (cycle < max_cycles && result < 0) {
        // Rising edge
        dut->clk = 1;
        dut->eval();
        if (tfp) tfp->dump(tick++);

        // Check tohost: the tb_top wrapper maps address 0x80001000 to a register
        // that is accessible as dut->tohost
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
