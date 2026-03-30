package tbn

import chisel3._
import chisel3.util._

class AsyncCDC extends Module {
  val io = IO(new Bundle {})

  val c_Async_FastProducer_seq = RegInit(0.U(32.W))
  val c_Async_FastReceiver_last = RegInit(0.U(32.W))

  // AsyncQueue: cmd
  val q_cmd = Module(new TbnAsyncFifo(32, 4))

  // AsyncQueue: resp
  val q_resp = Module(new TbnAsyncFifo(32, 4))

  val r_Async_FastProducer_go_can_fire = q_cmd.io.enq.ready
  val r_Async_FastProducer_go_will_fire = r_Async_FastProducer_go_can_fire

  val r_Async_SlowConsumer_go_can_fire = q_cmd.io.deq.valid && q_resp.io.enq.ready
  val r_Async_SlowConsumer_go_will_fire = r_Async_SlowConsumer_go_can_fire

  val r_Async_FastReceiver_go_can_fire = q_resp.io.deq.valid
  val r_Async_FastReceiver_go_will_fire = r_Async_FastReceiver_go_can_fire

  c_Async_FastProducer_seq := c_Async_FastProducer_seq
  c_Async_FastReceiver_last := c_Async_FastReceiver_last
  q_cmd.io.enq.valid := false.B
  q_cmd.io.enq.bits := 0.U
  q_resp.io.enq.valid := false.B
  q_resp.io.enq.bits := 0.U

  // Rule: Async_FastProducer.go
  when (r_Async_FastProducer_go_will_fire) {
    q_cmd.io.enq.bits := c_Async_FastProducer_seq
    q_cmd.io.enq.valid := true.B
    c_Async_FastProducer_seq := (c_Async_FastProducer_seq + 1.U)
  }
  // Rule: Async_SlowConsumer.go
  when (r_Async_SlowConsumer_go_will_fire) {
    q_resp.io.enq.bits := (q_cmd.io.deq.bits + 100.U)
    q_resp.io.enq.valid := true.B
  }
  // Rule: Async_FastReceiver.go
  when (r_Async_FastReceiver_go_will_fire) {
    c_Async_FastReceiver_last := q_resp.io.deq.bits
  }

  q_cmd.io.deq.ready := r_Async_SlowConsumer_go_will_fire
  q_resp.io.deq.ready := r_Async_FastReceiver_go_will_fire
}
