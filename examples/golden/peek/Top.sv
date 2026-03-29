module Top (
    input  wire clk,
    input  wire rst_n
);

    // Tourbillon provenance: 8c1acc702ca5f598572d8725c54efbc54b7d1ee0b55ae4339cd653ebf13c2aca
    localparam logic [255:0] TBN_PROVENANCE = 256'h8c1acc702ca5f598572d8725c54efbc54b7d1ee0b55ae4339cd653ebf13c2aca;

    // Cell: Writer.reg
    logic [31:0] c_Writer_reg_q;
    logic [31:0] c_Writer_reg_d;
    logic c_Writer_reg_en;

    wire r_Writer_go_can_fire = 1'b1;
    wire r_Writer_go_will_fire = r_Writer_go_can_fire;

    wire r_Reader_go_can_fire = 1'b1;
    wire r_Reader_go_will_fire = r_Reader_go_can_fire;

    /* verilator lint_off LATCH */  // all signals have explicit defaults above
    always_comb begin
        c_Writer_reg_d = c_Writer_reg_q;
        c_Writer_reg_en = 1'b0;

        // Rule: Writer.go
        if (r_Writer_go_will_fire) begin
            c_Writer_reg_d = (c_Writer_reg_q + 1);
            c_Writer_reg_en = 1'b1;
        end
        // Rule: Reader.go
        if (r_Reader_go_will_fire) begin
        end
    end

    always_ff @(posedge clk or negedge rst_n) begin
        if (!rst_n)
            c_Writer_reg_q <= 32'd0;
        else if (c_Writer_reg_en)
            c_Writer_reg_q <= c_Writer_reg_d;
    end

endmodule
