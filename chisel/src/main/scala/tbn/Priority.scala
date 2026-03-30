package tbn

import chisel3._
import chisel3.util._

class Priority extends Module {
  val io = IO(new Bundle {})

  val c_Priority_Proc_count = RegInit(10.U(32.W))

  val r_Priority_Proc_inc_can_fire = true.B
  val r_Priority_Proc_inc_will_fire = r_Priority_Proc_inc_can_fire
  val r_Priority_Proc_dec_can_fire = true.B
  val r_Priority_Proc_dec_will_fire = r_Priority_Proc_dec_can_fire && !r_Priority_Proc_inc_will_fire

  c_Priority_Proc_count := c_Priority_Proc_count

  // Rule: Priority_Proc.inc
  when (r_Priority_Proc_inc_will_fire) {
    c_Priority_Proc_count := (c_Priority_Proc_count + 1.U)
  }
  // Rule: Priority_Proc.dec
  when (r_Priority_Proc_dec_will_fire) {
    c_Priority_Proc_count := (c_Priority_Proc_count - 1.U)
  }

}
