package tbn

import chisel3._
import chisel3.util._

class Top extends Module {
  val io = IO(new Bundle {})

  val c_Counter_count = RegInit(0.U(32.W))

  val r_Counter_inc_can_fire = true.B
  val r_Counter_inc_will_fire = r_Counter_inc_can_fire
  val r_Counter_dec_can_fire = true.B
  val r_Counter_dec_will_fire = r_Counter_dec_can_fire && !r_Counter_inc_will_fire

  c_Counter_count := c_Counter_count

  // Rule: Counter.inc
  when (r_Counter_inc_will_fire) {
    c_Counter_count := (c_Counter_count + 1.U)
  }
  // Rule: Counter.dec
  when (r_Counter_dec_will_fire) {
    c_Counter_count := (c_Counter_count - 1.U)
  }

}
