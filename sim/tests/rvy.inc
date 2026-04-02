# rvy.inc — RV32IY CHERI instruction macros for hand-assembled tests
#
# GNU as macros for CHERI Y-extension instructions that lack toolchain
# support. Eliminates raw .word directives in test assembly.
#
# R-type encoding: funct7[31:25] | rs2[24:20] | rs1[19:15] | funct3[14:12] | rd[11:7] | 0110011

# ---------------------------------------------------------------------------
# Capability manipulation (R-type, opcode = 0110011)
# ---------------------------------------------------------------------------

# YADD rd, rs1, rs2 — increment address by rs2, preserving tag
.macro YADD rd, rs1, rs2
    .word (0b0000110 << 25) | (\rs2 << 20) | (\rs1 << 15) | (0b000 << 12) | (\rd << 7) | 0x33
.endm

# YADDRW rd, rs1, rs2 — set address to rs2 value, preserving tag+metadata
.macro YADDRW rd, rs1, rs2
    .word (0b0000110 << 25) | (\rs2 << 20) | (\rs1 << 15) | (0b001 << 12) | (\rd << 7) | 0x33
.endm

# YPERMC rd, rs1, rs2 — restrict permissions (AND mask)
.macro YPERMC rd, rs1, rs2
    .word (0b0000110 << 25) | (\rs2 << 20) | (\rs1 << 15) | (0b010 << 12) | (\rd << 7) | 0x33
.endm

# YBNDSW rd, rs1, rs2 — set bounds [addr, addr+rs2)
.macro YBNDSW rd, rs1, rs2
    .word (0b0000111 << 25) | (\rs2 << 20) | (\rs1 << 15) | (0b000 << 12) | (\rd << 7) | 0x33
.endm

# YSEAL rd, rs1, rs2 — seal rs1 with otype from rs2
.macro YSEAL rd, rs1, rs2
    .word (0b0001001 << 25) | (\rs2 << 20) | (\rs1 << 15) | (0b000 << 12) | (\rd << 7) | 0x33
.endm

# YUNSEAL rd, rs1, rs2 — unseal rs1 if otype matches rs2
.macro YUNSEAL rd, rs1, rs2
    .word (0b0001010 << 25) | (\rs2 << 20) | (\rs1 << 15) | (0b000 << 12) | (\rd << 7) | 0x33
.endm

# PACKY rd, rs1, rs2 — clear tag (int from cap)
.macro PACKY rd, rs1, rs2
    .word (0b0000100 << 25) | (\rs2 << 20) | (\rs1 << 15) | (0b011 << 12) | (\rd << 7) | 0x33
.endm

# ---------------------------------------------------------------------------
# Capability inspection (unary, funct7=0001000, sub-op in rs2 field)
# ---------------------------------------------------------------------------

# YTAGR rd, rs1 — read tag bit
.macro YTAGR rd, rs1
    .word (0b0001000 << 25) | (0 << 20) | (\rs1 << 15) | (0b000 << 12) | (\rd << 7) | 0x33
.endm

# YPERMR rd, rs1 — read permissions
.macro YPERMR rd, rs1
    .word (0b0001000 << 25) | (1 << 20) | (\rs1 << 15) | (0b000 << 12) | (\rd << 7) | 0x33
.endm

# YTYPER rd, rs1 — read otype
.macro YTYPER rd, rs1
    .word (0b0001000 << 25) | (2 << 20) | (\rs1 << 15) | (0b000 << 12) | (\rd << 7) | 0x33
.endm

# YTOPR rd, rs1 — read top
.macro YTOPR rd, rs1
    .word (0b0001000 << 25) | (4 << 20) | (\rs1 << 15) | (0b000 << 12) | (\rd << 7) | 0x33
.endm

# YBASER rd, rs1 — read base
.macro YBASER rd, rs1
    .word (0b0001000 << 25) | (5 << 20) | (\rs1 << 15) | (0b000 << 12) | (\rd << 7) | 0x33
.endm

# YLENR rd, rs1 — read length
.macro YLENR rd, rs1
    .word (0b0001000 << 25) | (6 << 20) | (\rs1 << 15) | (0b000 << 12) | (\rd << 7) | 0x33
.endm

# YAMASK rd, rs1 — read alignment mask
.macro YAMASK rd, rs1
    .word (0b0001000 << 25) | (7 << 20) | (\rs1 << 15) | (0b000 << 12) | (\rd << 7) | 0x33
.endm

# ---------------------------------------------------------------------------
# Capability loads/stores (funct3 = 011, standard LOAD/STORE opcodes)
# ---------------------------------------------------------------------------

# LC rd, offset(rs1) — load capability (65-bit, 2 beats)
.macro LC rd, offset, rs1
    .word ((\offset & 0xFFF) << 20) | (\rs1 << 15) | (0b011 << 12) | (\rd << 7) | 0x03
.endm

# SC rs2, offset(rs1) — store capability (65-bit, 2 beats)
.macro SC rs2, offset, rs1
    .word (((\offset >> 5) & 0x7F) << 25) | (\rs2 << 20) | (\rs1 << 15) | (0b011 << 12) | ((\offset & 0x1F) << 7) | 0x23
.endm

# ---------------------------------------------------------------------------
# CSR instructions (SYSTEM opcode = 1110011)
# ---------------------------------------------------------------------------

# CSRRW rd, csr, rs1 — read/write CSR (full 65-bit capability swap)
.macro CSRRW rd, csr, rs1
    .word ((\csr & 0xFFF) << 20) | (\rs1 << 15) | (0b001 << 12) | (\rd << 7) | 0x73
.endm

# Convenience: CSRW csr, rs1 (write-only, discard old value)
.macro CSRW csr, rs1
    CSRRW 0, \csr, \rs1
.endm

# Convenience: CSRR rd, csr (read, destructively writes 0)
.macro CSRR rd, csr
    CSRRW \rd, \csr, 0
.endm

# MRET — return from machine trap
.macro MRET
    .word 0x30200073
.endm

# ---------------------------------------------------------------------------
# CSR address constants
# ---------------------------------------------------------------------------
.equ CSR_MTVEC,  0x305
.equ CSR_MEPC,   0x341
.equ CSR_MCAUSE, 0x342
.equ CSR_MTVAL,  0x343

# ---------------------------------------------------------------------------
# Standard trap handler: skip faulting instruction, count traps in x27
# Place at 0x80000004 (after j over it at _start).
# Uses x27 (trap count), x28 (scratch), x29 (scratch).
# ---------------------------------------------------------------------------
.macro TRAP_HANDLER_SKIP
trap_handler:
    addi x27, x27, 1
    CSRR 28, CSR_MEPC              # x28 = faulting PC cap
    li x29, 4
    YADD 28, 28, 29                # x28 += 4 (preserves tag)
    CSRW CSR_MEPC, 28              # mepc = PC+4
    MRET
.endm

# Safety-net handler: unexpected trap = fail (write 0xFE to tohost)
.macro TRAP_HANDLER_FAIL
fail_trap:
    lui x5, 0x80001
    li x6, 0xFE
    sw x6, 0(x5)
    j .
.endm

# Install trap handler at addr (integer pointer OK for mtvec)
.macro INSTALL_TRAP addr
    lui x5, %hi(\addr)
    addi x5, x5, %lo(\addr)
    CSRW CSR_MTVEC, 5
.endm
