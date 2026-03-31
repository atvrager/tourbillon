/// Integration tests for the SPI DMA Bridge V2 (examples/spi2tlul_v2.tbn).
use std::path::Path;

fn load_src() -> String {
    let manifest_dir = Path::new(env!("CARGO_MANIFEST_DIR"));
    let tbn_path = manifest_dir.join("examples/spi2tlul_v2.tbn");
    std::fs::read_to_string(&tbn_path)
        .unwrap_or_else(|e| panic!("cannot read {}: {e}", tbn_path.display()))
}

// ---------------------------------------------------------------------------
// 1. Type-check + deadlock analysis
// ---------------------------------------------------------------------------

#[test]
fn v2_check_ok() {
    let src = load_src();
    tbn::check(&src, "spi2tlul_v2.tbn").unwrap();
}

// ---------------------------------------------------------------------------
// 2. Build produces expected SV files
// ---------------------------------------------------------------------------

#[test]
fn v2_build_produces_sv_files() {
    let src = load_src();
    let files = tbn::build(&src, "spi2tlul_v2.tbn", None).unwrap();
    let names: Vec<&str> = files.iter().map(|f| f.name.as_str()).collect();

    assert!(
        names.contains(&"Spi2TLULV2.sv"),
        "missing Spi2TLULV2.sv: {names:?}"
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
// 3. Separate clock domains
// ---------------------------------------------------------------------------

#[test]
fn v2_separate_clock_domains() {
    let src = load_src();
    let files = tbn::build(&src, "spi2tlul_v2.tbn", None).unwrap();
    let sv = &files
        .iter()
        .find(|f| f.name == "Spi2TLULV2.sv")
        .unwrap()
        .content;

    assert!(sv.contains("input  wire clk"), "missing system clk");
    assert!(sv.contains("input  wire rst_n"), "missing system rst_n");
    assert!(sv.contains("input  wire spi_clk"), "missing spi_clk");
    assert!(sv.contains("input  wire spi_rst_n"), "missing spi_rst_n");
    assert!(sv.contains(".clk(spi_clk)"), "SPI FIFOs should use spi_clk");
}

// ---------------------------------------------------------------------------
// 4. Three AsyncQueue CDC crossings
// ---------------------------------------------------------------------------

#[test]
fn v2_async_queue_instances() {
    let src = load_src();
    let files = tbn::build(&src, "spi2tlul_v2.tbn", None).unwrap();
    let sv = &files
        .iter()
        .find(|f| f.name == "Spi2TLULV2.sv")
        .unwrap()
        .content;

    assert!(sv.contains("aq_desc_cdc_inst"), "missing desc_cdc");
    assert!(sv.contains("aq_wr_data_cdc_inst"), "missing wr_data_cdc");
    assert!(sv.contains("aq_rd_data_cdc_inst"), "missing rd_data_cdc");
    assert!(sv.contains("tbn_async_fifo"), "missing tbn_async_fifo");
}

// ---------------------------------------------------------------------------
// 5. External queue port directions
// ---------------------------------------------------------------------------

#[test]
fn v2_external_queue_port_directions() {
    let src = load_src();
    let files = tbn::build(&src, "spi2tlul_v2.tbn", None).unwrap();
    let sv = &files
        .iter()
        .find(|f| f.name == "Spi2TLULV2.sv")
        .unwrap()
        .content;

    // MOSI in, MISO out
    assert!(sv.contains("q_mosi_pin_deq_valid"), "missing MOSI deq");
    assert!(sv.contains("q_miso_pin_enq_valid"), "missing MISO enq");

    // TL-A out, TL-D in
    assert!(sv.contains("q_tl_a_enq_valid"), "missing TL-A enq");
    assert!(sv.contains("q_tl_d_deq_valid"), "missing TL-D deq");
}

// ---------------------------------------------------------------------------
// 6. No DPI imports
// ---------------------------------------------------------------------------

#[test]
fn v2_no_dpi_imports() {
    let src = load_src();
    let files = tbn::build(&src, "spi2tlul_v2.tbn", None).unwrap();
    let sv = &files
        .iter()
        .find(|f| f.name == "Spi2TLULV2.sv")
        .unwrap()
        .content;

    assert!(!sv.contains("import \"DPI-C\""), "should not have DPI");
}

// ---------------------------------------------------------------------------
// 7. External queues have no FIFO
// ---------------------------------------------------------------------------

#[test]
fn v2_external_queues_no_fifo() {
    let src = load_src();
    let files = tbn::build(&src, "spi2tlul_v2.tbn", None).unwrap();
    let sv = &files
        .iter()
        .find(|f| f.name == "Spi2TLULV2.sv")
        .unwrap()
        .content;

    assert!(!sv.contains("q_mosi_pin_inst"), "MOSI no FIFO");
    assert!(!sv.contains("q_miso_pin_inst"), "MISO no FIFO");
    assert!(!sv.contains("q_tl_a_inst"), "TL-A no FIFO");
    assert!(!sv.contains("q_tl_d_inst"), "TL-D no FIFO");
}

// ---------------------------------------------------------------------------
// 8. Register clock domains
// ---------------------------------------------------------------------------

#[test]
fn v2_register_clock_domains() {
    let src = load_src();
    let files = tbn::build(&src, "spi2tlul_v2.tbn", None).unwrap();
    let sv = &files
        .iter()
        .find(|f| f.name == "Spi2TLULV2.sv")
        .unwrap()
        .content;

    assert!(sv.contains("posedge spi_clk"), "SPI regs on spi_clk");
    assert!(sv.contains("posedge clk"), "sys regs on clk");
}

// ---------------------------------------------------------------------------
// 9. Record types emitted as packed structs
// ---------------------------------------------------------------------------

#[test]
fn v2_record_types_emitted() {
    let src = load_src();
    let files = tbn::build(&src, "spi2tlul_v2.tbn", None).unwrap();
    let sv = &files
        .iter()
        .find(|f| f.name == "Spi2TLULV2.sv")
        .unwrap()
        .content;

    assert!(sv.contains("TlA"), "missing TlA struct");
    assert!(sv.contains("TlD"), "missing TlD struct");
    assert!(sv.contains("DmaDesc"), "missing DmaDesc struct");
}

// ---------------------------------------------------------------------------
// 10. DOT graph has SPI domain subgraph
// ---------------------------------------------------------------------------

#[test]
fn v2_dot_graph_domains() {
    let src = load_src();
    let dots = tbn::emit_graph(&src, "spi2tlul_v2.tbn").unwrap();
    assert!(!dots.is_empty(), "should produce DOT graph");
    let dot = &dots[0];

    assert!(dot.contains("cluster_spi"), "missing SPI domain subgraph");
    assert!(dot.contains("color=red"), "missing async edge color");
}

// ---------------------------------------------------------------------------
// 11. V2 is smaller than V1
// ---------------------------------------------------------------------------

#[test]
fn v2_smaller_than_v1() {
    let v2_src = load_src();
    let manifest_dir = Path::new(env!("CARGO_MANIFEST_DIR"));
    let v1_path = manifest_dir.join("examples/spi2tlul.tbn");
    let v1_src = std::fs::read_to_string(&v1_path).unwrap();

    let v2_lines = v2_src.lines().count();
    let v1_lines = v1_src.lines().count();
    assert!(
        v2_lines < v1_lines,
        "V2 ({v2_lines} lines) should be smaller than V1 ({v1_lines} lines)"
    );
}
