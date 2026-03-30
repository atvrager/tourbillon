package tbn

import chisel3._
import chisel3.util._

class DmaDesc extends Bundle {
  val op = UInt(8.W)
  val addr = UInt(32.W)
  val len = UInt(16.W)
}

class TlA extends Bundle {
  val opcode = UInt(3.W)
  val size = UInt(3.W)
  val source = UInt(8.W)
  val address = UInt(32.W)
  val mask = UInt(16.W)
  val data = UInt(128.W)
}

class TlD extends Bundle {
  val opcode = UInt(3.W)
  val size = UInt(3.W)
  val source = UInt(8.W)
  val data = UInt(128.W)
  val error = Bool()
}

class Spi2TLULV2 extends Module {
  val io = IO(new Bundle {
    val q_mosi_pin = Flipped(Decoupled(UInt(1.W)))
    val q_miso_pin = Decoupled(UInt(1.W))
    val q_tl_a = Decoupled(UInt(190.W))
    val q_tl_d = Flipped(Decoupled(UInt(143.W)))
  })

  val OP_READ = 1.U
  val OP_WRITE = 2.U
  val TL_GET = 4.U
  val TL_PUT_FULL = 0.U
  val TL_SIZE = 4.U

  val c_SpiByteAssembler_sr = RegInit(0.U(8.W))
  val c_SpiByteAssembler_count = RegInit(0.U(4.W))
  val c_SpiFrameParser_phase = RegInit(0.U(4.W))
  val c_SpiFrameParser_op_reg = RegInit(0.U(8.W))
  val c_SpiFrameParser_addr_reg = RegInit(0.U(32.W))
  val c_SpiFrameParser_len_reg = RegInit(0.U(16.W))
  val c_SpiFrameParser_wr_remain = RegInit(0.U(32.W))
  val c_SpiBulkDeserializer_word_buf = RegInit(0.U(128.W))
  val c_SpiBulkDeserializer_byte_idx = RegInit(0.U(5.W))
  val c_SpiMisoShifter_sr = RegInit(0.U(8.W))
  val c_SpiMisoShifter_count = RegInit(0.U(4.W))
  val c_DmaEngine_phase = RegInit(0.U(4.W))
  val c_DmaEngine_dma_addr = RegInit(0.U(32.W))
  val c_DmaEngine_dma_len = RegInit(0.U(16.W))
  val c_DmaEngine_beat_cnt = RegInit(0.U(16.W))
  val c_DmaEngine_word_acc = RegInit(0.U(128.W))
  val c_DmaEngine_byte_ptr = RegInit(0.U(5.W))

  // Queue: spi_byte_q
  val q_spi_byte_q = Module(new Queue(UInt(8.W), 2))

  // Queue: miso_byte_q
  val q_miso_byte_q = Module(new Queue(UInt(8.W), 2))

  // AsyncQueue: desc_cdc
  val q_desc_cdc = Module(new TbnAsyncFifo(56, 2))

  // AsyncQueue: wr_data_cdc
  val q_wr_data_cdc = Module(new TbnAsyncFifo(8, 16))

  // AsyncQueue: rd_data_cdc
  val q_rd_data_cdc = Module(new TbnAsyncFifo(128, 4))

  val r_SpiByteAssembler_tick_can_fire = io.q_mosi_pin.valid
  val r_SpiByteAssembler_tick_will_fire = r_SpiByteAssembler_tick_can_fire

  val r_SpiFrameParser_tick_can_fire = true.B
  val r_SpiFrameParser_tick_will_fire = r_SpiFrameParser_tick_can_fire

  val r_SpiBulkDeserializer_tick_can_fire = true.B
  val r_SpiBulkDeserializer_tick_will_fire = r_SpiBulkDeserializer_tick_can_fire

  val r_SpiMisoShifter_tick_can_fire = true.B
  val r_SpiMisoShifter_tick_will_fire = r_SpiMisoShifter_tick_can_fire

  val r_DmaEngine_tick_can_fire = true.B
  val r_DmaEngine_tick_will_fire = r_DmaEngine_tick_can_fire

