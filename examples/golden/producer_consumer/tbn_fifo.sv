module tbn_fifo #(
    parameter WIDTH = 8,
    parameter DEPTH = 2,
    parameter INIT_COUNT = 0,
    parameter [WIDTH-1:0] INIT_VALUE = '0
)(
    input  wire              clk,
    input  wire              rst_n,
    input  wire              enq_valid,
    output wire              enq_ready,
    input  wire [WIDTH-1:0]  enq_data,
    output wire              deq_valid,
    input  wire              deq_ready,
    output wire [WIDTH-1:0]  deq_data
);
    localparam AWIDTH = (DEPTH < 2) ? 1 : $clog2(DEPTH);

    reg [WIDTH-1:0]  storage [0:DEPTH-1];
    reg [AWIDTH-1:0] rd_ptr;
    reg [AWIDTH-1:0] wr_ptr;
    reg [AWIDTH:0]   count;

    assign enq_ready = (count < DEPTH);
    assign deq_valid = (count != 0);
    assign deq_data  = storage[rd_ptr];

    wire do_enq = enq_valid & enq_ready;
    wire do_deq = deq_ready & deq_valid;

    always_ff @(posedge clk or negedge rst_n) begin
        if (!rst_n) begin
            rd_ptr <= '0;
            count  <= INIT_COUNT;
            wr_ptr <= INIT_COUNT[AWIDTH-1:0];
            for (integer _i = 0; _i < INIT_COUNT && _i < DEPTH; _i++)
                storage[_i] <= INIT_VALUE;
        end else begin
            if (do_enq) begin
                storage[wr_ptr] <= enq_data;
                if (wr_ptr == AWIDTH'(DEPTH - 1))
                    wr_ptr <= '0;
                else
                    wr_ptr <= wr_ptr + 1;
            end
            if (do_deq) begin
                if (rd_ptr == AWIDTH'(DEPTH - 1))
                    rd_ptr <= '0;
                else
                    rd_ptr <= rd_ptr + 1;
            end
            case ({do_enq, do_deq})
                2'b10:   count <= count + 1;
                2'b01:   count <= count - 1;
                default: ;
            endcase
        end
    end
endmodule
