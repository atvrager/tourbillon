package tbn

import chisel3._
import chisel3.util._

class BusReq extends Bundle {
  val we = Bool()
  val addr = UInt(32.W)
  val data = UInt(32.W)
}

class Decoded extends Bundle {
  val op = UInt(4.W)
  val rd = UInt(5.W)
  val rs1_val = UInt(32.W)
  val rs2_val = UInt(32.W)
  val imm = UInt(32.W)
  val pc = UInt(32.W)
  val mem = UInt(2.W)
  val wb = Bool()
  val is_br = Bool()
  val funct3 = UInt(3.W)
  val opcode = UInt(7.W)
}

class ExecStash extends Bundle {
  val phase = UInt(32.W)
  val rd = UInt(5.W)
  val funct3 = UInt(3.W)
  val rs2_val = UInt(32.W)
  val result = UInt(32.W)
  val next_pc = UInt(32.W)
}

object MemOp {
  val Load = 0.U
  val Store = 1.U
  val `None` = 2.U
}

class Marie extends Module {
  val io = IO(new Bundle {
    val q_CPUCore_imem_read_req = Decoupled(UInt(32.W))
    val q_CPUCore_imem_read_resp = Flipped(Decoupled(UInt(32.W)))
    val q_CPUCore_imem_write_req = Decoupled(UInt(64.W))
    val q_UartPhy_tx_pin = Decoupled(UInt(1.W))
    val q_UartPhy_rx_pin = Flipped(Decoupled(UInt(1.W)))
    val q_UartPhy_rts_pin = Decoupled(UInt(1.W))
    val q_UartPhy_cts_pin = Flipped(Decoupled(UInt(1.W)))
    val q_dev_mem_read_req = Decoupled(UInt(32.W))
    val q_dev_mem_read_resp = Flipped(Decoupled(UInt(32.W)))
    val q_dev_mem_write_req = Decoupled(UInt(64.W))
  })

  val BAUD_DIV = 33.U
  val MEM_REGION = 8.U
  val UART_REGION = 1.U

  val c_CPUCore_Execute_stash = RegInit(0.U(136.W))
  val c_CPUCore_Writeback_regfile = RegInit(0.U(1024.W))
  val c_UartPhy_UartTx_sr = RegInit(1023.U(10.W))
  val c_UartPhy_UartTx_baud_cnt = RegInit(0.U(16.W))
  val c_UartPhy_UartTx_bit_cnt = RegInit(0.U(4.W))
  val c_UartPhy_UartRx_rx_ph = RegInit(0.U(2.W))
  val c_UartPhy_UartRx_rx_sr = RegInit(0.U(8.W))
  val c_UartPhy_UartRx_rx_bc = RegInit(0.U(16.W))
  val c_UartPhy_UartRx_rx_bn = RegInit(0.U(4.W))
  val c_CpuDmemAdapter_phase = RegInit(0.U(32.W))
  val c_Xbar_xphase = RegInit(0.U(32.W))
  val c_MemDevice_mphase = RegInit(0.U(32.W))

  // Queue: CPUCore_fetch_q
  val q_CPUCore_fetch_q = Module(new Queue(UInt(64.W), 1))

  // Queue: CPUCore_decode_q
  val q_CPUCore_decode_q = Module(new Queue(UInt(151.W), 1))

  // Queue: CPUCore_wb_q
  val q_CPUCore_wb_q = Module(new Queue(UInt(38.W), 1))

  // Queue: CPUCore_next_pc_q
  // TODO: init tokens not directly supported in Chisel Queue
  val q_CPUCore_next_pc_q = Module(new Queue(UInt(32.W), 1))

  // Queue: adapter_rd_req
  val q_adapter_rd_req = Module(new Queue(UInt(32.W), 1))

  // Queue: adapter_rd_resp
  val q_adapter_rd_resp = Module(new Queue(UInt(32.W), 1))

  // Queue: adapter_wr_req
  val q_adapter_wr_req = Module(new Queue(UInt(64.W), 1))

