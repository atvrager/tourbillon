package tbn

import chisel3._
import chisel3.util._

/// Simulation-friendly async FIFO module for Tourbillon.
/// In simulation, this is just a synchronous Queue (functionally equivalent).
/// For real CDC, this would be replaced by the gray-code tbn_async_fifo.sv.
class TbnAsyncFifo(width: Int, depth: Int) extends Module {
  val io = IO(new Bundle {
    val enq = Flipped(Decoupled(UInt(width.W)))
    val deq = Decoupled(UInt(width.W))
  })

  // For simulation: a standard synchronous Queue is functionally equivalent
  // to an async FIFO (both are valid/ready FIFOs with the same interface).
  val q = Module(new Queue(UInt(width.W), depth))
  q.io.enq <> io.enq
  io.deq <> q.io.deq
}
