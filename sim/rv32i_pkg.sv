// rv32i_pkg.sv — RV32I support package for Tourbillon simulation
// Hand-written SystemVerilog utility functions for decoding and executing
// the RV32I base integer instruction set.

/* verilator lint_off UNUSEDSIGNAL */  // decode functions use bit slices of instr
package rv32i_pkg;

  // -----------------------------------------------------------------------
  // Opcode constants
  // -----------------------------------------------------------------------
  localparam logic [6:0] OP_R_TYPE  = 7'b0110011;
  localparam logic [6:0] OP_I_ALU   = 7'b0010011;
  localparam logic [6:0] OP_LOAD    = 7'b0000011;
  localparam logic [6:0] OP_STORE   = 7'b0100011;
  localparam logic [6:0] OP_BRANCH  = 7'b1100011;
  localparam logic [6:0] OP_JAL     = 7'b1101111;
  localparam logic [6:0] OP_JALR    = 7'b1100111;
  localparam logic [6:0] OP_LUI     = 7'b0110111;
  localparam logic [6:0] OP_AUIPC   = 7'b0010111;

  // -----------------------------------------------------------------------
  // ALU operation encoding (4 bits)
  // -----------------------------------------------------------------------
  localparam logic [3:0] ALU_ADD  = 4'd0;
  localparam logic [3:0] ALU_SUB  = 4'd1;
  localparam logic [3:0] ALU_SLL  = 4'd2;
  localparam logic [3:0] ALU_SLT  = 4'd3;
  localparam logic [3:0] ALU_SLTU = 4'd4;
  localparam logic [3:0] ALU_XOR  = 4'd5;
  localparam logic [3:0] ALU_SRL  = 4'd6;
  localparam logic [3:0] ALU_SRA  = 4'd7;
  localparam logic [3:0] ALU_OR   = 4'd8;
  localparam logic [3:0] ALU_AND  = 4'd9;

  // -----------------------------------------------------------------------
  // Field extraction helpers
  // -----------------------------------------------------------------------

  function automatic [4:0] decode_rd(input [31:0] instr);
    return instr[11:7];
  endfunction

  function automatic [4:0] decode_rs1(input [31:0] instr);
    return instr[19:15];
  endfunction

  function automatic [4:0] decode_rs2(input [31:0] instr);
    return instr[24:20];
  endfunction

  function automatic [2:0] decode_funct3(input [31:0] instr);
    return instr[14:12];
  endfunction

  function automatic [6:0] decode_funct7(input [31:0] instr);
    return instr[31:25];
  endfunction

  // -----------------------------------------------------------------------
  // Immediate extraction and sign-extension
  // -----------------------------------------------------------------------

  function automatic [31:0] decode_imm(input [31:0] instr, input [6:0] opcode);
    logic [31:0] imm;
    case (opcode)
      // I-type: instr[31:20] sign-extended
      OP_I_ALU, OP_LOAD, OP_JALR: begin
        imm = {{20{instr[31]}}, instr[31:20]};
      end

      // S-type: {instr[31:25], instr[11:7]} sign-extended
      OP_STORE: begin
        imm = {{20{instr[31]}}, instr[31:25], instr[11:7]};
      end

      // B-type: {instr[31], instr[7], instr[30:25], instr[11:8], 1'b0} sign-extended
      OP_BRANCH: begin
        imm = {{19{instr[31]}}, instr[31], instr[7], instr[30:25], instr[11:8], 1'b0};
      end

      // U-type: {instr[31:12], 12'b0}
      OP_LUI, OP_AUIPC: begin
        imm = {instr[31:12], 12'b0};
      end

      // J-type: {instr[31], instr[19:12], instr[20], instr[30:21], 1'b0} sign-extended
      OP_JAL: begin
        imm = {{11{instr[31]}}, instr[31], instr[19:12], instr[20], instr[30:21], 1'b0};
      end

      // R-type and others: no immediate
      default: begin
        imm = 32'b0;
      end
    endcase
    return imm;
  endfunction

  // -----------------------------------------------------------------------
  // ALU operation decode from instruction fields
  // -----------------------------------------------------------------------

  function automatic [3:0] decode_alu_op(
    input [6:0] opcode,
    input [2:0] funct3,
    input [6:0] funct7
  );
    logic [3:0] op;
    case (opcode)
      OP_R_TYPE: begin
        case (funct3)
          3'b000: op = (funct7[5]) ? ALU_SUB : ALU_ADD;
          3'b001: op = ALU_SLL;
          3'b010: op = ALU_SLT;
          3'b011: op = ALU_SLTU;
          3'b100: op = ALU_XOR;
          3'b101: op = (funct7[5]) ? ALU_SRA : ALU_SRL;
          3'b110: op = ALU_OR;
          3'b111: op = ALU_AND;
          default: op = ALU_ADD;
        endcase
      end

      OP_I_ALU: begin
        case (funct3)
          3'b000: op = ALU_ADD;   // ADDI (no SUBI in RV32I)
          3'b001: op = ALU_SLL;   // SLLI
          3'b010: op = ALU_SLT;   // SLTI
          3'b011: op = ALU_SLTU;  // SLTIU
          3'b100: op = ALU_XOR;   // XORI
          3'b101: op = (funct7[5]) ? ALU_SRA : ALU_SRL;  // SRAI / SRLI
          3'b110: op = ALU_OR;    // ORI
          3'b111: op = ALU_AND;   // ANDI
          default: op = ALU_ADD;
        endcase
      end

      // LOAD, STORE, JALR, JAL, AUIPC, LUI all use ADD for address/value computation
      default: begin
        op = ALU_ADD;
      end
    endcase
    return op;
  endfunction

  // -----------------------------------------------------------------------
  // ALU
  // -----------------------------------------------------------------------

  function automatic [31:0] alu(input [3:0] alu_op, input [31:0] a, input [31:0] b);
    logic [31:0] result;
    case (alu_op)
      ALU_ADD:  result = a + b;
      ALU_SUB:  result = a - b;
      ALU_SLL:  result = a << b[4:0];
      ALU_SLT:  result = {31'b0, $signed(a) < $signed(b)};
      ALU_SLTU: result = {31'b0, a < b};
      ALU_XOR:  result = a ^ b;
      ALU_SRL:  result = a >> b[4:0];
      ALU_SRA:  result = $unsigned($signed(a) >>> b[4:0]);
      ALU_OR:   result = a | b;
      ALU_AND:  result = a & b;
      default:  result = 32'b0;
    endcase
    return result;
  endfunction

  // -----------------------------------------------------------------------
  // Branch helpers
  // -----------------------------------------------------------------------

  // Decode MemOp: 0=Load, 1=Store, 2=None (matches Tourbillon enum encoding)
  function automatic [1:0] decode_mem_op(input [6:0] opcode);
    if (opcode == OP_LOAD)
      return 2'd0;  // Load
    else if (opcode == OP_STORE)
      return 2'd1;  // Store
    else
      return 2'd2;  // None
  endfunction

  localparam logic [6:0] OP_FENCE  = 7'b0001111;
  localparam logic [6:0] OP_SYSTEM = 7'b1110011;

  // Decode needs_wb: true for ALU, LOAD, LUI, AUIPC, JAL, JALR
  // False for STORE, BRANCH, FENCE, SYSTEM, or when rd=x0
  function automatic decode_needs_wb(input [6:0] opcode, input [4:0] rd);
    return (rd != 5'd0) &&
           (opcode != OP_STORE) && (opcode != OP_BRANCH) &&
           (opcode != OP_FENCE) && (opcode != OP_SYSTEM);
  endfunction

  function automatic is_branch(input [6:0] opcode);
    return (opcode == OP_BRANCH) || (opcode == OP_JAL) || (opcode == OP_JALR);
  endfunction


  function automatic branch_taken(input [2:0] funct3, input [31:0] a, input [31:0] b);
    logic taken;
    case (funct3)
      3'b000: taken = (a == b);                          // BEQ
      3'b001: taken = (a != b);                          // BNE
      3'b100: taken = ($signed(a) < $signed(b));         // BLT
      3'b101: taken = ($signed(a) >= $signed(b));        // BGE
      3'b110: taken = (a < b);                           // BLTU
      3'b111: taken = (a >= b);                          // BGEU
      default: taken = 1'b0;
    endcase
    return taken;
  endfunction

  // -----------------------------------------------------------------------
  // Opcode-aware result computation (fixes LUI, AUIPC, JAL, JALR, R-type)
  // -----------------------------------------------------------------------

  function automatic [31:0] compute_result(
    input [3:0] alu_op, input [6:0] opcode,
    input [31:0] rs1, input [31:0] rs2, input [31:0] imm, input [31:0] pc
  );
    case (opcode)
      OP_LUI:    return imm;
      OP_AUIPC:  return pc + imm;
      OP_JAL:    return pc + 32'd4;     // return address
      OP_JALR:   return pc + 32'd4;     // return address
      OP_R_TYPE: return alu(alu_op, rs1, rs2);  // R-type uses rs2, not imm
      default:   return alu(alu_op, rs1, imm);  // I-type ALU, loads, etc.
    endcase
  endfunction

  // -----------------------------------------------------------------------
  // Branch/jump target computation
  // -----------------------------------------------------------------------

  function automatic [31:0] compute_branch_target(
    input [6:0] opcode, input [31:0] pc, input [31:0] rs1, input [31:0] imm
  );
    if (opcode == OP_JALR)
      return (rs1 + imm) & 32'hFFFFFFFE;  // JALR: (rs1+imm) with bit 0 cleared
    else
      return pc + imm;  // JAL, conditional branches: PC-relative
  endfunction

  // -----------------------------------------------------------------------
  // Unified taken check (branches + unconditional jumps)
  // -----------------------------------------------------------------------

  function automatic is_taken(
    input [6:0] opcode, input [2:0] funct3, input [31:0] a, input [31:0] b
  );
    if (opcode == OP_JAL || opcode == OP_JALR)
      return 1'b1;
    return branch_taken(funct3, a, b);
  endfunction

  // -----------------------------------------------------------------------
  // Compute next PC: branch target if taken, else pc+4
  // -----------------------------------------------------------------------

  function automatic [31:0] compute_next_pc(
    input [6:0] opcode, input [2:0] funct3,
    input [31:0] rs1_val, input [31:0] rs2_val,
    input [31:0] imm, input [31:0] pc
  );
    // Only branch/jump opcodes can redirect
    if (!is_branch(opcode))
      return pc + 32'd4;
    if (is_taken(opcode, funct3, rs1_val, rs2_val))
      return compute_branch_target(opcode, pc, rs1_val, imm);
    else
      return pc + 32'd4;
  endfunction

  // -----------------------------------------------------------------------
  // Sub-word load extension (LB, LH, LBU, LHU, LW)
  // -----------------------------------------------------------------------

  function automatic [31:0] load_extend(
    input [2:0] funct3, input [31:0] word, input [1:0] byte_offset
  );
    logic [7:0]  b;
    logic [15:0] h;
    case (byte_offset)
      2'b00: begin b = word[ 7: 0]; h = word[15: 0]; end
      2'b01: begin b = word[15: 8]; h = word[23: 8]; end
      2'b10: begin b = word[23:16]; h = word[31:16]; end
      2'b11: begin b = word[31:24]; h = {word[7:0], word[31:24]}; end // wrap
      default: begin b = 8'b0; h = 16'b0; end
    endcase
    case (funct3)
      3'b000: return {{24{b[7]}}, b};      // LB  — sign-extend byte
      3'b001: return {{16{h[15]}}, h};     // LH  — sign-extend halfword
      3'b010: return word;                  // LW  — full word
      3'b100: return {24'b0, b};            // LBU — zero-extend byte
      3'b101: return {16'b0, h};            // LHU — zero-extend halfword
      default: return word;
    endcase
  endfunction

  // -----------------------------------------------------------------------
  // Sub-word store merge — merge partial write into existing word
  // -----------------------------------------------------------------------

  function automatic [31:0] store_merge(
    input [2:0] funct3, input [31:0] old_word, input [31:0] data, input [1:0] byte_offset
  );
    logic [31:0] result;
    result = old_word;
    case (funct3)
      3'b000: begin  // SB — store byte
        case (byte_offset)
          2'b00: result[ 7: 0] = data[7:0];
          2'b01: result[15: 8] = data[7:0];
          2'b10: result[23:16] = data[7:0];
          2'b11: result[31:24] = data[7:0];
          default: ;
        endcase
      end
      3'b001: begin  // SH — store halfword
        case (byte_offset)
          2'b00: result[15: 0] = data[15:0];
          2'b10: result[31:16] = data[15:0];
          default: ;  // misaligned — ignore for now
        endcase
      end
      3'b010: begin  // SW — store word
        result = data;
      end
      default: result = old_word;
    endcase
    return result;
  endfunction

  // -----------------------------------------------------------------------
  // Decode opcode from raw instruction (for pipeline use)
  // -----------------------------------------------------------------------

  function automatic [6:0] decode_opcode(input [31:0] instr);
    return instr[6:0];
  endfunction

endpackage
