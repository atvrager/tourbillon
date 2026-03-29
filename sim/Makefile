# Tourbillon RV32I Verilator simulation Makefile

TBN       ?= ../target/debug/tbn
SV_DIR    ?= sv_out
VERILATOR ?= verilator
OBJ_DIR   := obj_dir
SIM_BIN   := $(OBJ_DIR)/sim_cpu

# RISC-V toolchain
RISCV_PREFIX ?= riscv64-elf-
AS            = $(RISCV_PREFIX)as
LD            = $(RISCV_PREFIX)ld
OBJCOPY       = $(RISCV_PREFIX)objcopy

# Default test binary
TEST ?= tests/smoke.bin

# riscv-tests location
RISCV_TESTS_DIR ?= /opt/riscv-tests/share/riscv-tests/isa

SOC_SV_DIR ?= soc_sv_out
SOC_OBJ_DIR := soc_obj_dir
SOC_BIN     := $(SOC_OBJ_DIR)/sim_soc

.PHONY: sv verilate build run test-smoke test-hex riscv-tests clean
.PHONY: soc-sv soc-build soc-hello soc-riscv-tests rtl-check rtl-export

# Generate SystemVerilog from Tourbillon source
sv:
	mkdir -p $(SV_DIR)
	$(TBN) build ../examples/rv32i.tbn -o $(SV_DIR)

