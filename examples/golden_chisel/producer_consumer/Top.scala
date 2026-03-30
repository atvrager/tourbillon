package tbn

import chisel3._
import chisel3.util._

class Top extends Module {
  val io = IO(new Bundle {})

  // Queue: q
  val q_q = Module(new Queue(UInt(32.W), 4))

  val r_Producer_go_can_fire = q_q.io.enq.ready
  val r_Producer_go_will_fire = r_Producer_go_can_fire

  val r_Consumer_go_can_fire = q_q.io.deq.valid
  val r_Consumer_go_will_fire = r_Consumer_go_can_fire

  q_q.io.enq.valid := false.B
  q_q.io.enq.bits := 0.U

  // Rule: Producer.go
  when (r_Producer_go_will_fire) {
    q_q.io.enq.bits := 42.U
    q_q.io.enq.valid := true.B
  }
  // Rule: Consumer.go
  when (r_Consumer_go_will_fire) {
  }

  q_q.io.deq.ready := r_Consumer_go_will_fire
}
