# cheri_capmem.S — LC/SC (capability load/store) + tag memory tests
#
# Tests 2-beat capability transfers and tag propagation:
# - SC stores 65-bit cap (2 words + tag) to memory
# - LC loads 65-bit cap (2 words + tag) from memory
# - Integer SW clears tag at the capability-aligned address
#
# LC x5, off(x6):  0x00033283 (off=0), 0x00833283 (off=8)
# SC x7, off(x6):  0x00733023 (off=0), 0x00733423 (off=8)
# YTAGR x5, x7:    0x100382b3

    .section .text
    .globl _start
_start:

# =========================================================================
# Test 1: SC stores a capability, LC loads it back with tag preserved
# =========================================================================
    li x10, 1
    # Create a tagged capability via AUIPC
    auipc x7, 0              # x7 = PCC-derived cap (tag=1)
    # Store it to data memory at 0x80002000 via integer pointer
    lui x6, 0x80002           # x6 = 0x80002000 (tag=0, integer)
    .word 0x00733023          # SC x7, 0(x6) — store cap at 0x80002000
    # Load it back
    .word 0x00033283          # LC x5, 0(x6) — load cap from 0x80002000
    # Check tag is preserved
    .word 0x100282b3          # YTAGR x5, x5 — read tag of loaded cap
    li x8, 1
    bne x5, x8, fail         # tag should be 1

# =========================================================================
# Test 2: Integer SW to same address clears the tag
# =========================================================================
    li x10, 2
    # Write an integer word to 0x80002000 (overlapping the stored cap)
    li x7, 0x42
    sw x7, 0(x6)             # SW clears tag at 0x80002000's 8-byte region
    # Load cap back — tag should now be 0
    .word 0x00033283          # LC x5, 0(x6)
    .word 0x100282b3          # YTAGR x5, x5
    bne x5, x0, fail         # tag should be 0 (cleared by SW)

# =========================================================================
# Test 3: SC at offset 8, LC at offset 8
# =========================================================================
    li x10, 3
    auipc x7, 0              # x7 = fresh tagged cap
    .word 0x00733423          # SC x7, 8(x6) — store cap at 0x80002008
    .word 0x00833283          # LC x5, 8(x6) — load cap from 0x80002008
    .word 0x100282b3          # YTAGR x5, x5
    li x8, 1
    bne x5, x8, fail         # tag should be 1

# =========================================================================
# Test 4: LC from address that was never SC'd — tag should be 0
# =========================================================================
    li x10, 4
    .word 0x01033283          # LC x5, 16(x6) — load from 0x80002010 (never written via SC)
    .word 0x100282b3          # YTAGR x5, x5
    bne x5, x0, fail         # tag should be 0

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
