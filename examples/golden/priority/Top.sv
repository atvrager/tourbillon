module Top (
    input  wire clk,
    input  wire rst_n
);

    // Tourbillon provenance: 3b15b1e1eb39bcffcc40e46f046cc1d5de78c2f730a09e4e2d1248871cd9f64c
    localparam logic [255:0] TBN_PROVENANCE = 256'h3b15b1e1eb39bcffcc40e46f046cc1d5de78c2f730a09e4e2d1248871cd9f64c;

    // Cell: Counter.count
    logic [31:0] c_Counter_count_q;
    logic [31:0] c_Counter_count_d;
    logic c_Counter_count_en;

    wire r_Counter_inc_can_fire = 1'b1;
    wire r_Counter_inc_will_fire = r_Counter_inc_can_fire;
    wire r_Counter_dec_can_fire = 1'b1;
    wire r_Counter_dec_will_fire = r_Counter_dec_can_fire & ~(r_Counter_inc_will_fire);

    /* verilator lint_off LATCH */  // all signals have explicit defaults above
    always_comb begin
        c_Counter_count_d = c_Counter_count_q;
        c_Counter_count_en = 1'b0;

        // Rule: Counter.inc
        if (r_Counter_inc_will_fire) begin
            c_Counter_count_d = (c_Counter_count_q + 1);
            c_Counter_count_en = 1'b1;
        end
        // Rule: Counter.dec
        if (r_Counter_dec_will_fire) begin
            c_Counter_count_d = (c_Counter_count_q - 1);
            c_Counter_count_en = 1'b1;
        end
    end

    always_ff @(posedge clk or negedge rst_n) begin
        if (!rst_n)
            c_Counter_count_q <= 32'd0;
        else if (c_Counter_count_en)
            c_Counter_count_q <= c_Counter_count_d;
    end

endmodule
