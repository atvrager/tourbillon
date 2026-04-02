// cheri_pkg.sv — CHERI capability support package for Tourbillon RV32IY
//
// Pure SystemVerilog functions (synthesisable, no DPI-C needed).
// Implements CHERI Concentrate compressed capabilities for RV32 (CLEN=64+tag).
//
// Capability format (65 bits total):
//   [64]    Tag — 1=valid capability, 0=integer/invalid
//   [63:32] Metadata — permissions, bounds (compressed), otype
//   [31:0]  Address — 32-bit virtual address
//
// Metadata layout [63:32]:
//   [63:58] SDP   — software-defined permissions (6 bits)
//   [57:55] AP_M  — architectural perms + mode (3 bits: R, W, X)
//   [54]    GL    — global flag
//   [53:50] CT    — capability type / otype (4 bits, 0=unsealed)
//   [49]    EF    — exact format (1=E is 0, byte-granularity)
//   [48:39] TE    — top mantissa encoding (10 bits)
//   [38:32] BE    — base mantissa encoding high bits (7 bits)
//
// CHERI Concentrate parameters:
//   MW=10 (mantissa width), EW=5 (exponent width), CAP_MAX_E=24

/* verilator lint_off UNUSEDSIGNAL */
/* verilator lint_off UNUSEDPARAM */
package cheri_pkg;

  // -----------------------------------------------------------------------
  // Constants
  // -----------------------------------------------------------------------
  localparam MW = 10;
  localparam EW = 5;
  localparam CAP_MAX_E = 24;

  // Permission bits within AP_M [57:55]
  localparam PERM_R = 3'b001;
  localparam PERM_W = 3'b010;
  localparam PERM_X = 3'b100;

  // Root capability metadata: all permissions, full address space [0, 2^32)
  // SDP=0x3F, AP_M=0x7 (RWX), GL=1, CT=0 (unsealed), EF=0 (inexact)
  // E=24: T_stored={top>>3=32, E[4:3]=2'b11}=10'b0100000011, B_hi=7'b0
  localparam logic [31:0] ROOT_META = {
      6'b111111,        // [63:58] SDP — all software perms
      3'b111,           // [57:55] AP_M — RWX
      1'b1,             // [54]    GL — global
      4'b0000,          // [53:50] CT — unsealed
      1'b0,             // [49]    EF — inexact (E=24 for full range)
      10'b0100000011,   // [48:39] TE — top mantissa + E[4:3]
      7'b0000000        // [38:32] BE — base mantissa high
  };

  // Null capability (65 bits, all zeros including tag)
  localparam logic [64:0] CAP_NULL = 65'h0;

  // -----------------------------------------------------------------------
  // Field extraction
  // -----------------------------------------------------------------------

  function automatic [31:0] cap_get_addr(input [64:0] cap);
    return cap[31:0];
  endfunction

  function automatic cap_get_tag(input [64:0] cap);
    return cap[64];
  endfunction

  function automatic [31:0] cap_get_meta(input [64:0] cap);
    return cap[63:32];
  endfunction

  function automatic [5:0] cap_get_sdp(input [64:0] cap);
    return cap[63:58];
  endfunction

  function automatic [2:0] cap_get_perms(input [64:0] cap);
    return cap[57:55];
  endfunction

  function automatic cap_get_global(input [64:0] cap);
    return cap[54];
  endfunction

  function automatic [3:0] cap_get_otype(input [64:0] cap);
    return cap[53:50];
  endfunction

  function automatic cap_get_ef(input [64:0] cap);
    return cap[49];
  endfunction

  function automatic [9:0] cap_get_te(input [64:0] cap);
    return cap[48:39];
  endfunction

  function automatic [6:0] cap_get_be(input [64:0] cap);
    return cap[38:32];
  endfunction

  // -----------------------------------------------------------------------
  // Bounds decode (CHERI Concentrate)
  // -----------------------------------------------------------------------

  // Decode the exponent E from stored fields
  function automatic [4:0] cap_decode_e(input [64:0] cap);
    if (cap[49])  // EF=1 → exact, E=0
      return 5'd0;
    else          // EF=0 → E encoded in low bits of TE and BE
      return {cap[40:39], cap[34:32]};  // {TE[1:0], addr-relative BE[2:0]}
  endfunction

  // Decode base address from compressed bounds
  function automatic [31:0] cap_get_base(input [64:0] cap);
    logic [4:0]  e;
    logic [9:0]  b_mantissa;
    logic [31:0] base;

    e = cap_decode_e(cap);

    if (cap[49]) begin  // EF=1, exact
      b_mantissa = {cap[38:32], 3'b000};
    end else begin
      b_mantissa = {cap[38:32], 3'b000};  // low bits used for E encoding
    end

    // Base = B_mantissa << E
    base = {22'b0, b_mantissa} << e;
    return base;
  endfunction

  // Decode top (may be 33 bits for wrap-around)
  function automatic [32:0] cap_get_top(input [64:0] cap);
    logic [4:0]  e;
    logic [9:0]  t_mantissa;
    logic [32:0] top;

    e = cap_decode_e(cap);

    if (cap[49]) begin  // EF=1, exact
      t_mantissa = cap[48:39];
    end else begin
      t_mantissa = {cap[48:41], 2'b00};  // TE[9:2], low bits used for E
    end

    // Top = T_mantissa << E
    top = {23'b0, t_mantissa} << e;
    return top;
  endfunction

  function automatic [31:0] cap_get_length(input [64:0] cap);
    logic [32:0] top;
    logic [31:0] base;
    top = cap_get_top(cap);
    base = cap_get_base(cap);
    return top[31:0] - base;
  endfunction

  // -----------------------------------------------------------------------
  // Capability construction / derivation
  // -----------------------------------------------------------------------

  // Set the address field, preserving tag and metadata
  function automatic [64:0] cap_set_addr(input [64:0] cap, input [31:0] addr);
    return {cap[64:32], addr};
  endfunction

  // Increment address by offset, preserving tag and metadata
  function automatic [64:0] cap_inc_offset(input [64:0] cap, input [31:0] offset);
    return {cap[64:32], cap[31:0] + offset};
  endfunction

  // Create a null capability with the given integer as address (tag=0)
  function automatic [64:0] cap_from_int(input [31:0] val);
    return {1'b0, 32'h0, val};
  endfunction

  // Root capability at a given address (full permissions, full bounds)
  function automatic [64:0] cap_root_at(input [31:0] addr);
    return {1'b1, ROOT_META, addr};
  endfunction

  // Root capability at address 0
  function automatic [64:0] cap_root();
    return cap_root_at(32'h0);
  endfunction

  // Clear the tag bit (intentionally invalidate)
  function automatic [64:0] cap_clear_tag(input [64:0] cap);
    return {1'b0, cap[63:0]};
  endfunction

  // -----------------------------------------------------------------------
  // Permission operations
  // -----------------------------------------------------------------------

  // Restrict permissions (monotonic: can only remove, never add)
  function automatic [64:0] cap_and_perm(input [64:0] cap, input [2:0] mask);
    logic [64:0] result;
    result = cap;
    result[57:55] = cap[57:55] & mask;
    return result;
  endfunction

  // -----------------------------------------------------------------------
  // Bounds setting (CHERI Concentrate encoder)
  // -----------------------------------------------------------------------

  // Set bounds: narrow capability to [addr, addr+length)
  // Returns new capability with encoded bounds. May lose precision for large lengths.
  // Phase 2: full CHERI Concentrate encoder. Simplified for correctness over optimality.
  function automatic [64:0] cap_set_bounds(input [64:0] cap, input [31:0] length);
    logic [31:0] addr;
    logic [4:0]  e;
    logic [9:0]  t_mantissa;
    logic [6:0]  b_hi;
    logic        ef;
    logic [31:0] top_addr;
    logic [64:0] result;
    logic [31:0] shifted_addr;
    logic [31:0] shifted_top;

    addr = cap[31:0];
    top_addr = addr + length;

    // Find minimum exponent E such that both base and top fit in MW bits
    e = 5'd0;
    for (int i = 0; i < CAP_MAX_E; i++) begin
      if ((length >> (i + MW)) != 0)
        e = 5'(i + 1);
    end

    if (e == 0) begin
      // Exact: EF=1
      ef = 1'b1;
      b_hi = addr[9:3];
      t_mantissa = top_addr[9:0];
    end else begin
      // Inexact: EF=0, encode E in low bits
      ef = 1'b0;
      shifted_addr = addr >> e;
      shifted_top = top_addr >> e;
      b_hi = shifted_addr[9:3];
      t_mantissa = {shifted_top[9:2], e[4:3]};
    end

    result = cap;
    result[49] = ef;
    result[48:39] = t_mantissa;
    result[38:32] = b_hi;
    // Low 3 bits of BE encode E[2:0] when EF=0
    if (!ef)
      result[34:32] = e[2:0];
    return result;
  endfunction

  // -----------------------------------------------------------------------
  // Sealing / unsealing
  // -----------------------------------------------------------------------

  function automatic [64:0] cap_seal(input [64:0] cap, input [64:0] sealer);
    logic [64:0] result;
    result = cap;
    result[53:50] = sealer[3:0];  // otype from sealer's low 4 address bits
    return result;
  endfunction

  function automatic [64:0] cap_unseal(input [64:0] cap, input [64:0] unsealer);
    logic [64:0] result;
    result = cap;
    // Verify sealer matches otype
    if (cap[53:50] == unsealer[3:0])
      result[53:50] = 4'b0000;  // Clear otype (unsealed)
    return result;
  endfunction

  // -----------------------------------------------------------------------
  // Bounds and permission checking
  // -----------------------------------------------------------------------

  // Check if access [addr+offset, addr+offset+size) is within bounds
  // Returns 1 if valid, 0 if violation
  function automatic cap_check_bounds(input [64:0] cap, input [31:0] offset, input [31:0] size);
    logic [31:0] eff_addr;
    logic [32:0] top;
    logic [31:0] base;

    if (!cap[64])
      return 1'b0;  // Invalid capability (tag=0)

    eff_addr = cap[31:0] + offset;
    top = cap_get_top(cap);
    base = cap_get_base(cap);

    // Check: base <= eff_addr and eff_addr + size <= top
    return (eff_addr >= base) && ({1'b0, eff_addr} + {1'b0, size} <= top);
  endfunction

  // Check permission: required is a mask of PERM_R, PERM_W, PERM_X
  function automatic cap_check_perm(input [64:0] cap, input [2:0] required);
    if (!cap[64])
      return 1'b0;  // Invalid capability
    return (cap[57:55] & required) == required;
  endfunction

  // -----------------------------------------------------------------------
  // Special capability register access (cspecialrw)
  // -----------------------------------------------------------------------
  // SCR indices:
  localparam [4:0] SCR_PCC    = 5'd0;
  localparam [4:0] SCR_DDC    = 5'd1;
  localparam [4:0] SCR_MTVEC  = 5'd28;
  localparam [4:0] SCR_MEPC   = 5'd29;
  localparam [4:0] SCR_MTDC   = 5'd30;

endpackage
