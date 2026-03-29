// Verilator testbench for Tourbillon RV32I core
// Drives clock/reset and monitors the tohost register for pass/fail.
//
// Supports two memory loading modes:
//   1. +memfile=path.hex  — $readmemh in tb_top.sv (hex file)
//   2. path/to/elf        — ELF loading via Verilator public access (positional arg)

#include <verilated.h>
#include <verilated_fst_c.h>
#include "Vtb_top.h"

#include <cstdio>
#include <cstdlib>
#include <cstring>
#include <cstdint>
#include <string>
#include <vector>
#include <time.h>
#include <unistd.h>

// ---------------------------------------------------------------------------
// Minimal ELF32 loader
// ---------------------------------------------------------------------------

#pragma pack(push, 1)
struct Elf32_Ehdr {
    uint8_t  e_ident[16];
    uint16_t e_type;
    uint16_t e_machine;
    uint32_t e_version;
    uint32_t e_entry;
    uint32_t e_phoff;
    uint32_t e_shoff;
    uint32_t e_flags;
    uint16_t e_ehsize;
    uint16_t e_phentsize;
    uint16_t e_phnum;
    uint16_t e_shentsize;
    uint16_t e_shnum;
    uint16_t e_shstrndx;
};

struct Elf32_Phdr {
    uint32_t p_type;
    uint32_t p_offset;
    uint32_t p_vaddr;
    uint32_t p_paddr;
    uint32_t p_filesz;
    uint32_t p_memsz;
    uint32_t p_flags;
    uint32_t p_align;
};

struct Elf32_Shdr {
    uint32_t sh_name;
    uint32_t sh_type;
    uint32_t sh_flags;
    uint32_t sh_addr;
    uint32_t sh_offset;
    uint32_t sh_size;
    uint32_t sh_link;
    uint32_t sh_info;
    uint32_t sh_addralign;
    uint32_t sh_entsize;
};

struct Elf32_Sym {
    uint32_t st_name;
    uint32_t st_value;
    uint32_t st_size;
    uint8_t  st_info;
    uint8_t  st_other;
    uint16_t st_shndx;
};
#pragma pack(pop)

static const uint32_t PT_LOAD = 1;
static const uint32_t SHT_SYMTAB = 2;
static const uint32_t SHT_STRTAB = 3;
static const uint32_t MEM_BASE = 0x80000000;

/// Convert ELF to a temporary hex file for $readmemh loading.
static std::string elf_to_hex(const char *path) {
    FILE *f = fopen(path, "rb");
    if (!f) { fprintf(stderr, "[tb] Cannot open ELF: %s\n", path); return ""; }

    Elf32_Ehdr ehdr;
    if (fread(&ehdr, sizeof(ehdr), 1, f) != 1) { fclose(f); return ""; }
    if (memcmp(ehdr.e_ident, "\x7f""ELF", 4) != 0) { fclose(f); return ""; }

    uint32_t mem[16384] = {};
    uint32_t max_idx = 0;
    for (int i = 0; i < ehdr.e_phnum; i++) {
        Elf32_Phdr phdr;
        fseek(f, ehdr.e_phoff + i * ehdr.e_phentsize, SEEK_SET);
        if (fread(&phdr, sizeof(phdr), 1, f) != 1) continue;
        if (phdr.p_type != PT_LOAD || phdr.p_filesz == 0) continue;

        uint8_t *buf = new uint8_t[phdr.p_filesz];
        fseek(f, phdr.p_offset, SEEK_SET);
        size_t nread = fread(buf, 1, phdr.p_filesz, f);

        for (size_t j = 0; j < nread; j += 4) {
            uint32_t word = 0;
            for (size_t k = 0; k < 4 && (j + k) < nread; k++)
                word |= ((uint32_t)buf[j + k]) << (k * 8);
            uint32_t addr = phdr.p_paddr + j;
            if (addr >= MEM_BASE) {
                uint32_t idx = (addr - MEM_BASE) >> 2;
                if (idx < 16384) {
                    mem[idx] = word;
                    if (idx > max_idx) max_idx = idx;
                }
            }
        }
        delete[] buf;
    }
    fclose(f);

    char tmp[] = "/tmp/tbn_cpu_XXXXXX.hex";
    int fd = mkstemps(tmp, 4);
    if (fd < 0) { perror("mkstemps"); return ""; }
    FILE *hf = fdopen(fd, "w");
    for (uint32_t i = 0; i <= max_idx; i++)
        fprintf(hf, "%08x\n", mem[i]);
    fclose(hf);
    return std::string(tmp);
}

