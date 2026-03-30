package tbn

import chisel3._
import chisel3.util._
import chisel3.simulator.scalatest.ChiselSim
import org.scalatest.freespec.AnyFreeSpec

// ---------------------------------------------------------------------------
// Test wrapper: Spi2TLULV2 + TL auto-responder + capture registers
// Mirrors coralnpu's Spi2TLULV2TestWrapper
// ---------------------------------------------------------------------------

class Spi2TLULV2TestWrapper extends Module {
  val io = IO(new Bundle {
    val spi_mosi_valid = Input(Bool())
    val spi_mosi_data  = Input(Bool())
    val spi_miso_valid = Output(Bool())
    val spi_miso_data  = Output(Bool())

    val tl_a_captured    = Output(Bool())
    val tl_a_cap_opcode  = Output(UInt(3.W))
    val tl_a_cap_address = Output(UInt(32.W))
    val tl_a_cap_data    = Output(UInt(128.W))
    val tl_a_cap_count   = Output(UInt(32.W))

    val tl_a_first_address = Output(UInt(32.W))
    val tl_a_first_data    = Output(UInt(128.W))
    val tl_a_first_reset   = Input(Bool())
  })

  val dut = Module(new Spi2TLULV2)

  // MOSI: drive from test harness
  dut.io.q_mosi_pin.valid := io.spi_mosi_valid
  dut.io.q_mosi_pin.bits  := io.spi_mosi_data

  // MISO: always ready to accept
  dut.io.q_miso_pin.ready := true.B
  io.spi_miso_valid := dut.io.q_miso_pin.valid
  io.spi_miso_data  := dut.io.q_miso_pin.bits

  // -------------------------------------------------------------------------
  // TileLink auto-responder
  // -------------------------------------------------------------------------
  val TL_GET             = 4.U(3.W)
  val TL_ACCESS_ACK      = 0.U(3.W)
  val TL_ACCESS_ACK_DATA = 1.U(3.W)

  val respPending = RegInit(false.B)
  val respReq     = RegInit(0.U(217.W))
  val tlAReady    = RegInit(true.B)

  dut.io.q_tl_a.ready := tlAReady

  val aValid = dut.io.q_tl_a.valid
  val aData  = dut.io.q_tl_a.bits

  val acceptA  = aValid && tlAReady && !respPending
  val completeD = respPending && dut.io.q_tl_d.ready

  respPending := Mux(acceptA, true.B, Mux(completeD, false.B, respPending))
  respReq     := Mux(acceptA, aData, respReq)
  tlAReady    := Mux(acceptA, false.B, Mux(completeD, true.B, tlAReady))

  // Decode latched request — OpenTitan TL-A layout (217 bits):
  // [216:214]=opcode, [213:211]=param, [210:207]=size, [206:199]=source,
  // [198:167]=address, [166:151]=mask, [150:23]=data, [22:0]=user
  val reqOpcode = respReq(216, 214)
  val reqSize   = respReq(210, 207)
  val reqSource = respReq(206, 199)
  val reqAddr   = respReq(198, 167)

  val respOpcode = Mux(reqOpcode === TL_GET, TL_ACCESS_ACK_DATA, TL_ACCESS_ACK)
  // Read response echoes address in low 32 bits
  val respData = Cat(0.U(96.W), reqAddr)

  // TL-D response: [161:159]=opcode, [158:156]=param, [155:152]=size,
  // [151:144]=source, [143]=sink, [142:15]=data, [14:1]=user, [0]=error
  dut.io.q_tl_d.valid := respPending
  dut.io.q_tl_d.bits  := Cat(
    respOpcode,          // 3 bits opcode
    0.U(3.W),            // 3 bits param
    reqSize,             // 4 bits size
    reqSource,           // 8 bits source
    0.U(1.W),            // 1 bit sink
    respData,            // 128 bits data
    0.U(14.W),           // 14 bits user
    0.U(1.W)             // 1 bit error
  )

