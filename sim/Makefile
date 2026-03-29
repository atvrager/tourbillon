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

.PHONY: sv verilate build run test-smoke riscv-tests clean

# Generate SystemVerilog from Tourbillon source
sv:
	mkdir -p $(SV_DIR)
	$(TBN) build ../examples/rv32i.tbn -o $(SV_DIR)

# Compile SV to C++ via Verilator and build the simulator
verilate: sv
	$(VERILATOR) --cc --exe --build --trace -Wall -Wno-fatal \
		--top-module tb_top \
		-I$(SV_DIR) \
		tb_top.sv rv32i_pkg.sv \
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

# Assemble and run the smoke test
test-smoke: build tests/smoke.bin
	$(SIM_BIN) tests/smoke.bin

# Run the full riscv-tests rv32ui suite
# Expects RISCV_TESTS_DIR to point to the built riscv-tests directory
RISCV_TESTS_DIR ?= /opt/riscv-tests/share/riscv-tests/isa
riscv-tests: build
	@pass=0; fail=0; \
	for t in $(RISCV_TESTS_DIR)/rv32ui-p-*; do \
		test -f "$$t" || continue; \
		case "$$t" in *.dump) continue;; esac; \
		name=$$(basename $$t); \
		if $(SIM_BIN) $$t; then \
			echo "  PASS $$name"; \
			pass=$$((pass + 1)); \
		else \
			echo "  FAIL $$name"; \
			fail=$$((fail + 1)); \
		fi; \
	done; \
	echo ""; \
	echo "Results: $$pass passed, $$fail failed"; \
	test $$fail -eq 0

# Remove build artifacts
clean:
	rm -rf $(OBJ_DIR) $(SV_DIR) tests/*.bin tests/*.o tests/*.elf
	rm -f sim_trace.vcd
