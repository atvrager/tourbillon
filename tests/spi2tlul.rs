/// Integration tests for the SPI-to-TileLink-UL bridge (examples/spi2tlul.tbn).
use std::path::Path;

fn load_spi2tlul_src() -> String {
    let manifest_dir = Path::new(env!("CARGO_MANIFEST_DIR"));
    let tbn_path = manifest_dir.join("examples/spi2tlul.tbn");
    std::fs::read_to_string(&tbn_path)
        .unwrap_or_else(|e| panic!("cannot read {}: {e}", tbn_path.display()))
}

// ---------------------------------------------------------------------------
// 1. Type-check + deadlock analysis passes
// ---------------------------------------------------------------------------

#[test]
fn spi2tlul_check_ok() {
    let src = load_spi2tlul_src();
    tbn::check(&src, "spi2tlul.tbn").unwrap();
}

// ---------------------------------------------------------------------------
// 2. Build produces the expected set of SV files
// ---------------------------------------------------------------------------

#[test]
fn spi2tlul_build_produces_sv_files() {
    let src = load_spi2tlul_src();
    let files = tbn::build(&src, "spi2tlul.tbn", None).unwrap();
    let names: Vec<&str> = files.iter().map(|f| f.name.as_str()).collect();

    assert!(
        names.contains(&"Spi2TLUL.sv"),
        "missing Spi2TLUL.sv: {names:?}"
    );
    assert!(
        names.contains(&"tbn_fifo.sv"),
        "missing tbn_fifo.sv: {names:?}"
    );
    assert!(
        names.contains(&"tbn_async_fifo.sv"),
        "missing tbn_async_fifo.sv: {names:?}"
    );
}

// ---------------------------------------------------------------------------
// 3. System and SPI domain use different clock ports
// ---------------------------------------------------------------------------

#[test]
fn spi2tlul_separate_clock_domains() {
    let src = load_spi2tlul_src();
    let files = tbn::build(&src, "spi2tlul.tbn", None).unwrap();
    let sv = &files
        .iter()
        .find(|f| f.name == "Spi2TLUL.sv")
        .unwrap()
        .content;

    // Module must have both clock/reset pairs as distinct ports
    assert!(sv.contains("input  wire clk"), "missing system clk port");
    assert!(
        sv.contains("input  wire rst_n"),
        "missing system rst_n port"
    );
    assert!(sv.contains("input  wire spi_clk"), "missing spi_clk port");
    assert!(
        sv.contains("input  wire spi_rst_n"),
        "missing spi_rst_n port"
    );

    // SPI-domain FIFOs must use spi_clk, NOT the system clk
    assert!(
        sv.contains("q_spi_byte_q_inst"),
        "missing spi_byte_q FIFO instance"
    );
    // The SPI-domain FIFO should be clocked by spi_clk
    assert!(
        sv.contains(".clk(spi_clk)"),
        "SPI-domain FIFO should use spi_clk"
    );
}

// ---------------------------------------------------------------------------
// 4. AsyncQueue CDC wiring: spi2sys goes spi→sys, sys2spi goes sys→spi
// ---------------------------------------------------------------------------

#[test]
fn spi2tlul_async_queue_clock_wiring() {
    let src = load_spi2tlul_src();
    let files = tbn::build(&src, "spi2tlul.tbn", None).unwrap();
    let sv = &files
        .iter()
        .find(|f| f.name == "Spi2TLUL.sv")
        .unwrap()
        .content;

    // spi2sys: writes from SPI domain, reads from system domain
    assert!(
        sv.contains("aq_spi2sys_inst"),
        "missing spi2sys async FIFO instance"
    );

    // sys2spi: writes from system domain, reads from SPI domain
    assert!(
        sv.contains("aq_sys2spi_inst"),
        "missing sys2spi async FIFO instance"
    );

    // bulk_data_cdc: writes from system domain, reads from SPI domain
    assert!(
        sv.contains("aq_bulk_data_cdc_inst"),
        "missing bulk_data_cdc async FIFO instance"
    );

    // All three AsyncQueues should use tbn_async_fifo
    assert!(
        sv.contains("tbn_async_fifo"),
        "missing tbn_async_fifo instantiation"
    );
}

// ---------------------------------------------------------------------------
// 5. External queue port directions: MOSI in, MISO out, TL-A out, TL-D in
// ---------------------------------------------------------------------------

#[test]
fn spi2tlul_external_queue_port_directions() {
    let src = load_spi2tlul_src();
    let files = tbn::build(&src, "spi2tlul.tbn", None).unwrap();
    let sv = &files
        .iter()
        .find(|f| f.name == "Spi2TLUL.sv")
        .unwrap()
        .content;

    // MOSI: process consumes → deq port (input data, output ready)
    assert!(
        sv.contains("q_mosi_pin_deq_valid"),
        "missing MOSI deq_valid"
    );
    assert!(
        sv.contains("q_mosi_pin_deq_ready"),
        "missing MOSI deq_ready"
    );
    assert!(sv.contains("q_mosi_pin_deq_data"), "missing MOSI deq_data");

    // MISO: process produces → enq port (output data, input ready)
    assert!(
        sv.contains("q_miso_pin_enq_valid"),
        "missing MISO enq_valid"
    );
    assert!(sv.contains("q_miso_pin_enq_data"), "missing MISO enq_data");

    // TL-A: process produces → enq port (output)
    assert!(sv.contains("q_tl_a_enq_valid"), "missing TL-A enq_valid");
    assert!(sv.contains("q_tl_a_enq_data"), "missing TL-A enq_data");

    // TL-D: process consumes → deq port (input)
    assert!(sv.contains("q_tl_d_deq_valid"), "missing TL-D deq_valid");
    assert!(sv.contains("q_tl_d_deq_data"), "missing TL-D deq_data");
}

