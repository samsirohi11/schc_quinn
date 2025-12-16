//! SCHC Compressor Mode
//!
//! Provides actual header compression and decompression for transmitted packets.
//! Compresses IP/UDP/QUIC headers, keeping Ethernet frame for routing.

use pnet_packet::ip::IpNextHeaderProtocol;
use pnet_packet::ipv4::MutableIpv4Packet;
use pnet_packet::udp::MutableUdpPacket;
use pnet_packet::{ipv4, udp};
use schc::{
    build_tree, compress_packet, decompress_packet, Direction, FieldContext, Rule, RuleSet,
    TreeNode,
};
use std::net::{IpAddr, SocketAddr};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

/// Statistics from SCHC compression operations
#[derive(Debug, Default)]
pub struct SchcCompressorStats {
    pub packets_compressed: AtomicUsize,
    pub packets_decompressed: AtomicUsize,
    pub compression_failures: AtomicUsize,
    pub decompression_failures: AtomicUsize,
    /// Total original header bits (IP+UDP+QUIC headers)
    pub total_original_header_bits: AtomicUsize,
    /// Total compressed header bits
    pub total_compressed_header_bits: AtomicUsize,
}

impl SchcCompressorStats {
    pub fn report(&self) {
        let compressed = self.packets_compressed.load(Ordering::Relaxed);
        let decompressed = self.packets_decompressed.load(Ordering::Relaxed);
        let comp_failures = self.compression_failures.load(Ordering::Relaxed);
        let decomp_failures = self.decompression_failures.load(Ordering::Relaxed);
        let original = self.total_original_header_bits.load(Ordering::Relaxed);
        let compressed_bits = self.total_compressed_header_bits.load(Ordering::Relaxed);
        let saved = original.saturating_sub(compressed_bits);

        println!("--- SCHC Compressor Statistics ---");
        println!("* Packets compressed: {}", compressed);
        println!("* Packets decompressed: {}", decompressed);
        println!("* Compression failures: {}", comp_failures);
        println!("* Decompression failures: {}", decomp_failures);
        println!(
            "* Total original header: {} bits ({:.1} bytes)",
            original,
            original as f64 / 8.0
        );
        println!(
            "* Total compressed header: {} bits ({:.1} bytes)",
            compressed_bits,
            compressed_bits as f64 / 8.0
        );
        if original > 0 {
            println!(
                "* Compression savings: {} bits ({:.1}%, ratio {:.2}:1)",
                saved,
                100.0 * saved as f64 / original as f64,
                original as f64 / compressed_bits.max(1) as f64
            );
        }
    }
}

/// Result of compressing a packet
#[derive(Debug, Clone)]
pub struct CompressResult {
    /// Combined SCHC data (rule ID + residues) + original payload
    pub compressed_packet: Vec<u8>,
    /// Size of original IP+UDP+QUIC headers in bytes
    pub original_header_size: usize,
    /// Size of compressed SCHC data in bytes
    pub compressed_header_size: usize,
    /// Rule ID that matched
    pub rule_id: u32,
    /// Whether compression was successful
    pub success: bool,
}

/// Result of decompressing a packet
#[derive(Debug, Clone)]
pub struct DecompressResult {
    /// Reconstructed IP+UDP headers + QUIC payload
    pub decompressed_packet: Vec<u8>,
    /// Rule ID that was used
    pub rule_id: u32,
}

/// SCHC Compressor for actual packet compression/decompression
pub struct SchcCompressor {
    tree: TreeNode,
    rules: Vec<Rule>,
    field_context: FieldContext,
    stats: SchcCompressorStats,
    debug: bool,
}

impl SchcCompressor {
    /// Create a new SCHC compressor from rules and field context files
    pub fn from_files(
        rules_path: &str,
        field_context_path: &str,
        debug: bool,
    ) -> anyhow::Result<Self> {
        let ruleset = RuleSet::from_file(rules_path)?;
        let field_context = FieldContext::from_file(field_context_path)?;
        let tree = build_tree(&ruleset.rules, &field_context);

        if debug {
            println!("\n--- SCHC Compressor Rule Tree ---");
            schc::display_tree(&tree);
        }

        Ok(Self {
            tree,
            rules: ruleset.rules,
            field_context,
            stats: SchcCompressorStats::default(),
            debug,
        })
    }

