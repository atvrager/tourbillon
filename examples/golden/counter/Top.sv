/* verilator lint_off UNUSEDSIGNAL */
/* verilator lint_off UNUSEDPARAM */
module Top (
    input  wire clk,
    input  wire rst_n
);

    // Tourbillon provenance: 3d45af0d2486b96ddee741146f65f451ea742a5d34828b3c3ca7b1db6fae66d8
    localparam logic [255:0] TBN_PROVENANCE = 256'h3d45af0d2486b96ddee741146f65f451ea742a5d34828b3c3ca7b1db6fae66d8;

    // Cell: Counter.count
    logic [31:0] c_Counter_count_q;
    logic [31:0] c_Counter_count_d;
    logic c_Counter_count_en;

    wire r_Counter_tick_can_fire = 1'b1;
    wire r_Counter_tick_will_fire = r_Counter_tick_can_fire;

    /* verilator lint_off LATCH */  // all signals have explicit defaults above
    always_comb begin
        c_Counter_count_d = c_Counter_count_q;
        c_Counter_count_en = 1'b0;

        // Rule: Counter.tick
        if (r_Counter_tick_will_fire) begin
            c_Counter_count_d = (c_Counter_count_q + 1);
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
