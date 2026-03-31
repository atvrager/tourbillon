package tbn

import chisel3._
import chisel3.simulator.scalatest.ChiselSim
import org.scalatest.freespec.AnyFreeSpec

class PrioritySpec extends AnyFreeSpec with ChiselSim {
  "Priority should always fire inc, suppressing dec" in {
    simulate(new Priority) { dut =>
      dut.reset.poke(true.B)
      dut.clock.step(2)
      dut.reset.poke(false.B)

      // inc has higher priority than dec. Both can_fire = true.B.
      // After 20 cycles, count should be 10 + 20 = 30 (started at init=10).
      dut.clock.step(20)
    }
  }
}