    /// Compress a QUIC packet.
    ///
    /// Takes the QUIC payload (what Quinn transmits) along with source/dest addresses.
    /// Builds a synthetic IP/UDP frame, compresses IP+UDP+QUIC headers.
    /// Returns compressed SCHC data + original payload (after QUIC headers).
    pub fn compress(
        &self,
        quic_payload: &[u8],
        source_addr: SocketAddr,
        dest_addr: SocketAddr,
        is_outgoing: bool,
        node_id: &str,
    ) -> CompressResult {
        // Build synthetic Ethernet+IP+UDP frame for SCHC compression
        let synthetic_packet = self.build_synthetic_packet(quic_payload, source_addr, dest_addr);

        let direction = if is_outgoing {
            Direction::Up
        } else {
            Direction::Down
        };

        if self.debug {
            let dir_str = if is_outgoing { "UP" } else { "DOWN" };
            println!(
                "\n[SCHC Compress] {} → {} [{}] payload: {} bytes",
                source_addr,
                dest_addr,
                dir_str,
                quic_payload.len()
            );
        }

        match compress_packet(
            &self.tree,
            &synthetic_packet,
            direction,
            &self.rules,
            &self.field_context,
            self.debug,
        ) {
            Ok(result) => {
                // The compressed result contains:
                // - result.data: the SCHC compressed header (rule ID + residues)
                // - We need to append the payload (data after the headers)

                // Calculate header sizes
                let ip_header_size = 20; // IPv4 basic header
                let udp_header_size = 8;
                let _ethernet_header_size = 14;

                // QUIC header size varies - we compressed it, residue is in result.data
                // The original_header_bits includes IP+UDP+QUIC headers
                let original_header_bytes = (result.original_header_bits + 7) / 8;
                let compressed_header_bytes = (result.compressed_header_bits + 7) / 8;

                // Calculate QUIC payload offset (after QUIC headers)
                // The QUIC headers we compressed are at the start of quic_payload
                // We need to extract just the application data
                let quic_header_bytes = original_header_bytes
                    .saturating_sub(ip_header_size + udp_header_size);
                let app_payload_start = quic_header_bytes.min(quic_payload.len());
                let app_payload = &quic_payload[app_payload_start..];

                // Build compressed packet: SCHC data + application payload
                let mut compressed_packet = result.data.clone();
                compressed_packet.extend_from_slice(app_payload);

                // Track header compression stats (like observer)
                self.stats.packets_compressed.fetch_add(1, Ordering::Relaxed);
                self.stats
                    .total_original_header_bits
                    .fetch_add(result.original_header_bits, Ordering::Relaxed);
                self.stats
                    .total_compressed_header_bits
                    .fetch_add(result.compressed_header_bits, Ordering::Relaxed);

                if self.debug {
                    let dir_str = if is_outgoing { "UP" } else { "DOWN" };
                    let original_bytes = (result.original_header_bits + 7) / 8;
                    let compressed_bytes = (result.compressed_header_bits + 7) / 8;
                    let saved_bytes = original_bytes.saturating_sub(compressed_bytes);
                    println!(
                        "[SCHC Compress @ {}] [{}] Full packet header: {} → {} bytes (saved {} bytes)",
                        node_id,
                        dir_str,
                        original_bytes,
                        compressed_bytes,
                        saved_bytes
                    );
                }

                CompressResult {
                    compressed_packet,
                    original_header_size: original_header_bytes,
                    compressed_header_size: compressed_header_bytes,
                    rule_id: result.rule_id,
                    success: true,
                }
            }
            Err(e) => {
                self.stats.compression_failures.fetch_add(1, Ordering::Relaxed);
                if self.debug {
                    println!("[SCHC Compress] Failed: {:?}", e);
                }
                CompressResult {
                    compressed_packet: quic_payload.to_vec(), // Return original on failure
                    original_header_size: 0,
                    compressed_header_size: 0,
                    rule_id: 0,
                    success: false,
                }
            }
        }
    }

