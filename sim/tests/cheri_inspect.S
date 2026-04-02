# cheri_inspect.S — CHERI capability inspection instruction tests
#
# Tests YTOPR, YBASER, YLENR, YTYPER, YAMASK round-trip after YBNDSW.
# Uses YADDRW to force well-aligned addresses for exact round-trip.
#
# Encodings (R-type: funct7 | rs2 | rs1 | funct3 | rd | 0110011):
# YADDRW x6,x6,x7: 0x0C731333
# YBNDSW x6,x6,x7: 0x0E730333
# YTOPR  x5,x6:    0x104302B3
# YBASER x5,x6:    0x105302B3
# YLENR  x5,x6:    0x106302B3
# YTYPER x5,x6:    0x102302B3
# YAMASK x5,x6:    0x107302B3

    .section .text
    .globl _start
_start:

# =========================================================================
# Test 1: YBNDSW(256) → YTOPR → verify top = base + 256
# =========================================================================
    li x10, 1
    auipc x6, 0                 # x6 = PCC-derived cap (tag=1)
    lui x7, 0x80000
    .word 0x0C731333             # YADDRW x6, x6, x7 → addr = 0x80000000
    li x7, 256
    .word 0x0E730333             # YBNDSW x6, x6, x7 → bounds [0x80000000, 0x80000100)
    .word 0x104302B3             # YTOPR x5, x6
    lui x8, 0x80000
    addi x8, x8, 256            # expected top = 0x80000100
    bne x5, x8, fail

# =========================================================================
# Test 2: YBASER → verify base = 0x80000000
# =========================================================================
    li x10, 2
    .word 0x105302B3             # YBASER x5, x6
    lui x8, 0x80000
    bne x5, x8, fail

# =========================================================================
# Test 3: YLENR → verify length = 256
# =========================================================================
    li x10, 3
    .word 0x106302B3             # YLENR x5, x6
    li x8, 256
    bne x5, x8, fail

# =========================================================================
# Test 4: YTYPER on unsealed cap → should be 0
# =========================================================================
    li x10, 4
    .word 0x102302B3             # YTYPER x5, x6
    bne x5, x0, fail

# =========================================================================
# Test 5: YAMASK on E=0 (exact) → should be 0xFFFFFFFF
# =========================================================================
    li x10, 5
    .word 0x107302B3             # YAMASK x5, x6
    li x8, -1                   # 0xFFFFFFFF
    bne x5, x8, fail

# =========================================================================
# PASS
# =========================================================================
    lui x5, 0x80001
    li x6, 1
    sw x6, 0(x5)
1:  j 1b

fail:
    slli x10, x10, 1
    lui x5, 0x80001
    sw x10, 0(x5)
1:  j 1b
