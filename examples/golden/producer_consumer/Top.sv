/* verilator lint_off UNUSEDSIGNAL */
/* verilator lint_off UNUSEDPARAM */
module Top (
    input  wire clk,
    input  wire rst_n
);

    // Tourbillon provenance: 5e300462b3e9bb974350af1a391161e9cf6c751e5ecd8ffc22726a2d77af11ba
    localparam logic [255:0] TBN_PROVENANCE = 256'h5e300462b3e9bb974350af1a391161e9cf6c751e5ecd8ffc22726a2d77af11ba;

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

    /* verilator lint_off LATCH */  // all signals have explicit defaults above
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