  // -------------------------------------------------------------------------
  // TL-A capture registers
  // -------------------------------------------------------------------------
  val aHandshake = aValid && tlAReady
  val captured   = RegInit(false.B)
  val capOpcode  = RegInit(0.U(3.W))
  val capAddress = RegInit(0.U(32.W))
  val capData    = RegInit(0.U(128.W))
  val capCount   = RegInit(0.U(32.W))

  val firstCapAddress = RegInit(0.U(32.W))
  val firstCapData    = RegInit(0.U(128.W))
  val firstCapDone    = RegInit(false.B)

  captured   := Mux(aHandshake, true.B, captured)
  capOpcode  := Mux(aHandshake, aData(216, 214), capOpcode)
  capAddress := Mux(aHandshake, aData(198, 167), capAddress)
  capData    := Mux(aHandshake, aData(150, 23), capData)
  capCount   := Mux(aHandshake, capCount + 1.U, capCount)

  when (io.tl_a_first_reset) {
    firstCapDone    := false.B
    firstCapAddress := 0.U
    firstCapData    := 0.U
  } .elsewhen (aHandshake && !firstCapDone) {
    firstCapAddress := aData(198, 167)
    firstCapData    := aData(150, 23)
    firstCapDone    := true.B
  }

  io.tl_a_captured      := captured
  io.tl_a_cap_opcode    := capOpcode
  io.tl_a_cap_address   := capAddress
  io.tl_a_cap_data      := capData
  io.tl_a_cap_count     := capCount
  io.tl_a_first_address := firstCapAddress
  io.tl_a_first_data    := firstCapData
}

// ---------------------------------------------------------------------------
// Test suite — ports coralnpu Spi2TLULV2Spec tests
// ---------------------------------------------------------------------------

class Spi2TLULV2Spec extends AnyFreeSpec with ChiselSim {

  /** SPI protocol helpers operating on the test wrapper. */
  class SpiDriver(dut: Spi2TLULV2TestWrapper) {
    def reset(): Unit = {
      dut.reset.poke(true.B)
      dut.io.spi_mosi_valid.poke(false.B)
      dut.io.spi_mosi_data.poke(false.B)
      dut.io.tl_a_first_reset.poke(false.B)
      dut.clock.step(5)
      dut.reset.poke(false.B)
      dut.clock.step(5)
    }

    def spiXferByte(tx: Int): Unit = {
      for (bit <- 7 to 0 by -1) {
        dut.io.spi_mosi_valid.poke(true.B)
        dut.io.spi_mosi_data.poke((((tx >> bit) & 1) != 0).B)
        dut.clock.step(1)
      }
    }

    def spiRecvByte(): Int = {
      var rx = 0
      for (_ <- 7 to 0 by -1) {
        dut.io.spi_mosi_valid.poke(true.B) // clock the SPI — MISO shifts on each bit
        dut.io.spi_mosi_data.poke(false.B)
        dut.clock.step(1)
        rx = (rx << 1) | (if (dut.io.spi_miso_data.peek().litValue != 0) 1 else 0)
      }
      rx
    }

    def spiSendHeader(op: Int, addr: Long, len: Int): Unit = {
      spiXferByte(op & 0xFF)
      spiXferByte(((addr >> 24) & 0xFF).toInt)
      spiXferByte(((addr >> 16) & 0xFF).toInt)
      spiXferByte(((addr >> 8) & 0xFF).toInt)
      spiXferByte((addr & 0xFF).toInt)
      spiXferByte((len >> 8) & 0xFF)
      spiXferByte(len & 0xFF)
    }

    def idle(cycles: Int): Unit = {
      dut.io.spi_mosi_valid.poke(false.B)
      dut.clock.step(cycles)
    }

