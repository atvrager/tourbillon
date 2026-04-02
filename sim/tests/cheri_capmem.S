# cheri_capmem.S — LC/SC (capability load/store) + tag memory tests

.include "tests/rvy.inc"

    .section .text
    .globl _start
_start:
    j 1f
    TRAP_HANDLER_FAIL

1:  INSTALL_TRAP 0x80000004

# Test 1: SC stores a capability, LC loads it back with tag preserved
    li x10, 1
    auipc x7, 0              # x7 = PCC-derived cap (tag=1)
    lui x6, 0x80002           # x6 = 0x80002000 (integer ptr)
    SC 7, 0, 6                # SC x7, 0(x6)
    LC 5, 0, 6                # LC x5, 0(x6)
    YTAGR 5, 5
    li x8, 1
    bne x5, x8, fail

# Test 2: Integer SW clears the tag
    li x10, 2
    li x7, 0x42
    sw x7, 0(x6)             # SW clears tag
    LC 5, 0, 6
    YTAGR 5, 5
    bne x5, x0, fail         # tag should be 0

# Test 3: SC at offset 8, LC at offset 8
    li x10, 3
    auipc x7, 0
    SC 7, 8, 6                # SC x7, 8(x6)
    LC 5, 8, 6                # LC x5, 8(x6)
    YTAGR 5, 5
    li x8, 1
    bne x5, x8, fail

# Test 4: LC from unwritten address → tag=0
    li x10, 4
    LC 5, 16, 6               # LC x5, 16(x6)
    YTAGR 5, 5
    bne x5, x0, fail

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
