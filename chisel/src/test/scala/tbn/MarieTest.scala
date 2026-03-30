package tbn

import chisel3._
import chisel3.simulator.scalatest.ChiselSim
import org.scalatest.freespec.AnyFreeSpec

class MarieSpec extends AnyFreeSpec with ChiselSim {
  "Marie SoC should elaborate and simulate" in {
    simulate(new Marie) { dut =>
      dut.reset.poke(true.B)
      dut.clock.step(2)
      dut.reset.poke(false.B)

      // imem: instruction memory port — provide NOP (addi x0, x0, 0 = 0x00000013)
      dut.io.q_CPUCore_imem_read_req.ready.poke(true.B)
      dut.io.q_CPUCore_imem_read_resp.valid.poke(true.B)
      dut.io.q_CPUCore_imem_read_resp.bits.poke("h00000013".U) // NOP
      dut.io.q_CPUCore_imem_write_req.ready.poke(true.B)

      // dmem: data memory port — always ready, return zero
      dut.io.q_dev_mem_read_req.ready.poke(true.B)
      dut.io.q_dev_mem_read_resp.valid.poke(true.B)
      dut.io.q_dev_mem_read_resp.bits.poke(0.U)
      dut.io.q_dev_mem_write_req.ready.poke(true.B)

      // UART: idle
      dut.io.q_UartPhy_tx_pin.ready.poke(true.B)
      dut.io.q_UartPhy_rx_pin.valid.poke(false.B)
      dut.io.q_UartPhy_rx_pin.bits.poke(0.U)
      dut.io.q_UartPhy_rts_pin.ready.poke(true.B)
      dut.io.q_UartPhy_cts_pin.valid.poke(true.B)
      dut.io.q_UartPhy_cts_pin.bits.poke(0.U)

      // Run the SoC for 100 cycles executing NOPs
      dut.clock.step(100)
    }
  }

  "Marie SoC should survive 500 cycles" in {
    simulate(new Marie) { dut =>
      dut.reset.poke(true.B)
      dut.clock.step(5)
      dut.reset.poke(false.B)

      // Provide NOP on imem, zero on dmem, idle UART
      dut.io.q_CPUCore_imem_read_req.ready.poke(true.B)
      dut.io.q_CPUCore_imem_read_resp.valid.poke(true.B)
      dut.io.q_CPUCore_imem_read_resp.bits.poke("h00000013".U)
      dut.io.q_CPUCore_imem_write_req.ready.poke(true.B)
      dut.io.q_dev_mem_read_req.ready.poke(true.B)
      dut.io.q_dev_mem_read_resp.valid.poke(true.B)
      dut.io.q_dev_mem_read_resp.bits.poke(0.U)
      dut.io.q_dev_mem_write_req.ready.poke(true.B)
      dut.io.q_UartPhy_tx_pin.ready.poke(true.B)
      dut.io.q_UartPhy_rx_pin.valid.poke(false.B)
      dut.io.q_UartPhy_rx_pin.bits.poke(0.U)
      dut.io.q_UartPhy_rts_pin.ready.poke(true.B)
      dut.io.q_UartPhy_cts_pin.valid.poke(true.B)
      dut.io.q_UartPhy_cts_pin.bits.poke(0.U)

      dut.clock.step(500)
    }
  }
}
