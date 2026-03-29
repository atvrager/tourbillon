// soc_tb.cpp — Multi-clock Verilator testbench for the Marie Antoinette SoC
//
// Drives three independent clock domains (cpu, xbar, dev) and monitors
// the tohost register for riscv-tests pass/fail. Implements the DPI
// uart_tx function for UART output.
//
// Memory loading modes:
//   1. +memfile=path.hex  — $readmemh in soc_top.sv
//   2. path/to/elf        — ELF loading via Verilator public access

#include <verilated.h>
#include <verilated_fst_c.h>
#include "Vsoc_top.h"
#include "Vsoc_top___024root.h"

#include <cstdio>
#include <cstdlib>
#include <cstring>
#include <cstdint>

// ---------------------------------------------------------------------------
// DPI: uart_tx — prints characters to stdout
// ---------------------------------------------------------------------------

extern "C" void uart_tx(unsigned char ch) {
    fputc(ch, stdout);
    fflush(stdout);
}

// ---------------------------------------------------------------------------
// Minimal ELF32 loader (same as tb_cpu.cpp)
// ---------------------------------------------------------------------------

#pragma pack(push, 1)
struct Elf32_Ehdr {
    uint8_t  e_ident[16];
    uint16_t e_type, e_machine;
    uint32_t e_version, e_entry, e_phoff, e_shoff, e_flags;
    uint16_t e_ehsize, e_phentsize, e_phnum, e_shentsize, e_shnum, e_shstrndx;
};
struct Elf32_Phdr {
    uint32_t p_type, p_offset, p_vaddr, p_paddr, p_filesz, p_memsz, p_flags, p_align;
};
struct Elf32_Shdr {
    uint32_t sh_name, sh_type, sh_flags, sh_addr, sh_offset, sh_size;
    uint32_t sh_link, sh_info, sh_addralign, sh_entsize;
};
struct Elf32_Sym {
    uint32_t st_name, st_value, st_size;
    uint8_t  st_info, st_other;
    uint16_t st_shndx;
};
#pragma pack(pop)

static const uint32_t PT_LOAD    = 1;
static const uint32_t SHT_SYMTAB = 2;
static const uint32_t MEM_BASE   = 0x80000000;

static uint32_t load_elf(const char *path, Vsoc_top *dut) {
    FILE *f = fopen(path, "rb");
    if (!f) { fprintf(stderr, "[soc_tb] Cannot open ELF: %s\n", path); return 0; }

    Elf32_Ehdr ehdr;
    if (fread(&ehdr, sizeof(ehdr), 1, f) != 1) { fclose(f); return 0; }
    if (memcmp(ehdr.e_ident, "\x7f""ELF", 4) != 0) { fclose(f); return 0; }

    // Load segments into BOTH imem and dmem
    for (int i = 0; i < ehdr.e_phnum; i++) {
        Elf32_Phdr phdr;
        fseek(f, ehdr.e_phoff + i * ehdr.e_phentsize, SEEK_SET);
        if (fread(&phdr, sizeof(phdr), 1, f) != 1) continue;
        if (phdr.p_type != PT_LOAD || phdr.p_filesz == 0) continue;

        uint8_t *buf = new uint8_t[phdr.p_filesz];
        fseek(f, phdr.p_offset, SEEK_SET);
        size_t nread = fread(buf, 1, phdr.p_filesz, f);

        uint32_t base = phdr.p_paddr;
        for (size_t j = 0; j < nread; j += 4) {
            uint32_t word = 0;
            for (size_t k = 0; k < 4 && (j + k) < nread; k++)
                word |= ((uint32_t)buf[j + k]) << (k * 8);
            uint32_t addr = base + j;
            if (addr >= MEM_BASE) {
                uint32_t idx = (addr - MEM_BASE) >> 2;
                if (idx < 16384) {
                    dut->rootp->soc_top__DOT__imem__DOT__storage[idx] = word;
                    dut->rootp->soc_top__DOT__dmem__DOT__storage[idx] = word;
                }
            }
        }
        delete[] buf;
    }

    // Find tohost symbol
    uint32_t tohost_addr = 0;
    for (int i = 0; i < ehdr.e_shnum; i++) {
        Elf32_Shdr shdr;
        fseek(f, ehdr.e_shoff + i * ehdr.e_shentsize, SEEK_SET);
        if (fread(&shdr, sizeof(shdr), 1, f) != 1) continue;
        if (shdr.sh_type != SHT_SYMTAB) continue;

        Elf32_Shdr strtab;
        fseek(f, ehdr.e_shoff + shdr.sh_link * ehdr.e_shentsize, SEEK_SET);
        if (fread(&strtab, sizeof(strtab), 1, f) != 1) continue;

        char *str = new char[strtab.sh_size];
        fseek(f, strtab.sh_offset, SEEK_SET);
        fread(str, 1, strtab.sh_size, f);

        int nsyms = shdr.sh_size / shdr.sh_entsize;
        for (int s = 0; s < nsyms; s++) {
            Elf32_Sym sym;
            fseek(f, shdr.sh_offset + s * shdr.sh_entsize, SEEK_SET);
            if (fread(&sym, sizeof(sym), 1, f) != 1) continue;
            if (sym.st_name < strtab.sh_size && strcmp(&str[sym.st_name], "tohost") == 0) {
                tohost_addr = sym.st_value;
                break;
            }
        }
        delete[] str;
        if (tohost_addr) break;
    }

    fclose(f);
    return tohost_addr;
}

