# Device Manifest Peripheral — Design Document

**Status:** Draft / Research  
**Scope:** Bus-agnostic hardware device discovery for custom SoC interconnects  
**Target prototype:** Tourbillon HDL

---

## 1. Problem Statement

Embedded SoC designs lack a universal, hardware-level device discovery mechanism. The dominant approaches each have serious drawbacks:

- **Device trees** are maintained out-of-band, can drift from hardware reality, and require parser infrastructure in firmware.
- **Exception-based probing** (try an access, catch the fault) is fragile, platform-specific, and dangerous if a stale address maps to a different device's register.
- **Bus-specific enumeration** (PCIe config space, I²C scan) doesn't generalize across interconnects and isn't available on simple fabrics like TileLink-UL, AXI-Lite, or Wishbone.

We want a mechanism where peripherals **self-report** their existence into a hardware directory that software can read with plain loads — no device tree, no exceptions, no bus-specific features beyond basic reads and writes.

## 2. Design Goals

| Priority | Goal |
|----------|------|
| **Must** | Work on any interconnect that supports single-beat reads and writes. |
| **Must** | Allow peripherals to register at runtime (post-reset), not only at elaboration time. |
| **Must** | Provide the CPU with a consistent, race-free snapshot of all present devices. |
| **Must** | Be safe in the presence of absent or partially-initialized peripherals. |
| **Should** | Keep peripheral-side gate cost minimal (small FSM, no FIFO, no complex logic). |
| **Should** | Support a hybrid static + dynamic model (ROM entries from elaboration, plus runtime registration). |
| **Nice** | Extensible to future capability advertisement without breaking the base protocol. |

Non-goals: high-throughput data transfer, hot-plug (post-seal insertion), security/attestation of peripheral identity.

## 3. Architecture Overview

A single **manifest peripheral** is instantiated on the interconnect at a well-known, fixed base address. It exposes a register interface with three regions:

```
┌──────────────────────────────────────────────┐
│                  HEADER (RO)                 │  0x000 – 0x01F
│  magic, version, max_slots, populated, seal  │
├──────────────────────────────────────────────┤
│              CLAIM REGISTER (RO*)            │  0x010
│  read-to-claim: returns next free slot idx   │
├──────────────────────────────────────────────┤
│              SLOT ARRAY (RW → RO)            │  0x100 + N * 0x20
│  per-slot: key, base, size, irq, flags, valid│
└──────────────────────────────────────────────┘
  * Side-effecting read (counter increment)
```

Peripherals are **writers** during the registration window. The CPU is the **reader** after sealing. The two phases never overlap.

## 4. Identity: The Device Key

Each peripheral carries a **64-bit device key**, set as a synthesis-time parameter at module instantiation. The key uniquely identifies the peripheral type (not instance) within the system.

### 4.1 Recommended Key Encoding

```
 63        48 47        32 31        16 15         0
┌────────────┬────────────┬────────────┬────────────┐
│   vendor   │   class    │  device_id │  revision  │
└────────────┴────────────┴────────────┴────────────┘
```

- **vendor** (16 bits): JEDEC manufacturer ID, or a locally-administered namespace (MSB set for local).
- **class** (16 bits): functional category (UART, SPI, timer, DMA, custom, ...).
- **device_id** (16 bits): specific implementation within vendor+class.
- **revision** (16 bits): hardware revision, for driver compatibility checks.

Alternative: a 64-bit hash of a human-readable string like `"myvendor:uart:v2"`, computed at elaboration time. The encoding is a convention, not enforced by hardware. The manifest stores and returns the raw 64 bits.

### 4.2 Instance Discrimination

Two instances of the same UART share the same key but differ in `base_addr`. The CPU-side driver binds on key match and distinguishes instances by address, exactly as PCIe allows multiple devices with the same vendor/device ID.

## 5. Registration Protocol

### 5.1 The Claim–Fill–Commit Sequence

After reset, each peripheral's init FSM executes:

