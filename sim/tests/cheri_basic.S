# cheri_basic.S — Hand-assembled CHERI instruction tests
#
# Tests basic RVY capability instructions using the Phase 2 pipeline.
# Uses .word directives with precomputed encodings since we need the
# CHERI toolchain for native assembly (make cheri-toolchain to install).
#
# R-type: funct7[31:25] | rs2[24:20] | rs1[19:15] | funct3[14:12] | rd[11:7] | opcode[6:0]
#
# YADD  rd,rs1,rs2: funct7=0000110, f3=000, op=0110011
# YADDRW rd,rs1,rs2: funct7=0000110, f3=001, op=0110011
# YTAGR rd,rs1:     funct7=0001000, rs2=00000, f3=000, op=0110011
# YPERMR rd,rs1:    funct7=0001000, rs2=00001, f3=000, op=0110011
# YLENR rd,rs1:     funct7=0001000, rs2=00110, f3=000, op=0110011
# PACKY rd,rs1,rs2: funct7=0000100, f3=011, op=0110011

    .section .text
    .globl _start
_start:

# Test 1: YTAGR on x0 → should be 0
    li x10, 1
    # YTAGR x5, x0 = 0001000_00000_00000_000_00101_0110011 = 0x100002b3
    .word 0x100002b3
    bne x5, x0, fail

# Test 2: YPERMR on x0 → should be 0
    li x10, 2
    # YPERMR x5, x0 = 0001000_00001_00000_000_00101_0110011 = 0x101002b3
    .word 0x101002b3
    bne x5, x0, fail

# Test 3: AUIPC produces a non-zero result
    li x10, 3
    auipc x5, 0
    beq x5, x0, fail

# Test 4: YADD increments address
    li x10, 4
    auipc x6, 0             # x6 = cap at current PC
    li x7, 100
    # YADD x5, x6, x7 = 0000110_00111_00110_000_00101_0110011 = 0x0c7302b3
    .word 0x0c7302b3
    beq x5, x6, fail        # should differ

# Test 5: YADDRW sets address
    li x10, 5
    auipc x6, 0
    li x7, 0x42
    # YADDRW x5, x6, x7 = 0000110_00111_00110_001_00101_0110011 = 0x0c7312b3
    .word 0x0c7312b3
    li x8, 0x42
    bne x5, x8, fail

# Test 6: YLENR on x0 → should be 0
    li x10, 6
    # YLENR x5, x0 = 0001000_00110_00000_000_00101_0110011 = 0x106002b3
    .word 0x106002b3
    bne x5, x0, fail

# Test 7: PACKY clears tag, preserves address
    li x10, 7
    li x6, 0x55
    # PACKY x5, x6, x0 = 0000100_00000_00110_011_00101_0110011 = 0x080332b3
    .word 0x080332b3
    li x8, 0x55
    bne x5, x8, fail

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