// ---------------------------------------------------------------------------
// 6. No DPI imports in generated SV
// ---------------------------------------------------------------------------

#[test]
fn spi2tlul_no_dpi_imports() {
    let src = load_spi2tlul_src();
    let files = tbn::build(&src, "spi2tlul.tbn", None).unwrap();
    let sv = &files
        .iter()
        .find(|f| f.name == "Spi2TLUL.sv")
        .unwrap()
        .content;

    assert!(
        !sv.contains("import \"DPI-C\""),
        "generated SV should not contain DPI imports"
    );
}

// ---------------------------------------------------------------------------
// 7. External queues have no FIFO instantiation (pin queues are wires)
// ---------------------------------------------------------------------------

#[test]
fn spi2tlul_external_queues_no_fifo() {
    let src = load_spi2tlul_src();
    let files = tbn::build(&src, "spi2tlul.tbn", None).unwrap();
    let sv = &files
        .iter()
        .find(|f| f.name == "Spi2TLUL.sv")
        .unwrap()
        .content;

    // External queues become module ports, NOT internal FIFOs
    assert!(
        !sv.contains("q_mosi_pin_inst"),
        "MOSI external queue should not have a FIFO instance"
    );
    assert!(
        !sv.contains("q_miso_pin_inst"),
        "MISO external queue should not have a FIFO instance"
    );
    assert!(
        !sv.contains("q_tl_a_inst"),
        "TL-A external queue should not have a FIFO instance"
    );
    assert!(
        !sv.contains("q_tl_d_inst"),
        "TL-D external queue should not have a FIFO instance"
    );
}

// ---------------------------------------------------------------------------
// 8. SPI-domain registers use spi_clk, system-domain registers use clk
// ---------------------------------------------------------------------------

#[test]
fn spi2tlul_register_clock_domains() {
    let src = load_spi2tlul_src();
    let files = tbn::build(&src, "spi2tlul.tbn", None).unwrap();
    let sv = &files
        .iter()
        .find(|f| f.name == "Spi2TLUL.sv")
        .unwrap()
        .content;

    // SPI-domain processes (SpiByteAssembler, SpiMisoShifter, etc.)
    // should have always_ff blocks clocked by spi_clk
    assert!(
        sv.contains("posedge spi_clk"),
        "SPI-domain registers should be clocked by spi_clk"
    );

    // System-domain processes (SpiRegMap, TlFsm, WriteBuffer)
    // should have always_ff blocks clocked by clk
    assert!(
        sv.contains("posedge clk"),
        "system-domain registers should be clocked by clk"
    );
}

// ---------------------------------------------------------------------------
// 9. TlA and TlD record types are emitted as packed structs
// ---------------------------------------------------------------------------

#[test]
fn spi2tlul_record_types_emitted() {
    let src = load_spi2tlul_src();
    let files = tbn::build(&src, "spi2tlul.tbn", None).unwrap();
    let sv = &files
        .iter()
        .find(|f| f.name == "Spi2TLUL.sv")
        .unwrap()
        .content;

    assert!(
        sv.contains("typedef struct packed") && sv.contains("TlA"),
        "TlA should be emitted as a packed struct"
    );
    assert!(
        sv.contains("TlD"),
        "TlD should be emitted as a packed struct"
    );
    assert!(
        sv.contains("TlCmd"),
        "TlCmd should be emitted as a packed struct"
    );
}

// ---------------------------------------------------------------------------
// 10. DOT graph: SPI and system domain subgraphs present
// ---------------------------------------------------------------------------

#[test]
fn spi2tlul_dot_graph_domains() {
    let src = load_spi2tlul_src();
    let dots = tbn::emit_graph(&src, "spi2tlul.tbn").unwrap();
    assert!(!dots.is_empty(), "should produce at least one DOT graph");
    let dot = &dots[0];

    // Should have domain subgraphs
    assert!(
        dot.contains("cluster_spi"),
        "missing SPI domain subgraph in DOT"
    );

    // Should have async edges (red color for CDC crossings)
    assert!(
        dot.contains("color=red"),
        "missing red color for async edges in DOT"
    );
}

// ---------------------------------------------------------------------------
// 11. Constants are emitted as localparam
// ---------------------------------------------------------------------------

#[test]
fn spi2tlul_constants_as_localparam() {
    let src = load_spi2tlul_src();
    let files = tbn::build(&src, "spi2tlul.tbn", None).unwrap();
    let sv = &files
        .iter()
        .find(|f| f.name == "Spi2TLUL.sv")
        .unwrap()
        .content;

    assert!(
        sv.contains("localparam TL_GET = 4"),
        "TL_GET should be emitted as localparam"
    );
    assert!(
        sv.contains("localparam TL_PUT_FULL = 0"),
        "TL_PUT_FULL should be emitted as localparam"
    );
    assert!(
        sv.contains("localparam REG_TL_CMD = 6"),
        "REG_TL_CMD should be emitted as localparam"
    );
}
