module Top (
    input  wire clk,
    input  wire rst_n
);

    // Queue: data
    logic        q_data_enq_valid;
    wire         q_data_enq_ready;
    logic [31:0] q_data_enq_data;
    wire         q_data_deq_valid;
    logic        q_data_deq_ready;
    wire  [31:0] q_data_deq_data;

    tbn_fifo #(.WIDTH(32), .DEPTH(2)) q_data_inst (
        .clk(clk),
        .rst_n(rst_n),
        .enq_valid(q_data_enq_valid),
        .enq_ready(q_data_enq_ready),
        .enq_data(q_data_enq_data),
        .deq_valid(q_data_deq_valid),
        .deq_ready(q_data_deq_ready),
        .deq_data(q_data_deq_data)
    );

    // Queue: hi
    logic        q_hi_enq_valid;
    wire         q_hi_enq_ready;
    logic [31:0] q_hi_enq_data;
    wire         q_hi_deq_valid;
    logic        q_hi_deq_ready;
    wire  [31:0] q_hi_deq_data;

    tbn_fifo #(.WIDTH(32), .DEPTH(2)) q_hi_inst (
        .clk(clk),
        .rst_n(rst_n),
        .enq_valid(q_hi_enq_valid),
        .enq_ready(q_hi_enq_ready),
        .enq_data(q_hi_enq_data),
        .deq_valid(q_hi_deq_valid),
        .deq_ready(q_hi_deq_ready),
        .deq_data(q_hi_deq_data)
    );

    // Queue: lo
    logic        q_lo_enq_valid;
    wire         q_lo_enq_ready;
    logic [31:0] q_lo_enq_data;
    wire         q_lo_deq_valid;
    logic        q_lo_deq_ready;
    wire  [31:0] q_lo_deq_data;

    tbn_fifo #(.WIDTH(32), .DEPTH(2)) q_lo_inst (
        .clk(clk),
        .rst_n(rst_n),
        .enq_valid(q_lo_enq_valid),
        .enq_ready(q_lo_enq_ready),
        .enq_data(q_lo_enq_data),
        .deq_valid(q_lo_deq_valid),
        .deq_ready(q_lo_deq_ready),
        .deq_data(q_lo_deq_data)
    );

    wire r_Source_go_can_fire = q_data_enq_ready;
    wire r_Source_go_will_fire = r_Source_go_can_fire;

    wire r_Router_go_can_fire = q_data_deq_valid & q_hi_enq_ready & q_lo_enq_ready;
    wire r_Router_go_will_fire = r_Router_go_can_fire;

    wire r_SinkHigh_go_can_fire = q_hi_deq_valid;
    wire r_SinkHigh_go_will_fire = r_SinkHigh_go_can_fire;

    wire r_SinkLow_go_can_fire = q_lo_deq_valid;
    wire r_SinkLow_go_will_fire = r_SinkLow_go_can_fire;

    always_comb begin
        q_data_enq_valid = 1'b0;
        q_data_enq_data = '0;
        q_hi_enq_valid = 1'b0;
        q_hi_enq_data = '0;
        q_lo_enq_valid = 1'b0;
        q_lo_enq_data = '0;

        // Rule: Source.go
        if (r_Source_go_will_fire) begin
            q_data_enq_data = 50;
            q_data_enq_valid = 1'b1;
        end
        // Rule: Router.go
        if (r_Router_go_will_fire) begin
            if ((q_data_deq_data > 100)) begin
                q_hi_enq_data = q_data_deq_data;
                q_hi_enq_valid = 1'b1;
            end else begin
                q_lo_enq_data = q_data_deq_data;
                q_lo_enq_valid = 1'b1;
            end
        end
        // Rule: SinkHigh.go
        if (r_SinkHigh_go_will_fire) begin
        end
        // Rule: SinkLow.go
        if (r_SinkLow_go_will_fire) begin
        end
    end

    assign q_data_deq_ready = r_Router_go_will_fire;
    assign q_hi_deq_ready = r_SinkHigh_go_will_fire;
    assign q_lo_deq_ready = r_SinkLow_go_will_fire;

endmodule
