package tbn

import chisel3._
import chisel3.util._

class Top extends Module {
  val io = IO(new Bundle {})

  val c_Counter_count = RegInit(0.U(32.W))

  val r_Counter_tick_can_fire = true.B
  val r_Counter_tick_will_fire = r_Counter_tick_can_fire

  c_Counter_count := c_Counter_count

  // Rule: Counter.tick
  when (r_Counter_tick_will_fire) {
    c_Counter_count := (c_Counter_count + 1.U)
  }

}
