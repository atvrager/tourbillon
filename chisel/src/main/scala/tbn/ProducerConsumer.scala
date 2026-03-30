package tbn

import chisel3._
import chisel3.util._

class ProducerConsumer extends Module {
  val io = IO(new Bundle {})

  val c_PC_Producer_seq = RegInit(0.U(32.W))
  val c_PC_Consumer_last = RegInit(0.U(32.W))

  // Queue: q
  val q_q = Module(new Queue(UInt(32.W), 4))

  val r_PC_Producer_go_can_fire = q_q.io.enq.ready
  val r_PC_Producer_go_will_fire = r_PC_Producer_go_can_fire

  val r_PC_Consumer_go_can_fire = q_q.io.deq.valid
  val r_PC_Consumer_go_will_fire = r_PC_Consumer_go_can_fire

  c_PC_Producer_seq := c_PC_Producer_seq
  c_PC_Consumer_last := c_PC_Consumer_last
  q_q.io.enq.valid := false.B
  q_q.io.enq.bits := 0.U

  // Rule: PC_Producer.go
  when (r_PC_Producer_go_will_fire) {
    q_q.io.enq.bits := c_PC_Producer_seq
    q_q.io.enq.valid := true.B
    c_PC_Producer_seq := (c_PC_Producer_seq + 1.U)
  }
  // Rule: PC_Consumer.go
  when (r_PC_Consumer_go_will_fire) {
    c_PC_Consumer_last := q_q.io.deq.bits
  }

  q_q.io.deq.ready := r_PC_Consumer_go_will_fire
}
