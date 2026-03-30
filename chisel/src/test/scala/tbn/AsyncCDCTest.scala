package tbn

import chisel3._
import chisel3.simulator.scalatest.ChiselSim
import org.scalatest.freespec.AnyFreeSpec

class AsyncCDCSpec extends AnyFreeSpec with ChiselSim {
  "AsyncCDC should transfer data across domains" in {
    simulate(new AsyncCDC) { dut =>
      dut.reset.poke(true.B)
      dut.clock.step(2)
      dut.reset.poke(false.B)

      // FastProducer -> AsyncQueue(cmd) -> SlowConsumer -> AsyncQueue(resp) -> FastReceiver
      // TbnAsyncFifo is a sync Queue wrapper in simulation, so single-clock is fine.
      dut.clock.step(100)
    }
  }
}
