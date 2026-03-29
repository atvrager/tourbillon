module CPU (
    input  wire clk,
    input  wire rst_n
);

    // Tourbillon provenance: f19ff434d174deab36dc820f0e92118afcaa65dc453da4f14a8938c213386098
    localparam logic [255:0] TBN_PROVENANCE = 256'hf19ff434d174deab36dc820f0e92118afcaa65dc453da4f14a8938c213386098;

    typedef struct packed {
        logic [3:0] op;
        logic [4:0] rd;
        logic [31:0] rs1_val;
        logic [31:0] rs2_val;
        logic [31:0] imm;
        logic [31:0] pc;
        logic [1:0] mem;
        logic wb;
        logic is_br;
        logic [2:0] funct3;
    } Decoded;

    typedef enum logic [1:0] {
        Load = 0,
        Store = 1,
        None = 2
    } MemOp;

    // Queue: fetch_q
    logic        q_fetch_q_enq_valid;
    wire         q_fetch_q_enq_ready;
    logic [63:0] q_fetch_q_enq_data;
    wire         q_fetch_q_deq_valid;
    logic        q_fetch_q_deq_ready;
    wire  [63:0] q_fetch_q_deq_data;

    tbn_fifo #(.WIDTH(64), .DEPTH(2)) q_fetch_q_inst (
        .clk(clk),
        .rst_n(rst_n),
        .enq_valid(q_fetch_q_enq_valid),
        .enq_ready(q_fetch_q_enq_ready),
        .enq_data(q_fetch_q_enq_data),
        .deq_valid(q_fetch_q_deq_valid),
        .deq_ready(q_fetch_q_deq_ready),
        .deq_data(q_fetch_q_deq_data)
    );

    // Queue: decode_q
    logic        q_decode_q_enq_valid;
    wire         q_decode_q_enq_ready;
    Decoded q_decode_q_enq_data;
    wire         q_decode_q_deq_valid;
    logic        q_decode_q_deq_ready;
    wire  [143:0] q_decode_q_deq_data_raw;
    Decoded q_decode_q_deq_data;
    assign q_decode_q_deq_data = q_decode_q_deq_data_raw;

    tbn_fifo #(.WIDTH(144), .DEPTH(2)) q_decode_q_inst (
        .clk(clk),
        .rst_n(rst_n),
        .enq_valid(q_decode_q_enq_valid),
        .enq_ready(q_decode_q_enq_ready),
        .enq_data(q_decode_q_enq_data),
        .deq_valid(q_decode_q_deq_valid),
        .deq_ready(q_decode_q_deq_ready),
        .deq_data(q_decode_q_deq_data_raw)
    );

    // Queue: redir_q
    logic        q_redir_q_enq_valid;
    wire         q_redir_q_enq_ready;
    logic [31:0] q_redir_q_enq_data;
    wire         q_redir_q_deq_valid;
    logic        q_redir_q_deq_ready;
    wire  [31:0] q_redir_q_deq_data;

    tbn_fifo #(.WIDTH(32), .DEPTH(1)) q_redir_q_inst (
        .clk(clk),
        .rst_n(rst_n),
        .enq_valid(q_redir_q_enq_valid),
        .enq_ready(q_redir_q_enq_ready),
        .enq_data(q_redir_q_enq_data),
        .deq_valid(q_redir_q_deq_valid),
        .deq_ready(q_redir_q_deq_ready),
        .deq_data(q_redir_q_deq_data)
    );

    // Queue: wb_q
    logic        q_wb_q_enq_valid;
    wire         q_wb_q_enq_ready;
    logic [36:0] q_wb_q_enq_data;
    wire         q_wb_q_deq_valid;
    logic        q_wb_q_deq_ready;
    wire  [36:0] q_wb_q_deq_data;

    tbn_fifo #(.WIDTH(37), .DEPTH(2)) q_wb_q_inst (
        .clk(clk),
        .rst_n(rst_n),
        .enq_valid(q_wb_q_enq_valid),
        .enq_ready(q_wb_q_enq_ready),
        .enq_data(q_wb_q_enq_data),
        .deq_valid(q_wb_q_deq_valid),
        .deq_ready(q_wb_q_deq_ready),
        .deq_data(q_wb_q_deq_data)
    );

    // Queue: imem_read_req
    logic        q_imem_read_req_enq_valid;
    wire         q_imem_read_req_enq_ready;
    logic [31:0] q_imem_read_req_enq_data;
    wire         q_imem_read_req_deq_valid;
    logic        q_imem_read_req_deq_ready;
    wire  [31:0] q_imem_read_req_deq_data;

    tbn_fifo #(.WIDTH(32), .DEPTH(1)) q_imem_read_req_inst (
        .clk(clk),
        .rst_n(rst_n),
        .enq_valid(q_imem_read_req_enq_valid),
        .enq_ready(q_imem_read_req_enq_ready),
        .enq_data(q_imem_read_req_enq_data),
        .deq_valid(q_imem_read_req_deq_valid),
        .deq_ready(q_imem_read_req_deq_ready),
        .deq_data(q_imem_read_req_deq_data)
    );

    // Queue: imem_read_resp
    logic        q_imem_read_resp_enq_valid;
    wire         q_imem_read_resp_enq_ready;
    logic [31:0] q_imem_read_resp_enq_data;
    wire         q_imem_read_resp_deq_valid;
    logic        q_imem_read_resp_deq_ready;
    wire  [31:0] q_imem_read_resp_deq_data;

    tbn_fifo #(.WIDTH(32), .DEPTH(1)) q_imem_read_resp_inst (
        .clk(clk),
        .rst_n(rst_n),
        .enq_valid(q_imem_read_resp_enq_valid),
        .enq_ready(q_imem_read_resp_enq_ready),
        .enq_data(q_imem_read_resp_enq_data),
        .deq_valid(q_imem_read_resp_deq_valid),
        .deq_ready(q_imem_read_resp_deq_ready),
        .deq_data(q_imem_read_resp_deq_data)
    );

    // Queue: imem_write_req
    logic        q_imem_write_req_enq_valid;
    wire         q_imem_write_req_enq_ready;
    logic [63:0] q_imem_write_req_enq_data;
    wire         q_imem_write_req_deq_valid;
    logic        q_imem_write_req_deq_ready;
    wire  [63:0] q_imem_write_req_deq_data;

    tbn_fifo #(.WIDTH(64), .DEPTH(1)) q_imem_write_req_inst (
        .clk(clk),
        .rst_n(rst_n),
        .enq_valid(q_imem_write_req_enq_valid),
        .enq_ready(q_imem_write_req_enq_ready),
        .enq_data(q_imem_write_req_enq_data),
        .deq_valid(q_imem_write_req_deq_valid),
        .deq_ready(q_imem_write_req_deq_ready),
        .deq_data(q_imem_write_req_deq_data)
    );

    // Queue: dmem_read_req
    logic        q_dmem_read_req_enq_valid;
    wire         q_dmem_read_req_enq_ready;
    logic [31:0] q_dmem_read_req_enq_data;
    wire         q_dmem_read_req_deq_valid;
    logic        q_dmem_read_req_deq_ready;
    wire  [31:0] q_dmem_read_req_deq_data;

    tbn_fifo #(.WIDTH(32), .DEPTH(1)) q_dmem_read_req_inst (
        .clk(clk),
        .rst_n(rst_n),
        .enq_valid(q_dmem_read_req_enq_valid),
        .enq_ready(q_dmem_read_req_enq_ready),
        .enq_data(q_dmem_read_req_enq_data),
        .deq_valid(q_dmem_read_req_deq_valid),
        .deq_ready(q_dmem_read_req_deq_ready),
        .deq_data(q_dmem_read_req_deq_data)
    );

    // Queue: dmem_read_resp
    logic        q_dmem_read_resp_enq_valid;
    wire         q_dmem_read_resp_enq_ready;
    logic [31:0] q_dmem_read_resp_enq_data;
    wire         q_dmem_read_resp_deq_valid;
    logic        q_dmem_read_resp_deq_ready;
    wire  [31:0] q_dmem_read_resp_deq_data;

    tbn_fifo #(.WIDTH(32), .DEPTH(1)) q_dmem_read_resp_inst (
        .clk(clk),
        .rst_n(rst_n),
        .enq_valid(q_dmem_read_resp_enq_valid),
        .enq_ready(q_dmem_read_resp_enq_ready),
        .enq_data(q_dmem_read_resp_enq_data),
        .deq_valid(q_dmem_read_resp_deq_valid),
        .deq_ready(q_dmem_read_resp_deq_ready),
        .deq_data(q_dmem_read_resp_deq_data)
    );

    // Queue: dmem_write_req
    logic        q_dmem_write_req_enq_valid;
    wire         q_dmem_write_req_enq_ready;
    logic [63:0] q_dmem_write_req_enq_data;
    wire         q_dmem_write_req_deq_valid;
    logic        q_dmem_write_req_deq_ready;
    wire  [63:0] q_dmem_write_req_deq_data;

    tbn_fifo #(.WIDTH(64), .DEPTH(1)) q_dmem_write_req_inst (
        .clk(clk),
        .rst_n(rst_n),
        .enq_valid(q_dmem_write_req_enq_valid),
        .enq_ready(q_dmem_write_req_enq_ready),
        .enq_data(q_dmem_write_req_enq_data),
        .deq_valid(q_dmem_write_req_deq_valid),
        .deq_ready(q_dmem_write_req_deq_ready),
        .deq_data(q_dmem_write_req_deq_data)
    );

    // Cell: Fetch.pc
    logic [31:0] c_Fetch_pc_q;
    logic [31:0] c_Fetch_pc_d;
    logic c_Fetch_pc_en;

    // Cell: Writeback.regfile
    logic [1023:0] c_Writeback_regfile_q;
    logic [1023:0] c_Writeback_regfile_d;
    logic c_Writeback_regfile_en;

    wire r_Fetch_tick_can_fire = q_imem_read_resp_deq_valid & q_fetch_q_enq_ready & q_imem_read_req_enq_ready;
    wire r_Fetch_tick_will_fire = r_Fetch_tick_can_fire;

    wire r_Decode_crack_can_fire = q_fetch_q_deq_valid & q_decode_q_enq_ready;
    wire r_Decode_crack_will_fire = r_Decode_crack_can_fire;

    wire r_Execute_go_can_fire = q_dmem_read_resp_deq_valid & q_decode_q_deq_valid & q_dmem_read_req_enq_ready & q_dmem_write_req_enq_ready & q_redir_q_enq_ready & q_wb_q_enq_ready;
    wire r_Execute_go_will_fire = r_Execute_go_can_fire;

    wire r_Writeback_commit_can_fire = q_wb_q_deq_valid;
    wire r_Writeback_commit_will_fire = r_Writeback_commit_can_fire;

    wire r__Mem_imem_read_can_fire = q_imem_read_req_deq_valid & q_imem_read_resp_enq_ready;
    wire r__Mem_imem_read_will_fire = r__Mem_imem_read_can_fire;
    wire r__Mem_imem_write_can_fire = q_imem_write_req_deq_valid;
    wire r__Mem_imem_write_will_fire = r__Mem_imem_write_can_fire;

    wire r__Mem_dmem_read_can_fire = q_dmem_read_req_deq_valid & q_dmem_read_resp_enq_ready;
    wire r__Mem_dmem_read_will_fire = r__Mem_dmem_read_can_fire;
    wire r__Mem_dmem_write_can_fire = q_dmem_write_req_deq_valid;
    wire r__Mem_dmem_write_will_fire = r__Mem_dmem_write_can_fire;

    always_comb begin
        c_Fetch_pc_d = c_Fetch_pc_q;
        c_Fetch_pc_en = 1'b0;
        c_Writeback_regfile_d = c_Writeback_regfile_q;
        c_Writeback_regfile_en = 1'b0;
        q_fetch_q_enq_valid = 1'b0;
        q_fetch_q_enq_data = '0;
        q_decode_q_enq_valid = 1'b0;
        q_decode_q_enq_data = '0;
        q_redir_q_enq_valid = 1'b0;
        q_redir_q_enq_data = '0;
        q_wb_q_enq_valid = 1'b0;
        q_wb_q_enq_data = '0;
        q_imem_read_req_enq_valid = 1'b0;
        q_imem_read_req_enq_data = '0;
        q_imem_read_resp_enq_valid = 1'b0;
        q_imem_read_resp_enq_data = '0;
        q_imem_write_req_enq_valid = 1'b0;
        q_imem_write_req_enq_data = '0;
        q_dmem_read_req_enq_valid = 1'b0;
        q_dmem_read_req_enq_data = '0;
        q_dmem_read_resp_enq_valid = 1'b0;
        q_dmem_read_resp_enq_data = '0;
        q_dmem_write_req_enq_valid = 1'b0;
        q_dmem_write_req_enq_data = '0;

        // Rule: Fetch.tick
        if (r_Fetch_tick_will_fire) begin
            q_imem_read_req_enq_data = c_Fetch_pc_q;
            q_imem_read_req_enq_valid = 1'b1;
            q_fetch_q_enq_data = {c_Fetch_pc_q, q_imem_read_resp_deq_data};
            q_fetch_q_enq_valid = 1'b1;
            if ({q_redir_q_deq_valid, q_redir_q_deq_data}[32]) begin
                c_Fetch_pc_d = {q_redir_q_deq_valid, q_redir_q_deq_data}[31:0];
                c_Fetch_pc_en = 1'b1;
            end else if (!{q_redir_q_deq_valid, q_redir_q_deq_data}[32]) begin
                c_Fetch_pc_d = (c_Fetch_pc_q + 4);
                c_Fetch_pc_en = 1'b1;
            end
        end
        // Rule: Decode.crack
        if (r_Decode_crack_will_fire) begin
            if (1'b1) begin
                q_decode_q_enq_data = Decoded'{op: decode_alu_op((q_fetch_q_deq_data[31:0] & 127), decode_funct3(q_fetch_q_deq_data[31:0]), decode_funct7(q_fetch_q_deq_data[31:0])), rd: decode_rd(q_fetch_q_deq_data[31:0]), rs1_val: c_Writeback_regfile_q[decode_rs1(q_fetch_q_deq_data[31:0])], rs2_val: c_Writeback_regfile_q[decode_rs2(q_fetch_q_deq_data[31:0])], imm: decode_imm(q_fetch_q_deq_data[31:0], (q_fetch_q_deq_data[31:0] & 127)), pc: q_fetch_q_deq_data[63:32], mem: decode_mem_op((q_fetch_q_deq_data[31:0] & 127)), wb: decode_needs_wb((q_fetch_q_deq_data[31:0] & 127)), is_br: is_branch((q_fetch_q_deq_data[31:0] & 127)), funct3: decode_funct3(q_fetch_q_deq_data[31:0])};
                q_decode_q_enq_valid = 1'b1;
            end else if (1'b0) begin
            end
        end
        // Rule: Execute.go
        if (r_Execute_go_will_fire) begin
            if (q_decode_q_deq_data.mem == Load) begin
                q_dmem_read_req_enq_data = alu(q_decode_q_deq_data.op, q_decode_q_deq_data.rs1_val, q_decode_q_deq_data.imm);
                q_dmem_read_req_enq_valid = 1'b1;
                if (q_decode_q_deq_data.wb) begin
                    q_wb_q_enq_data = {q_decode_q_deq_data.rd, q_dmem_read_resp_deq_data};
                    q_wb_q_enq_valid = 1'b1;
                end
            end else if (q_decode_q_deq_data.mem == Store) begin
                q_dmem_write_req_enq_data = {alu(q_decode_q_deq_data.op, q_decode_q_deq_data.rs1_val, q_decode_q_deq_data.imm), q_decode_q_deq_data.rs2_val};
                q_dmem_write_req_enq_valid = 1'b1;
            end else if (q_decode_q_deq_data.mem == None) begin
                if (q_decode_q_deq_data.wb) begin
                    q_wb_q_enq_data = {q_decode_q_deq_data.rd, alu(q_decode_q_deq_data.op, q_decode_q_deq_data.rs1_val, q_decode_q_deq_data.imm)};
                    q_wb_q_enq_valid = 1'b1;
                end
            end
            if ((q_decode_q_deq_data.is_br && branch_taken(q_decode_q_deq_data.funct3, q_decode_q_deq_data.rs1_val, q_decode_q_deq_data.rs2_val))) begin
                q_redir_q_enq_data = alu(q_decode_q_deq_data.op, q_decode_q_deq_data.rs1_val, q_decode_q_deq_data.imm);
                q_redir_q_enq_valid = 1'b1;
            end
        end
        // Rule: Writeback.commit
        if (r_Writeback_commit_will_fire) begin
            logic [1023:0] _upd_0;
            _upd_0 = c_Writeback_regfile_q;
            _upd_0[q_wb_q_deq_data[36:32] * 32 +: 32] = q_wb_q_deq_data[31:0];
            c_Writeback_regfile_d = _upd_0;
            c_Writeback_regfile_en = 1'b1;
        end
        // Rule: _Mem_imem.read
        if (r__Mem_imem_read_will_fire) begin
            q_imem_read_resp_enq_data = 0;
            q_imem_read_resp_enq_valid = 1'b1;
        end
        // Rule: _Mem_imem.write
        if (r__Mem_imem_write_will_fire) begin
        end
        // Rule: _Mem_dmem.read
        if (r__Mem_dmem_read_will_fire) begin
            q_dmem_read_resp_enq_data = 0;
            q_dmem_read_resp_enq_valid = 1'b1;
        end
        // Rule: _Mem_dmem.write
        if (r__Mem_dmem_write_will_fire) begin
        end
    end

    assign q_fetch_q_deq_ready = r_Decode_crack_will_fire;
    assign q_decode_q_deq_ready = r_Execute_go_will_fire;
    assign q_redir_q_deq_ready = (r_Fetch_tick_will_fire & q_redir_q_deq_valid);
    assign q_wb_q_deq_ready = r_Writeback_commit_will_fire;
    assign q_imem_read_req_deq_ready = r__Mem_imem_read_will_fire;
    assign q_imem_read_resp_deq_ready = r_Fetch_tick_will_fire;
    assign q_imem_write_req_deq_ready = r__Mem_imem_write_will_fire;
    assign q_dmem_read_req_deq_ready = r__Mem_dmem_read_will_fire;
    assign q_dmem_read_resp_deq_ready = r_Execute_go_will_fire;
    assign q_dmem_write_req_deq_ready = r__Mem_dmem_write_will_fire;

    always_ff @(posedge clk or negedge rst_n) begin
        if (!rst_n)
            c_Fetch_pc_q <= 32'd2147483648;
        else if (c_Fetch_pc_en)
            c_Fetch_pc_q <= c_Fetch_pc_d;
    end

    always_ff @(posedge clk or negedge rst_n) begin
        if (!rst_n)
            c_Writeback_regfile_q <= 1024'd0;
        else if (c_Writeback_regfile_en)
            c_Writeback_regfile_q <= c_Writeback_regfile_d;
    end

endmodule
