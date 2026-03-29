module Top (
    input  wire clk,
    input  wire rst_n
);

    // Cell: Counter.count
    logic [31:0] c_Counter_count_q;
    logic [31:0] c_Counter_count_d;
    logic c_Counter_count_en;

    wire r_Counter_tick_can_fire = 1'b1;
    wire r_Counter_tick_will_fire = r_Counter_tick_can_fire;

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
