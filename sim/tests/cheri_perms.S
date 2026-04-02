# cheri_perms.S — CHERI permission restriction and enforcement tests
#
# Tests YPERMC (permission restrict) and load/store enforcement:
# - Verify full perms from PCC
# - YPERMC to remove write permission
# - Store through read-only cap → suppressed (data unchanged)
# - Load through write-only cap → returns 0
# - Load through read-permitted cap → succeeds
#
# Encodings (R-type: funct7 | rs2 | rs1 | funct3 | rd | 0110011):
# YPERMR x5,x6:     0x101302B3
# YPERMC x6,x6,x7:  0x0C732333
# YADDRW x6,x6,x7:  0x0C731333

    .section .text
    .globl _start
_start:

# =========================================================================
# Test 1: AUIPC gives full perms (RWX = 7)
# =========================================================================
    li x10, 1
    auipc x6, 0
    .word 0x101302B3             # YPERMR x5, x6
    li x8, 7
    bne x5, x8, fail

# =========================================================================
# Test 2: YPERMC restricts permissions (remove W → R+X = 5)
# =========================================================================
    li x10, 2
    li x7, 5                    # mask = R+X (bit pattern 101, no W)
    .word 0x0C732333             # YPERMC x6, x6, x7
    .word 0x101302B3             # YPERMR x5, x6
    li x8, 5
    bne x5, x8, fail

# =========================================================================
# Test 3: Store through read-only cap (no W perm) → suppressed
# =========================================================================
    li x10, 3
    # Write known value via integer pointer
    lui x9, 0x80002              # x9 = 0x80002000 (integer, tag=0)
    li x3, 0x42
    sw x3, 0(x9)                 # store 0x42 at 0x80002000 via integer ptr
    # Create cap with R+X (no W) pointing to 0x80002000
    auipc x6, 0                  # fresh PCC-derived cap (tag=1, RWX)
    lui x7, 0x80002
    .word 0x0C731333             # YADDRW x6, x6, x7 → addr = 0x80002000
    li x7, 5                    # mask = R+X (no W)
    .word 0x0C732333             # YPERMC x6, x6, x7
    # Attempt store through restricted cap — should be suppressed
    li x3, 0x99
    sw x3, 0(x6)                 # should fail (no W perm on tagged cap)
    # Read back via integer pointer — value should be unchanged
    lw x3, 0(x9)
    li x8, 0x42
    bne x3, x8, fail            # should still be 0x42

# =========================================================================
# Test 4: Load through write-only cap (no R) → returns 0
# =========================================================================
    li x10, 4
    auipc x6, 0
    lui x7, 0x80002
    .word 0x0C731333             # YADDRW x6, x6, x7 → addr = 0x80002000
    li x7, 2                    # mask = W only (no R, no X)
    .word 0x0C732333             # YPERMC x6, x6, x7
    li x3, 0x77                 # sentinel (non-zero)
    lw x3, 0(x6)                # load should return 0 (no R perm)
    bne x3, x0, fail

# =========================================================================
# Test 5: Load through R-permitted cap still works
# =========================================================================
    li x10, 5
    auipc x6, 0
    lui x7, 0x80002
    .word 0x0C731333             # YADDRW x6, x6, x7 → addr = 0x80002000
    # Keep full perms (PCC-derived: RWX = 7, R is set)
    lw x3, 0(x6)                # should succeed
    li x8, 0x42                 # should read 0x42 (stored in test 3)
    bne x3, x8, fail

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
