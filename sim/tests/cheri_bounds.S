# cheri_bounds.S — CHERI bounds checking test
#
# Tests capability bounds enforcement with real exceptions:
# - Out-of-bounds loads trap to mtvec, handler skips faulting instruction
# - In-bounds loads succeed normally

.include "tests/rvy.inc"

    .section .text
    .globl _start
_start:
    j 1f
    TRAP_HANDLER_SKIP

1:  li x27, 0
    INSTALL_TRAP 0x80000004

# Test 1: Verify PCC-derived cap has tag=1
    li x10, 1
    auipc x6, 0
    YTAGR 5, 6
    li x8, 1
    bne x5, x8, fail

# Test 2: Set 16-byte bounds, verify tag preserved
    li x10, 2
    auipc x6, 0
    li x7, 16
    YBNDSW 6, 6, 7             # bounds [addr, addr+16)
    YTAGR 5, 6
    li x8, 1
    bne x5, x8, fail

# Test 3: In-bounds load at offset 0
    li x10, 3
    lw x3, 0(x6)

# Test 4: Out-of-bounds load → trap, sentinel preserved
    li x10, 4
    li x3, 0x12345678
    lw x3, 20(x6)              # out of bounds → trap → skip
    li x8, 0x12345678
    bne x3, x8, fail
    li x8, 1
    bne x27, x8, fail          # 1 trap so far

# Test 5: Boundary-exact load (offset 12 + size 4 = top)
    li x10, 5
    li x3, 0
    lw x3, 12(x6)              # just in bounds

# Test 6: One past boundary → trap
    li x10, 6
    li x3, 0xDEAD
    lw x3, 16(x6)              # past top → trap → skip
    li x8, 0xDEAD
    bne x3, x8, fail
    li x8, 2
    bne x27, x8, fail          # 2 traps total

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
