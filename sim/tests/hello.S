# hello.S — "Hello, World!" for the Marie Antoinette SoC
#
# Discovers the UART base address via the device manifest, then writes
# each character of "Hello, World!\n" to the UART TX register.
# Writes 1 to tohost to signal pass.
#
# Memory map:
#   0x00000000 = Manifest device (region 0x0)
#   0x80001000 = tohost (write 1 to pass, >1 to fail)

    .section .text
    .globl _start

.equ MANIFEST,     0x00000000
.equ SLOT1_BASE,   0x120        # Manifest + 0x100 + 1*0x20
.equ SLOT_BASE_ADDR_OFF, 0x08   # base_addr field within slot
.equ TOHOST,       0x80001000

_start:
    # ---- Discover UART base address from manifest slot 1 ----
    lui     a3, %hi(MANIFEST)
    addi    a3, a3, SLOT1_BASE      # a3 = &slot1
    lw      a1, SLOT_BASE_ADDR_OFF(a3)  # a1 = UART base address (discovered)

    # a0 = pointer to string
    la      a0, hello_str

print_loop:
    lb      a2, 0(a0)         # load next byte
    beqz    a2, done           # null terminator → done

    sw      a2, 0(a1)         # write to UART TX (discovered address)
    addi    a0, a0, 1         # advance pointer
    j       print_loop

done:
    # Write 1 to tohost → PASS
    lui     a3, %hi(TOHOST)
    li      a4, 1
    sw      a4, %lo(TOHOST)(a3)

spin:
    j       spin               # halt

    .section .rodata
hello_str:
    .asciz "Hello, World!\n"

    .section .tohost, "aw", @progbits
    .globl tohost
tohost:
    .word 0
