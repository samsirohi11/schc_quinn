//! SCHC Observer Mode
//!
//! Provides header compression observation without modifying transmitted packets.
//! Useful for measuring potential SCHC compression gains in simulated networks.

use pnet_packet::ip::IpNextHeaderProtocol;
use pnet_packet::ipv4::MutableIpv4Packet;
use pnet_packet::udp::MutableUdpPacket;
use pnet_packet::{ipv4, udp};
use schc::{build_tree, compress_packet, Direction, FieldContext, Rule, RuleSet, TreeNode};
use std::net::{IpAddr, SocketAddr};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

/// Statistics from SCHC compression observation
#[derive(Debug, Default)]
pub struct SchcStats {
    pub packets_processed: AtomicUsize,
    pub packets_matched: AtomicUsize,
    pub total_original_bits: AtomicUsize,
    pub total_compressed_bits: AtomicUsize,
}

impl SchcStats {
    pub fn report(&self) {
        let processed = self.packets_processed.load(Ordering::Relaxed);
        let matched = self.packets_matched.load(Ordering::Relaxed);
        let original = self.total_original_bits.load(Ordering::Relaxed);
        let compressed = self.total_compressed_bits.load(Ordering::Relaxed);
        let saved = original.saturating_sub(compressed);
        
        println!("--- SCHC Observer Statistics ---");
        println!("* Packets processed: {}", processed);
        println!("* Packets matched: {} ({:.1}%)", matched, 
                 if processed > 0 { 100.0 * matched as f64 / processed as f64 } else { 0.0 });
        println!("* Total original header: {} bits ({:.1} bytes)", original, original as f64 / 8.0);
        println!("* Total compressed header: {} bits ({:.1} bytes)", compressed, compressed as f64 / 8.0);
        if original > 0 {
            println!("* Compression savings: {} bits ({:.1}%, ratio {:.2}:1)", 
                     saved, 
                     100.0 * saved as f64 / original as f64,
                     original as f64 / compressed.max(1) as f64);
        }
    }
}

/// SCHC Observer context for compression analysis
pub struct SchcObserver {
    tree: TreeNode,
    rules: Vec<Rule>,
    field_context: FieldContext,
    stats: SchcStats,
    debug: bool,
}

impl SchcObserver {
    /// Create a new SCHC observer from rules and field context files
    pub fn from_files(
        rules_path: &str,
        field_context_path: &str,
        debug: bool,
    ) -> anyhow::Result<Self> {
        let ruleset = RuleSet::from_file(rules_path)?;
        let field_context = FieldContext::from_file(field_context_path)?;
        let tree = build_tree(&ruleset.rules, &field_context);
        
        if debug {
            println!("\n--- SCHC Rule Tree ---");
            schc::display_tree(&tree);
        }
        
        Ok(Self {
            tree,
            rules: ruleset.rules,
            field_context,
            stats: SchcStats::default(),
            debug,
        })
    }

    /// Observe compression for a UDP payload (QUIC packet)
    ///
    /// This does NOT modify the packet - it only measures potential compression.
    /// The quic_payload is the actual QUIC packet data from Quinn workbench.
    /// source_addr and dest_addr are the actual simulation addresses.
    pub fn observe(
        &self,
        quic_payload: &[u8],
        source_addr: SocketAddr,
        dest_addr: SocketAddr,
        is_outgoing: bool,
    ) {
        self.stats.packets_processed.fetch_add(1, Ordering::Relaxed);

        // Build a proper Ethernet+IPv4+UDP frame around the QUIC payload
        // using the actual simulation addresses (like pcap_exporter does)
        let synthetic_packet = self.build_synthetic_packet(quic_payload, source_addr, dest_addr);
        
        let direction = if is_outgoing {
            Direction::Up
        } else {
            Direction::Down
        };

        let packet_num = self.stats.packets_processed.load(Ordering::Relaxed);

        if self.debug {
            let dir_str = if is_outgoing { "UP" } else { "DOWN" };
            println!("\n╔══════════════════════════════════════════════════════════════════════════════");
            println!(
                "║ [SCHC] Packet {} [{}] - QUIC payload: {} bytes",
                packet_num, dir_str, quic_payload.len()
            );
            println!("║ {} → {}", source_addr, dest_addr);
            println!(
                "║ QUIC first byte: 0x{:02x} ({})",
                quic_payload.get(0).copied().unwrap_or(0),
                if quic_payload
                    .get(0)
                    .map(|b| b & 0x80 != 0)
                    .unwrap_or(false)
                {
                    "Long Header"
                } else {
                    "Short Header"
                }
            );
            if quic_payload.len() >= 5 {
                let version = u32::from_be_bytes([
                    quic_payload[1],
                    quic_payload[2],
                    quic_payload[3],
                    quic_payload[4],
                ]);
                println!("║ QUIC version: 0x{:08x}", version);
            }
            println!("╟──────────────────────────────────────────────────────────────────────────────");
        }

        // Call compress_packet with debug flag to show tree traversal
        match compress_packet(
            &self.tree,
            &synthetic_packet,
            direction,
            &self.rules,
            &self.field_context,
            self.debug, // Pass debug flag to see tree traversal output
        ) {
            Ok(result) => {
                self.stats.packets_matched.fetch_add(1, Ordering::Relaxed);
                self.stats
                    .total_original_bits
                    .fetch_add(result.original_header_bits, Ordering::Relaxed);
                self.stats
                    .total_compressed_bits
                    .fetch_add(result.compressed_header_bits, Ordering::Relaxed);

                if self.debug {
                    let original_bytes = result.original_header_bits as f64 / 8.0;
                    let compressed_bytes = result.compressed_header_bits as f64 / 8.0;
                    let savings_bits = result.savings_bits();
                    let savings_pct = if result.original_header_bits > 0 {
                        100.0 * savings_bits as f64 / result.original_header_bits as f64
                    } else {
                        0.0
                    };

                    println!("╟──────────────────────────────────────────────────────────────────────────────");
                    println!("║ COMPRESSION RESULT");
                    println!("║ Rule: {}/{}", result.rule_id, result.rule_id_length);
                    println!("║ Original header:   {:>6} bits ({:>6.1} bytes)", result.original_header_bits, original_bytes);
                    println!("║ Compressed header: {:>6} bits ({:>6.1} bytes)", result.compressed_header_bits, compressed_bytes);
                    println!("║ Savings:           {:>6} bits ({:>5.1}%)", savings_bits, savings_pct);
                    println!("║ Original data:     {}", hex_preview(&result.original_header_data, 32));
                    println!("║ Compressed data:   {}", hex_preview(&result.data, 32));
                    println!("╚══════════════════════════════════════════════════════════════════════════════");
                }
            }
            Err(e) => {
                // No matching rule - packet not compressible
                if self.debug {
                    println!("║ NO MATCH: {:?}", e);
                    println!("╚══════════════════════════════════════════════════════════════════════════════");
                }
            }
        }
    }

