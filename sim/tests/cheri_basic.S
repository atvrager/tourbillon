# cheri_basic.S — Basic RVY capability instruction tests

.include "tests/rvy.inc"

    .section .text
    .globl _start
_start:
    j 1f
    TRAP_HANDLER_FAIL

1:  INSTALL_TRAP 0x80000004

# Test 1: YTAGR on x0 → 0
    li x10, 1
    YTAGR 5, 0
    bne x5, x0, fail

# Test 2: YPERMR on x0 → 0
    li x10, 2
    YPERMR 5, 0
    bne x5, x0, fail

# Test 3: AUIPC produces non-zero
    li x10, 3
    auipc x5, 0
    beq x5, x0, fail

# Test 4: YADD increments address
    li x10, 4
    auipc x6, 0
    li x7, 100
    YADD 5, 6, 7
    beq x5, x6, fail

# Test 5: YADDRW sets address
    li x10, 5
    auipc x6, 0
    li x7, 0x42
    YADDRW 5, 6, 7
    li x8, 0x42
    bne x5, x8, fail

# Test 6: YLENR on x0 → 0
    li x10, 6
    YLENR 5, 0
    bne x5, x0, fail

# Test 7: PACKY clears tag, preserves address
    li x10, 7
    li x6, 0x55
    PACKY 5, 6, 0
    li x8, 0x55
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
