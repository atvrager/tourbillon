# cheri_inspect.S — CHERI capability inspection instruction tests

.include "tests/rvy.inc"

    .section .text
    .globl _start
_start:
    j 1f
    TRAP_HANDLER_FAIL

1:  INSTALL_TRAP 0x80000004

# Test 1: YBNDSW(256) → YTOPR → top = base + 256
    li x10, 1
    auipc x6, 0
    lui x7, 0x80000
    YADDRW 6, 6, 7              # addr = 0x80000000
    li x7, 256
    YBNDSW 6, 6, 7              # bounds [0x80000000, 0x80000100)
    YTOPR 5, 6
    lui x8, 0x80000
    addi x8, x8, 256
    bne x5, x8, fail

# Test 2: YBASER → base = 0x80000000
    li x10, 2
    YBASER 5, 6
    lui x8, 0x80000
    bne x5, x8, fail

# Test 3: YLENR → length = 256
    li x10, 3
    YLENR 5, 6
    li x8, 256
    bne x5, x8, fail

# Test 4: YTYPER on unsealed → 0
    li x10, 4
    YTYPER 5, 6
    bne x5, x0, fail

# Test 5: YAMASK on E=0 → 0xFFFFFFFF
    li x10, 5
    YAMASK 5, 6
    li x8, -1
    bne x5, x8, fail

# PASS
    lui x5, 0x80001
    li x6, 1
    sw x6, 0(x5)
1:  j 1b

fail:
    slli x10, x10, 1
    lui x5, 0x80001
    sw x10, 0(x5)
1:  j 1b
