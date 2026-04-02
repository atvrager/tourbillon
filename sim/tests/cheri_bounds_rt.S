# cheri_bounds_rt.S — CHERI Concentrate encode/decode round-trip tests

.include "tests/rvy.inc"

    .section .text
    .globl _start
_start:
    j 1f
    TRAP_HANDLER_FAIL

1:  INSTALL_TRAP 0x80000004

# Test 1: Small exact bounds (16 bytes, E=0)
    li x10, 1
    auipc x6, 0
    lui x7, 0x80000
    YADDRW 6, 6, 7
    li x7, 16
    YBNDSW 6, 6, 7              # [0x80000000, 0x80000010)
    YLENR 5, 6
    li x8, 16
    bne x5, x8, fail
    YBASER 5, 6
    lui x8, 0x80000
    bne x5, x8, fail
    YTOPR 5, 6
    lui x8, 0x80000
    addi x8, x8, 16
    bne x5, x8, fail

# Test 2: Medium bounds (4096 bytes, E=3)
    li x10, 2
    auipc x6, 0
    lui x7, 0x80000
    YADDRW 6, 6, 7
    lui x7, 1                   # 4096
    YBNDSW 6, 6, 7
    YLENR 5, 6
    lui x8, 1
    bne x5, x8, fail

# Test 3: Large bounds (65536 bytes, E=7)
    li x10, 3
    auipc x6, 0
    lui x7, 0x80000
    YADDRW 6, 6, 7
    lui x7, 0x10                # 65536
    YBNDSW 6, 6, 7
    YLENR 5, 6
    lui x8, 0x10
    bne x5, x8, fail

# Test 4: 1-byte bounds (E=0)
    li x10, 4
    auipc x6, 0
    lui x7, 0x80000
    YADDRW 6, 6, 7
    li x7, 1
    YBNDSW 6, 6, 7
    YLENR 5, 6
    li x8, 1
    bne x5, x8, fail

# Test 5: Bounds at non-zero base (0x80001000, 256 bytes, E=0)
    li x10, 5
    auipc x6, 0
    lui x7, 0x80001
    YADDRW 6, 6, 7
    li x7, 256
    YBNDSW 6, 6, 7              # [0x80001000, 0x80001100)
    YBASER 5, 6
    lui x8, 0x80001
    bne x5, x8, fail
    YTOPR 5, 6
    lui x8, 0x80001
    addi x8, x8, 256
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