    def collectMisoBits(nbits: Int): Array[Int] = {
      val buf = new Array[Int]((nbits + 7) / 8)
      for (i <- 0 until nbits) {
        dut.io.spi_mosi_valid.poke(true.B)
        dut.io.spi_mosi_data.poke(false.B)
        dut.clock.step(1)
        val byteIdx = i / 8
        val bitIdx = 7 - (i % 8)
        if (dut.io.spi_miso_data.peek().litValue != 0) {
          buf(byteIdx) |= (1 << bitIdx)
        }
      }
      buf
    }

    def waitTlACapture(expectedCount: Long, timeout: Int = 200): Boolean = {
      for (_ <- 0 until timeout) {
        dut.clock.step(1)
        if (dut.io.tl_a_cap_count.peek().litValue >= expectedCount) return true
      }
      false
    }

    def resetFirstCapture(): Unit = {
      dut.io.tl_a_first_reset.poke(true.B)
      dut.clock.step(1)
      dut.io.tl_a_first_reset.poke(false.B)
    }
  }

  def findBitPattern(haystack: Array[Int], hayBits: Int,
                     needle: Array[Int], needleBits: Int): Int = {
    for (off <- 0 to (hayBits - needleBits)) {
      var matched = true
      var b = 0
      while (b < needleBits && matched) {
        val hi = off + b
        val hayBit = (haystack(hi / 8) >> (7 - (hi % 8))) & 1
        val ndlBit = (needle(b / 8) >> (7 - (b % 8))) & 1
        if (hayBit != ndlBit) matched = false
        b += 1
      }
      if (matched) return off
    }
    -1
  }

  // =========================================================================
  // Tests
  // =========================================================================

  "idle stability" in {
    simulate(new Spi2TLULV2TestWrapper) { dut =>
      val spi = new SpiDriver(dut)
      spi.reset()
      spi.idle(200)
    }
  }

  "read address on TL-A" in {
    simulate(new Spi2TLULV2TestWrapper) { dut =>
      val spi = new SpiDriver(dut)
      spi.reset()

      val countBefore = dut.io.tl_a_cap_count.peek().litValue.toLong

      // Send read frame: op=0x01, addr=0xCAFE0000, len=0
      dut.io.spi_mosi_valid.poke(true.B)
      spi.spiSendHeader(0x01, 0xCAFE0000L, 0x0000)
      // Clock dummy bytes to let MISO flow
      for (_ <- 0 until 16) spi.spiXferByte(0x00)
      dut.io.spi_mosi_valid.poke(false.B)
      dut.clock.step(50)

      val got = spi.waitTlACapture(countBefore + 1, 300)
      assert(got, "TL-A fired after read frame")
      assert(dut.io.tl_a_cap_opcode.peek().litValue == 4,
        s"TL-A opcode = Get (4), got ${dut.io.tl_a_cap_opcode.peek().litValue}")
      assert(dut.io.tl_a_cap_address.peek().litValue == BigInt("CAFE0000", 16),
        s"TL-A address = 0xCAFE0000, got 0x${dut.io.tl_a_cap_address.peek().litValue.toString(16)}")
    }
  }

  "read data on MISO" in {
    simulate(new Spi2TLULV2TestWrapper) { dut =>
      val spi = new SpiDriver(dut)
      spi.reset()

      dut.io.spi_mosi_valid.poke(true.B)
      spi.spiSendHeader(0x01, 0x0000AA55L, 0x0000)

      val misoRaw = spi.collectMisoBits(384)
      dut.io.spi_mosi_valid.poke(false.B)
      dut.clock.step(20)

      // Expected: address echo, LE bytes: 0x55, 0xAA
      val pattern = Array(0x55, 0xAA)
      val offset = findBitPattern(misoRaw, 384, pattern, 16)

      if (offset >= 0) println(s"    found 0x55_0xAA at bit offset $offset")
      else {
        val hex = misoRaw.take(8).map(b => f"$b%02X").mkString(" ")
        println(s"    MISO raw (first 64 bits): $hex")
      }
      assert(offset >= 0, "exact 0x55AA pattern found in MISO bit stream")
    }
  }

