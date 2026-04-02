// manifest_pkg.sv — Helper functions for device manifest slot access
//
// SlotData is a packed 224-bit (7×32) record. These functions extract
// or update individual 32-bit fields by index.
//
// Field layout (MSB-first in the packed struct):
//   field 0: key_hi     [223:192]
//   field 1: key_lo     [191:160]
//   field 2: base_addr  [159:128]
//   field 3: aperture   [127:96]
//   field 4: irq        [95:64]
//   field 5: flags      [63:32]
//   field 6: valid      [31:0]

package manifest_pkg;

  // Read a 32-bit field from a 224-bit packed slot
  function automatic [31:0] slot_read(
    input [223:0] slot,
    input [2:0]   field_idx
  );
    case (field_idx)
      3'd0: slot_read = slot[223:192]; // key_hi
      3'd1: slot_read = slot[191:160]; // key_lo
      3'd2: slot_read = slot[159:128]; // base_addr
      3'd3: slot_read = slot[127:96];  // aperture
      3'd4: slot_read = slot[95:64];   // irq
      3'd5: slot_read = slot[63:32];   // flags
      3'd6: slot_read = slot[31:0];    // valid
      default: slot_read = 32'h0;
    endcase
  endfunction

  // Write a 32-bit field into a 224-bit packed slot
  function automatic [223:0] slot_write(
    input [223:0] slot,
    input [2:0]   field_idx,
    input [31:0]  value
  );
    slot_write = slot;
    case (field_idx)
      3'd0: slot_write[223:192] = value;
      3'd1: slot_write[191:160] = value;
      3'd2: slot_write[159:128] = value;
      3'd3: slot_write[127:96]  = value;
      3'd4: slot_write[95:64]   = value;
      3'd5: slot_write[63:32]   = value;
      3'd6: slot_write[31:0]    = value;
      default: ;
    endcase
  endfunction

endpackage