  // AsyncQueue: uart_bus_req
  val q_uart_bus_req = Module(new TbnAsyncFifo(65, 4))

  // AsyncQueue: uart_bus_resp
  val q_uart_bus_resp = Module(new TbnAsyncFifo(32, 4))

  // Queue: UartPhy_tx_q
  val q_UartPhy_tx_q = Module(new Queue(UInt(8.W), 16))

  // Queue: UartPhy_rx_q
  val q_UartPhy_rx_q = Module(new Queue(UInt(8.W), 2))

  // AsyncQueue: cpu_bus_req
  val q_cpu_bus_req = Module(new TbnAsyncFifo(65, 4))

  // AsyncQueue: cpu_bus_resp
  val q_cpu_bus_resp = Module(new TbnAsyncFifo(32, 4))

  // AsyncQueue: mem_bus_req
  val q_mem_bus_req = Module(new TbnAsyncFifo(65, 4))

  // AsyncQueue: mem_bus_resp
  val q_mem_bus_resp = Module(new TbnAsyncFifo(32, 4))

  val r_CPUCore_Fetch_tick_can_fire = io.q_CPUCore_imem_read_resp.valid && q_CPUCore_next_pc_q.io.deq.valid && q_CPUCore_fetch_q.io.enq.ready && io.q_CPUCore_imem_read_req.ready
  val r_CPUCore_Fetch_tick_will_fire = r_CPUCore_Fetch_tick_can_fire

  val r_CPUCore_Decode_crack_can_fire = q_CPUCore_fetch_q.io.deq.valid
  val r_CPUCore_Decode_crack_will_fire = r_CPUCore_Decode_crack_can_fire

  val r_CPUCore_Execute_tick_can_fire = true.B
  val r_CPUCore_Execute_tick_will_fire = r_CPUCore_Execute_tick_can_fire

  val r_CPUCore_Writeback_commit_can_fire = q_CPUCore_wb_q.io.deq.valid
  val r_CPUCore_Writeback_commit_will_fire = r_CPUCore_Writeback_commit_can_fire

  val r_CpuDmemAdapter_tick_can_fire = true.B
  val r_CpuDmemAdapter_tick_will_fire = r_CpuDmemAdapter_tick_can_fire

  val r_Xbar_tick_can_fire = true.B
  val r_Xbar_tick_will_fire = r_Xbar_tick_can_fire

  val r_MemDevice_tick_can_fire = true.B
  val r_MemDevice_tick_will_fire = r_MemDevice_tick_can_fire

  val r_UartPhy_UartDevice_handle_can_fire = q_uart_bus_req.io.deq.valid
  val r_UartPhy_UartDevice_handle_will_fire = r_UartPhy_UartDevice_handle_can_fire

  val r_UartPhy_UartTx_tick_can_fire = io.q_UartPhy_tx_pin.ready
  val r_UartPhy_UartTx_tick_will_fire = r_UartPhy_UartTx_tick_can_fire

  val r_UartPhy_UartRx_tick_can_fire = io.q_UartPhy_rts_pin.ready
  val r_UartPhy_UartRx_tick_will_fire = r_UartPhy_UartRx_tick_can_fire

