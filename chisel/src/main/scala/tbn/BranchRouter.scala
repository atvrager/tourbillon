package tbn

import chisel3._
import chisel3.util._

class BranchRouter extends Module {
  val io = IO(new Bundle {})

  val c_Branch_Source_seq = RegInit(0.U(32.W))
  val c_Branch_SinkHi_last = RegInit(0.U(32.W))
  val c_Branch_SinkLo_last = RegInit(0.U(32.W))

  // Queue: data
  val q_data = Module(new Queue(UInt(32.W), 2))

  // Queue: hi
  val q_hi = Module(new Queue(UInt(32.W), 2))

  // Queue: lo
  val q_lo = Module(new Queue(UInt(32.W), 2))

  val r_Branch_Source_go_can_fire = q_data.io.enq.ready
  val r_Branch_Source_go_will_fire = r_Branch_Source_go_can_fire

  val r_Branch_Router_go_can_fire = q_data.io.deq.valid
  val r_Branch_Router_go_will_fire = r_Branch_Router_go_can_fire

  val r_Branch_SinkHi_go_can_fire = q_hi.io.deq.valid
  val r_Branch_SinkHi_go_will_fire = r_Branch_SinkHi_go_can_fire

  val r_Branch_SinkLo_go_can_fire = q_lo.io.deq.valid
  val r_Branch_SinkLo_go_will_fire = r_Branch_SinkLo_go_can_fire

  c_Branch_Source_seq := c_Branch_Source_seq
  c_Branch_SinkHi_last := c_Branch_SinkHi_last
  c_Branch_SinkLo_last := c_Branch_SinkLo_last
  q_data.io.enq.valid := false.B
  q_data.io.enq.bits := 0.U
  q_hi.io.enq.valid := false.B
  q_hi.io.enq.bits := 0.U
  q_lo.io.enq.valid := false.B
  q_lo.io.enq.bits := 0.U

  // Rule: Branch_Source.go
  when (r_Branch_Source_go_will_fire) {
    q_data.io.enq.bits := c_Branch_Source_seq
    q_data.io.enq.valid := true.B
    c_Branch_Source_seq := (c_Branch_Source_seq + 1.U)
  }
  // Rule: Branch_Router.go
  when (r_Branch_Router_go_will_fire) {
    when ((q_data.io.deq.bits > 5.U)) {
      q_hi.io.enq.bits := q_data.io.deq.bits
      q_hi.io.enq.valid := true.B
    } .otherwise {
      q_lo.io.enq.bits := q_data.io.deq.bits
      q_lo.io.enq.valid := true.B
    }
  }
  // Rule: Branch_SinkHi.go
  when (r_Branch_SinkHi_go_will_fire) {
    c_Branch_SinkHi_last := q_hi.io.deq.bits
  }
  // Rule: Branch_SinkLo.go
  when (r_Branch_SinkLo_go_will_fire) {
    c_Branch_SinkLo_last := q_lo.io.deq.bits
  }

  q_data.io.deq.ready := r_Branch_Router_go_will_fire
  q_hi.io.deq.ready := r_Branch_SinkHi_go_will_fire
  q_lo.io.deq.ready := r_Branch_SinkLo_go_will_fire
}