# Compile SV to C++ via Verilator and build the simulator
NPROC := $(shell nproc 2>/dev/null || echo 4)
verilate: sv
	$(VERILATOR) --cc --exe --build --trace-fst -Wall -Wno-fatal \
		--top-module tb_top \
		\
		-j $(NPROC) \
		-I$(SV_DIR) \
		tb_top.sv rv32i_pkg.sv mem_model.sv \
		$(SV_DIR)/*.sv \
		tb_cpu.cpp \
		-o sim_cpu \
		--Mdir $(OBJ_DIR)

# Build the simulation binary
build: verilate

# Run simulation with a test binary
run: build
	$(SIM_BIN) $(TEST)

# Assemble smoke test
tests/smoke.bin: tests/smoke.S
	$(AS) -march=rv32i -mabi=ilp32 -o tests/smoke.o tests/smoke.S
	$(LD) -Ttext=0x80000000 -melf32lriscv -o tests/smoke.elf tests/smoke.o
	$(OBJCOPY) -O binary tests/smoke.elf tests/smoke.bin
	@rm -f tests/smoke.o tests/smoke.elf

# Run smoke test from hex file (no RISC-V toolchain needed)
test-hex: build
	$(SIM_BIN) +memfile=tests/smoke.hex

# Assemble and run the smoke test
test-smoke: build tests/smoke.bin
	$(SIM_BIN) tests/smoke.bin

# Run the full riscv-tests rv32ui suite
# Uses tests built by build_tests.sh in tests/rv32ui/
RV32UI_DIR ?= tests/rv32ui
riscv-tests: build
	@pass=0; fail=0; skip=0; \
	for t in $(RV32UI_DIR)/*; do \
		test -f "$$t" || continue; \
		case "$$t" in *.dump|*.S|*.o) continue;; *fence_i*) skip=$$((skip + 1)); continue;; esac; \
		name=$$(basename $$t); \
		if $(SIM_BIN) $$t 2>/dev/null; then \
			echo "  PASS $$name"; \
			pass=$$((pass + 1)); \
		else \
			echo "  FAIL $$name"; \
			fail=$$((fail + 1)); \
		fi; \
	done; \
	echo ""; \
	echo "Results: $$pass passed, $$fail failed, $$skip skipped"; \
	test $$fail -eq 0

# =========================================================================
# Marie Antoinette SoC targets
# =========================================================================

# Generate SoC SystemVerilog
soc-sv:
	mkdir -p $(SOC_SV_DIR)
	$(TBN) build ../examples/marie.tbn -o $(SOC_SV_DIR)

# Build SoC simulator
soc-build: soc-sv
	$(VERILATOR) --cc --exe --build --trace-fst -Wall -Wno-fatal \
		--top-module soc_top \
		-j $(NPROC) \
		-I$(SOC_SV_DIR) \
		soc_top.sv rv32i_pkg.sv mem_model.sv \
		$(SOC_SV_DIR)/*.sv \
		soc_tb.cpp \
		-o sim_soc \
		--Mdir $(SOC_OBJ_DIR)

# Build and run Hello World test
tests/hello.elf: tests/hello.S
	$(AS) -march=rv32i -mabi=ilp32 -o tests/hello.o tests/hello.S
	$(LD) -Ttext=0x80000000 -melf32lriscv -o tests/hello.elf tests/hello.o
	@rm -f tests/hello.o

soc-hello: soc-build tests/hello.elf
	$(SOC_BIN) tests/hello.elf

# Run riscv-tests through the SoC fabric
soc-riscv-tests: soc-build
	@pass=0; fail=0; skip=0; \
	for t in $(RV32UI_DIR)/*; do \
		test -f "$$t" || continue; \
		case "$$t" in *.dump|*.S|*.o) continue;; *fence_i*) skip=$$((skip + 1)); continue;; esac; \
		name=$$(basename $$t); \
		if $(SOC_BIN) $$t 2>/dev/null; then \
			echo "  PASS $$name"; \
			pass=$$((pass + 1)); \
		else \
			echo "  FAIL $$name"; \
			fail=$$((fail + 1)); \
		fi; \
	done; \
	echo ""; \
	echo "SoC Results: $$pass passed, $$fail failed, $$skip skipped"; \
	test $$fail -eq 0

# =========================================================================
# FPGA RTL targets
# =========================================================================

RTL_DIR := ../rtl

# Lint-check the FPGA toplevel (structural wiring only).
# Xilinx primitives (IBUFDS, MMCME4_ADV, BUFGCE) are stubbed via bbox-sys
# since we don't have the Vivado unisim library on the lint path.
rtl-check: soc-sv
	$(VERILATOR) --lint-only -Wall -Wno-fatal \
		-I$(SOC_SV_DIR) \
		$(RTL_DIR)/marie_top.sv $(RTL_DIR)/xilinx_stubs.sv rv32i_pkg.sv \
		$(SOC_SV_DIR)/*.sv

# =========================================================================
# RTL export — zip package for Vivado
# =========================================================================
# Produces marie_soc_rtl.zip with topo-sorted file list ready for
# read_verilog / add_files in Vivado.  Drop in, add your XDC, and build.

RTL_EXPORT_DIR := rtl_export
RTL_ZIP        := marie_soc_rtl.zip

rtl-export: soc-sv
	@rm -rf $(RTL_EXPORT_DIR) $(RTL_ZIP)
	@mkdir -p $(RTL_EXPORT_DIR)
	@# Copy sources in topo order (packages → leaves → middle → top)
	@cp rv32i_pkg.sv                   $(RTL_EXPORT_DIR)/1_rv32i_pkg.sv
	@cp $(SOC_SV_DIR)/tbn_fifo.sv      $(RTL_EXPORT_DIR)/2_tbn_fifo.sv
	@cp $(SOC_SV_DIR)/tbn_async_fifo.sv $(RTL_EXPORT_DIR)/3_tbn_async_fifo.sv
	@cp $(SOC_SV_DIR)/Marie.sv         $(RTL_EXPORT_DIR)/4_Marie.sv
	@cp $(RTL_DIR)/marie_top.sv        $(RTL_EXPORT_DIR)/5_marie_top.sv
	@# Generate filelist.f (relative paths, topo-sorted)
	@printf '// Marie Antoinette SoC — Vivado source file list (topo-sorted)\n' \
		> $(RTL_EXPORT_DIR)/filelist.f
	@printf '// Add files in this order, or use: read_verilog -sv [glob *.sv]\n' \
		>> $(RTL_EXPORT_DIR)/filelist.f
	@printf '// Xilinx primitives (IBUFDS, MMCME4_ADV, BUFGCE) come from unisim.\n' \
		>> $(RTL_EXPORT_DIR)/filelist.f
	@for f in 1_rv32i_pkg.sv 2_tbn_fifo.sv 3_tbn_async_fifo.sv 4_Marie.sv 5_marie_top.sv; do \
		echo "$$f" >> $(RTL_EXPORT_DIR)/filelist.f; \
	done
	@cd $(RTL_EXPORT_DIR) && zip -q ../$(RTL_ZIP) filelist.f *.sv
	@echo "Exported: $(RTL_ZIP)"
	@unzip -l $(RTL_ZIP) | grep '\.sv\|filelist'

# Remove build artifacts
clean:
	rm -rf $(OBJ_DIR) $(SV_DIR) $(SOC_OBJ_DIR) $(SOC_SV_DIR) $(RTL_EXPORT_DIR) $(RTL_ZIP)
	rm -f tests/*.bin tests/*.o tests/*.elf
	rm -f sim_trace.vcd sim_trace.fst soc_trace.fst
