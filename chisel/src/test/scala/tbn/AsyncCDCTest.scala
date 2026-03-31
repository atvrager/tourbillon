package tbn

import chisel3._
import chisel3.simulator.scalatest.ChiselSim
import org.scalatest.freespec.AnyFreeSpec

class AsyncCDCTestWrapper extends Module {
  val io = IO(new Bundle {
    val fast_clk_in = Input(Bool())
    val slow_clk_in = Input(Bool())
  })

  val dut = Module(new AsyncCDC)
  dut.io.fast_clk   := io.fast_clk_in.asClock
  dut.io.fast_rst_n := !reset.asBool
  dut.io.slow_clk   := io.slow_clk_in.asClock
  dut.io.slow_rst_n := !reset.asBool
}

class AsyncCDCSpec extends AnyFreeSpec with ChiselSim {
  "AsyncCDC should transfer data across domains" in {
    simulate(new AsyncCDCTestWrapper) { dut =>
      dut.reset.poke(true.B)
      dut.io.fast_clk_in.poke(false.B)
      dut.io.slow_clk_in.poke(false.B)
      dut.clock.step(5)
      dut.reset.poke(false.B)

      // Toggle both domain clocks at different rates relative to sys clock.
      // fast = every sys cycle, slow = every 3 sys cycles.
      var fastLevel = false
      var slowCount = 0
      var slowLevel = false
      for (_ <- 0 until 500) {
        fastLevel = !fastLevel
        dut.io.fast_clk_in.poke(fastLevel.B)
        slowCount += 1
        if (slowCount >= 3) {
          slowLevel = !slowLevel
          dut.io.slow_clk_in.poke(slowLevel.B)
          slowCount = 0
        }
        dut.clock.step(1)
      }
    }
  }
}
