# manifest.S — Device manifest + clocks peripheral test
#
# Tests manifest reads, CLAIM, SEAL, and clock frequency queries.
# Runs through the SoC bus fabric (soc-manifest-test target).
#
# Memory map:
#   0x00000000 = Manifest device (region 0x0)
#   0x20000000 = Clocks device   (region 0x2)
#   0x80001000 = tohost

    .section .text
    .globl _start

# Test number tracking: x30 = current test number (1-based)
# Fail: write (test_num << 1) to tohost
# Pass: write 1 to tohost

.equ TOHOST,      0x80001000
.equ MANIFEST,    0x00000000
.equ CLOCKS,      0x20000000

# Manifest header offsets
.equ OFF_MAGIC,     0x00
.equ OFF_VERSION,   0x04
.equ OFF_MAX_SLOTS, 0x08
.equ OFF_POPULATED, 0x0C
.equ OFF_CLAIM,     0x10
.equ OFF_STATUS,    0x14
.equ OFF_SEAL,      0x1C

# Slot offsets from slot base (0x100 + slot*0x20)
.equ SLOT_BASE_ADDR, 0x08
.equ SLOT_VALID,     0x18

# Expected values
.equ EXPECT_MAGIC,   0xDE01D100
.equ EXPECT_VALID,   0x564C4944

_start:
    li      x30, 1              # test counter

    # ------------------------------------------------------------------
    # Test 1: Read manifest MAGIC
    # ------------------------------------------------------------------
    lui     a0, %hi(MANIFEST)
    lw      a1, OFF_MAGIC(a0)
    lui     a2, %hi(EXPECT_MAGIC)
    addi    a2, a2, %lo(EXPECT_MAGIC)
    bne     a1, a2, fail
    addi    x30, x30, 1

    # ------------------------------------------------------------------
    # Test 2: Read VERSION → 1
    # ------------------------------------------------------------------
    lw      a1, OFF_VERSION(a0)
    li      a2, 1
    bne     a1, a2, fail
    addi    x30, x30, 1

    # ------------------------------------------------------------------
    # Test 3: Read MAX_SLOTS → 4
    # ------------------------------------------------------------------
    lw      a1, OFF_MAX_SLOTS(a0)
    li      a2, 4
    bne     a1, a2, fail
    addi    x30, x30, 1

    # ------------------------------------------------------------------
    # Test 4: Read POPULATED → 2
    # ------------------------------------------------------------------
    lw      a1, OFF_POPULATED(a0)
    li      a2, 2
    bne     a1, a2, fail
    addi    x30, x30, 1

    # ------------------------------------------------------------------
    # Test 5: Read STATUS → 0 (unsealed)
    # ------------------------------------------------------------------
    lw      a1, OFF_STATUS(a0)
    bne     a1, zero, fail
    addi    x30, x30, 1

    # ------------------------------------------------------------------
    # Test 6: Read slot 0 BASE_ADDR → 0x80000000
    # Slot 0 base = MANIFEST + 0x100 + 0*0x20 = 0x100
    # lw immediate max is ±2048, so 0x100 is fine
    # ------------------------------------------------------------------
    addi    a3, a0, 0x100       # a3 = slot 0 base
    lw      a1, SLOT_BASE_ADDR(a3)  # offset 0x08 within slot
    lui     a2, 0x80000
    bne     a1, a2, fail
    addi    x30, x30, 1

    # ------------------------------------------------------------------
    # Test 7: Read slot 0 VALID → 0x564C4944
    # ------------------------------------------------------------------
    lw      a1, SLOT_VALID(a3)  # offset 0x18 within slot
    lui     a2, %hi(EXPECT_VALID)
    addi    a2, a2, %lo(EXPECT_VALID)
    bne     a1, a2, fail
    addi    x30, x30, 1

    # ------------------------------------------------------------------
    # Test 8: Read slot 1 BASE_ADDR → 0x10000000
    # Slot 1 base = MANIFEST + 0x100 + 1*0x20 = 0x120
    # ------------------------------------------------------------------
    addi    a3, a0, 0x120       # a3 = slot 1 base
    lw      a1, SLOT_BASE_ADDR(a3)
    lui     a2, 0x10000
    bne     a1, a2, fail
    addi    x30, x30, 1

    # ------------------------------------------------------------------
    # Test 9: CLAIM → 2 (next free slot), then CLAIM again → 3
    # ------------------------------------------------------------------
    lw      a1, OFF_CLAIM(a0)   # first CLAIM
    li      a2, 2
    bne     a1, a2, fail
    addi    x30, x30, 1

    # Test 10: Second CLAIM → 3
    lw      a1, OFF_CLAIM(a0)
    li      a2, 3
    bne     a1, a2, fail
    addi    x30, x30, 1

    # ------------------------------------------------------------------
    # Test 11: SEAL (write 1 to offset 0x1C)
    # ------------------------------------------------------------------
    li      a4, 1
    sw      a4, OFF_SEAL(a0)
    addi    x30, x30, 1

    # ------------------------------------------------------------------
    # Test 12: Read STATUS → 1 (sealed)
    # ------------------------------------------------------------------
    lw      a1, OFF_STATUS(a0)
    li      a2, 1
    bne     a1, a2, fail
    addi    x30, x30, 1

    # ------------------------------------------------------------------
    # Test 13: CLAIM after seal → 0xFFFFFFFF
    # ------------------------------------------------------------------
    lw      a1, OFF_CLAIM(a0)
    li      a2, -1              # 0xFFFFFFFF
    bne     a1, a2, fail
    addi    x30, x30, 1

    # ------------------------------------------------------------------
    # Test 14: Read clocks — CPU freq = 100000000 (0x05F5E100)
    # ------------------------------------------------------------------
    lui     a5, %hi(CLOCKS)     # a5 = 0x20000000
    lw      a1, 0(a5)           # offset 0: CPU_FREQ_HZ
    lui     a2, %hi(100000000)
    addi    a2, a2, %lo(100000000)
    bne     a1, a2, fail
    addi    x30, x30, 1

    # ------------------------------------------------------------------
    # Test 15: Read clocks — DEV freq = 50000000 (0x02FAF080)
    # ------------------------------------------------------------------
    lw      a1, 8(a5)           # offset 8: DEV_FREQ_HZ
    lui     a2, %hi(50000000)
    addi    a2, a2, %lo(50000000)
    bne     a1, a2, fail
    addi    x30, x30, 1

    # ------------------------------------------------------------------
    # PASS
    # ------------------------------------------------------------------
    lui     a0, %hi(TOHOST)
    li      a1, 1
    sw      a1, %lo(TOHOST)(a0)
spin:
    j       spin

fail:
    # Write (test_num << 1) to tohost
    slli    a1, x30, 1
    lui     a0, %hi(TOHOST)
    sw      a1, %lo(TOHOST)(a0)
    j       spin

    .section .tohost, "aw", @progbits
    .globl tohost
tohost:
    .word 0
