// soc_pkg.sv — Single source of truth for clock frequencies
//
// All simulation, FPGA, and firmware consumers reference these constants.
// The Tourbillon .tbn code references identifiers by name (e.g., CPU_FREQ_HZ),
// which resolve to these localparams via soc_pkg at SV compile time.

package soc_pkg;
  localparam [31:0] CPU_FREQ_HZ  = 32'd100_000_000;  // 100 MHz
  localparam [31:0] XBAR_FREQ_HZ = 32'd150_000_000;  // 150 MHz
  localparam [31:0] DEV_FREQ_HZ  = 32'd50_000_000;   //  50 MHz
  localparam [31:0] NUM_CLOCKS   = 32'd3;
endpackage
