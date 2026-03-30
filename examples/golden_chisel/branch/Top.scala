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

  // Rule: Source.go
  val q_data_go_enq_val = 50.U
  // Rule: Router.go
  val q_hi_go_enq_val = Mux((q_data.io.deq.bits > 100.U), q_data.io.deq.bits, 0.U)
  val q_lo_go_enq_val = Mux((q_data.io.deq.bits > 100.U), 0.U, q_data.io.deq.bits)
  // Rule: SinkHigh.go
  // Rule: SinkLow.go

  q_data.io.deq.ready := r_Router_go_will_fire
  q_data.io.enq.valid := r_Source_go_will_fire
  q_data.io.enq.bits := Mux(r_Source_go_will_fire, q_data_go_enq_val, 0.U)

  q_hi.io.deq.ready := r_SinkHigh_go_will_fire
  q_hi.io.enq.valid := r_Router_go_will_fire
  q_hi.io.enq.bits := Mux(r_Router_go_will_fire, q_hi_go_enq_val, 0.U)

  q_lo.io.deq.ready := r_SinkLow_go_will_fire
  q_lo.io.enq.valid := r_Router_go_will_fire
  q_lo.io.enq.bits := Mux(r_Router_go_will_fire, q_lo_go_enq_val, 0.U)

}
