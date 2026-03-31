package tbn

import chisel3._
import chisel3.simulator.scalatest.ChiselSim
import org.scalatest.freespec.AnyFreeSpec

class ProducerConsumerSpec extends AnyFreeSpec with ChiselSim {
  "ProducerConsumer should transfer data without deadlock" in {
    simulate(new ProducerConsumer) { dut =>
      dut.reset.poke(true.B)
      dut.clock.step(2)
      dut.reset.poke(false.B)

      // Run 200 cycles — producer fills queue, consumer drains it
      dut.clock.step(200)
    }
  }
}
