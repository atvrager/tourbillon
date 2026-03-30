package tbn

import chisel3._
import chisel3.util._

class Top extends Module {
  val io = IO(new Bundle {})

  val c_Writer_reg = RegInit(0.U(32.W))

  val r_Writer_go_can_fire = true.B
  val r_Writer_go_will_fire = r_Writer_go_can_fire

  val r_Reader_go_can_fire = true.B
  val r_Reader_go_will_fire = r_Reader_go_can_fire

  c_Writer_reg := c_Writer_reg

  // Rule: Writer.go
  when (r_Writer_go_will_fire) {
    c_Writer_reg := (c_Writer_reg + 1.U)
  }
  // Rule: Reader.go
  when (r_Reader_go_will_fire) {
  }

}
