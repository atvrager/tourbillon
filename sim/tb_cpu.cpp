// Verilator testbench for Tourbillon RV32I core
// Drives clock/reset and monitors the tohost register for pass/fail.
//
// Supports two memory loading modes:
//   1. +memfile=path.hex  — $readmemh in tb_top.sv (hex file)
//   2. path/to/elf        — ELF loading via Verilator public access (positional arg)

#include <verilated.h>
#include <verilated_fst_c.h>
#include "Vtb_top.h"
#include "Vtb_top___024root.h"

#include <cstdio>
#include <cstdlib>
#include <cstring>
#include <cstdint>
#include <time.h>

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

// Load an ELF file into memory arrays.
// Returns the tohost address (0 if not found in symbol table).
static uint32_t load_elf(const char *path, Vtb_top *dut) {
    FILE *f = fopen(path, "rb");
    if (!f) {
        fprintf(stderr, "[tb] Cannot open ELF: %s\n", path);
        return 0;
    }

    // Read ELF header
    Elf32_Ehdr ehdr;
    if (fread(&ehdr, sizeof(ehdr), 1, f) != 1) {
        fprintf(stderr, "[tb] Failed to read ELF header\n");
        fclose(f);
        return 0;
    }

    // Check ELF magic
    if (ehdr.e_ident[0] != 0x7f || ehdr.e_ident[1] != 'E' ||
        ehdr.e_ident[2] != 'L'  || ehdr.e_ident[3] != 'F') {
        fprintf(stderr, "[tb] Not an ELF file: %s\n", path);
        fclose(f);
        return 0;
    }

    // Load PT_LOAD segments
    for (int i = 0; i < ehdr.e_phnum; i++) {
        Elf32_Phdr phdr;
        fseek(f, ehdr.e_phoff + i * ehdr.e_phentsize, SEEK_SET);
        if (fread(&phdr, sizeof(phdr), 1, f) != 1) continue;
        if (phdr.p_type != PT_LOAD) continue;
        if (phdr.p_filesz == 0) continue;

        // Read segment data
        uint8_t *buf = new uint8_t[phdr.p_filesz];
        fseek(f, phdr.p_offset, SEEK_SET);
        size_t nread = fread(buf, 1, phdr.p_filesz, f);

        // Copy to memory word by word
        uint32_t base = phdr.p_paddr;
        for (size_t j = 0; j < nread; j += 4) {
            uint32_t word = 0;
            for (size_t k = 0; k < 4 && (j + k) < nread; k++) {
                word |= ((uint32_t)buf[j + k]) << (k * 8);
            }
            uint32_t addr = base + j;
            if (addr >= MEM_BASE) {
                uint32_t idx = (addr - MEM_BASE) >> 2;
                if (idx < 16384) {
                    dut->rootp->tb_top__DOT__imem__DOT__storage[idx] = word;
                    dut->rootp->tb_top__DOT__dmem__DOT__storage[idx] = word;
                }
            }
        }
        delete[] buf;
    }

    // Search symbol table for "tohost"
    uint32_t tohost_addr = 0;
    for (int i = 0; i < ehdr.e_shnum; i++) {
        Elf32_Shdr shdr;
        fseek(f, ehdr.e_shoff + i * ehdr.e_shentsize, SEEK_SET);
        if (fread(&shdr, sizeof(shdr), 1, f) != 1) continue;
        if (shdr.sh_type != SHT_SYMTAB) continue;

        // Read associated string table
        Elf32_Shdr strtab_shdr;
        fseek(f, ehdr.e_shoff + shdr.sh_link * ehdr.e_shentsize, SEEK_SET);
        if (fread(&strtab_shdr, sizeof(strtab_shdr), 1, f) != 1) continue;

        char *strtab = new char[strtab_shdr.sh_size];
        fseek(f, strtab_shdr.sh_offset, SEEK_SET);
        fread(strtab, 1, strtab_shdr.sh_size, f);

        // Search symbols
        int nsyms = shdr.sh_size / shdr.sh_entsize;
        for (int s = 0; s < nsyms; s++) {
            Elf32_Sym sym;
            fseek(f, shdr.sh_offset + s * shdr.sh_entsize, SEEK_SET);
            if (fread(&sym, sizeof(sym), 1, f) != 1) continue;
            if (sym.st_name < strtab_shdr.sh_size &&
                strcmp(&strtab[sym.st_name], "tohost") == 0) {
                tohost_addr = sym.st_value;
                break;
            }
        }
        delete[] strtab;
        if (tohost_addr) break;
    }

    fclose(f);
    return tohost_addr;
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

    // --- Instantiate DUT ---
    Vtb_top *dut = new Vtb_top;

    // --- Load ELF if provided ---
    if (elf_file) {
        load_elf(elf_file, dut);
    }

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

    // --- Pre-load done_q credit token ---
    // The single-issue pipeline needs an initial credit in done_q.
    // Must be done AFTER reset (which clears FIFO state).
    {
        auto &root = *dut->rootp;
        root.tb_top__DOT__cpu_inst__DOT__q_done_q_inst__DOT__storage[0] = 1;
        root.tb_top__DOT__cpu_inst__DOT__q_done_q_inst__DOT__wr_ptr = 1;
        root.tb_top__DOT__cpu_inst__DOT__q_done_q_inst__DOT__count = 1;
    }

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
