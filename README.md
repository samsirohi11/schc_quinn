# SCHC-Quinn: QUIC Header Compression for Space Communication

A Rust implementation of SCHC (Static Context Header Compression) integrated with the Quinn QUIC workbench for simulating header compression in space communication scenarios.

## Overview

This project combines two components:

1. **SCHC Compressor** (`schc/`) - A streaming, rule-based header compression engine supporting IPv4/IPv6/UDP/QUIC (github: [SCHC](https://github.com/samsirohi11/schc_r_c))
2. **Quic Workbench** (`workbench/`) - An in-memory QUIC network simulator with time warping for deep-space RTT scenarios (github: [Quic Workbench](https://github.com/deepspaceip/dipt-quic-workbench))

The integration enables **observer mode** compression analysis: measuring potential SCHC compression gains on QUIC traffic without modifying the actual packets (since decompression isn't yet implemented).

## Quick Start

```bash
# Build with SCHC observer support
cd workbench
cargo build --release --features schc-observer

# Run Earth-Moon simulation with SCHC compression analysis
cargo run --release --features schc-observer --bin quinn-workbench -- quic \
  --network-graph test-data/earth-moon/networkgraph-1orbiter-1moonasset.json \
  --network-events test-data/earth-moon/events.json \
  --client-ip-address 192.168.40.1 \
  --server-ip-address 192.168.41.2 \
  --requests 3 \
  --schc-observer \
  --schc-rules ../schc/quic_test.json \
  --schc-field-context ../schc/field-context.json \
  --schc-nodes MoonOrbiter1

# Enable verbose debug output to see rule matching
cargo run --release --features schc-observer --bin quinn-workbench -- quic \
  ... \
  --schc-debug
```

## Project Structure

```
schc_quinn/
├── .git/
├── .gitignore
├── README.md
│
├── schc/                        # SCHC compression library
│   ├── Cargo.toml
│   ├── src/                     # Core implementation
│   │   ├── lib.rs               # Library entry point
│   │   ├── parser.rs            # Streaming packet parser
│   │   ├── compressor.rs        # Compression actions (CDAs)
│   │   ├── matcher.rs           # Matching operators (MOs)
│   │   ├── tree.rs              # Rule tree building
│   │   └── streaming_tree.rs    # Unified parse+match+compress
│   ├── quic_rules.json          # Full QUIC compression rules
│   ├── quic_test.json           # Simplified test rules
│   └── field-context.json       # Field size definitions
│
└── workbench/                   # Quinn QUIC simulator
    ├── Cargo.toml
    ├── in-memory-network/       # Network simulation layer
    │   └── src/schc_observer.rs # SCHC integration module
    ├── quinn-workbench/         # CLI application
    ├── test-data/               # Network scenarios
    │   └── earth-moon/          # Earth-Moon communication
    └── quinn_workbench_architecture.md  # Detailed docs
```

## SCHC CLI Options

| Option                      | Description                                        |
| --------------------------- | -------------------------------------------------- |
| `--schc-observer`           | Enable SCHC compression analysis                   |
| `--schc-rules PATH`         | Path to SCHC rules JSON file                       |
| `--schc-field-context PATH` | Path to field context JSON file                    |
| `--schc-nodes NODE1,NODE2`  | Limit observation to specific router nodes         |
| `--schc-debug`              | Show detailed rule matching and compression output |

## Example Output

```
--- SCHC Observer ---
* Rules: ../schc/rules/quic_test.json
* Field context: ../schc/rules/field-context.json
* Enabled nodes: MoonOrbiter1
...
--- SCHC Observer Statistics ---
* Packets processed: 25
* Packets matched: 25 (100.0%)
* Total original header: 360 bits (45.0 bytes)
* Total compressed header: 375 bits (46.9 bytes)
* Compression savings: 0 bits (0.0%, ratio 0.96:1)
```

## Architecture

See [Quinn Workbench Architecture](workbench/quinn_workbench_architecture.md) for detailed documentation on the simulation engine.

## Status

- ✅ SCHC compressor with rule tree matching
- ✅ QUIC header parsing (long/short headers)
- ✅ Quinn workbench integration (observer mode)
- 🔲 SCHC decompression
- 🔲 Actual packet compression (transmit compressed data)
- 🔲 Fragmentation/reassembly

## References

- [RFC 8724 - SCHC](https://www.rfc-editor.org/rfc/rfc8724)
- [RFC 9000 - QUIC](https://www.rfc-editor.org/rfc/rfc9000)
- [Quinn QUIC Implementation](https://github.com/quinn-rs/quinn)

## License

- SCHC Compressor: MIT
- Quinn Workbench: MIT/Apache-2.0