  c_CPUCore_Execute_stash := c_CPUCore_Execute_stash
  c_CPUCore_Writeback_regfile := c_CPUCore_Writeback_regfile
  c_UartPhy_UartTx_sr := c_UartPhy_UartTx_sr
  c_UartPhy_UartTx_baud_cnt := c_UartPhy_UartTx_baud_cnt
  c_UartPhy_UartTx_bit_cnt := c_UartPhy_UartTx_bit_cnt
  c_UartPhy_UartRx_rx_ph := c_UartPhy_UartRx_rx_ph
  c_UartPhy_UartRx_rx_sr := c_UartPhy_UartRx_rx_sr
  c_UartPhy_UartRx_rx_bc := c_UartPhy_UartRx_rx_bc
  c_UartPhy_UartRx_rx_bn := c_UartPhy_UartRx_rx_bn
  c_CpuDmemAdapter_phase := c_CpuDmemAdapter_phase
  c_Xbar_xphase := c_Xbar_xphase
  c_MemDevice_mphase := c_MemDevice_mphase
  q_CPUCore_fetch_q.io.enq.valid := false.B
  q_CPUCore_fetch_q.io.enq.bits := 0.U
  q_CPUCore_decode_q.io.enq.valid := false.B
  q_CPUCore_decode_q.io.enq.bits := 0.U
  q_CPUCore_wb_q.io.enq.valid := false.B
  q_CPUCore_wb_q.io.enq.bits := 0.U
  q_CPUCore_next_pc_q.io.enq.valid := false.B
  q_CPUCore_next_pc_q.io.enq.bits := 0.U
  q_adapter_rd_req.io.enq.valid := false.B
  q_adapter_rd_req.io.enq.bits := 0.U
  q_adapter_rd_resp.io.enq.valid := false.B
  q_adapter_rd_resp.io.enq.bits := 0.U
  q_adapter_wr_req.io.enq.valid := false.B
  q_adapter_wr_req.io.enq.bits := 0.U
  io.q_CPUCore_imem_read_req.valid := false.B
  io.q_CPUCore_imem_read_req.bits := 0.U
  io.q_CPUCore_imem_write_req.valid := false.B
  io.q_CPUCore_imem_write_req.bits := 0.U
  q_uart_bus_req.io.enq.valid := false.B
  q_uart_bus_req.io.enq.bits := 0.U
  q_uart_bus_resp.io.enq.valid := false.B
  q_uart_bus_resp.io.enq.bits := 0.U
  q_UartPhy_tx_q.io.enq.valid := false.B
  q_UartPhy_tx_q.io.enq.bits := 0.U
  q_UartPhy_rx_q.io.enq.valid := false.B
  q_UartPhy_rx_q.io.enq.bits := 0.U
  io.q_UartPhy_tx_pin.valid := false.B
  io.q_UartPhy_tx_pin.bits := 0.U
  io.q_UartPhy_rts_pin.valid := false.B
  io.q_UartPhy_rts_pin.bits := 0.U
  io.q_dev_mem_read_req.valid := false.B
  io.q_dev_mem_read_req.bits := 0.U
  io.q_dev_mem_write_req.valid := false.B
  io.q_dev_mem_write_req.bits := 0.U
  q_cpu_bus_req.io.enq.valid := false.B
  q_cpu_bus_req.io.enq.bits := 0.U
  q_cpu_bus_resp.io.enq.valid := false.B
  q_cpu_bus_resp.io.enq.bits := 0.U
  q_mem_bus_req.io.enq.valid := false.B
  q_mem_bus_req.io.enq.bits := 0.U
  q_mem_bus_resp.io.enq.valid := false.B
  q_mem_bus_resp.io.enq.bits := 0.U
  q_UartPhy_tx_q.io.deq.ready := false.B
  q_uart_bus_resp.io.deq.ready := false.B
  io.q_UartPhy_rx_pin.ready := false.B
  q_adapter_rd_resp.io.deq.ready := false.B
  io.q_UartPhy_cts_pin.ready := false.B
  q_cpu_bus_req.io.deq.ready := false.B
  q_adapter_rd_req.io.deq.ready := false.B
  q_UartPhy_rx_q.io.deq.ready := false.B
  q_cpu_bus_resp.io.deq.ready := false.B
  q_adapter_wr_req.io.deq.ready := false.B
  q_CPUCore_decode_q.io.deq.ready := false.B
  io.q_dev_mem_read_resp.ready := false.B
  q_mem_bus_req.io.deq.ready := false.B
  q_mem_bus_resp.io.deq.ready := false.B

