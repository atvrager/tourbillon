.section .text
.globl _start
_start:
    addi x1, x0, 1
    addi x2, x0, 2
    add  x3, x1, x2
    # x3 should be 3
    # Write 1 to tohost (pass)
    li   x4, 0x80001000
    li   x5, 1
    sw   x5, 0(x4)
    # Spin
1:  j    1b