```
1. READ   MANIFEST_BASE + 0x010          → slot index N
2. WRITE  MANIFEST_BASE + 0x100 + N*0x20 + 0x00  ← key[63:32]
3. WRITE  MANIFEST_BASE + 0x100 + N*0x20 + 0x04  ← key[31:0]
4. WRITE  MANIFEST_BASE + 0x100 + N*0x20 + 0x08  ← self_base_addr
5. WRITE  MANIFEST_BASE + 0x100 + N*0x20 + 0x0C  ← aperture_size_log2
6. WRITE  MANIFEST_BASE + 0x100 + N*0x20 + 0x10  ← irq_number
7. WRITE  MANIFEST_BASE + 0x100 + N*0x20 + 0x14  ← capability_flags
8. WRITE  MANIFEST_BASE + 0x100 + N*0x20 + 0x18  ← VALID_MAGIC (0x56_4C_49_44)
```

### 5.2 Atomicity via Claim

The CLAIM register is a hardware counter that returns its current value and increments on every read. This is the sole mutual-exclusion primitive. No compare-and-swap, no exclusive-access, no bus-specific feature is required — only a side-effecting read, which every reasonable interconnect supports.

Each peripheral gets a unique slot index regardless of concurrent registration, because the counter serializes slot assignment even if the bus arbitrates between simultaneous read requests from multiple peripherals.

### 5.3 Commit Safety

The VALID_MAGIC write in step 8 is the **commit barrier**. The CPU ignores any slot whose valid word is not set. This means:

- A peripheral that resets or stalls mid-registration leaves no partial entry visible.
- An unpopulated slot (claimed but never committed) is invisible to software.
- The manifest tolerates peripherals that are instantiated but held in reset.

### 5.4 Error Sentinel

If a peripheral reads CLAIM after the manifest is sealed or all slots are exhausted, the returned value is `0xFFFF_FFFF`. The peripheral FSM should check for this and halt (enter DONE without writing). This is a soft failure — the peripheral still functions, it's just not discoverable via the manifest.

## 6. The Seal Protocol

### 6.1 Software-Initiated Seal

After the CPU determines that all peripherals have registered (by polling the populated count, waiting a fixed number of cycles, or using a more sophisticated readiness signal), it writes any nonzero value to the SEAL register (`MANIFEST_BASE + 0x01C`).

Post-seal effects:

- CLAIM reads return `0xFFFF_FFFF`.
- Writes to any slot offset are silently dropped.
- The SEALED flag in the header reads as 1.
- The slot array is effectively ROM from this point forward.

### 6.2 Optional Hardware Auto-Seal

An optional synthesis-time parameter `AUTO_SEAL_CYCLES` can be set. If nonzero, the manifest starts a countdown from reset and auto-seals when the counter expires. The software seal still works and takes effect immediately if it arrives before the timeout. This is a safety net against firmware that forgets to seal.

### 6.3 Why Seal at All?

The seal creates a **consistency boundary**. Without it, the CPU could observe a partially-registered peripheral mid-flight. The two-phase protocol (register, then seal, then read) gives software a clean snapshot guarantee with zero synchronization complexity.

## 7. Register Map (Detailed)

### 7.1 Header Region: `0x000 – 0x01F`

| Offset | Name          | Width | Access | Description |
|--------|---------------|-------|--------|-------------|
| 0x000  | MAGIC         | 32    | RO     | `0xDEV1_D1R0` — identifies a manifest peripheral |
| 0x004  | VERSION       | 32    | RO     | Protocol version (currently `0x0000_0001`) |
| 0x008  | MAX_SLOTS     | 32    | RO     | Maximum number of device slots (set at synthesis) |
| 0x00C  | POPULATED     | 32    | RO     | Number of slots with valid commits |
| 0x010  | CLAIM         | 32    | RO*    | Side-effecting read: returns next slot index, increments |
| 0x014  | STATUS        | 32    | RO     | Bit 0: SEALED. Bits 31:1 reserved. |
| 0x018  | SEAL_TIMER    | 32    | RO     | Remaining auto-seal cycles (0 if disabled or expired) |
| 0x01C  | SEAL          | 32    | WO     | Write any nonzero value to seal the manifest |

### 7.2 Slot Region: `0x100 + N * 0x20`

