package tbn

import chisel3._
import chisel3.simulator.scalatest.ChiselSim
import org.scalatest.freespec.AnyFreeSpec

class PeekSpec extends AnyFreeSpec with ChiselSim {
  "PeekTest should allow concurrent read via peek" in {
    simulate(new PeekTest) { dut =>
      dut.reset.poke(true.B)
      dut.clock.step(2)
      dut.reset.poke(false.B)

      // Writer increments reg each cycle, Reader peeks it.
      // peek() sees the old value (pre-write-back), same as SV semantics.
      dut.clock.step(50)
    }
  }
}
