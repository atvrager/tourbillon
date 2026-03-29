//! FST waveform trace reader for debugging Verilator simulations.
//!
//! Usage:
//!   tbn wave trace.fst                     # dump all signals, all times
//!   tbn wave trace.fst -l                  # list signal names only
//!   tbn wave trace.fst -f tohost           # filter signals matching "tohost"
//!   tbn wave trace.fst -f cpu_clk --from 100 --to 200

use std::collections::BTreeMap;
use std::fs::File;
use std::io::BufReader;
use std::path::Path;

use fst_reader::{FstFilter, FstHierarchyEntry, FstReader, FstSignalHandle, FstSignalValue};

/// Signal metadata collected during hierarchy traversal.
struct SignalInfo {
    full_path: String,
    length: u32,
}

pub fn run(
    path: &Path,
    filter: Option<&str>,
    from: Option<u64>,
    to: Option<u64>,
    list_only: bool,
) -> Result<(), String> {
    let file = File::open(path).map_err(|e| format!("failed to open {}: {e}", path.display()))?;
    let buf = BufReader::new(file);
    let mut reader = FstReader::open(buf).map_err(|e| format!("failed to parse FST: {e}"))?;

    let header = reader.get_header();
    eprintln!(
        "FST: {} | vars: {} | time: {}..{} | version: {}",
        path.display(),
        header.var_count,
        header.start_time,
        header.end_time,
        header.version,
    );

    // Phase 1: read hierarchy to build signal name → handle map
    let mut signals: BTreeMap<usize, SignalInfo> = BTreeMap::new();
    let mut scope_stack: Vec<String> = vec![];

    reader
        .read_hierarchy(|entry| match entry {
            FstHierarchyEntry::Scope { name, .. } => {
                scope_stack.push(name);
            }
            FstHierarchyEntry::UpScope => {
                scope_stack.pop();
            }
            FstHierarchyEntry::Var {
                name,
                length,
                handle,
                ..
            } => {
                let full_path = if scope_stack.is_empty() {
                    name.clone()
                } else {
                    format!("{}.{}", scope_stack.join("."), name)
                };
                signals.insert(handle.get_index(), SignalInfo { full_path, length });
            }
            _ => {}
        })
        .map_err(|e| format!("failed to read hierarchy: {e}"))?;

    // Apply name filter
    let filtered: Vec<(usize, &SignalInfo)> = signals
        .iter()
        .filter(|(_, info)| {
            if let Some(f) = filter {
                info.full_path.contains(f)
            } else {
                true
            }
        })
        .map(|(&idx, info)| (idx, info))
        .collect();

    if list_only {
        for (_, info) in &filtered {
            println!("{} [{}]", info.full_path, info.length);
        }
        eprintln!("{} signals", filtered.len());
        return Ok(());
    }

    if filtered.is_empty() {
        eprintln!("no signals match filter");
        return Ok(());
    }

    // Phase 2: read signal values
    let include_handles: Vec<FstSignalHandle> = filtered
        .iter()
        .map(|(idx, _)| FstSignalHandle::from_index(*idx))
        .collect();

    let fst_filter = FstFilter {
        start: from.unwrap_or(0),
        end: to,
        include: Some(include_handles),
    };

    // Collect transitions: (timestamp, handle_index, value_string)
    let mut transitions: Vec<(u64, usize, String)> = vec![];

    reader
        .read_signals(&fst_filter, |time, handle, value| {
            if let Some(t0) = from
                && time < t0
            {
                return;
            }
            if let Some(t1) = to
                && time > t1
            {
                return;
            }
            let val_str = match value {
                FstSignalValue::String(bytes) => String::from_utf8_lossy(bytes).to_string(),
                FstSignalValue::Real(r) => format!("{r}"),
            };
            transitions.push((time, handle.get_index(), val_str));
        })
        .map_err(|e| format!("failed to read signals: {e}"))?;

    // Sort by timestamp, then by signal name for deterministic output
    transitions.sort_by(|a, b| a.0.cmp(&b.0).then_with(|| a.1.cmp(&b.1)));

    // Print transitions
    for (time, handle_idx, val) in &transitions {
        if let Some(info) = signals.get(handle_idx) {
            let display_val = format_signal_value(val, info.length);
            println!("@{time:>10} {:<60} = {display_val}", info.full_path);
        }
    }

    eprintln!(
        "{} transitions across {} signals",
        transitions.len(),
        filtered.len()
    );
    Ok(())
}

/// Format a signal value for display. Binary strings get hex conversion for wide signals.
fn format_signal_value(val: &str, width: u32) -> String {
    // Check if it's a binary string (0s and 1s only)
    if val.chars().all(|c| c == '0' || c == '1') && width > 4 {
        // Convert binary to hex
        let padded = format!("{:0>width$}", val, width = (width as usize).div_ceil(4) * 4);
        let hex: String = padded
            .as_bytes()
            .chunks(4)
            .map(|nibble| {
                let n: u8 = nibble.iter().fold(0u8, |acc, &b| (acc << 1) | (b - b'0'));
                format!("{n:x}")
            })
            .collect();
        format!("0x{hex}")
    } else if val.contains('x') || val.contains('z') {
        // Contains unknowns — show as-is
        val.to_string()
    } else {
        val.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn format_binary_to_hex() {
        assert_eq!(format_signal_value("11111111", 8), "0xff");
        assert_eq!(format_signal_value("00000001", 8), "0x01");
        assert_eq!(
            format_signal_value("10000000000000000000000000000000", 32),
            "0x80000000"
        );
    }

    #[test]
    fn format_narrow_signal() {
        // 1-bit signals stay as-is
        assert_eq!(format_signal_value("1", 1), "1");
        assert_eq!(format_signal_value("0", 1), "0");
    }

    #[test]
    fn format_with_unknowns() {
        assert_eq!(format_signal_value("xxxx", 4), "xxxx");
    }
}