  c_SpiByteAssembler_sr := c_SpiByteAssembler_sr
  c_SpiByteAssembler_count := c_SpiByteAssembler_count
  c_SpiFrameParser_phase := c_SpiFrameParser_phase
  c_SpiFrameParser_op_reg := c_SpiFrameParser_op_reg
  c_SpiFrameParser_addr_reg := c_SpiFrameParser_addr_reg
  c_SpiFrameParser_len_reg := c_SpiFrameParser_len_reg
  c_SpiFrameParser_wr_remain := c_SpiFrameParser_wr_remain
  c_SpiBulkDeserializer_word_buf := c_SpiBulkDeserializer_word_buf
  c_SpiBulkDeserializer_byte_idx := c_SpiBulkDeserializer_byte_idx
  c_SpiMisoShifter_sr := c_SpiMisoShifter_sr
  c_SpiMisoShifter_count := c_SpiMisoShifter_count
  c_DmaEngine_phase := c_DmaEngine_phase
  c_DmaEngine_dma_addr := c_DmaEngine_dma_addr
  c_DmaEngine_dma_len := c_DmaEngine_dma_len
  c_DmaEngine_beat_cnt := c_DmaEngine_beat_cnt
  c_DmaEngine_word_acc := c_DmaEngine_word_acc
  c_DmaEngine_byte_ptr := c_DmaEngine_byte_ptr
  io.q_miso_pin.valid := false.B
  io.q_miso_pin.bits := 0.U
  io.q_tl_a.valid := false.B
  io.q_tl_a.bits := 0.U
  q_spi_byte_q.io.enq.valid := false.B
  q_spi_byte_q.io.enq.bits := 0.U
  q_miso_byte_q.io.enq.valid := false.B
  q_miso_byte_q.io.enq.bits := 0.U
  q_desc_cdc.io.enq.valid := false.B
  q_desc_cdc.io.enq.bits := 0.U
  q_wr_data_cdc.io.enq.valid := false.B
  q_wr_data_cdc.io.enq.bits := 0.U
  q_rd_data_cdc.io.enq.valid := false.B
  q_rd_data_cdc.io.enq.bits := 0.U
  io.q_tl_d.ready := false.B
  q_miso_byte_q.io.deq.ready := false.B
  q_rd_data_cdc.io.deq.ready := false.B
  q_desc_cdc.io.deq.ready := false.B
  q_spi_byte_q.io.deq.ready := false.B
  q_wr_data_cdc.io.deq.ready := false.B

