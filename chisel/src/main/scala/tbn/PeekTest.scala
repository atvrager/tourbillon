package tbn

import chisel3._
import chisel3.util._

class PeekTest extends Module {
  val io = IO(new Bundle {})

  val c_Peek_Writer_reg = RegInit(0.U(32.W))

  val r_Peek_Writer_go_can_fire = true.B
  val r_Peek_Writer_go_will_fire = r_Peek_Writer_go_can_fire

  val r_Peek_Reader_go_can_fire = true.B
  val r_Peek_Reader_go_will_fire = r_Peek_Reader_go_can_fire

  c_Peek_Writer_reg := c_Peek_Writer_reg

  // Rule: Peek_Writer.go
  when (r_Peek_Writer_go_will_fire) {
    c_Peek_Writer_reg := (c_Peek_Writer_reg + 1.U)
  }
  // Rule: Peek_Reader.go
  when (r_Peek_Reader_go_will_fire) {
  }

}
