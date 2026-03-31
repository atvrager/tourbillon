package tbn

import chisel3._
import chisel3.util._

class Top extends Module {
  val io = IO(new Bundle {})

  // Queue: data
  val q_data = Module(new Queue(UInt(32.W), 2))

  // Queue: hi
  val q_hi = Module(new Queue(UInt(32.W), 2))

  // Queue: lo
  val q_lo = Module(new Queue(UInt(32.W), 2))

  val r_Source_go_can_fire = q_data.io.enq.ready
  val r_Source_go_will_fire = r_Source_go_can_fire

  val r_Router_go_can_fire = q_data.io.deq.valid
  val r_Router_go_will_fire = r_Router_go_can_fire

  val r_SinkHigh_go_can_fire = q_hi.io.deq.valid
  val r_SinkHigh_go_will_fire = r_SinkHigh_go_can_fire

  val r_SinkLow_go_can_fire = q_lo.io.deq.valid
  val r_SinkLow_go_will_fire = r_SinkLow_go_can_fire

  q_data.io.enq.valid := false.B
  q_data.io.enq.bits := 0.U
  q_hi.io.enq.valid := false.B
  q_hi.io.enq.bits := 0.U
  q_lo.io.enq.valid := false.B
  q_lo.io.enq.bits := 0.U

  // Rule: Source.go
  when (r_Source_go_will_fire) {
    q_data.io.enq.bits := 50.U
    q_data.io.enq.valid := true.B
  }
  // Rule: Router.go
  when (r_Router_go_will_fire) {
    when ((q_data.io.deq.bits > 100.U)) {
      q_hi.io.enq.bits := q_data.io.deq.bits
      q_hi.io.enq.valid := true.B
    } .otherwise {
      q_lo.io.enq.bits := q_data.io.deq.bits
      q_lo.io.enq.valid := true.B
    }
  }
  // Rule: SinkHigh.go
  when (r_SinkHigh_go_will_fire) {
  }
  // Rule: SinkLow.go
  when (r_SinkLow_go_will_fire) {
  }

  q_data.io.deq.ready := r_Router_go_will_fire
  q_hi.io.deq.ready := r_SinkHigh_go_will_fire
  q_lo.io.deq.ready := r_SinkLow_go_will_fire
}
