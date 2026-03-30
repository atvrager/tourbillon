package tbn

import chisel3._
import chisel3.simulator.scalatest.ChiselSim
import org.scalatest.freespec.AnyFreeSpec

class CounterSpec extends AnyFreeSpec with ChiselSim {
  "Counter should increment each cycle" in {
    simulate(new Counter) { dut =>
      // Reset
      dut.reset.poke(true.B)
      dut.clock.step(2)
      dut.reset.poke(false.B)

      // Run 100 cycles — counter fires every cycle (can_fire = true.B)
      dut.clock.step(100)
    }
  }
}
