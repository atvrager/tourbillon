# Tourbillon RV32I Verilator simulation Makefile

TBN       ?= ../target/debug/tbn
SV_DIR    ?= sv_out
VERILATOR ?= verilator
OBJ_DIR   := obj_dir
SIM_BIN   := $(OBJ_DIR)/sim_cpu

# RISC-V toolchain
RISCV_PREFIX ?= riscv64-unknown-elf-
AS            = $(RISCV_PREFIX)as
LD            = $(RISCV_PREFIX)ld
OBJCOPY       = $(RISCV_PREFIX)objcopy

# Default test binary
TEST ?= tests/smoke.bin

# riscv-tests location
RISCV_TESTS_DIR ?= /opt/riscv-tests/share/riscv-tests/isa

.PHONY: sv verilate build run test-smoke test-hex riscv-tests clean

# Generate SystemVerilog from Tourbillon source
sv:
	mkdir -p $(SV_DIR)
	$(TBN) build ../examples/rv32i.tbn -o $(SV_DIR)

# Compile SV to C++ via Verilator and build the simulator
NPROC := $(shell nproc 2>/dev/null || echo 4)
verilate: sv
	$(VERILATOR) --cc --exe --build --trace-fst -Wall -Wno-fatal \
		--top-module tb_top \
		--public-flat-rw \
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

# Remove build artifacts
clean:
	rm -rf $(OBJ_DIR) $(SV_DIR) tests/*.bin tests/*.o tests/*.elf
	rm -f sim_trace.vcd sim_trace.fst