// ---------------------------------------------------------------------------
// Main
// ---------------------------------------------------------------------------

static void usage(const char *prog) {
    fprintf(stderr, "Usage: %s [--cycles N] [--trace] [+memfile=<hex>] [elf_file]\n", prog);
    exit(1);
}

int main(int argc, char **argv) {
    Verilated::commandArgs(argc, argv);

    // --- Parse CLI arguments ---
    uint64_t max_cycles = 100000;
    bool trace_en = false;
    const char *elf_file = nullptr;

    for (int i = 1; i < argc; i++) {
        if (strcmp(argv[i], "--cycles") == 0) {
            if (i + 1 >= argc) usage(argv[0]);
            max_cycles = strtoull(argv[++i], nullptr, 0);
        } else if (strcmp(argv[i], "--trace") == 0) {
            trace_en = true;
        } else if (argv[i][0] != '+' && argv[i][0] != '-') {
            elf_file = argv[i];
        }
    }

    // --- Convert ELF to hex and re-exec with +memfile= ---
    if (elf_file) {
        std::string hex_path = elf_to_hex(elf_file);
        if (hex_path.empty()) return 1;

        std::string plusarg = "+memfile=" + hex_path;
        std::vector<char*> new_argv;
        for (int i = 0; i < argc; i++) {
            if (argv[i] == elf_file) continue;
            new_argv.push_back(argv[i]);
        }
        new_argv.push_back(const_cast<char*>(plusarg.c_str()));
        new_argv.push_back(nullptr);
        execv(argv[0], new_argv.data());
        perror("execv");
        unlink(hex_path.c_str());
        return 1;
    }

    // --- Instantiate DUT ---
    Vtb_top *dut = new Vtb_top;

    // --- VCD trace setup ---
    VerilatedFstC *tfp = nullptr;
    if (trace_en) {
        Verilated::traceEverOn(true);
        tfp = new VerilatedFstC;
        dut->trace(tfp, 99);
        tfp->open("sim_trace.fst");
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

    // done_q preload is now handled in tb_top.sv initial block

    // --- Main simulation loop ---
    struct timespec ts_start, ts_end;
    clock_gettime(CLOCK_MONOTONIC, &ts_start);
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
                fprintf(stderr, "[tb] FAIL: tohost = 0x%08x (test %u failed) at cycle %lu\n",
                        tohost, tohost >> 1, cycle);
            }
        }

        // Falling edge
        dut->clk = 0;
        dut->eval();
        if (tfp) tfp->dump(tick++);

        cycle++;
    }

    // --- Report result ---
    clock_gettime(CLOCK_MONOTONIC, &ts_end);
    double elapsed = (ts_end.tv_sec - ts_start.tv_sec)
                   + (ts_end.tv_nsec - ts_start.tv_nsec) * 1e-9;
    double khz = (elapsed > 0) ? (cycle / elapsed / 1000.0) : 0;
    if (result == 0) {
        printf("[tb] PASS after %lu cycles (%.3f s, %.0f kHz)\n", cycle, elapsed, khz);
    } else if (result < 0) {
        fprintf(stderr, "[tb] TIMEOUT after %lu cycles (%.3f s, %.0f kHz)\n", cycle, elapsed, khz);
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
