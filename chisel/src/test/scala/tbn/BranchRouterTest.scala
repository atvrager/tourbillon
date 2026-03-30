package tbn

import chisel3._
import chisel3.simulator.scalatest.ChiselSim
import org.scalatest.freespec.AnyFreeSpec

class BranchRouterSpec extends AnyFreeSpec with ChiselSim {
  "BranchRouter should route values through queues" in {
    simulate(new BranchRouter) { dut =>
      dut.reset.poke(true.B)
      dut.clock.step(2)
      dut.reset.poke(false.B)

      // Source produces 0, 1, 2, ... Router sends <=5 to low, >5 to high.
      // Queue depths are 2, so backpressure stalls.
      dut.clock.step(500)
    }
  }
}