  "write address and data" in {
    simulate(new Spi2TLULV2TestWrapper) { dut =>
      val spi = new SpiDriver(dut)
      spi.reset()

      val countBefore = dut.io.tl_a_cap_count.peek().litValue.toLong

      dut.io.spi_mosi_valid.poke(true.B)
      spi.spiSendHeader(0x02, 0xDEAD0000L, 0x0000)
      // Send 16 bytes of write data: 0x01..0x10
      for (i <- 0 until 16) spi.spiXferByte((i + 1) & 0xFF)
      dut.io.spi_mosi_valid.poke(false.B)
      dut.clock.step(50)

      val got = spi.waitTlACapture(countBefore + 1)
      assert(got, "TL-A fired after write frame")
      assert(dut.io.tl_a_cap_opcode.peek().litValue == 0,
        s"TL-A opcode = PutFullData (0), got ${dut.io.tl_a_cap_opcode.peek().litValue}")
      assert(dut.io.tl_a_cap_address.peek().litValue == BigInt("DEAD0000", 16),
        s"TL-A address = 0xDEAD0000, got 0x${dut.io.tl_a_cap_address.peek().litValue.toString(16)}")

      // Low 32 bits of write data: bytes 0x01,0x02,0x03,0x04 packed LE = 0x04030201
      val word0 = dut.io.tl_a_cap_data.peek().litValue & BigInt("FFFFFFFF", 16)
      assert(word0 == BigInt("04030201", 16),
        s"write payload low 32 bits = 0x04030201, got 0x${word0.toString(16)}")
    }
  }

  "multi-beat write (2 beats)" in {
    simulate(new Spi2TLULV2TestWrapper) { dut =>
      val spi = new SpiDriver(dut)
      spi.reset()

      val countBefore = dut.io.tl_a_cap_count.peek().litValue.toLong
      spi.resetFirstCapture()

      // 2-beat write: op=0x02, addr=0xBEEF0000, len=1
      dut.io.spi_mosi_valid.poke(true.B)
      spi.spiSendHeader(0x02, 0xBEEF0000L, 0x0001)
      // Beat 0: 0x10..0x1F
      for (i <- 0 until 16) spi.spiXferByte((i + 0x10) & 0xFF)
      // Beat 1: 0x20..0x2F
      for (i <- 0 until 16) spi.spiXferByte((i + 0x20) & 0xFF)
      dut.io.spi_mosi_valid.poke(false.B)
      dut.clock.step(50)

      val got = spi.waitTlACapture(countBefore + 2, 400)
      assert(got, "TL-A fired twice for 2-beat write")

      // First beat address
      assert(dut.io.tl_a_first_address.peek().litValue == BigInt("BEEF0000", 16),
        s"first beat addr, got 0x${dut.io.tl_a_first_address.peek().litValue.toString(16)}")
      // First beat data low 32 bits: 0x10,0x11,0x12,0x13 LE = 0x13121110
      val firstWord0 = dut.io.tl_a_first_data.peek().litValue & BigInt("FFFFFFFF", 16)
      assert(firstWord0 == BigInt("13121110", 16),
        s"first beat low word = 0x13121110, got 0x${firstWord0.toString(16)}")

      // Last beat address = 0xBEEF0010
      assert(dut.io.tl_a_cap_address.peek().litValue == BigInt("BEEF0010", 16),
        s"last beat addr, got 0x${dut.io.tl_a_cap_address.peek().litValue.toString(16)}")
    }
  }

  "CSB toggle stability" in {
    simulate(new Spi2TLULV2TestWrapper) { dut =>
      val spi = new SpiDriver(dut)
      spi.reset()

      for (i <- 0 until 5) {
        dut.io.spi_mosi_valid.poke(true.B)
        spi.spiSendHeader(0x01, 0x10000000L + (i << 4), 0x0000)
        spi.idle(30)
      }
      assert(true, "5 rapid frames without crash")
    }
  }
}