  // Rule: SpiByteAssembler.tick
  when (r_SpiByteAssembler_tick_will_fire) {
    when ((c_SpiByteAssembler_count === 7.U)) {
      q_spi_byte_q.io.enq.bits := ((c_SpiByteAssembler_sr << 1.U) | io.q_mosi_pin.bits)
      q_spi_byte_q.io.enq.valid := true.B
      c_SpiByteAssembler_sr := 0.U
      c_SpiByteAssembler_count := 0.U
    } .otherwise {
      c_SpiByteAssembler_sr := ((c_SpiByteAssembler_sr << 1.U) | io.q_mosi_pin.bits)
      c_SpiByteAssembler_count := (c_SpiByteAssembler_count + 1.U)
    }
  }
  // Rule: SpiFrameParser.tick
  when (r_SpiFrameParser_tick_will_fire) {
    when ((c_SpiFrameParser_phase === 7.U)) {
      q_desc_cdc.io.enq.bits := Cat(c_SpiFrameParser_op_reg, c_SpiFrameParser_addr_reg, c_SpiFrameParser_len_reg)
      q_desc_cdc.io.enq.valid := true.B
      when ((c_SpiFrameParser_op_reg === 2.U)) {
        c_SpiFrameParser_phase := 8.U
        c_SpiFrameParser_op_reg := c_SpiFrameParser_op_reg
        c_SpiFrameParser_addr_reg := c_SpiFrameParser_addr_reg
        c_SpiFrameParser_len_reg := c_SpiFrameParser_len_reg
        c_SpiFrameParser_wr_remain := ((c_SpiFrameParser_len_reg + 1.U) << 4.U)
      } .otherwise {
        c_SpiFrameParser_phase := 9.U
        c_SpiFrameParser_op_reg := c_SpiFrameParser_op_reg
        c_SpiFrameParser_addr_reg := c_SpiFrameParser_addr_reg
        c_SpiFrameParser_len_reg := c_SpiFrameParser_len_reg
        c_SpiFrameParser_wr_remain := 0.U
      }
    } .otherwise {
      when ((c_SpiFrameParser_phase === 8.U)) {
        when ((c_SpiFrameParser_wr_remain === 0.U)) {
          c_SpiFrameParser_phase := 9.U
          c_SpiFrameParser_op_reg := c_SpiFrameParser_op_reg
          c_SpiFrameParser_addr_reg := c_SpiFrameParser_addr_reg
          c_SpiFrameParser_len_reg := c_SpiFrameParser_len_reg
          c_SpiFrameParser_wr_remain := 0.U
        } .otherwise {
          when (q_spi_byte_q.io.deq.valid) {
            q_spi_byte_q.io.deq.ready := true.B
            q_wr_data_cdc.io.enq.bits := q_spi_byte_q.io.deq.bits
            q_wr_data_cdc.io.enq.valid := true.B
            c_SpiFrameParser_phase := 8.U
            c_SpiFrameParser_op_reg := c_SpiFrameParser_op_reg
            c_SpiFrameParser_addr_reg := c_SpiFrameParser_addr_reg
            c_SpiFrameParser_len_reg := c_SpiFrameParser_len_reg
            c_SpiFrameParser_wr_remain := (c_SpiFrameParser_wr_remain - 1.U)
          } .otherwise {
            c_SpiFrameParser_phase := 8.U
            c_SpiFrameParser_op_reg := c_SpiFrameParser_op_reg
            c_SpiFrameParser_addr_reg := c_SpiFrameParser_addr_reg
            c_SpiFrameParser_len_reg := c_SpiFrameParser_len_reg
            c_SpiFrameParser_wr_remain := c_SpiFrameParser_wr_remain
          }
        }
      } .otherwise {
        when ((c_SpiFrameParser_phase === 9.U)) {
          c_SpiFrameParser_phase := 9.U
          c_SpiFrameParser_op_reg := c_SpiFrameParser_op_reg
          c_SpiFrameParser_addr_reg := c_SpiFrameParser_addr_reg
          c_SpiFrameParser_len_reg := c_SpiFrameParser_len_reg
          c_SpiFrameParser_wr_remain := 0.U
        } .otherwise {
          when (q_spi_byte_q.io.deq.valid) {
            q_spi_byte_q.io.deq.ready := true.B
            when ((c_SpiFrameParser_phase === 0.U)) {
              c_SpiFrameParser_phase := 1.U
              c_SpiFrameParser_op_reg := q_spi_byte_q.io.deq.bits
              c_SpiFrameParser_addr_reg := 0.U
              c_SpiFrameParser_len_reg := 0.U
              c_SpiFrameParser_wr_remain := 0.U
            } .otherwise {
              when ((c_SpiFrameParser_phase === 1.U)) {
                c_SpiFrameParser_phase := 2.U
                c_SpiFrameParser_op_reg := c_SpiFrameParser_op_reg
                c_SpiFrameParser_addr_reg := (q_spi_byte_q.io.deq.bits << 24.U)
                c_SpiFrameParser_len_reg := 0.U
                c_SpiFrameParser_wr_remain := 0.U
              } .otherwise {
                when ((c_SpiFrameParser_phase === 2.U)) {
                  c_SpiFrameParser_phase := 3.U
                  c_SpiFrameParser_op_reg := c_SpiFrameParser_op_reg
                  c_SpiFrameParser_addr_reg := (c_SpiFrameParser_addr_reg | (q_spi_byte_q.io.deq.bits << 16.U))
                  c_SpiFrameParser_len_reg := 0.U
                  c_SpiFrameParser_wr_remain := 0.U
                } .otherwise {
                  when ((c_SpiFrameParser_phase === 3.U)) {
                    c_SpiFrameParser_phase := 4.U
                    c_SpiFrameParser_op_reg := c_SpiFrameParser_op_reg
                    c_SpiFrameParser_addr_reg := (c_SpiFrameParser_addr_reg | (q_spi_byte_q.io.deq.bits << 8.U))
                    c_SpiFrameParser_len_reg := 0.U
                    c_SpiFrameParser_wr_remain := 0.U
                  } .otherwise {
                    when ((c_SpiFrameParser_phase === 4.U)) {
                      c_SpiFrameParser_phase := 5.U
                      c_SpiFrameParser_op_reg := c_SpiFrameParser_op_reg
                      c_SpiFrameParser_addr_reg := (c_SpiFrameParser_addr_reg | q_spi_byte_q.io.deq.bits)
                      c_SpiFrameParser_len_reg := 0.U
                      c_SpiFrameParser_wr_remain := 0.U
                    } .otherwise {
                      when ((c_SpiFrameParser_phase === 5.U)) {
                        c_SpiFrameParser_phase := 6.U
                        c_SpiFrameParser_op_reg := c_SpiFrameParser_op_reg
                        c_SpiFrameParser_addr_reg := c_SpiFrameParser_addr_reg
                        c_SpiFrameParser_len_reg := (q_spi_byte_q.io.deq.bits << 8.U)
                        c_SpiFrameParser_wr_remain := 0.U
                      } .otherwise {
                        c_SpiFrameParser_phase := 7.U
                        c_SpiFrameParser_op_reg := c_SpiFrameParser_op_reg
                        c_SpiFrameParser_addr_reg := c_SpiFrameParser_addr_reg
                        c_SpiFrameParser_len_reg := (c_SpiFrameParser_len_reg | q_spi_byte_q.io.deq.bits)
                        c_SpiFrameParser_wr_remain := 0.U
                      }
                    }
                  }
                }
              }
            }
          } .otherwise {
            c_SpiFrameParser_phase := c_SpiFrameParser_phase
            c_SpiFrameParser_op_reg := c_SpiFrameParser_op_reg
            c_SpiFrameParser_addr_reg := c_SpiFrameParser_addr_reg
            c_SpiFrameParser_len_reg := c_SpiFrameParser_len_reg
            c_SpiFrameParser_wr_remain := c_SpiFrameParser_wr_remain
          }
        }
      }
    }
  }
  // Rule: SpiBulkDeserializer.tick
  when (r_SpiBulkDeserializer_tick_will_fire) {
    when ((c_SpiBulkDeserializer_byte_idx === 0.U)) {
      when (q_rd_data_cdc.io.deq.valid) {
        q_rd_data_cdc.io.deq.ready := true.B
        q_miso_byte_q.io.enq.bits := q_rd_data_cdc.io.deq.bits(7, 0)
        q_miso_byte_q.io.enq.valid := true.B
        c_SpiBulkDeserializer_word_buf := (q_rd_data_cdc.io.deq.bits >> 8.U)
        c_SpiBulkDeserializer_byte_idx := 1.U
      } .otherwise {
        c_SpiBulkDeserializer_word_buf := 0.U
        c_SpiBulkDeserializer_byte_idx := 0.U
      }
    } .otherwise {
      q_miso_byte_q.io.enq.bits := c_SpiBulkDeserializer_word_buf(7, 0)
      q_miso_byte_q.io.enq.valid := true.B
      when ((c_SpiBulkDeserializer_byte_idx === 15.U)) {
        c_SpiBulkDeserializer_word_buf := 0.U
        c_SpiBulkDeserializer_byte_idx := 0.U
      } .otherwise {
        c_SpiBulkDeserializer_word_buf := (c_SpiBulkDeserializer_word_buf >> 8.U)
        c_SpiBulkDeserializer_byte_idx := (c_SpiBulkDeserializer_byte_idx + 1.U)
      }
    }
  }
  // Rule: SpiMisoShifter.tick
  when (r_SpiMisoShifter_tick_will_fire) {
    when ((c_SpiMisoShifter_count === 0.U)) {
      when (q_miso_byte_q.io.deq.valid) {
        q_miso_byte_q.io.deq.ready := true.B
        io.q_miso_pin.bits := q_miso_byte_q.io.deq.bits(7, 7)
        io.q_miso_pin.valid := true.B
        c_SpiMisoShifter_sr := (q_miso_byte_q.io.deq.bits << 1.U)
        c_SpiMisoShifter_count := 7.U
      } .otherwise {
        io.q_miso_pin.bits := 0.U
        io.q_miso_pin.valid := true.B
        c_SpiMisoShifter_sr := 0.U
        c_SpiMisoShifter_count := 0.U
      }
    } .otherwise {
      io.q_miso_pin.bits := c_SpiMisoShifter_sr(7, 7)
      io.q_miso_pin.valid := true.B
      c_SpiMisoShifter_sr := (c_SpiMisoShifter_sr << 1.U)
      c_SpiMisoShifter_count := (c_SpiMisoShifter_count - 1.U)
    }
  }
  // Rule: DmaEngine.tick
  when (r_DmaEngine_tick_will_fire) {
    when ((c_DmaEngine_phase === 0.U)) {
      when (q_desc_cdc.io.deq.valid) {
        q_desc_cdc.io.deq.ready := true.B
        when ((q_desc_cdc.io.deq.bits(55, 48) === 1.U)) {
          c_DmaEngine_phase := 1.U
        } .otherwise {
          when ((q_desc_cdc.io.deq.bits(55, 48) === 2.U)) {
            c_DmaEngine_phase := 3.U
          } .otherwise {
            c_DmaEngine_phase := 0.U
          }
        }
        c_DmaEngine_dma_addr := q_desc_cdc.io.deq.bits(47, 16)
        c_DmaEngine_dma_len := q_desc_cdc.io.deq.bits(15, 0)
        c_DmaEngine_beat_cnt := 0.U
        c_DmaEngine_word_acc := 0.U
        c_DmaEngine_byte_ptr := 0.U
      } .otherwise {
        c_DmaEngine_phase := 0.U
        c_DmaEngine_dma_addr := c_DmaEngine_dma_addr
        c_DmaEngine_dma_len := c_DmaEngine_dma_len
        c_DmaEngine_beat_cnt := c_DmaEngine_beat_cnt
        c_DmaEngine_word_acc := c_DmaEngine_word_acc
        c_DmaEngine_byte_ptr := c_DmaEngine_byte_ptr
      }
    } .otherwise {
      when ((c_DmaEngine_phase === 1.U)) {
        io.q_tl_a.bits := Cat(4.U, 4.U, 0.U, (c_DmaEngine_dma_addr + (c_DmaEngine_beat_cnt << 4.U)), 65535.U, 0.U)
        io.q_tl_a.valid := true.B
        c_DmaEngine_phase := 2.U
        c_DmaEngine_dma_addr := c_DmaEngine_dma_addr
        c_DmaEngine_dma_len := c_DmaEngine_dma_len
        c_DmaEngine_beat_cnt := c_DmaEngine_beat_cnt
        c_DmaEngine_word_acc := c_DmaEngine_word_acc
        c_DmaEngine_byte_ptr := c_DmaEngine_byte_ptr
      } .otherwise {
        when ((c_DmaEngine_phase === 2.U)) {
          when (io.q_tl_d.valid) {
            io.q_tl_d.ready := true.B
            q_rd_data_cdc.io.enq.bits := io.q_tl_d.bits(128, 1)
            q_rd_data_cdc.io.enq.valid := true.B
            when ((c_DmaEngine_beat_cnt === c_DmaEngine_dma_len)) {
              c_DmaEngine_phase := 0.U
            } .otherwise {
              c_DmaEngine_phase := 1.U
            }
            c_DmaEngine_dma_addr := c_DmaEngine_dma_addr
            c_DmaEngine_dma_len := c_DmaEngine_dma_len
            c_DmaEngine_beat_cnt := (c_DmaEngine_beat_cnt + 1.U)
            c_DmaEngine_word_acc := c_DmaEngine_word_acc
            c_DmaEngine_byte_ptr := c_DmaEngine_byte_ptr
          } .otherwise {
            c_DmaEngine_phase := 2.U
            c_DmaEngine_dma_addr := c_DmaEngine_dma_addr
            c_DmaEngine_dma_len := c_DmaEngine_dma_len
            c_DmaEngine_beat_cnt := c_DmaEngine_beat_cnt
            c_DmaEngine_word_acc := c_DmaEngine_word_acc
            c_DmaEngine_byte_ptr := c_DmaEngine_byte_ptr
          }
        } .otherwise {
          when ((c_DmaEngine_phase === 3.U)) {
            when (q_wr_data_cdc.io.deq.valid) {
              q_wr_data_cdc.io.deq.ready := true.B
              when ((c_DmaEngine_byte_ptr === 15.U)) {
                c_DmaEngine_phase := 4.U
                c_DmaEngine_dma_addr := c_DmaEngine_dma_addr
                c_DmaEngine_dma_len := c_DmaEngine_dma_len
                c_DmaEngine_beat_cnt := c_DmaEngine_beat_cnt
                c_DmaEngine_word_acc := (c_DmaEngine_word_acc | (q_wr_data_cdc.io.deq.bits << (c_DmaEngine_byte_ptr << 3.U)))
                c_DmaEngine_byte_ptr := 0.U
              } .otherwise {
                c_DmaEngine_phase := 3.U
                c_DmaEngine_dma_addr := c_DmaEngine_dma_addr
                c_DmaEngine_dma_len := c_DmaEngine_dma_len
                c_DmaEngine_beat_cnt := c_DmaEngine_beat_cnt
                c_DmaEngine_word_acc := (c_DmaEngine_word_acc | (q_wr_data_cdc.io.deq.bits << (c_DmaEngine_byte_ptr << 3.U)))
                c_DmaEngine_byte_ptr := (c_DmaEngine_byte_ptr + 1.U)
              }
            } .otherwise {
              c_DmaEngine_phase := 3.U
              c_DmaEngine_dma_addr := c_DmaEngine_dma_addr
              c_DmaEngine_dma_len := c_DmaEngine_dma_len
              c_DmaEngine_beat_cnt := c_DmaEngine_beat_cnt
              c_DmaEngine_word_acc := c_DmaEngine_word_acc
              c_DmaEngine_byte_ptr := c_DmaEngine_byte_ptr
            }
          } .otherwise {
            when ((c_DmaEngine_phase === 4.U)) {
              io.q_tl_a.bits := Cat(0.U, 4.U, 0.U, (c_DmaEngine_dma_addr + (c_DmaEngine_beat_cnt << 4.U)), 65535.U, c_DmaEngine_word_acc)
              io.q_tl_a.valid := true.B
              c_DmaEngine_phase := 5.U
              c_DmaEngine_dma_addr := c_DmaEngine_dma_addr
              c_DmaEngine_dma_len := c_DmaEngine_dma_len
              c_DmaEngine_beat_cnt := c_DmaEngine_beat_cnt
              c_DmaEngine_word_acc := 0.U
              c_DmaEngine_byte_ptr := 0.U
            } .otherwise {
              when (io.q_tl_d.valid) {
                io.q_tl_d.ready := true.B
                when ((c_DmaEngine_beat_cnt === c_DmaEngine_dma_len)) {
                  c_DmaEngine_phase := 0.U
                } .otherwise {
                  c_DmaEngine_phase := 3.U
                }
                c_DmaEngine_dma_addr := c_DmaEngine_dma_addr
                c_DmaEngine_dma_len := c_DmaEngine_dma_len
                c_DmaEngine_beat_cnt := (c_DmaEngine_beat_cnt + 1.U)
                c_DmaEngine_word_acc := 0.U
                c_DmaEngine_byte_ptr := 0.U
              } .otherwise {
                c_DmaEngine_phase := 5.U
                c_DmaEngine_dma_addr := c_DmaEngine_dma_addr
                c_DmaEngine_dma_len := c_DmaEngine_dma_len
                c_DmaEngine_beat_cnt := c_DmaEngine_beat_cnt
                c_DmaEngine_word_acc := c_DmaEngine_word_acc
                c_DmaEngine_byte_ptr := c_DmaEngine_byte_ptr
              }
            }
          }
        }
      }
    }
  }

  io.q_mosi_pin.ready := r_SpiByteAssembler_tick_will_fire
}
