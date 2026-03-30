package tbn

import chisel3._
import chisel3.simulator.scalatest.ChiselSim
import org.scalatest.freespec.AnyFreeSpec

class Spi2TLULV2Spec extends AnyFreeSpec with ChiselSim {
  "Spi2TLULV2 should elaborate and simulate" in {
    simulate(new Spi2TLULV2) { dut =>
      dut.reset.poke(true.B)
      dut.clock.step(2)
      dut.reset.poke(false.B)

      // Drive MOSI pin idle (valid=false means no SPI traffic)
      dut.io.q_mosi_pin.valid.poke(false.B)
      dut.io.q_mosi_pin.bits.poke(0.U)

      // MISO pin: always ready to accept
      dut.io.q_miso_pin.ready.poke(true.B)

      // TL-A: always ready to accept requests
      dut.io.q_tl_a.ready.poke(true.B)

      // TL-D: no response available
      dut.io.q_tl_d.valid.poke(false.B)
      dut.io.q_tl_d.bits.poke(0.U)

      // Run idle for 50 cycles — should be stable with no SPI traffic
      dut.clock.step(50)

      // Now send a byte on MOSI (bit-serial, MSB first): opcode 0x01 = READ
      for (bit <- Seq(0, 0, 0, 0, 0, 0, 0, 1)) {
        dut.io.q_mosi_pin.valid.poke(true.B)
        dut.io.q_mosi_pin.bits.poke(bit.U)
        dut.clock.step(1)
      }
      dut.io.q_mosi_pin.valid.poke(false.B)

      // Let the pipeline process
      dut.clock.step(20)
    }
  }

  "Spi2TLULV2 should run 200 cycles idle without assertion failure" in {
    simulate(new Spi2TLULV2) { dut =>
      dut.reset.poke(true.B)
      dut.clock.step(2)
      dut.reset.poke(false.B)

      dut.io.q_mosi_pin.valid.poke(false.B)
      dut.io.q_mosi_pin.bits.poke(0.U)
      dut.io.q_miso_pin.ready.poke(true.B)
      dut.io.q_tl_a.ready.poke(true.B)
      dut.io.q_tl_d.valid.poke(false.B)
      dut.io.q_tl_d.bits.poke(0.U)

      dut.clock.step(200)
    }
  }
}
