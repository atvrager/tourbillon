package tbn

import chisel3._
import chisel3.simulator.scalatest.ChiselSim
import org.scalatest.freespec.AnyFreeSpec

class MarieTestWrapper extends Module {
  val io = IO(new Bundle {
    val cpu_clk_in  = Input(Bool())
    val xbar_clk_in = Input(Bool())
    val dev_clk_in  = Input(Bool())
  })

  val dut = Module(new Marie)

  // Domain clocks from Bool inputs
  dut.io.cpu_clk   := io.cpu_clk_in.asClock
  dut.io.cpu_rst_n := !reset.asBool
  dut.io.xbar_clk  := io.xbar_clk_in.asClock
  dut.io.xbar_rst_n := !reset.asBool
  dut.io.dev_clk   := io.dev_clk_in.asClock
  dut.io.dev_rst_n := !reset.asBool

  // imem: NOP (addi x0, x0, 0 = 0x00000013)
  dut.io.q_CPUCore_imem_read_req.ready  := true.B
  dut.io.q_CPUCore_imem_read_resp.valid := true.B
  dut.io.q_CPUCore_imem_read_resp.bits  := "h00000013".U
  dut.io.q_CPUCore_imem_write_req.ready := true.B

  // dmem
  dut.io.q_dev_mem_read_req.ready  := true.B
  dut.io.q_dev_mem_read_resp.valid := true.B
  dut.io.q_dev_mem_read_resp.bits  := 0.U
  dut.io.q_dev_mem_write_req.ready := true.B

  // UART: idle
  dut.io.q_UartPhy_tx_pin.ready  := true.B
  dut.io.q_UartPhy_rx_pin.valid  := false.B
  dut.io.q_UartPhy_rx_pin.bits   := 0.U
  dut.io.q_UartPhy_rts_pin.ready := true.B
  dut.io.q_UartPhy_cts_pin.valid := true.B
  dut.io.q_UartPhy_cts_pin.bits  := 0.U
}

class MarieSpec extends AnyFreeSpec with ChiselSim {
  "Marie SoC should elaborate and simulate with 3 clock domains" in {
    simulate(new MarieTestWrapper) { dut =>
      dut.reset.poke(true.B)
      dut.io.cpu_clk_in.poke(false.B)
      dut.io.xbar_clk_in.poke(false.B)
      dut.io.dev_clk_in.poke(false.B)
      dut.clock.step(5)
      dut.reset.poke(false.B)

      // Run 200 cycles with all 3 domain clocks toggling at different rates
      // cpu=every cycle, xbar=every 2, dev=every 3 (approximates 100/150/50 MHz)
      var xbarCount = 0
      var devCount = 0
      var cpuLevel = false
      var xbarLevel = false
      var devLevel = false
      for (_ <- 0 until 200) {
        cpuLevel = !cpuLevel
        dut.io.cpu_clk_in.poke(cpuLevel.B)

        xbarCount += 1
        if (xbarCount >= 2) {
          xbarLevel = !xbarLevel
          dut.io.xbar_clk_in.poke(xbarLevel.B)
          xbarCount = 0
        }

        devCount += 1
        if (devCount >= 3) {
          devLevel = !devLevel
          dut.io.dev_clk_in.poke(devLevel.B)
          devCount = 0
        }

        dut.clock.step(1)
      }
    }
  }
}
