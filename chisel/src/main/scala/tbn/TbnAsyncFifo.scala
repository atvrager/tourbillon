package tbn

import chisel3._
import chisel3.util._

/// Gray-code async FIFO for Tourbillon CDC crossings.
/// Direct Chisel port of tbn_async_fifo.sv.
///
/// Depth must be a power of 2. Uses 2-FF synchronizers for pointer
/// crossing and gray-code encoding for metastability safety.
///
/// The module's implicit clock/reset is unused — all state is explicitly
/// clocked via the wr_clk/rd_clk IO ports.
class TbnAsyncFifo(width: Int, depth: Int) extends Module {
  require(isPow2(depth), s"TbnAsyncFifo depth must be a power of 2, got $depth")
  val awidth = log2Ceil(depth)

  val io = IO(new Bundle {
    val enq     = Flipped(Decoupled(UInt(width.W)))
    val deq     = Decoupled(UInt(width.W))
    val wr_clk  = Input(Clock())
    val wr_rst  = Input(Reset())
    val rd_clk  = Input(Clock())
    val rd_rst  = Input(Reset())
  })

  // Storage (combinational read, written in wr domain)
  val storage = Reg(Vec(depth, UInt(width.W)))

  // --- Write-side (wr_clk domain) ---
  val wrPtrBin = withClockAndReset(io.wr_clk, io.wr_rst) { RegInit(0.U((awidth + 1).W)) }
  val wrPtrGray = wrPtrBin ^ (wrPtrBin >> 1)

  val rdPtrGraySync1 = withClockAndReset(io.wr_clk, io.wr_rst) { RegInit(0.U((awidth + 1).W)) }
  val rdPtrGraySync2 = withClockAndReset(io.wr_clk, io.wr_rst) { RegInit(0.U((awidth + 1).W)) }

  // --- Read-side (rd_clk domain) ---
  val rdPtrBin = withClockAndReset(io.rd_clk, io.rd_rst) { RegInit(0.U((awidth + 1).W)) }
  val rdPtrGray = rdPtrBin ^ (rdPtrBin >> 1)

  val wrPtrGraySync1 = withClockAndReset(io.rd_clk, io.rd_rst) { RegInit(0.U((awidth + 1).W)) }
  val wrPtrGraySync2 = withClockAndReset(io.rd_clk, io.rd_rst) { RegInit(0.U((awidth + 1).W)) }

  // Full/empty
  val full = (wrPtrGray(awidth) =/= rdPtrGraySync2(awidth)) &&
             (wrPtrGray(awidth - 1) =/= rdPtrGraySync2(awidth - 1)) &&
             (wrPtrGray(awidth - 2, 0) === rdPtrGraySync2(awidth - 2, 0))
  val empty = rdPtrGray === wrPtrGraySync2

  io.enq.ready := !full
  io.deq.valid := !empty
  io.deq.bits  := storage(rdPtrBin(awidth - 1, 0))

  val doEnq = io.enq.valid && io.enq.ready
  val doDeq = io.deq.ready && io.deq.valid

  // Write-side logic
  withClockAndReset(io.wr_clk, io.wr_rst) {
    rdPtrGraySync1 := rdPtrGray
    rdPtrGraySync2 := rdPtrGraySync1
    when(doEnq) {
      storage(wrPtrBin(awidth - 1, 0)) := io.enq.bits
      wrPtrBin := wrPtrBin + 1.U
    }
  }

  // Read-side logic
  withClockAndReset(io.rd_clk, io.rd_rst) {
    wrPtrGraySync1 := wrPtrGray
    wrPtrGraySync2 := wrPtrGraySync1
    when(doDeq) {
      rdPtrBin := rdPtrBin + 1.U
    }
  }
}