| Offset | Name          | Width | Access    | Description |
|--------|---------------|-------|-----------|-------------|
| +0x00  | KEY_HI        | 32    | RW → RO   | Device key bits [63:32] |
| +0x04  | KEY_LO        | 32    | RW → RO   | Device key bits [31:0] |
| +0x08  | BASE_ADDR     | 32    | RW → RO   | Peripheral's base address on the interconnect |
| +0x0C  | APERTURE      | 32    | RW → RO   | Aperture size as log₂(bytes) |
| +0x10  | IRQ           | 32    | RW → RO   | Interrupt number (`0xFFFF_FFFF` = no IRQ) |
| +0x14  | FLAGS         | 32    | RW → RO   | Capability/feature flags (device-specific) |
| +0x18  | VALID         | 32    | RW → RO   | `0x564C4944` ("VLID") = committed entry |
| +0x1C  | RESERVED      | 32    | —         | Reserved for future use |

"RW → RO" means writable during the registration window, read-only after seal.

## 8. Peripheral-Side FSM

### 8.1 State Machine

```
         reset
           │
           ▼
        ┌──────┐
        │ IDLE │──── (reset deasserted) ────┐
        └──────┘                            ▼
                                      ┌───────────┐
                              ┌───NO──│ CLAIM_CHK │
                              ▼       └─────┬─────┘
                          ┌──────┐      YES │ (slot != 0xFFFFFFFF)
                          │ FAIL │          ▼
                          └──────┘    ┌───────────┐
                                      │  WR_KEY_H │
                                      └─────┬─────┘
                                            ▼
                                      ┌───────────┐
                                      │  WR_KEY_L │
                                      └─────┬─────┘
                                            ▼
                                      ┌───────────┐
                                      │  WR_BASE  │
                                      └─────┬─────┘
                                            ▼
                                      ┌───────────┐
                                      │  WR_APER  │
                                      └─────┬─────┘
                                            ▼
                                      ┌───────────┐
                                      │  WR_IRQ   │
                                      └─────┬─────┘
                                            ▼
                                      ┌───────────┐
                                      │  WR_FLAGS │
                                      └─────┬─────┘
                                            ▼
                                      ┌───────────┐
                                      │ WR_COMMIT │ ← writes VALID_MAGIC
                                      └─────┬─────┘
                                            ▼
                                      ┌──────────┐
                                      │   DONE   │
                                      └──────────┘
```

### 8.2 Resource Cost

- 4-bit state register (9 states).
- A register holding the claimed slot index.
- A small address-offset mux.
- No FIFOs, no counters beyond the state register.
- Estimated cost: ~150–300 gates depending on bus adapter width. Negligible in any design large enough to have multiple peripherals.

### 8.3 Parameterization

The FSM is instantiated as a wrapper module with elaboration-time parameters:

```
manifest_registrar #(
    .MANIFEST_BASE  (32'h0000_0000),
    .DEVICE_KEY     (64'hACME_0001_UART_0002),
    .SELF_BASE      (32'h4000_0000),
    .APERTURE_LOG2  (12),            // 4 KiB
    .IRQ_NUMBER     (5),
    .FLAGS          (32'h0000_0001)
) u_reg (
    .clk            (clk),
    .rst_n          (rst_n),
    .bus_req        (periph_bus_req),  // bus-specific master port
    .bus_rsp        (periph_bus_rsp),
    .done           (reg_done),
    .fail           (reg_fail)
);
```

For vendor IP that cannot be modified, this wrapper sits alongside the peripheral and shares its bus master port (or is given a dedicated port on the interconnect).

## 9. CPU-Side Software Interface

### 9.1 Discovery

