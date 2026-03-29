# hello.S — "Hello, World!" for the Marie Antoinette SoC
#
# Writes each character of "Hello, World!\n" to UART TX register
# at 0x10000000, then writes 1 to tohost to signal pass.
#
# Memory map:
#   0x10000000 = UART TX data (write a byte to transmit)
#   0x80001000 = tohost (write 1 to pass, >1 to fail)

    .section .text
    .globl _start

_start:
    # a0 = pointer to string
    la      a0, hello_str

    # a1 = UART TX address (0x10000000)
    lui     a1, 0x10000

print_loop:
    lb      a2, 0(a0)         # load next byte
    beqz    a2, done           # null terminator → done

    sw      a2, 0(a1)         # write to UART TX
    addi    a0, a0, 1         # advance pointer
    j       print_loop

done:
    # Write 1 to tohost → PASS
    lui     a3, 0x80001        # a3 = 0x80001000
    li      a4, 1
    sw      a4, 0(a3)

spin:
    j       spin               # halt

    .section .rodata
hello_str:
    .asciz "Hello, World!\n"

    .section .tohost, "aw", @progbits
    .globl tohost
tohost:
    .word 0
