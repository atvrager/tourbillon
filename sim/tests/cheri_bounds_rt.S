# cheri_bounds_rt.S — CHERI Concentrate encode/decode round-trip tests
#
# Verifies YBNDSW encoder and YBASER/YTOPR/YLENR decoders produce
# consistent results at multiple scales and addresses.
# Uses YADDRW to force well-aligned addresses for exact round-trips.
#
# Encodings (R-type: funct7 | rs2 | rs1 | funct3 | rd | 0110011):
# YADDRW x6,x6,x7: 0x0C731333
# YBNDSW x6,x6,x7: 0x0E730333
# YTOPR  x5,x6:    0x104302B3
# YBASER x5,x6:    0x105302B3
# YLENR  x5,x6:    0x106302B3

    .section .text
    .globl _start
_start:

# =========================================================================
# Test 1: Small exact bounds (16 bytes, E=0)
# =========================================================================
    li x10, 1
    auipc x6, 0
    lui x7, 0x80000
    .word 0x0C731333             # YADDRW x6, x6, x7 → addr = 0x80000000
    li x7, 16
    .word 0x0E730333             # YBNDSW x6, x6, x7 → [0x80000000, 0x80000010)
    .word 0x106302B3             # YLENR x5, x6
    li x8, 16
    bne x5, x8, fail
    .word 0x105302B3             # YBASER x5, x6
    lui x8, 0x80000
    bne x5, x8, fail
    .word 0x104302B3             # YTOPR x5, x6
    lui x8, 0x80000
    addi x8, x8, 16
    bne x5, x8, fail

# =========================================================================
# Test 2: Medium bounds (4096 bytes, E=3)
# =========================================================================
    li x10, 2
    auipc x6, 0
    lui x7, 0x80000
    .word 0x0C731333             # YADDRW x6, x6, x7 → addr = 0x80000000
    lui x7, 1                   # x7 = 4096
    .word 0x0E730333             # YBNDSW x6, x6, x7
    .word 0x106302B3             # YLENR x5, x6
    lui x8, 1                   # x8 = 4096
    bne x5, x8, fail

# =========================================================================
# Test 3: Large bounds (65536 bytes, E=7)
# =========================================================================
    li x10, 3
    auipc x6, 0
    lui x7, 0x80000
    .word 0x0C731333             # YADDRW x6, x6, x7 → addr = 0x80000000
    lui x7, 0x10                # x7 = 65536
    .word 0x0E730333             # YBNDSW x6, x6, x7
    .word 0x106302B3             # YLENR x5, x6
    lui x8, 0x10                # x8 = 65536
    bne x5, x8, fail

# =========================================================================
# Test 4: Edge case — 1-byte bounds (E=0)
# =========================================================================
    li x10, 4
    auipc x6, 0
    lui x7, 0x80000
    .word 0x0C731333             # YADDRW x6, x6, x7 → addr = 0x80000000
    li x7, 1
    .word 0x0E730333             # YBNDSW x6, x6, x7
    .word 0x106302B3             # YLENR x5, x6
    li x8, 1
    bne x5, x8, fail

# =========================================================================
# Test 5: Bounds at non-zero base (0x80001000, 256 bytes, E=0)
# =========================================================================
    li x10, 5
    auipc x6, 0
    lui x7, 0x80001              # x7 = 0x80001000
    .word 0x0C731333             # YADDRW x6, x6, x7
    li x7, 256
    .word 0x0E730333             # YBNDSW x6, x6, x7 → [0x80001000, 0x80001100)
    .word 0x105302B3             # YBASER x5, x6
    lui x8, 0x80001
    bne x5, x8, fail
    .word 0x104302B3             # YTOPR x5, x6
    lui x8, 0x80001
    addi x8, x8, 256            # expected top = 0x80001100
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
