# cheri_perms.S — CHERI permission restriction and enforcement tests

.include "tests/rvy.inc"

    .section .text
    .globl _start
_start:
    j 1f
    TRAP_HANDLER_SKIP

1:  li x27, 0
    INSTALL_TRAP 0x80000004

# Test 1: AUIPC gives full perms (RWX = 7)
    li x10, 1
    auipc x6, 0
    YPERMR 5, 6
    li x8, 7
    bne x5, x8, fail

# Test 2: YPERMC restricts permissions (remove W → R+X = 5)
    li x10, 2
    li x7, 5                    # mask = R+X (no W)
    YPERMC 6, 6, 7
    YPERMR 5, 6
    li x8, 5
    bne x5, x8, fail

# Test 3: Store through read-only cap (no W) → traps
    li x10, 3
    lui x9, 0x80002              # x9 = 0x80002000 (integer ptr)
    li x3, 0x42
    sw x3, 0(x9)                 # store 0x42 via integer ptr
    auipc x6, 0                  # fresh tagged cap (RWX)
    lui x7, 0x80002
    YADDRW 6, 6, 7               # addr = 0x80002000
    li x7, 5                    # mask = R+X (no W)
    YPERMC 6, 6, 7
    li x3, 0x99
    sw x3, 0(x6)                 # traps (no W perm)
    lw x3, 0(x9)
    li x8, 0x42
    bne x3, x8, fail            # value unchanged
    li x8, 1
    bne x27, x8, fail           # 1 trap

# Test 4: Load through write-only cap (no R) → traps
    li x10, 4
    auipc x6, 0
    lui x7, 0x80002
    YADDRW 6, 6, 7
    li x7, 2                    # mask = W only
    YPERMC 6, 6, 7
    li x3, 0x77                 # sentinel
    lw x3, 0(x6)                # traps (no R perm)
    li x8, 0x77
    bne x3, x8, fail            # sentinel preserved
    li x8, 2
    bne x27, x8, fail           # 2 traps total

# Test 5: Load through R-permitted cap works
    li x10, 5
    auipc x6, 0
    lui x7, 0x80002
    YADDRW 6, 6, 7
    lw x3, 0(x6)                # R+W+X, succeeds
    li x8, 0x42
    bne x3, x8, fail

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