```c
#define MANIFEST_BASE    0x00000000
#define MANIFEST_MAGIC   0xDEV1D1R0
#define SLOT_BASE        (MANIFEST_BASE + 0x100)
#define SLOT_STRIDE      0x20
#define VALID_MAGIC      0x564C4944

struct manifest_slot {
    uint32_t key_hi;
    uint32_t key_lo;
    uint32_t base_addr;
    uint32_t aperture_log2;
    uint32_t irq;
    uint32_t flags;
    uint32_t valid;
    uint32_t _reserved;
};

void discover_devices(void) {
    assert(readl(MANIFEST_BASE) == MANIFEST_MAGIC);
    assert(readl(MANIFEST_BASE + 0x14) & 1);  // SEALED

    uint32_t max = readl(MANIFEST_BASE + 0x08);

    for (uint32_t i = 0; i < max; i++) {
        volatile struct manifest_slot *s =
            (void *)(SLOT_BASE + i * SLOT_STRIDE);

        if (s->valid != VALID_MAGIC)
            continue;

        uint64_t key = ((uint64_t)s->key_hi << 32) | s->key_lo;
        driver_bind(key, s->base_addr, 1u << s->aperture_log2, s->irq);
    }
}
```

### 9.2 Driver Binding

The driver table is a simple key → init function map:

```c
struct driver_entry {
    uint64_t key;
    void (*init)(uint32_t base, uint32_t size, uint32_t irq);
};

static const struct driver_entry drivers[] = {
    { 0xACME0001UART0002, uart_init },
    { 0xACME0002SPI_0001, spi_init  },
    // ...
};
```

This is structurally identical to Linux's `pci_device_id` match tables — familiar to any driver author.

## 10. Hybrid Static + Dynamic Model

For systems where most peripherals are known at elaboration time:

1. The manifest is pre-populated at synthesis with ROM-backed entries in slots 0 through K−1, each with VALID_MAGIC already set.
2. The CLAIM counter is initialized to K.
3. Runtime-registering peripherals claim slots K and above.
4. The CPU-side interface is identical in both cases.

This means a fully-static system pays no runtime registration cost but still uses the same software discovery path. The device tree can be eliminated entirely for conforming peripherals; non-conforming legacy devices fall back to a minimal device tree that only describes *them*.

## 11. Interconnect Companion: Default Responder

Independent of the manifest, the interconnect should include a **default slave** that responds to any address not decoded by a real peripheral. It returns a clean error response (e.g., data = 0, error flag set) rather than hanging the bus or causing an exception.

This makes speculative probing safe as a fallback, but the manifest remains the preferred discovery path. The two mechanisms compose: the manifest tells you what *should* exist; the default responder protects you from what *doesn't*.

## 12. Extensions and Future Work

**Capability lists.** The FLAGS field could include a "has extended capabilities" bit. If set, the peripheral exposes a capability linked list at a known offset within its own MMIO aperture (à la PCIe capability pointers). This keeps the manifest slot fixed-size while allowing unbounded capability advertisement.

**64-bit addressing.** For systems with >32-bit physical address spaces, the slot format can be extended to 48 or 64 bytes with wider BASE_ADDR and an additional APERTURE_HI field. The VERSION field in the header disambiguates.

**Manifest-of-manifests.** In multi-tile / chiplet designs, each tile has its own manifest. A top-level manifest could contain entries whose "device class" is "sub-manifest," with the base address pointing to a child manifest. Recursive discovery, same protocol.

**Tourbillon integration.** The manifest peripheral and registration FSM are natural candidates for Tourbillon's queue-centric model. The claim register is a single-element channel with generate-on-read semantics. The slot array is a bank of write-once registers. The seal transition maps cleanly to a linear-type "consume" operation that transitions the manifest from mutable to frozen. A Tourbillon prototype could use the type system to enforce the two-phase protocol at the HDL level — making it impossible to read a slot before seal or write after seal by construction.

## 13. Summary

The device manifest is a minimal peripheral (~1KB register space, ~300 gates per registrant FSM) that provides PCIe-style self-describing hardware on any interconnect capable of reads and writes. It separates **identity** (the 64-bit device key) from **enumeration** (the claim-fill-commit protocol) from **transport** (bus-specific read/write encoding), keeping each concern independent and replaceable.

The key design choices are:

- **Read-to-claim** for bus-agnostic mutual exclusion.
- **Write-to-commit** (VALID_MAGIC) for atomic visibility.
- **Seal** for a clean phase boundary between hardware registration and software discovery.
- **Parameterized wrapper FSM** for zero-effort integration with any peripheral, including unmodifiable vendor IP.
