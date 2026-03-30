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

SPI_SV_DIR  ?= spi_sv_out
SPI_OBJ_DIR := spi_obj_dir
SPI_BIN     := $(SPI_OBJ_DIR)/sim_spi

.PHONY: sv verilate build run test-smoke test-hex riscv-tests clean
.PHONY: soc-sv soc-build soc-hello soc-riscv-tests rtl-check
.PHONY: rtl-export rtl-export-standalone rtl-export-ext
.PHONY: spi-sv spi-build spi-test

# Generate SystemVerilog from Tourbillon source
sv:
	mkdir -p $(SV_DIR)
	$(TBN) build ../examples/cpu_core.tbn ../examples/rv32i.tbn -o $(SV_DIR)

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
	$(TBN) build ../examples/cpu_core.tbn ../examples/marie.tbn -o $(SOC_SV_DIR)

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

# Build hello.hex from hello.S (needed for FPGA BRAM init)
# objcopy -O verilog gives byte-addressed hex; verilog2hex.py converts to
# word-addressed $readmemh format suitable for Vivado BRAM initialisation.
tests/hello.hex: tests/hello.elf
	$(OBJCOPY) -O verilog tests/hello.elf tests/hello.hex.raw
	python3 verilog2hex.py tests/hello.hex.raw tests/hello.hex
	@rm -f tests/hello.hex.raw

# Default: standalone (internal BRAM with hello.hex — ready to run)
rtl-export: rtl-export-standalone

# Standalone: STANDALONE=1, hello.hex baked into BRAM
rtl-export-standalone: soc-sv tests/hello.elf tests/hello.hex
	@rm -rf $(RTL_EXPORT_DIR) $(RTL_ZIP)
	@mkdir -p $(RTL_EXPORT_DIR)
	@cp rv32i_pkg.sv                     $(RTL_EXPORT_DIR)/1_rv32i_pkg.sv
	@cp $(SOC_SV_DIR)/tbn_fifo.sv        $(RTL_EXPORT_DIR)/2_tbn_fifo.sv
	@cp $(SOC_SV_DIR)/tbn_async_fifo.sv  $(RTL_EXPORT_DIR)/3_tbn_async_fifo.sv
	@cp $(SOC_SV_DIR)/Marie.sv           $(RTL_EXPORT_DIR)/4_Marie.sv
	@cp $(RTL_DIR)/fpga_mem.sv           $(RTL_EXPORT_DIR)/5_fpga_mem.sv
	@cp $(RTL_DIR)/marie_top.sv          $(RTL_EXPORT_DIR)/6_marie_top.sv
	@cp tests/hello.hex                  $(RTL_EXPORT_DIR)/hello.hex
	@printf '// Marie Antoinette SoC — Vivado source file list (topo-sorted)\n' \
		> $(RTL_EXPORT_DIR)/filelist.f
	@printf '// Toplevel: marie_top #(.STANDALONE(1))\n' \
		>> $(RTL_EXPORT_DIR)/filelist.f
	@printf '// Internal BRAM initialised from hello.hex — prints Hello World on UART TX.\n' \
		>> $(RTL_EXPORT_DIR)/filelist.f
	@printf '// Vivado: set_property generic STANDALONE=1 [current_fileset]\n' \
		>> $(RTL_EXPORT_DIR)/filelist.f
	@for f in 1_rv32i_pkg.sv 2_tbn_fifo.sv 3_tbn_async_fifo.sv \
	          4_Marie.sv 5_fpga_mem.sv 6_marie_top.sv; do \
		echo "$$f" >> $(RTL_EXPORT_DIR)/filelist.f; \
	done
	@cd $(RTL_EXPORT_DIR) && zip -q ../$(RTL_ZIP) filelist.f hello.hex *.sv
	@echo "Exported: $(RTL_ZIP) (standalone — STANDALONE=1 with hello.hex)"
	@unzip -l $(RTL_ZIP) | grep '\.sv\|\.hex\|filelist'

# External memory: STANDALONE=0, memory ports exposed (no hex, no fpga_mem)
rtl-export-ext: soc-sv
	@rm -rf $(RTL_EXPORT_DIR) $(RTL_ZIP)
	@mkdir -p $(RTL_EXPORT_DIR)
	@cp rv32i_pkg.sv                     $(RTL_EXPORT_DIR)/1_rv32i_pkg.sv
	@cp $(SOC_SV_DIR)/tbn_fifo.sv        $(RTL_EXPORT_DIR)/2_tbn_fifo.sv
	@cp $(SOC_SV_DIR)/tbn_async_fifo.sv  $(RTL_EXPORT_DIR)/3_tbn_async_fifo.sv
	@cp $(SOC_SV_DIR)/Marie.sv           $(RTL_EXPORT_DIR)/4_Marie.sv
	@cp $(RTL_DIR)/marie_top.sv          $(RTL_EXPORT_DIR)/5_marie_top.sv
	@printf '// Marie Antoinette SoC — Vivado source file list (topo-sorted)\n' \
		> $(RTL_EXPORT_DIR)/filelist.f
	@printf '// Toplevel: marie_top #(.STANDALONE(0))\n' \
		>> $(RTL_EXPORT_DIR)/filelist.f
	@printf '// External memory ports — wire imem/dmem to your SRAM controller.\n' \
		>> $(RTL_EXPORT_DIR)/filelist.f
	@for f in 1_rv32i_pkg.sv 2_tbn_fifo.sv 3_tbn_async_fifo.sv \
	          4_Marie.sv 5_marie_top.sv; do \
		echo "$$f" >> $(RTL_EXPORT_DIR)/filelist.f; \
	done
	@cd $(RTL_EXPORT_DIR) && zip -q ../$(RTL_ZIP) filelist.f *.sv
	@echo "Exported: $(RTL_ZIP) (external memory — STANDALONE=0)"
	@unzip -l $(RTL_ZIP) | grep '\.sv\|filelist'

# =========================================================================
# SPI-to-TileLink-UL bridge targets
# =========================================================================

# Generate SV from Tourbillon source
spi-sv:
	mkdir -p $(SPI_SV_DIR)
	$(TBN) build ../examples/spi2tlul.tbn -o $(SPI_SV_DIR)

# Build SPI bridge simulator
spi-build: spi-sv
	$(VERILATOR) --cc --exe --build --trace-fst -Wall -Wno-fatal \
		--top-module spi_tb_top \
		-j $(NPROC) \
		-I$(SPI_SV_DIR) \
		spi_tb_top.sv \
		$(SPI_SV_DIR)/*.sv \
		spi_tb.cpp \
		-o sim_spi \
		--Mdir $(SPI_OBJ_DIR)

# Build and run the SPI testbench
spi-test: spi-build
	$(SPI_BIN)

# Remove build artifacts
clean:
	rm -rf $(OBJ_DIR) $(SV_DIR) $(SOC_OBJ_DIR) $(SOC_SV_DIR) $(SPI_OBJ_DIR) $(SPI_SV_DIR) $(RTL_EXPORT_DIR) $(RTL_ZIP)
	rm -f tests/*.bin tests/*.o tests/*.elf
	rm -f sim_trace.vcd sim_trace.fst soc_trace.fst spi_trace.fst
