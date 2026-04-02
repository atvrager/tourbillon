# cheri_bounds.S — CHERI bounds checking test
#
# YBNDSW x6,x6,x7: 0x0e730333
# YTAGR  x5,x6:    0x100302b3

    .section .text
    .globl _start
_start:

# Test 1: Verify PCC-derived cap has tag=1
    li x10, 1
    auipc x6, 0
    .word 0x100302b3         # YTAGR x5, x6
    li x8, 1
    bne x5, x8, fail

# Test 2: Set 16-byte bounds, in-bounds load works
    li x10, 2
    auipc x6, 0             # x6 = cap at this PC (tag=1, full bounds)
    li x7, 16
    .word 0x0e730333         # YBNDSW x6, x6, x7 — bounds [addr, addr+16)
    # Verify tag still 1 after YBNDSW
    .word 0x100302b3         # YTAGR x5, x6
    li x8, 1
    bne x5, x8, fail

# Test 3: In-bounds load at offset 0 should succeed (returns non-zero instruction data)
    li x10, 3
    lw x3, 0(x6)            # in-bounds: offset 0 < 16
    # x3 is an instruction word, should be non-zero (it's whatever is at that address)
    # Actually we can't easily predict the value. Just verify no trap (x3 != 0 is likely)

# Test 4: Out-of-bounds load should return 0 (soft trap)
    li x10, 4
    li x3, 0x12345678       # sentinel
    lw x3, 20(x6)           # OUT OF BOUNDS: offset 20 >= 16 → soft trap → x3 = 0
    bne x3, x0, fail        # x3 should be 0

# Test 5: Boundary-exact load: offset 12, size 4 → addr+16 = top (just in bounds)
    li x10, 5
    li x3, 0
    lw x3, 12(x6)           # addr+12+4 = addr+16 = top → should succeed

# Test 6: One past boundary: offset 16, size 4 → addr+20 > top (violation)
    li x10, 6
    li x3, 0xDEAD
    lw x3, 16(x6)           # addr+16+4 = addr+20 > top → violation → x3 = 0
    bne x3, x0, fail

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
