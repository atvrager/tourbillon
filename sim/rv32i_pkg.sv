// rv32i_pkg.sv — RV32I support package for Tourbillon simulation
// Hand-written SystemVerilog utility functions for decoding and executing
// the RV32I base integer instruction set.

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

  // Decode needs_wb: true for all except STORE and BRANCH
  function automatic decode_needs_wb(input [6:0] opcode);
    return (opcode != OP_STORE) && (opcode != OP_BRANCH);
  endfunction

  function automatic is_branch(input [6:0] opcode);
    return (opcode == OP_BRANCH);
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

endpackage