// ---------------------------------------------------------------------------
// Main
// ---------------------------------------------------------------------------

int main(int argc, char **argv) {
    Verilated::commandArgs(argc, argv);

    uint64_t max_cycles = 500000;  // Higher default — CDC adds latency
    bool trace_en = false;
    const char *elf_file = nullptr;

    for (int i = 1; i < argc; i++) {
        if (strcmp(argv[i], "--cycles") == 0 && i + 1 < argc)
            max_cycles = strtoull(argv[++i], nullptr, 0);
        else if (strcmp(argv[i], "--trace") == 0)
            trace_en = true;
        else if (argv[i][0] != '+' && argv[i][0] != '-')
            elf_file = argv[i];
    }

    Vsoc_top *dut = new Vsoc_top;

    if (elf_file) load_elf(elf_file, dut);

    VerilatedFstC *tfp = nullptr;
    if (trace_en) {
        Verilated::traceEverOn(true);
        tfp = new VerilatedFstC;
        dut->trace(tfp, 99);
        tfp->open("soc_trace.fst");
    }

    // --- Reset ---
    dut->cpu_clk = 0; dut->xbar_clk = 0; dut->dev_clk = 0;
    dut->cpu_rst_n = 0; dut->xbar_rst_n = 0; dut->dev_rst_n = 0;

    // Hold reset for 10 ticks on all domains
    uint64_t t = 0;
    for (int i = 0; i < 20; i++) {
        dut->cpu_clk  = !dut->cpu_clk;
        dut->xbar_clk = !dut->xbar_clk;
        dut->dev_clk  = !dut->dev_clk;
        dut->eval();
        if (tfp) tfp->dump(t++);
    }
    dut->cpu_rst_n = 1; dut->xbar_rst_n = 1; dut->dev_rst_n = 1;

    // --- Pre-load next_pc_q with reset vector ---
    // The init=0x80000000 in the .tbn translates to an init_tokens value,
    // but the FIFO needs actual data. Pre-load storage[0] with the reset PC.
    {
        auto &r = *dut->rootp;
        r.soc_top__DOT__marie_inst__DOT__q_CPUCore_next_pc_q_inst__DOT__storage[0] = 0x80000000;
        r.soc_top__DOT__marie_inst__DOT__q_CPUCore_next_pc_q_inst__DOT__wr_ptr = 1;
        r.soc_top__DOT__marie_inst__DOT__q_CPUCore_next_pc_q_inst__DOT__count = 1;
    }

    // --- Simulation ---
    // All three clock domains run in lock-step (same frequency).
    // The async FIFOs still provide structural CDC but with no actual
    // frequency difference. Multi-rate clocking is a future enhancement
    // that requires splitting the CPU pipeline stages for true CDC.
    uint64_t cycle = 0;
    int result = -1;

    while (cycle < max_cycles && result < 0) {
        // Rising edge — all domains
        dut->cpu_clk  = 1;
        dut->xbar_clk = 1;
        dut->dev_clk  = 1;
        dut->eval();
        if (tfp) tfp->dump(t++);

        // Check tohost
        uint32_t th = dut->tohost;
        if (th != 0) {
            if (th == 1) {
                result = 0;
            } else {
                result = 1;
                fprintf(stderr, "[soc_tb] FAIL: tohost = 0x%08x (test %u) at cycle %lu\n",
                        th, th >> 1, cycle);
            }
        }

        // Falling edge — all domains
        dut->cpu_clk  = 0;
        dut->xbar_clk = 0;
        dut->dev_clk  = 0;
        dut->eval();
        if (tfp) tfp->dump(t++);

        cycle++;
    }

    if (result == 0) {
        printf("[soc_tb] PASS after %lu cycles\n", cycle);
    } else if (result < 0) {
        fprintf(stderr, "[soc_tb] TIMEOUT after %lu cycles\n", cycle);
        result = 1;
    }

    if (tfp) { tfp->close(); delete tfp; }
    dut->final();
    delete dut;
    return result;
}
