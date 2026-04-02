# swatch.S — Swatch Internet Time peripheral test
#
# Tests the SwatchDevice: read default values, write to seed,
# read back, verify the free-running prescaler advances.
# Discovers Swatch base address via manifest slot 2.
# Runs through the SoC bus fabric (soc-swatch-test target).
#
# Memory map:
#   0x00000000 = Manifest device (region 0x0)
#   0x30000000 = Swatch device   (region 0x3)
#   0x80001000 = tohost

    .section .text
    .globl _start

# Test number tracking: x30 = current test number (1-based)
# Fail: write (test_num << 1) to tohost
# Pass: write 1 to tohost

.equ TOHOST,      0x80001000
.equ MANIFEST,    0x00000000
.equ SWATCH,      0x30000000

# Manifest slot layout
.equ SLOT2_ADDR,   0x140        # Manifest + 0x100 + 2*0x20
.equ SLOT_BASE_ADDR_OFF, 0x08
.equ SLOT_KEY_LO_OFF,    0x04
.equ SLOT_VALID_OFF,     0x18

# Swatch register offsets (word offset = addr[3:2])
.equ OFF_BEAT,      0x00       # Read: current beat (0-999)
.equ OFF_CENTIBEAT, 0x04       # Read: centibeat (0-99)
.equ OFF_PRESCALER, 0x08       # Read: raw prescaler ticks

# Expected values
.equ EXPECT_VALID,   0x564C4944  # "VLID"
.equ EXPECT_KEY_LO,  0x53574348  # "SWCH"

_start:
    li      x30, 1              # test counter

    # ------------------------------------------------------------------
    # Test 1: Discover Swatch via manifest — slot 2 valid
    # ------------------------------------------------------------------
    lui     a0, %hi(MANIFEST)
    addi    a3, a0, SLOT2_ADDR      # a3 = &slot2
    lw      a1, SLOT_VALID_OFF(a3)
    lui     a2, %hi(EXPECT_VALID)
    addi    a2, a2, %lo(EXPECT_VALID)
    bne     a1, a2, fail
    addi    x30, x30, 1

    # ------------------------------------------------------------------
    # Test 2: Slot 2 key_lo = "SWCH" (0x53574348)
    # ------------------------------------------------------------------
    lw      a1, SLOT_KEY_LO_OFF(a3)
    lui     a2, %hi(EXPECT_KEY_LO)
    addi    a2, a2, %lo(EXPECT_KEY_LO)
    bne     a1, a2, fail
    addi    x30, x30, 1

    # ------------------------------------------------------------------
    # Test 3: Slot 2 base_addr = 0x30000000
    # ------------------------------------------------------------------
    lw      a1, SLOT_BASE_ADDR_OFF(a3)
    lui     a2, 0x30000
    bne     a1, a2, fail
    addi    x30, x30, 1

    # ------------------------------------------------------------------
    # Test 4: Read default beat → 0 (reset state)
    # ------------------------------------------------------------------
    lui     a4, %hi(SWATCH)     # a4 = swatch base (discovered or direct)
    lw      a1, OFF_BEAT(a4)
    bne     a1, zero, fail
    addi    x30, x30, 1

    # ------------------------------------------------------------------
    # Test 5: Read default centibeat → 0
    # ------------------------------------------------------------------
    lw      a1, OFF_CENTIBEAT(a4)
    bne     a1, zero, fail
    addi    x30, x30, 1

    # ------------------------------------------------------------------
    # Test 6: Write beat = 762, read back → 762
    # ------------------------------------------------------------------
    li      a5, 762
    sw      a5, OFF_BEAT(a4)
    lw      a1, OFF_BEAT(a4)
    li      a2, 762
    bne     a1, a2, fail
    addi    x30, x30, 1

    # ------------------------------------------------------------------
    # Test 7: Write centibeat = 50, read back → 50
    # ------------------------------------------------------------------
    li      a5, 50
    sw      a5, OFF_CENTIBEAT(a4)
    lw      a1, OFF_CENTIBEAT(a4)
    li      a2, 50
    bne     a1, a2, fail
    addi    x30, x30, 1

    # ------------------------------------------------------------------
    # Test 8: Prescaler advances between reads (free-running)
    # Read prescaler twice; second should be greater (or wrapped,
    # but with ~43M ticks per centibeat, two bus reads apart won't wrap)
    # ------------------------------------------------------------------
    lw      a1, OFF_PRESCALER(a4)  # first read
    lw      a2, OFF_PRESCALER(a4)  # second read
    bgeu    a1, a2, fail           # a2 must be > a1
    addi    x30, x30, 1

    # ------------------------------------------------------------------
    # Test 9: Beat value persists after prescaler reads
    # (verify seed wasn't corrupted)
    # ------------------------------------------------------------------
    lw      a1, OFF_BEAT(a4)
    li      a2, 762
    bne     a1, a2, fail
    addi    x30, x30, 1

    # ------------------------------------------------------------------
    # Test 10: Centibeat value persists
    # ------------------------------------------------------------------
    lw      a1, OFF_CENTIBEAT(a4)
    li      a2, 50
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