    /// Build a packet for SCHC parsing using actual simulation addresses.
    ///
    /// The SCHC parser expects full Ethernet+IP+UDP frames.
    /// We construct proper headers using pnet_packet (same approach as pcap_exporter).
    fn build_synthetic_packet(
        &self,
        quic_payload: &[u8],
        source_addr: SocketAddr,
        dest_addr: SocketAddr,
    ) -> Vec<u8> {
        // Extract IPv4 addresses (simulation only uses IPv4)
        let IpAddr::V4(source_ip) = source_addr.ip() else {
            panic!("SCHC observer only supports IPv4");
        };
        let IpAddr::V4(dest_ip) = dest_addr.ip() else {
            panic!("SCHC observer only supports IPv4");
        };

        // Use a working buffer (similar to pcap_exporter)
        let mut buffer = vec![0u8; 2000];

        // Build UDP packet first
        let udp_packet_length = 8 + quic_payload.len() as u16;
        {
            let mut udp_writer = MutableUdpPacket::new(&mut buffer).unwrap();
            udp_writer.set_source(source_addr.port());
            udp_writer.set_destination(dest_addr.port());
            udp_writer.set_length(udp_packet_length);
            udp_writer.set_payload(quic_payload);
            let checksum = udp::ipv4_checksum(&udp_writer.to_immutable(), &source_ip, &dest_ip);
            udp_writer.set_checksum(checksum);
        }
        let udp_packet = buffer[0..udp_packet_length as usize].to_vec();

        // Build IPv4 packet with UDP as payload
        let ip_packet_length = 20 + udp_packet_length;
        {
            let mut ip_writer = MutableIpv4Packet::new(&mut buffer).unwrap();
            ip_writer.set_version(4);
            ip_writer.set_header_length(5); // No options
            ip_writer.set_dscp(0);
            ip_writer.set_ecn(0);
            ip_writer.set_total_length(ip_packet_length);
            ip_writer.set_identification(0);
            ip_writer.set_flags(0b010); // Don't fragment
            ip_writer.set_fragment_offset(0);
            ip_writer.set_ttl(64);
            ip_writer.set_next_level_protocol(IpNextHeaderProtocol::new(17)); // UDP
            ip_writer.set_source(source_ip);
            ip_writer.set_destination(dest_ip);
            ip_writer.set_payload(&udp_packet);
            let checksum = ipv4::checksum(&ip_writer.to_immutable());
            ip_writer.set_checksum(checksum);
        }
        let ip_packet = buffer[0..ip_packet_length as usize].to_vec();

        // Build final frame with Ethernet header
        let mut frame = Vec::with_capacity(14 + ip_packet.len());
        // Ethernet header (14 bytes)
        frame.extend_from_slice(&[0x00, 0x00, 0x00, 0x00, 0x00, 0x00]); // Dst MAC (placeholder)
        frame.extend_from_slice(&[0x00, 0x00, 0x00, 0x00, 0x00, 0x00]); // Src MAC (placeholder)
        frame.extend_from_slice(&[0x08, 0x00]); // EtherType: IPv4
        frame.extend_from_slice(&ip_packet);

        frame
    }

    /// Get statistics
    pub fn stats(&self) -> &SchcStats {
        &self.stats
    }
}

/// Format bytes as hex string with optional truncation
fn hex_preview(data: &[u8], max_bytes: usize) -> String {
    if data.is_empty() {
        return "(empty)".to_string();
    }
    
    let display_bytes = data.len().min(max_bytes);
    let hex: String = data[..display_bytes]
        .iter()
        .map(|b| format!("{:02x}", b))
        .collect::<Vec<_>>()
        .join(" ");
    
    if data.len() > max_bytes {
        format!("{} ... ({} bytes total)", hex, data.len())
    } else {
        format!("{} ({} bytes)", hex, data.len())
    }
}

/// Thread-safe wrapper for SCHC observer
pub type SharedSchcObserver = Arc<SchcObserver>;
