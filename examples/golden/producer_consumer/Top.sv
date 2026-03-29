module Top (
    input  wire clk,
    input  wire rst_n
);

    // Queue: q
    logic        q_q_enq_valid;
    wire         q_q_enq_ready;
    logic [31:0] q_q_enq_data;
    wire         q_q_deq_valid;
    logic        q_q_deq_ready;
    wire  [31:0] q_q_deq_data;

    tbn_fifo #(.WIDTH(32), .DEPTH(4)) q_q_inst (
        .clk(clk),
        .rst_n(rst_n),
        .enq_valid(q_q_enq_valid),
        .enq_ready(q_q_enq_ready),
        .enq_data(q_q_enq_data),
        .deq_valid(q_q_deq_valid),
        .deq_ready(q_q_deq_ready),
        .deq_data(q_q_deq_data)
    );

    wire r_Producer_go_can_fire = q_q_enq_ready;
    wire r_Producer_go_will_fire = r_Producer_go_can_fire;

    wire r_Consumer_go_can_fire = q_q_deq_valid;
    wire r_Consumer_go_will_fire = r_Consumer_go_can_fire;

    always_comb begin
        q_q_enq_valid = 1'b0;
        q_q_enq_data = '0;

        // Rule: Producer.go
        if (r_Producer_go_will_fire) begin
            q_q_enq_data = 42;
            q_q_enq_valid = 1'b1;
        end
        // Rule: Consumer.go
        if (r_Consumer_go_will_fire) begin
        end
    end

    assign q_q_deq_ready = r_Consumer_go_will_fire;

endmodule
