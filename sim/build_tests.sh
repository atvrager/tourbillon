#!/bin/bash
# Build rv32ui riscv-tests with Tourbillon's minimal env (no CSR/trap support)
set -e

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
REPO_ROOT="$(dirname "$SCRIPT_DIR")"
RISCV_TESTS="$REPO_ROOT/riscv-tests"
CUSTOM_ENV="$SCRIPT_DIR/env"
OUT_DIR="$SCRIPT_DIR/tests/rv32ui"
CC="${RISCV_CC:-riscv64-elf-gcc}"

# Test list (all rv32ui-p tests except fence_i)
TESTS=(
    add addi and andi auipc
    beq bge bgeu blt bltu bne
    jal jalr
    lb lbu lh lhu lui lw
    or ori
    sb sh simple sll slli slt slti sltiu sltu
    sra srai srl srli sub sw
    xor xori
)

mkdir -p "$OUT_DIR"

LINK_SCRIPT="$SCRIPT_DIR/env/link.ld"
cat > "$LINK_SCRIPT" << 'LDEOF'
OUTPUT_ARCH( "riscv" )
ENTRY(_start)
SECTIONS
{
  . = 0x80000000;
  .text.init : { *(.text.init) }
  . = ALIGN(0x1000);
  .tohost : { *(.tohost) }
  . = ALIGN(0x1000);
  .text : { *(.text) }
  .data : { *(.data) }
  .bss : { *(.bss) }
  _end = .;
}
LDEOF

built=0
for t in "${TESTS[@]}"; do
    src="$RISCV_TESTS/isa/rv32ui/$t.S"
    if [ ! -f "$src" ]; then
        echo "SKIP $t (no source)"
        continue
    fi
    out="$OUT_DIR/$t"
    $CC -march=rv32i -mabi=ilp32 -static -nostdlib -nostartfiles \
        -I"$CUSTOM_ENV/p" \
        -I"$RISCV_TESTS/isa/macros/scalar" \
        -I"$RISCV_TESTS/env/p" \
        -T"$LINK_SCRIPT" \
        -o "$out" "$src" 2>/dev/null && built=$((built+1)) || echo "FAIL build $t"
done

echo "Built $built tests in $OUT_DIR"