  // Rule: CPUCore_Fetch.tick
  when (r_CPUCore_Fetch_tick_will_fire) {
    io.q_CPUCore_imem_read_req.bits := q_CPUCore_next_pc_q.io.deq.bits
    io.q_CPUCore_imem_read_req.valid := true.B
    q_CPUCore_fetch_q.io.enq.bits := Cat(q_CPUCore_next_pc_q.io.deq.bits, io.q_CPUCore_imem_read_resp.bits)
    q_CPUCore_fetch_q.io.enq.valid := true.B
  }
  // Rule: CPUCore_Decode.crack
  when (r_CPUCore_Decode_crack_will_fire) {
    when (true.B) {
      q_CPUCore_decode_q.io.enq.bits := Cat(0.U, 0.U, (c_CPUCore_Writeback_regfile >> (0.U * 32.U))(31, 0), (c_CPUCore_Writeback_regfile >> (0.U * 32.U))(31, 0), 0.U, q_CPUCore_fetch_q.io.deq.bits(63, 32), 0.U, 0.U, 0.U, 0.U, 0.U)
      q_CPUCore_decode_q.io.enq.valid := true.B
    } .elsewhen (false.B) {
    }
  }
  // Rule: CPUCore_Execute.tick
  when (r_CPUCore_Execute_tick_will_fire) {
    when ((c_CPUCore_Execute_stash(135, 104) === 0.U)) {
      when (q_CPUCore_decode_q.io.deq.valid) {
        q_CPUCore_decode_q.io.deq.ready := true.B
        when (q_CPUCore_decode_q.io.deq.bits(13, 12) === MemOp.Load) {
          q_adapter_rd_req.io.enq.bits := 0.U
          q_adapter_rd_req.io.enq.valid := true.B
          c_CPUCore_Execute_stash := Cat(1.U, q_CPUCore_decode_q.io.deq.bits(146, 142), q_CPUCore_decode_q.io.deq.bits(9, 7), q_CPUCore_decode_q.io.deq.bits(109, 78), 0.U, 0.U)
        } .elsewhen (q_CPUCore_decode_q.io.deq.bits(13, 12) === MemOp.Store) {
          q_adapter_rd_req.io.enq.bits := 0.U
          q_adapter_rd_req.io.enq.valid := true.B
          c_CPUCore_Execute_stash := Cat(2.U, q_CPUCore_decode_q.io.deq.bits(146, 142), q_CPUCore_decode_q.io.deq.bits(9, 7), q_CPUCore_decode_q.io.deq.bits(109, 78), 0.U, 0.U)
        } .elsewhen (q_CPUCore_decode_q.io.deq.bits(13, 12) === MemOp.`None`) {
          q_CPUCore_wb_q.io.enq.bits := Cat(q_CPUCore_decode_q.io.deq.bits(11), q_CPUCore_decode_q.io.deq.bits(146, 142), 0.U)
          q_CPUCore_wb_q.io.enq.valid := true.B
          q_CPUCore_next_pc_q.io.enq.bits := 0.U
          q_CPUCore_next_pc_q.io.enq.valid := true.B
          c_CPUCore_Execute_stash := Cat(0.U, 0.U, 0.U, 0.U, 0.U, 0.U)
        }
      } .otherwise {
        c_CPUCore_Execute_stash := c_CPUCore_Execute_stash
      }
    } .otherwise {
      when ((c_CPUCore_Execute_stash(135, 104) === 1.U)) {
        when (q_adapter_rd_resp.io.deq.valid) {
          q_adapter_rd_resp.io.deq.ready := true.B
          q_CPUCore_wb_q.io.enq.bits := Cat(true.B, c_CPUCore_Execute_stash(103, 99), 0.U)
          q_CPUCore_wb_q.io.enq.valid := true.B
          q_CPUCore_next_pc_q.io.enq.bits := c_CPUCore_Execute_stash(31, 0)
          q_CPUCore_next_pc_q.io.enq.valid := true.B
          c_CPUCore_Execute_stash := Cat(0.U, 0.U, 0.U, 0.U, 0.U, 0.U)
        } .otherwise {
          c_CPUCore_Execute_stash := c_CPUCore_Execute_stash
        }
      } .otherwise {
        when ((c_CPUCore_Execute_stash(135, 104) === 2.U)) {
          when (q_adapter_rd_resp.io.deq.valid) {
            q_adapter_rd_resp.io.deq.ready := true.B
            q_adapter_wr_req.io.enq.bits := Cat(c_CPUCore_Execute_stash(63, 32), 0.U)
            q_adapter_wr_req.io.enq.valid := true.B
            q_CPUCore_wb_q.io.enq.bits := Cat(false.B, c_CPUCore_Execute_stash(103, 99), c_CPUCore_Execute_stash(63, 32))
            q_CPUCore_wb_q.io.enq.valid := true.B
            q_CPUCore_next_pc_q.io.enq.bits := c_CPUCore_Execute_stash(31, 0)
            q_CPUCore_next_pc_q.io.enq.valid := true.B
            c_CPUCore_Execute_stash := Cat(0.U, 0.U, 0.U, 0.U, 0.U, 0.U)
          } .otherwise {
            c_CPUCore_Execute_stash := c_CPUCore_Execute_stash
          }
        } .otherwise {
          c_CPUCore_Execute_stash := c_CPUCore_Execute_stash
        }
      }
    }
  }
  // Rule: CPUCore_Writeback.commit
  when (r_CPUCore_Writeback_commit_will_fire) {
    when (q_CPUCore_wb_q.io.deq.bits(37)) {
      c_CPUCore_Writeback_regfile := c_CPUCore_Writeback_regfile
    } .otherwise {
      c_CPUCore_Writeback_regfile := c_CPUCore_Writeback_regfile
    }
  }
  // Rule: CpuDmemAdapter.tick
  when (r_CpuDmemAdapter_tick_will_fire) {
    when ((c_CpuDmemAdapter_phase === 0.U)) {
      when (q_adapter_rd_req.io.deq.valid) {
        q_adapter_rd_req.io.deq.ready := true.B
        q_cpu_bus_req.io.enq.bits := Cat(false.B, q_adapter_rd_req.io.deq.bits, 0.U)
        q_cpu_bus_req.io.enq.valid := true.B
        c_CpuDmemAdapter_phase := 1.U
      } .otherwise {
        when (q_adapter_wr_req.io.deq.valid) {
          q_adapter_wr_req.io.deq.ready := true.B
          q_cpu_bus_req.io.enq.bits := Cat(true.B, q_adapter_wr_req.io.deq.bits(63, 32), q_adapter_wr_req.io.deq.bits(31, 0))
          q_cpu_bus_req.io.enq.valid := true.B
          c_CpuDmemAdapter_phase := 2.U
        } .otherwise {
          c_CpuDmemAdapter_phase := 0.U
        }
      }
    } .otherwise {
      when ((c_CpuDmemAdapter_phase === 1.U)) {
        when (q_cpu_bus_resp.io.deq.valid) {
          q_cpu_bus_resp.io.deq.ready := true.B
          q_adapter_rd_resp.io.enq.bits := q_cpu_bus_resp.io.deq.bits
          q_adapter_rd_resp.io.enq.valid := true.B
          c_CpuDmemAdapter_phase := 0.U
        } .otherwise {
          c_CpuDmemAdapter_phase := c_CpuDmemAdapter_phase
        }
      } .otherwise {
        when (q_cpu_bus_resp.io.deq.valid) {
          q_cpu_bus_resp.io.deq.ready := true.B
          c_CpuDmemAdapter_phase := 0.U
        } .otherwise {
          c_CpuDmemAdapter_phase := c_CpuDmemAdapter_phase
        }
      }
    }
  }
  // Rule: Xbar.tick
  when (r_Xbar_tick_will_fire) {
    when ((c_Xbar_xphase === 0.U)) {
      when (q_cpu_bus_req.io.deq.valid) {
        q_cpu_bus_req.io.deq.ready := true.B
        when ((q_cpu_bus_req.io.deq.bits(63, 32)(31, 28) === 8.U)) {
          q_mem_bus_req.io.enq.bits := q_cpu_bus_req.io.deq.bits
          q_mem_bus_req.io.enq.valid := true.B
          c_Xbar_xphase := 1.U
        } .otherwise {
          when ((q_cpu_bus_req.io.deq.bits(63, 32)(31, 28) === 1.U)) {
            q_uart_bus_req.io.enq.bits := q_cpu_bus_req.io.deq.bits
            q_uart_bus_req.io.enq.valid := true.B
            c_Xbar_xphase := 2.U
          } .otherwise {
            q_cpu_bus_resp.io.enq.bits := 0.U
            q_cpu_bus_resp.io.enq.valid := true.B
            c_Xbar_xphase := 0.U
          }
        }
      } .otherwise {
        c_Xbar_xphase := 0.U
      }
    } .otherwise {
      when ((c_Xbar_xphase === 1.U)) {
        when (q_mem_bus_resp.io.deq.valid) {
          q_mem_bus_resp.io.deq.ready := true.B
          q_cpu_bus_resp.io.enq.bits := q_mem_bus_resp.io.deq.bits
          q_cpu_bus_resp.io.enq.valid := true.B
          c_Xbar_xphase := 0.U
        } .otherwise {
          c_Xbar_xphase := c_Xbar_xphase
        }
      } .otherwise {
        when (q_uart_bus_resp.io.deq.valid) {
          q_uart_bus_resp.io.deq.ready := true.B
          q_cpu_bus_resp.io.enq.bits := q_uart_bus_resp.io.deq.bits
          q_cpu_bus_resp.io.enq.valid := true.B
          c_Xbar_xphase := 0.U
        } .otherwise {
          c_Xbar_xphase := c_Xbar_xphase
        }
      }
    }
  }
  // Rule: MemDevice.tick
  when (r_MemDevice_tick_will_fire) {
    when ((c_MemDevice_mphase === 0.U)) {
      when (q_mem_bus_req.io.deq.valid) {
        q_mem_bus_req.io.deq.ready := true.B
        when (q_mem_bus_req.io.deq.bits(64)) {
          io.q_dev_mem_write_req.bits := Cat(q_mem_bus_req.io.deq.bits(63, 32), q_mem_bus_req.io.deq.bits(31, 0))
          io.q_dev_mem_write_req.valid := true.B
          q_mem_bus_resp.io.enq.bits := 0.U
          q_mem_bus_resp.io.enq.valid := true.B
          c_MemDevice_mphase := 0.U
        } .otherwise {
          io.q_dev_mem_read_req.bits := q_mem_bus_req.io.deq.bits(63, 32)
          io.q_dev_mem_read_req.valid := true.B
          c_MemDevice_mphase := 1.U
        }
      } .otherwise {
        c_MemDevice_mphase := 0.U
      }
    } .otherwise {
      when (io.q_dev_mem_read_resp.valid) {
        io.q_dev_mem_read_resp.ready := true.B
        q_mem_bus_resp.io.enq.bits := io.q_dev_mem_read_resp.bits
        q_mem_bus_resp.io.enq.valid := true.B
        c_MemDevice_mphase := 0.U
      } .otherwise {
        c_MemDevice_mphase := c_MemDevice_mphase
      }
    }
  }
  // Rule: UartPhy_UartDevice.handle
  when (r_UartPhy_UartDevice_handle_will_fire) {
    when (q_uart_bus_req.io.deq.bits(64)) {
      q_UartPhy_tx_q.io.enq.bits := q_uart_bus_req.io.deq.bits(31, 0)(7, 0)
      q_UartPhy_tx_q.io.enq.valid := true.B
      q_uart_bus_resp.io.enq.bits := 0.U
      q_uart_bus_resp.io.enq.valid := true.B
    } .otherwise {
      when (q_UartPhy_rx_q.io.deq.valid) {
        q_UartPhy_rx_q.io.deq.ready := true.B
        q_uart_bus_resp.io.enq.bits := q_UartPhy_rx_q.io.deq.bits
        q_uart_bus_resp.io.enq.valid := true.B
      } .otherwise {
        q_uart_bus_resp.io.enq.bits := 0.U
        q_uart_bus_resp.io.enq.valid := true.B
      }
    }
  }
  // Rule: UartPhy_UartTx.tick
  when (r_UartPhy_UartTx_tick_will_fire) {
    io.q_UartPhy_tx_pin.bits := c_UartPhy_UartTx_sr(0, 0)
    io.q_UartPhy_tx_pin.valid := true.B
    when ((c_UartPhy_UartTx_bit_cnt === 0.U)) {
      when (io.q_UartPhy_cts_pin.valid) {
        io.q_UartPhy_cts_pin.ready := true.B
        when ((io.q_UartPhy_cts_pin.bits === 0.U)) {
          when (q_UartPhy_tx_q.io.deq.valid) {
            q_UartPhy_tx_q.io.deq.ready := true.B
            c_UartPhy_UartTx_sr := (512.U | (q_UartPhy_tx_q.io.deq.bits << 1.U))
            c_UartPhy_UartTx_baud_cnt := 33.U
            c_UartPhy_UartTx_bit_cnt := 10.U
          } .otherwise {
            c_UartPhy_UartTx_sr := c_UartPhy_UartTx_sr
            c_UartPhy_UartTx_baud_cnt := 0.U
            c_UartPhy_UartTx_bit_cnt := 0.U
          }
        } .otherwise {
          c_UartPhy_UartTx_sr := c_UartPhy_UartTx_sr
          c_UartPhy_UartTx_baud_cnt := 0.U
          c_UartPhy_UartTx_bit_cnt := 0.U
        }
      } .otherwise {
        when (q_UartPhy_tx_q.io.deq.valid) {
          q_UartPhy_tx_q.io.deq.ready := true.B
          c_UartPhy_UartTx_sr := (512.U | (q_UartPhy_tx_q.io.deq.bits << 1.U))
          c_UartPhy_UartTx_baud_cnt := 33.U
          c_UartPhy_UartTx_bit_cnt := 10.U
        } .otherwise {
          c_UartPhy_UartTx_sr := c_UartPhy_UartTx_sr
          c_UartPhy_UartTx_baud_cnt := 0.U
          c_UartPhy_UartTx_bit_cnt := 0.U
        }
      }
    } .otherwise {
      when ((c_UartPhy_UartTx_baud_cnt === 0.U)) {
        c_UartPhy_UartTx_sr := ((c_UartPhy_UartTx_sr >> 1.U) | 512.U)
        c_UartPhy_UartTx_baud_cnt := 33.U
        c_UartPhy_UartTx_bit_cnt := (c_UartPhy_UartTx_bit_cnt - 1.U)
      } .otherwise {
        c_UartPhy_UartTx_sr := c_UartPhy_UartTx_sr
        c_UartPhy_UartTx_baud_cnt := (c_UartPhy_UartTx_baud_cnt - 1.U)
        c_UartPhy_UartTx_bit_cnt := c_UartPhy_UartTx_bit_cnt
      }
    }
  }
  // Rule: UartPhy_UartRx.tick
  when (r_UartPhy_UartRx_tick_will_fire) {
    io.q_UartPhy_rts_pin.bits := 0.U
    io.q_UartPhy_rts_pin.valid := true.B
    when (io.q_UartPhy_rx_pin.valid) {
      io.q_UartPhy_rx_pin.ready := true.B
      when ((c_UartPhy_UartRx_rx_ph === 0.U)) {
        when ((io.q_UartPhy_rx_pin.bits === 0.U)) {
          c_UartPhy_UartRx_rx_ph := 1.U
          c_UartPhy_UartRx_rx_bc := (33.U >> 1.U)
          c_UartPhy_UartRx_rx_bn := 0.U
          c_UartPhy_UartRx_rx_sr := 0.U
        } .otherwise {
          c_UartPhy_UartRx_rx_ph := 0.U
          c_UartPhy_UartRx_rx_bc := 0.U
          c_UartPhy_UartRx_rx_bn := 0.U
          c_UartPhy_UartRx_rx_sr := 0.U
        }
      } .otherwise {
        when ((c_UartPhy_UartRx_rx_bc === 0.U)) {
          when ((c_UartPhy_UartRx_rx_ph === 1.U)) {
            when ((io.q_UartPhy_rx_pin.bits === 0.U)) {
              c_UartPhy_UartRx_rx_ph := 2.U
              c_UartPhy_UartRx_rx_bc := 33.U
              c_UartPhy_UartRx_rx_bn := 0.U
              c_UartPhy_UartRx_rx_sr := 0.U
            } .otherwise {
              c_UartPhy_UartRx_rx_ph := 0.U
              c_UartPhy_UartRx_rx_bc := 0.U
              c_UartPhy_UartRx_rx_bn := 0.U
              c_UartPhy_UartRx_rx_sr := 0.U
            }
          } .otherwise {
            when ((c_UartPhy_UartRx_rx_ph === 2.U)) {
              when ((c_UartPhy_UartRx_rx_bn === 7.U)) {
                q_UartPhy_rx_q.io.enq.bits := ((c_UartPhy_UartRx_rx_sr >> 1.U) | (io.q_UartPhy_rx_pin.bits << 7.U))
                q_UartPhy_rx_q.io.enq.valid := true.B
                c_UartPhy_UartRx_rx_ph := 3.U
                c_UartPhy_UartRx_rx_bc := 33.U
                c_UartPhy_UartRx_rx_bn := 0.U
                c_UartPhy_UartRx_rx_sr := 0.U
              } .otherwise {
                c_UartPhy_UartRx_rx_ph := 2.U
                c_UartPhy_UartRx_rx_bc := 33.U
                c_UartPhy_UartRx_rx_bn := (c_UartPhy_UartRx_rx_bn + 1.U)
                c_UartPhy_UartRx_rx_sr := ((c_UartPhy_UartRx_rx_sr >> 1.U) | (io.q_UartPhy_rx_pin.bits << 7.U))
              }
            } .otherwise {
              c_UartPhy_UartRx_rx_ph := 0.U
              c_UartPhy_UartRx_rx_bc := 0.U
              c_UartPhy_UartRx_rx_bn := 0.U
              c_UartPhy_UartRx_rx_sr := 0.U
            }
          }
        } .otherwise {
          c_UartPhy_UartRx_rx_ph := c_UartPhy_UartRx_rx_ph
          c_UartPhy_UartRx_rx_bc := (c_UartPhy_UartRx_rx_bc - 1.U)
          c_UartPhy_UartRx_rx_bn := c_UartPhy_UartRx_rx_bn
          c_UartPhy_UartRx_rx_sr := c_UartPhy_UartRx_rx_sr
        }
      }
    } .otherwise {
      c_UartPhy_UartRx_rx_ph := c_UartPhy_UartRx_rx_ph
      c_UartPhy_UartRx_rx_bc := c_UartPhy_UartRx_rx_bc
      c_UartPhy_UartRx_rx_bn := c_UartPhy_UartRx_rx_bn
      c_UartPhy_UartRx_rx_sr := c_UartPhy_UartRx_rx_sr
    }
  }

  q_CPUCore_fetch_q.io.deq.ready := r_CPUCore_Decode_crack_will_fire
  q_CPUCore_wb_q.io.deq.ready := r_CPUCore_Writeback_commit_will_fire
  q_CPUCore_next_pc_q.io.deq.ready := r_CPUCore_Fetch_tick_will_fire
  io.q_CPUCore_imem_read_resp.ready := r_CPUCore_Fetch_tick_will_fire
  q_uart_bus_req.io.deq.ready := r_UartPhy_UartDevice_handle_will_fire
}