    /// Decompress a SCHC packet back to QUIC payload.
    ///
    /// Takes compressed SCHC data + payload, reconstructs the original QUIC packet.
    pub fn decompress(
        &self,
        compressed_data: &[u8],
        is_outgoing: bool,
        node_id: &str,
    ) -> Result<DecompressResult, String> {
        let direction = if is_outgoing {
            Direction::Up
        } else {
            Direction::Down
        };

        // Try to decompress the SCHC packet
        // Note: We need to figure out where the payload starts (after SCHC residues)
        match decompress_packet(
            compressed_data,
            &self.rules,
            direction,
            &self.field_context,
            None, // Payload will be extracted from compressed_data
        ) {
            Ok(result) => {
                // The decompressed packet contains reconstructed IP+UDP+QUIC headers
                // We need to extract just the QUIC portion for Quinn
                
                // bits_consumed tells us how many bits were the SCHC data (rule ID + residues)
                let schc_bytes = (result.bits_consumed + 7) / 8;
                let payload_start = schc_bytes.min(compressed_data.len());
                let original_payload = &compressed_data[payload_start..];

                // Reconstruct QUIC packet from decompressed headers
                // The full_data contains the reconstructed IP+UDP+QUIC headers
                // We skip IP (20 bytes) and UDP (8 bytes) to get QUIC packet for Quinn
                let quic_start = 20 + 8; // IP + UDP headers
                let quic_header = if result.full_data.len() > quic_start {
                    &result.full_data[quic_start..]
                } else {
                    &[]
                };

                // Combine QUIC header + payload
                let mut decompressed_packet = quic_header.to_vec();
                decompressed_packet.extend_from_slice(original_payload);

                self.stats.packets_decompressed.fetch_add(1, Ordering::Relaxed);

                if self.debug {
                    // Show header restoration (reverse of compression)
                    let dir_str = if is_outgoing { "UP" } else { "DOWN" };
                    let compressed_bytes = (result.bits_consumed + 7) / 8;
                    let restored_bytes = result.header_data.len();
                    let restored_saved = restored_bytes.saturating_sub(compressed_bytes);
                    println!(
                        "[SCHC Decompress @ {}] [{}] Full packet header: {} → {} bytes (restored {} bytes)",
                        node_id,
                        dir_str,
                        compressed_bytes,
                        restored_bytes,
                        restored_saved
                    );
                }

                Ok(DecompressResult {
                    decompressed_packet,
                    rule_id: result.rule_id,
                })
            }
            Err(e) => {
                self.stats.decompression_failures.fetch_add(1, Ordering::Relaxed);
                if self.debug {
                    println!("[SCHC Decompress] Failed: {:?}", e);
                }
                Err(format!("Decompression failed: {:?}", e))
            }
        }
    }

    /// Build a synthetic Ethernet+IP+UDP packet for SCHC compression.
    fn build_synthetic_packet(
        &self,
        quic_payload: &[u8],
        source_addr: SocketAddr,
        dest_addr: SocketAddr,
    ) -> Vec<u8> {
        // Extract IPv4 addresses (simulation only uses IPv4)
        let IpAddr::V4(source_ip) = source_addr.ip() else {
            panic!("SCHC compressor only supports IPv4");
        };
        let IpAddr::V4(dest_ip) = dest_addr.ip() else {
            panic!("SCHC compressor only supports IPv4");
        };

        // Use a working buffer
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
        frame.extend_from_slice(&[0x00, 0x00, 0x00, 0x00, 0x00, 0x00]); // Dst MAC
        frame.extend_from_slice(&[0x00, 0x00, 0x00, 0x00, 0x00, 0x00]); // Src MAC
        frame.extend_from_slice(&[0x08, 0x00]); // EtherType: IPv4
        frame.extend_from_slice(&ip_packet);

        frame
    }

    /// Get statistics
    pub fn stats(&self) -> &SchcCompressorStats {
        &self.stats
    }
}

/// Thread-safe wrapper for SCHC compressor
pub type SharedSchcCompressor = Arc<SchcCompressor>;
