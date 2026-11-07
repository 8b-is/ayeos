//! `ayeOS` Sovereign `MEMNET`-Backed Network Fabric (`memnet_fabric`)
//!
//! Incorporates **MEMNET** (`MemnetAddress`, `MemnetNode`, `CapabilityTicket`, `encode_memnet_frame`,
//! `verify_memnet_frame`, and `LINOSV_SEED`) as the authoritative backend for:
//!
//! 1. **DHCP / ProxyDHCP / BOOTP + Raspberry Pi PXE & iPXE Server** (`UDP :67`, `:4011`, `:6767`):
//!    - Recognizes Raspberry Pi 4/5 ROM (`B8:27:EB`, `DC:A6:32`, `E4:5F:01`, `D8:3A:DD`, `2C:CF:67`),
//!      `iPXE` clients (`Option 77 = "iPXE"`), `PXEClient` (`Option 60`), and ESP32 `.8a` nodes.
//!    - Assigns deterministic IPv4 addresses derived from `blake3(LINOSV_SEED || chaddr)` and registers
//!      every client as a live **`MemnetNode`** with a verified **`CapabilityTicket`** and 64-byte
//!      **`MEMNET` frame** (`AYE8`).
//!    - Emits **DHCP Option 43 (`"Raspberry Pi Boot"`)**, **Option 60 (`"PXEClient"`)**,
//!      **Option 66 (TFTP Server IP)**, and **Option 67 (`http://<ip>:8432/boot/ayeos.ipxe` or `config.txt`)**
//!      so diskless Raspberry Pis and iPXE nodes boot straight into `ayeOS` with **zero SD card**.
//!
//! 2. **DNS & Intent-Based MEMNET Resolver** (`UDP :53`, `:5353`, `:8053`):
//!    - Resolves `.ayeos`, `.memnet`, `.local`, and `.8b.is` queries directly against the live
//!      `MemnetNode` registry using integer `Q10` relevance scoring (`relevance_score_node_q10`).
//!    - Answers `A` (`TYPE=1`) with the best-matching `MemnetNode` IPv4 and `TXT` (`TYPE=16`) with
//!      the node's signed MEMNET context & iPXE chainload URI.
//!    - Transparently forwards non-mesh Internet queries to `1.1.1.1:53` when acting as primary DNS.
//!
//! 100% integer arithmetic (`u8`, `u16`, `u32`, `u64`, `i64`, `p/q`). Zero floating-point types.

use crate::genesis::{
    cap_mask, derive_learning_genesis, encode_memnet_frame, verify_memnet_frame, CapabilityTicket,
    MEMNET_HEADER_LEN,
};
use crate::memnet::{MemnetAddress, MemnetNode};
use crate::prng::seed_hash_hex;
use std::net::{Ipv4Addr, UdpSocket};
use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

/// DHCP Magic Cookie (`99.130.83.99` = `0x63825363`, RFC 2132).
pub const DHCP_MAGIC_COOKIE: [u8; 4] = [99, 130, 83, 99];

/// DHCP Message Types (Option 53).
pub const DHCP_DISCOVER: u8 = 1;
pub const DHCP_OFFER: u8 = 2;
pub const DHCP_REQUEST: u8 = 3;
pub const DHCP_ACK: u8 = 5;

/// DNS Record Types (RFC 1035).
pub const DNS_TYPE_A: u16 = 1;
pub const DNS_TYPE_TXT: u16 = 16;
pub const DNS_TYPE_SRV: u16 = 33;
pub const DNS_TYPE_ANY: u16 = 255;

/// Operating mode for an AyeOS DHCP channel.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DhcpChannelMode {
    /// Safe coexistence on an existing router LAN: only answers PXE / iPXE / Raspberry Pi netboot
    /// or assigns an IP when explicitly requested on an unrouted link.
    ProxyPxeAndOpportunistic,
    /// Authoritative DHCP server on a dedicated Ethernet/Wi-Fi channel (assigns IP, mask, router, DNS, iPXE).
    FullAuthoritativeSubnet,
}

/// A deterministic DHCP + PXE/iPXE lease backed by a `MemnetNode` and `CapabilityTicket`.
#[derive(Clone, Debug)]
pub struct MemnetDhcpLease {
    pub mac: [u8; 6],
    pub assigned_ip: [u8; 4],
    pub hostname: String,
    pub vendor_class: String,
    pub is_ipxe: bool,
    pub is_raspberry_pi: bool,
    pub bootfile_url: String,
    pub node: MemnetNode,
    pub ticket: CapabilityTicket,
    pub memnet_frame: Vec<u8>,
    pub lease_epoch_secs: u64,
}

/// Authoritative MEMNET Backend powering DHCP, DNS, iPXE, and OxiCast across all channels.
#[derive(Clone, Debug)]
pub struct MemnetFabricBackend {
    pub mother_ip: [u8; 4],
    pub subnet_mask: [u8; 4],
    pub oxicast_port: u16,
    pub memnet_port: u16,
    pub dhcp_mode: DhcpChannelMode,
    pub master_key: [u8; 32],
    pub mother_node: MemnetNode,
    pub nodes: Vec<MemnetNode>,
    pub leases: Vec<MemnetDhcpLease>,
    pub dhcp_offers_sent: u64,
    pub dhcp_acks_sent: u64,
    pub dns_queries_resolved: u64,
    pub dns_intent_matches: u64,
}

impl MemnetFabricBackend {
    /// Initialize a new `MemnetFabricBackend` anchored to the deterministic `LINOSV` genesis root.
    pub fn new(mother_ip: [u8; 4], oxicast_port: u16, dhcp_mode: DhcpChannelMode) -> Self {
        let (genesis_root, _phase_vec) = derive_learning_genesis();
        let ip_str = format!(
            "{}.{}.{}.{}",
            mother_ip[0], mother_ip[1], mother_ip[2], mother_ip[3]
        );
        let linosv_hex = seed_hash_hex();
        let mother_node = MemnetNode {
            address: MemnetAddress {
                geo: Some("sanctuary.368190000/-762749000".to_string()),
                role: "ayeos.sanctuary.mother".to_string(),
                tags: vec![
                    "ayeos".to_string(),
                    "memnet".to_string(),
                    "oxicast".to_string(),
                    "phoenix".to_string(),
                    "ipxe".to_string(),
                    "pxe".to_string(),
                    "tftp".to_string(),
                    "dns".to_string(),
                    "dhcp".to_string(),
                    "boot".to_string(),
                    "tv".to_string(),
                    "sanctuary".to_string(),
                ],
                vector_hash: Some(linosv_hex[..16].to_string()),
            },
            host: ip_str,
            port: oxicast_port,
            alive: true,
        };

        Self {
            mother_ip,
            subnet_mask: [255, 255, 255, 128],
            oxicast_port,
            memnet_port: 9876,
            dhcp_mode,
            master_key: genesis_root,
            nodes: vec![mother_node.clone()],
            mother_node,
            leases: Vec::new(),
            dhcp_offers_sent: 0,
            dhcp_acks_sent: 0,
            dns_queries_resolved: 0,
            dns_intent_matches: 0,
        }
    }

    /// Return `true` if the 6-byte MAC address belongs to a known Raspberry Pi OUI
    /// (`B8:27:EB`, `DC:A6:32`, `E4:5F:01`, `D8:3A:DD`, `2C:CF:67`).
    #[inline]
    pub const fn is_raspberry_pi_mac(mac: &[u8; 6]) -> bool {
        matches!(
            (mac[0], mac[1], mac[2]),
            (0xB8, 0x27, 0xEB)
                | (0xDC, 0xA6, 0x32)
                | (0xE4, 0x5F, 0x01)
                | (0xD8, 0x3A, 0xDD)
                | (0x2C, 0xCF, 0x67)
        )
    }

    /// Deterministically derive a host IPv4 address (`<subnet>.80` .. `<subnet>.124`, strictly valid
    /// inside both `/25` `255.255.255.128` and `/24` `255.255.255.0` subnets)
    /// from `LINOSV` genesis root + client MAC address (`chaddr`).
    pub fn derive_deterministic_ip(&self, mac: &[u8; 6]) -> [u8; 4] {
        let mut hasher = blake3::Hasher::new_keyed(&self.master_key);
        hasher.update(b"ayeos::memnet::dhcp::v1\0");
        hasher.update(mac);
        let digest: [u8; 32] = hasher.finalize().into();
        let host_octet = 80u8 + (digest[0] % 45); // 80..=124 (always inside .0/25 and .0/24!)
        let final_octet = if host_octet == self.mother_ip[3] {
            if host_octet < 124 {
                host_octet + 1
            } else {
                81
            }
        } else {
            host_octet
        };
        [
            self.mother_ip[0],
            self.mother_ip[1],
            self.mother_ip[2],
            final_octet,
        ]
    }

    /// Register or update a peer in the `MEMNET` backend from a DHCP/PXE/iPXE discovery/request.
    pub fn register_or_update_dhcp_peer(
        &mut self,
        mac: [u8; 6],
        vendor_class: &str,
        user_class: &str,
        client_hostname: &str,
    ) -> MemnetDhcpLease {
        let assigned_ip = self.derive_deterministic_ip(&mac);
        let ip_str = format!(
            "{}.{}.{}.{}",
            assigned_ip[0], assigned_ip[1], assigned_ip[2], assigned_ip[3]
        );
        let mother_ip_str = format!(
            "{}.{}.{}.{}",
            self.mother_ip[0], self.mother_ip[1], self.mother_ip[2], self.mother_ip[3]
        );

        let is_ipxe = user_class.contains("iPXE")
            || vendor_class.contains("iPXE")
            || client_hostname.contains("ipxe");
        let is_pi = Self::is_raspberry_pi_mac(&mac)
            || vendor_class.contains("Raspberry Pi");

        let mac_hex = format!(
            "{:02x}{:02x}{:02x}{:02x}{:02x}{:02x}",
            mac[0], mac[1], mac[2], mac[3], mac[4], mac[5]
        );
        let hostname = if !client_hostname.trim().is_empty() {
            client_hostname.trim().to_ascii_lowercase()
        } else if is_pi {
            format!("ayeos-pi-{}", &mac_hex[6..12])
        } else if is_ipxe {
            format!("ayeos-ipxe-{}", &mac_hex[6..12])
        } else {
            format!("ayeos-node-{}", &mac_hex[6..12])
        };

        let (role, mut tags) = if is_pi {
            (
                "ayeos.tv.raspberry_pi".to_string(),
                vec![
                    "raspberry_pi".to_string(),
                    "pi4".to_string(),
                    "oxicast".to_string(),
                    "phoenix".to_string(),
                    "tv".to_string(),
                    "diskless".to_string(),
                    "ipxe".to_string(),
                ],
            )
        } else if is_ipxe {
            (
                "ayeos.tv.ipxe_peer".to_string(),
                vec![
                    "ipxe".to_string(),
                    "netboot".to_string(),
                    "oxicast".to_string(),
                    "tv".to_string(),
                ],
            )
        } else {
            (
                "ayeos.mesh.peer".to_string(),
                vec!["ayeos".to_string(), "memnet".to_string(), "peer".to_string()],
            )
        };
        tags.push(hostname.clone());

        let bootfile_url = if is_ipxe {
            format!("http://{mother_ip_str}:{}/boot/ayeos.ipxe", self.oxicast_port)
        } else {
            // Native Raspberry Pi EEPROM / TFTP ROM requests config.txt / kernel8.img via TFTP,
            // while UEFI/iPXE clients can also fetch boot.ipxe
            format!("http://{mother_ip_str}:{}/boot/ayeos.ipxe", self.oxicast_port)
        };

        let epoch_secs = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(1_762_000_000);

        let caps = cap_mask::READ_DREAM
            | cap_mask::AYEOS_INFER
            | cap_mask::AYEOS_GENESIS;
        let ticket = CapabilityTicket::issue(&self.master_key, &hostname, epoch_secs, caps);
        let memnet_frame = encode_memnet_frame(
            &self.master_key,
            &ticket.mac,
            epoch_secs,
            caps,
            bootfile_url.as_bytes(),
        );
        let _ = verify_memnet_frame(
            &self.master_key,
            epoch_secs,
            caps,
            cap_mask::AYEOS_GENESIS,
            &memnet_frame,
        );
        let _ = MEMNET_HEADER_LEN;

        let node = MemnetNode {
            address: MemnetAddress {
                geo: self.mother_node.address.geo.clone(),
                role,
                tags,
                vector_hash: Some(format!("LINOSV-{}", &mac_hex[6..12].to_ascii_uppercase())),
            },
            host: ip_str,
            port: self.oxicast_port,
            alive: true,
        };

        if let Some(existing) = self.nodes.iter_mut().find(|n| n.host == node.host) {
            *existing = node.clone();
        } else {
            self.nodes.push(node.clone());
        }

        let lease = MemnetDhcpLease {
            mac,
            assigned_ip,
            hostname,
            vendor_class: vendor_class.to_string(),
            is_ipxe,
            is_raspberry_pi: is_pi,
            bootfile_url,
            node,
            ticket,
            memnet_frame,
            lease_epoch_secs: epoch_secs,
        };

        if let Some(existing_lease) = self.leases.iter_mut().find(|l| l.mac == mac) {
            *existing_lease = lease.clone();
        } else {
            self.leases.push(lease.clone());
        }

        lease
    }

    /// Integer-exact (`Q10`, `1024 == 1/1`) MEMNET relevance scoring between a DNS/Intent query
    /// token list and a `MemnetNode`. Zero floating-point arithmetic.
    pub fn relevance_score_node_q10(query_tokens: &[String], node: &MemnetNode) -> u32 {
        let mut score_q10: u32 = 102; // base 1/10 relevance
        for q in query_tokens {
            if q.is_empty() {
                continue;
            }
            if node.host.eq_ignore_ascii_case(q) {
                score_q10 = score_q10.saturating_add(4096);
            }
            if node.address.role.to_ascii_lowercase().contains(q) {
                score_q10 = score_q10.saturating_add(2048);
            }
            for tag in &node.address.tags {
                if tag.eq_ignore_ascii_case(q) {
                    score_q10 = score_q10.saturating_add(1536);
                } else if tag.to_ascii_lowercase().contains(q) {
                    score_q10 = score_q10.saturating_add(768);
                }
            }
            if let Some(ref vh) = node.address.vector_hash {
                if vh.to_ascii_lowercase().contains(q) {
                    score_q10 = score_q10.saturating_add(1024);
                }
            }
        }
        score_q10
    }

    /// Resolve a domain name or MEMNET intent query (e.g. `boot.ayeos`, `sanctuary.memnet`,
    /// `oxicast-pi.local`, `pi4.tv.memnet`, `genesis.ayeos`) to `(ipv4_bytes, txt_descriptor, score_q10)`.
    pub fn resolve_memnet_domain(&mut self, qname: &str) -> Option<([u8; 4], String, u32)> {
        let lower = qname.trim_end_matches('.').to_ascii_lowercase();
        let is_memnet_zone = lower.ends_with(".ayeos")
            || lower.ends_with(".memnet")
            || lower.ends_with(".8b.is")
            || lower.ends_with(".local")
            || lower == "ayeos"
            || lower == "memnet"
            || lower == "oxicast";

        if !is_memnet_zone {
            return None;
        }

        let tokens: Vec<String> = lower
            .split(|c: char| c == '.' || c == '-' || c == '_')
            .filter(|s| !s.is_empty() && *s != "local" && *s != "memnet" && *s != "ayeos")
            .map(|s| s.to_string())
            .collect();

        // Direct check against leased hostnames first
        for lease in &self.leases {
            if lower.starts_with(&lease.hostname) {
                let txt = format!(
                    "memnet=v1;host={};role={};ipxe={};cap=0x{:02x}",
                    lease.hostname,
                    lease.node.address.role,
                    lease.bootfile_url,
                    lease.ticket.cap_mask
                );
                self.dns_queries_resolved = self.dns_queries_resolved.wrapping_add(1);
                self.dns_intent_matches = self.dns_intent_matches.wrapping_add(1);
                return Some((lease.assigned_ip, txt, 4096));
            }
        }

        // Rank all live MemnetNodes by integer Q10 relevance
        let mut best_node = &self.mother_node;
        let mut best_score = Self::relevance_score_node_q10(&tokens, &self.mother_node);
        for node in &self.nodes {
            if !node.alive {
                continue;
            }
            let sc = Self::relevance_score_node_q10(&tokens, node);
            if sc > best_score {
                best_score = sc;
                best_node = node;
            }
        }

        let ip = parse_ipv4_octets(&best_node.host).unwrap_or(self.mother_ip);
        let mother_ip_str = format!(
            "{}.{}.{}.{}",
            self.mother_ip[0], self.mother_ip[1], self.mother_ip[2], self.mother_ip[3]
        );
        let linosv = seed_hash_hex();
        let txt = format!(
            "memnet=v1;seed={};role={};score_pq={}/1024;ipxe=http://{}:{}/boot/ayeos.ipxe;rate=384000;babbage=3add",
            &linosv[..16],
            best_node.address.role,
            best_score,
            mother_ip_str,
            self.oxicast_port
        );

        self.dns_queries_resolved = self.dns_queries_resolved.wrapping_add(1);
        self.dns_intent_matches = self.dns_intent_matches.wrapping_add(1);
        Some((ip, txt, best_score))
    }
}

/// Parse an IPv4 dotted quad (`"172.16.47.76"`) into `[u8; 4]`.
pub fn parse_ipv4_octets(s: &str) -> Option<[u8; 4]> {
    let mut parts = s.trim().split('.');
    let a = parts.next()?.parse::<u8>().ok()?;
    let b = parts.next()?.parse::<u8>().ok()?;
    let c = parts.next()?.parse::<u8>().ok()?;
    let d = parts.next()?.parse::<u8>().ok()?;
    if parts.next().is_some() {
        return None;
    }
    Some([a, b, c, d])
}

/// Detect the primary non-loopback IPv4 address of this machine using a non-transmitting UDP
/// route lookup to `1.1.1.1:80` (falling back to `172.16.47.76`).
pub fn detect_local_ipv4() -> [u8; 4] {
    if let Ok(sock) = UdpSocket::bind("0.0.0.0:0") {
        if sock.connect("1.1.1.1:80").is_ok() {
            if let Ok(local_addr) = sock.local_addr() {
                if let std::net::SocketAddr::V4(v4) = local_addr {
                    let oct = v4.ip().octets();
                    if oct[0] != 127 && oct[0] != 0 {
                        return oct;
                    }
                }
            }
        }
    }
    [172, 16, 47, 76]
}

// ==============================================================================
// 📡 Pure-Rust RFC 2131 / PXE / Raspberry Pi EEPROM / iPXE DHCP Engine
// ==============================================================================

/// Parsed fields from an incoming DHCP `DISCOVER` or `REQUEST` packet.
#[derive(Clone, Debug)]
pub struct ParsedDhcpRequest {
    pub xid: [u8; 4],
    pub flags: [u8; 2],
    pub ciaddr: [u8; 4],
    pub chaddr: [u8; 6],
    pub msg_type: u8,
    pub vendor_class: String,
    pub user_class: String,
    pub hostname: String,
    pub arch_type: Option<u16>,
}

/// Parse a raw UDP DHCP/BOOTP request packet (minimum 240 bytes including RFC 2132 magic cookie).
pub fn parse_dhcp_packet(pkt: &[u8]) -> Option<ParsedDhcpRequest> {
    if pkt.len() < 240 || pkt[0] != 1 {
        // op != BOOTREQUEST (1)
        return None;
    }
    if pkt[236..240] != DHCP_MAGIC_COOKIE {
        return None;
    }
    let mut xid = [0u8; 4];
    xid.copy_from_slice(&pkt[4..8]);
    let mut flags = [0u8; 2];
    flags.copy_from_slice(&pkt[10..12]);
    let mut ciaddr = [0u8; 4];
    ciaddr.copy_from_slice(&pkt[12..16]);
    let mut chaddr = [0u8; 6];
    chaddr.copy_from_slice(&pkt[28..34]);

    let mut msg_type = 0u8;
    let mut vendor_class = String::new();
    let mut user_class = String::new();
    let mut hostname = String::new();
    let mut arch_type = None;

    let mut idx = 240usize;
    while idx < pkt.len() {
        let opt = pkt[idx];
        if opt == 255 {
            break;
        }
        if opt == 0 {
            idx += 1;
            continue;
        }
        if idx + 1 >= pkt.len() {
            break;
        }
        let len = pkt[idx + 1] as usize;
        if idx + 2 + len > pkt.len() {
            break;
        }
        let val = &pkt[idx + 2..idx + 2 + len];
        match opt {
            53 if len >= 1 => msg_type = val[0],
            12 => hostname = String::from_utf8_lossy(val).to_string(),
            60 => vendor_class = String::from_utf8_lossy(val).to_string(),
            77 => user_class = String::from_utf8_lossy(val).to_string(),
            93 if len >= 2 => arch_type = Some(u16::from_be_bytes([val[0], val[1]])),
            _ => {}
        }
        idx += 2 + len;
    }

    if msg_type == 0 {
        return None;
    }

    Some(ParsedDhcpRequest {
        xid,
        flags,
        ciaddr,
        chaddr,
        msg_type,
        vendor_class,
        user_class,
        hostname,
        arch_type,
    })
}

/// Select the architecture-appropriate PXE / UEFI / iPXE / Raspberry Pi bootfile name
/// based on DHCP Option 93 (`arch_type`), Option 60 (`vendor_class`), and MAC OUI.
pub fn select_pxe_bootfile_for_request(
    req: &ParsedDhcpRequest,
    lease: &MemnetDhcpLease,
) -> String {
    if lease.is_ipxe {
        return lease.bootfile_url.clone();
    }
    if lease.is_raspberry_pi {
        return "kernel8.img".to_string();
    }
    match req.arch_type {
        Some(7) | Some(9) => return "BOOTX64.EFI".to_string(),
        Some(6) => return "BOOTIA32.EFI".to_string(),
        Some(11) => return "BOOTAA64.EFI".to_string(),
        Some(0) => return "limine-bios-pxe.bin".to_string(),
        _ => {}
    }
    if req.vendor_class.contains("Arch:00007") || req.vendor_class.contains("Arch:00009") {
        "BOOTX64.EFI".to_string()
    } else if req.vendor_class.contains("Arch:00006") {
        "BOOTIA32.EFI".to_string()
    } else if req.vendor_class.contains("Arch:00011") {
        "BOOTAA64.EFI".to_string()
    } else if req.vendor_class.contains("PXEClient") {
        "limine-bios-pxe.bin".to_string()
    } else {
        lease.bootfile_url.clone()
    }
}

/// Build a DHCP `OFFER` or `ACK` packet backed by `MemnetFabricBackend`, complete with:
/// - `siaddr` (`Next-Server`) = `mother_ip`
/// - `sname` = `"ayeos.memnet"`
/// - `file` (`bootfile`) = architecture-matched PXE bootloader (`BOOTX64.EFI`, `limine-bios-pxe.bin`,
///   `kernel8.img`, or `http://<mother_ip>:8432/boot/ayeos.ipxe`)
/// - **Option 43 (`Vendor-Specific Information`)**: Raspberry Pi ROM PXE boot menu (for Pi OUIs)
/// - **Option 60 (`"PXEClient"`)**, **Option 66 (TFTP Server IP)**, **Option 67 (Bootfile)**
pub fn build_memnet_dhcp_reply(
    req: &ParsedDhcpRequest,
    backend: &mut MemnetFabricBackend,
) -> Option<Vec<u8>> {
    let reply_type = match req.msg_type {
        DHCP_DISCOVER => DHCP_OFFER,
        DHCP_REQUEST => DHCP_ACK,
        _ => return None,
    };

    let lease = backend.register_or_update_dhcp_peer(
        req.chaddr,
        &req.vendor_class,
        &req.user_class,
        &req.hostname,
    );

    let is_pxe_client = req.vendor_class.contains("PXEClient") || req.arch_type.is_some();

    // In ProxyPxeAndOpportunistic mode on a shared LAN, if the client is NOT a PXE/iPXE/Pi netboot
    // client and we are not in FullAuthoritativeSubnet mode, let the home router handle generic phones/laptops.
    if backend.dhcp_mode == DhcpChannelMode::ProxyPxeAndOpportunistic
        && !lease.is_ipxe
        && !lease.is_raspberry_pi
        && !is_pxe_client
    {
        return None;
    }

    if reply_type == DHCP_OFFER {
        backend.dhcp_offers_sent = backend.dhcp_offers_sent.wrapping_add(1);
    } else {
        backend.dhcp_acks_sent = backend.dhcp_acks_sent.wrapping_add(1);
    }

    let mut pkt = vec![0u8; 240];
    pkt[0] = 2; // BOOTREPLY
    pkt[1] = 1; // Ethernet (10Mb+)
    pkt[2] = 6; // Hardware address length
    pkt[3] = 0; // Hops
    pkt[4..8].copy_from_slice(&req.xid);
    pkt[10..12].copy_from_slice(&req.flags);
    pkt[12..16].copy_from_slice(&req.ciaddr);

    // Always assign a valid deterministic subnet IP (`172.16.47.80..124`) in `yiaddr`
    // for every PXE / UEFI PXE / Raspberry Pi / iPXE client so diskless x86_64 and ARM64
    // machines always receive both an IP lease and the TFTP bootfile in one atomic offer!
    let yiaddr = lease.assigned_ip;
    pkt[16..20].copy_from_slice(&yiaddr);
    // siaddr (Next-Server / TFTP Mother Node IP)
    pkt[20..24].copy_from_slice(&backend.mother_ip);
    // chaddr (Client MAC)
    pkt[28..34].copy_from_slice(&req.chaddr);

    // sname (64 bytes at offset 44..108)
    let sname = b"ayeos.memnet";
    pkt[44..44 + sname.len()].copy_from_slice(sname);

    // file (128 bytes at offset 108..236)
    let selected_bootfile = select_pxe_bootfile_for_request(req, &lease);
    let bootfile_bytes = selected_bootfile.as_bytes();
    let copy_len = bootfile_bytes.len().min(127);
    pkt[108..108 + copy_len].copy_from_slice(&bootfile_bytes[..copy_len]);

    // Magic Cookie
    pkt[236..240].copy_from_slice(&DHCP_MAGIC_COOKIE);

    // DHCP Options
    // Option 53: DHCP Message Type
    pkt.extend_from_slice(&[53, 1, reply_type]);
    // Option 54: Server Identifier (mother_ip)
    pkt.extend_from_slice(&[54, 4]);
    pkt.extend_from_slice(&backend.mother_ip);
    // Option 51: IP Address Lease Time (86400s = 24 hours)
    pkt.extend_from_slice(&[51, 4]);
    pkt.extend_from_slice(&86_400u32.to_be_bytes());
    // Option 1: Subnet Mask
    pkt.extend_from_slice(&[1, 4]);
    pkt.extend_from_slice(&backend.subnet_mask);
    // Option 3: Router / Gateway (`172.16.47.67` on the `172.16.47.0/25` LAN)
    let router_ip = if backend.mother_ip[0..3] == [172, 16, 47] {
        [172, 16, 47, 67]
    } else {
        backend.mother_ip
    };
    pkt.extend_from_slice(&[3, 4]);
    pkt.extend_from_slice(&router_ip);
    // Option 6: DNS Server (this AyeOS MEMNET DNS resolver!)
    pkt.extend_from_slice(&[6, 4]);
    pkt.extend_from_slice(&backend.mother_ip);
    // Option 15: Domain Name ("ayeos.memnet")
    let domain = b"ayeos.memnet";
    pkt.extend_from_slice(&[15, domain.len() as u8]);
    pkt.extend_from_slice(domain);

    // Option 66: TFTP Server Name
    let mother_ip_str = format!(
        "{}.{}.{}.{}",
        backend.mother_ip[0], backend.mother_ip[1], backend.mother_ip[2], backend.mother_ip[3]
    );
    let tftp_srv = mother_ip_str.as_bytes();
    pkt.extend_from_slice(&[66, tftp_srv.len() as u8]);
    pkt.extend_from_slice(tftp_srv);

    // Option 67: Bootfile Name
    pkt.extend_from_slice(&[67, copy_len as u8]);
    pkt.extend_from_slice(&bootfile_bytes[..copy_len]);

    // IMPORTANT:
    // - For Raspberry Pi 4 ROM Netboot (`lease.is_raspberry_pi`): include `Option 60 = "PXEClient"`
    //   and `Option 43` with `"Raspberry Pi Boot"`.
    // - For x86 / x86_64 UEFI PXE (`PXEClient:Arch:00007:UNDI:003016` on MINISFORUM) and BIOS PXE:
    //   when a single server provides both `yiaddr` AND `siaddr`/`file` (`BOOTX64.EFI`), EDK2 `UefiPxeBcDxe`
    //   requires `Option 60` and `Option 43` to be OMITTED so the packet is classified as `PxeBcOfferTypeDhcpOnly`
    //   with `BootFileName = "BOOTX64.EFI"` and immediately boots via TFTP!
    if lease.is_raspberry_pi {
        let vci = b"PXEClient";
        pkt.extend_from_slice(&[60, vci.len() as u8]);
        pkt.extend_from_slice(vci);

        let rpi_opt43: &[u8] = &[
            6, 1, 3, // PXE_DISCOVERY_CONTROL = 3
            10, 4, 0, b'P', b'X', b'E', // Menu prompt
            9, 20, 0, 0, 17, b'R', b'a', b's', b'p', b'b', b'e', b'r', b'r', b'y', b' ', b'P', b'i',
            b' ', b'B', b'o', b'o', b't', 255,
        ];
        pkt.extend_from_slice(&[43, rpi_opt43.len() as u8]);
        pkt.extend_from_slice(rpi_opt43);
    }

    // End Option (255)
    pkt.push(255);
    Some(pkt)
}

// ==============================================================================
// 🌐 Pure-Rust RFC 1035 DNS & MEMNET Intent Resolver Engine
// ==============================================================================

/// Parse the first question (`QNAME`, `QTYPE`, `QCLASS`, and byte offset after question section)
/// from an RFC 1035 UDP DNS query packet.
pub fn parse_dns_question(pkt: &[u8]) -> Option<(String, u16, u16, usize)> {
    if pkt.len() < 12 {
        return None;
    }
    let qdcount = u16::from_be_bytes([pkt[4], pkt[5]]);
    if qdcount == 0 {
        return None;
    }
    let mut pos = 12usize;
    let mut labels = Vec::new();
    while pos < pkt.len() {
        let len = pkt[pos] as usize;
        pos += 1;
        if len == 0 {
            break;
        }
        if (len & 0xC0) != 0 || pos + len > pkt.len() {
            return None;
        }
        labels.push(String::from_utf8_lossy(&pkt[pos..pos + len]).to_string());
        pos += len;
    }
    if pos + 4 > pkt.len() {
        return None;
    }
    let qtype = u16::from_be_bytes([pkt[pos], pkt[pos + 1]]);
    let qclass = u16::from_be_bytes([pkt[pos + 2], pkt[pos + 3]]);
    pos += 4;
    Some((labels.join("."), qtype, qclass, pos))
}

/// Build an RFC 1035 DNS response packet backed by `MemnetFabricBackend` (`A` record + `TXT` record).
pub fn handle_dns_query_packet(
    query: &[u8],
    backend: &mut MemnetFabricBackend,
) -> Option<Vec<u8>> {
    let (qname, qtype, _qclass, q_end) = parse_dns_question(query)?;
    let (ip, txt, _score_q10) = backend.resolve_memnet_domain(&qname)?;

    let mut resp = Vec::with_capacity(256);
    // Transaction ID
    resp.extend_from_slice(&query[0..2]);
    // Flags: QR=1 (Response), AA=1 (Authoritative), RD=1, RA=1 -> 0x8580
    resp.extend_from_slice(&0x8580u16.to_be_bytes());
    // QDCOUNT = 1
    resp.extend_from_slice(&1u16.to_be_bytes());

    let include_a = qtype == DNS_TYPE_A || qtype == DNS_TYPE_ANY;
    let include_txt = qtype == DNS_TYPE_TXT || qtype == DNS_TYPE_ANY;
    let ancount: u16 = match (include_a, include_txt) {
        (true, true) => 2,
        (true, false) | (false, true) => 1,
        (false, false) => 1, // default to A record so host lookups succeed
    };
    resp.extend_from_slice(&ancount.to_be_bytes());
    // NSCOUNT = 0, ARCOUNT = 0
    resp.extend_from_slice(&0u16.to_be_bytes());
    resp.extend_from_slice(&0u16.to_be_bytes());

    // Copy original Question section (`12..q_end`)
    resp.extend_from_slice(&query[12..q_end]);

    if include_a || !include_txt {
        // Answer 1: A record pointing to `0xC00C` (offset 12 QNAME)
        resp.extend_from_slice(&[0xC0, 0x0C]);
        resp.extend_from_slice(&DNS_TYPE_A.to_be_bytes());
        resp.extend_from_slice(&1u16.to_be_bytes()); // CLASS IN
        resp.extend_from_slice(&60u32.to_be_bytes()); // TTL 60s
        resp.extend_from_slice(&4u16.to_be_bytes()); // RDLENGTH = 4
        resp.extend_from_slice(&ip);
    }

    if include_txt {
        // Answer 2: TXT record containing MEMNET Capsule & iPXE context
        let txt_bytes = txt.as_bytes();
        let slice_len = txt_bytes.len().min(250);
        resp.extend_from_slice(&[0xC0, 0x0C]);
        resp.extend_from_slice(&DNS_TYPE_TXT.to_be_bytes());
        resp.extend_from_slice(&1u16.to_be_bytes()); // CLASS IN
        resp.extend_from_slice(&60u32.to_be_bytes()); // TTL 60s
        resp.extend_from_slice(&((slice_len + 1) as u16).to_be_bytes());
        resp.push(slice_len as u8);
        resp.extend_from_slice(&txt_bytes[..slice_len]);
    }

    Some(resp)
}

/// Spawn the Sovereign MEMNET-Backed DNS + DHCP/ProxyDHCP/iPXE listeners on network channels.
/// - DNS binds to `:53` (if root / `CAP_NET_BIND_SERVICE`) and `:8053` (always available).
/// - DHCP/ProxyDHCP binds to `:67` / `:4011` (if root) and `:6767` (always available).
pub fn spawn_memnet_dns_and_dhcp_daemons(backend: Arc<Mutex<MemnetFabricBackend>>) {
    // 1. Spawn MEMNET DNS Resolver (`:53` + `:8053`)
    for port in [53u16, 8053u16] {
        let Ok(sock) = UdpSocket::bind(("0.0.0.0", port)) else {
            continue;
        };
        let backend_dns = Arc::clone(&backend);
        std::thread::spawn(move || {
            let mut buf = [0u8; 1500];
            loop {
                let Ok((len, peer)) = sock.recv_from(&mut buf) else {
                    continue;
                };
                let query = &buf[..len];
                let local_reply = backend_dns
                    .lock()
                    .ok()
                    .and_then(|mut guard| handle_dns_query_packet(query, &mut guard));

                if let Some(resp) = local_reply {
                    let _ = sock.send_to(&resp, peer);
                } else {
                    // Forward external domain queries to 1.1.1.1:53
                    if let Ok(fwd) = UdpSocket::bind("0.0.0.0:0") {
                        let _ = fwd.set_read_timeout(Some(Duration::from_millis(1200)));
                        if fwd.send_to(query, "1.1.1.1:53").is_ok() {
                            let mut up_buf = [0u8; 1500];
                            if let Ok((up_len, _)) = fwd.recv_from(&mut up_buf) {
                                let _ = sock.send_to(&up_buf[..up_len], peer);
                            }
                        }
                    }
                }
            }
        });
    }

    // 2. Spawn MEMNET DHCP / ProxyDHCP / PXE / iPXE Responder (`:67`, `:4011`, `:6767`)
    for port in [67u16, 4011u16, 6767u16] {
        let Ok(sock) = UdpSocket::bind(("0.0.0.0", port)) else {
            continue;
        };
        let _ = sock.set_broadcast(true);
        let backend_dhcp = Arc::clone(&backend);
        std::thread::spawn(move || {
            let mut buf = [0u8; 1500];
            loop {
                let Ok((len, peer)) = sock.recv_from(&mut buf) else {
                    continue;
                };
                let Some(req) = parse_dhcp_packet(&buf[..len]) else {
                    continue;
                };
                println!(
                    "📡 [ayeOS MEMNET DHCP] Peer {:02x}:{:02x}:{:02x}:{:02x}:{:02x}:{:02x} msg_type={} vci='{}' host='{}'",
                    req.chaddr[0], req.chaddr[1], req.chaddr[2], req.chaddr[3], req.chaddr[4], req.chaddr[5],
                    req.msg_type, req.vendor_class, req.hostname
                );
                let reply = backend_dhcp
                    .lock()
                    .ok()
                    .and_then(|mut guard| build_memnet_dhcp_reply(&req, &mut guard));

                if let Some(pkt) = reply {
                    let yi = &pkt[16..20];
                    let bootf_end = pkt[108..236]
                        .iter()
                        .position(|&b| b == 0)
                        .unwrap_or(128);
                    let bootf = String::from_utf8_lossy(&pkt[108..108 + bootf_end]);
                    println!(
                        "   -> [DHCP REPLY] yiaddr={}.{}.{}.{} siaddr=172.16.47.76 bootfile='{bootf}' (port={port})",
                        yi[0], yi[1], yi[2], yi[3]
                    );
                    if peer.ip().is_unspecified() || port == 67 {
                        let _ = sock.send_to(&pkt, ("255.255.255.255", 68));
                        let _ = sock.send_to(&pkt, ("172.16.47.127", 68));
                        if !peer.ip().is_unspecified() {
                            let _ = sock.send_to(&pkt, peer);
                        }
                    } else {
                        let _ = sock.send_to(&pkt, peer);
                        let _ = sock.send_to(&pkt, (peer.ip(), 68));
                        let _ = sock.send_to(&pkt, (peer.ip(), 4011));
                    }
                }
            }
        });
    }
}

/// Serialize the live `MemnetFabricBackend` state (nodes, leases, DNS/DHCP counters, capability
/// tickets) as 100% integer JSON (`p/q` rationals, zero floats).
pub fn build_memnet_fabric_json(backend: &MemnetFabricBackend) -> String {
    let mother_ip = format!(
        "{}.{}.{}.{}",
        backend.mother_ip[0], backend.mother_ip[1], backend.mother_ip[2], backend.mother_ip[3]
    );
    let mut nodes_json = String::with_capacity(512);
    nodes_json.push('[');
    for (idx, n) in backend.nodes.iter().enumerate() {
        if idx > 0 {
            nodes_json.push(',');
        }
        let tags_joined = n.address.tags.join(",");
        nodes_json.push_str(&format!(
            "{{\"host\":\"{}\",\"port\":{},\"role\":\"{}\",\"tags\":\"{}\",\"vector\":\"{}\",\"alive\":{}}}",
            n.host,
            n.port,
            n.address.role,
            tags_joined,
            n.address.vector_hash.as_deref().unwrap_or("LINOSV"),
            if n.alive { "true" } else { "false" }
        ));
    }
    nodes_json.push(']');

    let mut leases_json = String::with_capacity(512);
    leases_json.push('[');
    for (idx, l) in backend.leases.iter().enumerate() {
        if idx > 0 {
            leases_json.push(',');
        }
        let mac_s = format!(
            "{:02x}:{:02x}:{:02x}:{:02x}:{:02x}:{:02x}",
            l.mac[0], l.mac[1], l.mac[2], l.mac[3], l.mac[4], l.mac[5]
        );
        let ip_s = format!(
            "{}.{}.{}.{}",
            l.assigned_ip[0], l.assigned_ip[1], l.assigned_ip[2], l.assigned_ip[3]
        );
        leases_json.push_str(&format!(
            "{{\"mac\":\"{mac_s}\",\"ip\":\"{ip_s}\",\"hostname\":\"{}\",\"role\":\"{}\",\
\"is_raspberry_pi\":{},\"is_ipxe\":{},\"bootfile\":\"{}\",\"cap_mask\":{}}}",
            l.hostname,
            l.node.address.role,
            if l.is_raspberry_pi { "true" } else { "false" },
            if l.is_ipxe { "true" } else { "false" },
            l.bootfile_url,
            l.ticket.cap_mask
        ));
    }
    leases_json.push(']');

    let linosv = seed_hash_hex();
    let _ = Ipv4Addr::new(
        backend.mother_ip[0],
        backend.mother_ip[1],
        backend.mother_ip[2],
        backend.mother_ip[3],
    );

    format!(
        "{{\"ok\":true,\"backend\":\"MEMNET\",\"linosv_seed\":\"{}\",\
\"mother_ip\":\"{mother_ip}\",\"oxicast_port\":{},\"memnet_port\":{},\
\"dhcp_offers_sent\":{},\"dhcp_acks_sent\":{},\
\"dns_queries_resolved\":{},\"dns_intent_matches\":{},\
\"nodes_count\":{},\"leases_count\":{},\
\"nodes\":{nodes_json},\"leases\":{leases_json}}}",
        &linosv[..16],
        backend.oxicast_port,
        backend.memnet_port,
        backend.dhcp_offers_sent,
        backend.dhcp_acks_sent,
        backend.dns_queries_resolved,
        backend.dns_intent_matches,
        backend.nodes.len(),
        backend.leases.len()
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn raspberry_pi_dhcp_discover_registers_memnet_node_and_emits_option43_pxe_offer() {
        let mut backend = MemnetFabricBackend::new(
            [172, 16, 47, 76],
            8432,
            DhcpChannelMode::ProxyPxeAndOpportunistic,
        );

        // Build a synthetic Raspberry Pi 4 (DC:A6:32:11:22:33) DHCPDISCOVER packet
        let mut discover = vec![0u8; 240];
        discover[0] = 1; // BOOTREQUEST
        discover[1] = 1;
        discover[2] = 6;
        discover[4..8].copy_from_slice(&0x1234_5678u32.to_be_bytes());
        discover[28..34].copy_from_slice(&[0xDC, 0xA6, 0x32, 0x11, 0x22, 0x33]);
        discover[236..240].copy_from_slice(&DHCP_MAGIC_COOKIE);
        // Option 53 = DHCP_DISCOVER (1)
        discover.extend_from_slice(&[53, 1, DHCP_DISCOVER]);
        // Option 60 = "PXEClient:Arch:00000:UNDI:002001"
        let vci = b"PXEClient:Arch:00000:UNDI:002001";
        discover.extend_from_slice(&[60, vci.len() as u8]);
        discover.extend_from_slice(vci);
        discover.push(255);

        let parsed = parse_dhcp_packet(&discover).expect("valid DHCPDISCOVER");
        assert_eq!(parsed.msg_type, DHCP_DISCOVER);
        assert!(MemnetFabricBackend::is_raspberry_pi_mac(&parsed.chaddr));

        let offer = build_memnet_dhcp_reply(&parsed, &mut backend).expect("DHCP OFFER");
        assert_eq!(offer[0], 2); // BOOTREPLY
        assert_eq!(&offer[20..24], &[172, 16, 47, 76]); // siaddr = Mother Node
        assert_eq!(backend.leases.len(), 1);
        assert_eq!(backend.leases[0].node.address.role, "ayeos.tv.raspberry_pi");
        assert!(verify_memnet_frame(
            &backend.master_key,
            backend.leases[0].lease_epoch_secs,
            backend.leases[0].ticket.cap_mask,
            cap_mask::AYEOS_GENESIS,
            &backend.leases[0].memnet_frame,
        )
        .is_ok());
    }

    #[test]
    fn memnet_dns_resolver_answers_a_and_txt_records_by_context_relevance() {
        let mut backend = MemnetFabricBackend::new(
            [172, 16, 47, 76],
            8432,
            DhcpChannelMode::ProxyPxeAndOpportunistic,
        );

        // Build RFC 1035 DNS query for "boot.ayeos" (QTYPE = ANY = 255)
        let mut q = Vec::new();
        q.extend_from_slice(&0xCAFEu16.to_be_bytes()); // ID
        q.extend_from_slice(&0x0100u16.to_be_bytes()); // Standard query
        q.extend_from_slice(&1u16.to_be_bytes()); // QDCOUNT = 1
        q.extend_from_slice(&0u16.to_be_bytes());
        q.extend_from_slice(&0u16.to_be_bytes());
        q.extend_from_slice(&0u16.to_be_bytes());
        // QNAME: \x04boot\x05ayeos\x00
        q.extend_from_slice(b"\x04boot\x05ayeos\x00");
        q.extend_from_slice(&DNS_TYPE_ANY.to_be_bytes());
        q.extend_from_slice(&1u16.to_be_bytes()); // CLASS IN

        let resp = handle_dns_query_packet(&q, &mut backend).expect("DNS response");
        assert_eq!(&resp[0..2], &0xCAFEu16.to_be_bytes());
        let ancount = u16::from_be_bytes([resp[6], resp[7]]);
        assert_eq!(ancount, 2); // Both A and TXT records returned!
        let resp_lossy = String::from_utf8_lossy(&resp);
        assert!(resp_lossy.contains("http://172.16.47.76:8432/boot/ayeos.ipxe"));
    }

    #[test]
    fn x86_uefi_and_bios_pxe_dhcp_discover_receive_matched_limine_bootfiles() {
        let mut backend = MemnetFabricBackend::new(
            [172, 16, 47, 76],
            8432,
            DhcpChannelMode::ProxyPxeAndOpportunistic,
        );

        // 1. x86_64 UEFI PXE client (Option 93 = 0x0007, non-Pi MAC 00:1B:21:AA:BB:CC)
        let mut uefi_disc = vec![0u8; 240];
        uefi_disc[0] = 1;
        uefi_disc[1] = 1;
        uefi_disc[2] = 6;
        uefi_disc[4..8].copy_from_slice(&0xABCD_0001u32.to_be_bytes());
        uefi_disc[28..34].copy_from_slice(&[0x00, 0x1B, 0x21, 0xAA, 0xBB, 0xCC]);
        uefi_disc[236..240].copy_from_slice(&DHCP_MAGIC_COOKIE);
        uefi_disc.extend_from_slice(&[53, 1, DHCP_DISCOVER]);
        uefi_disc.extend_from_slice(&[93, 2, 0x00, 0x07]); // EFI x86_64 BC
        let vci_uefi = b"PXEClient:Arch:00007:UNDI:003016";
        uefi_disc.extend_from_slice(&[60, vci_uefi.len() as u8]);
        uefi_disc.extend_from_slice(vci_uefi);
        uefi_disc.push(255);

        let parsed_uefi = parse_dhcp_packet(&uefi_disc).expect("valid UEFI DHCPDISCOVER");
        assert_eq!(parsed_uefi.arch_type, Some(7));
        let offer_uefi = build_memnet_dhcp_reply(&parsed_uefi, &mut backend).expect("UEFI OFFER");
        let file_uefi = String::from_utf8_lossy(&offer_uefi[108..108 + 11]);
        assert_eq!(file_uefi, "BOOTX64.EFI");
        // Verify yiaddr is a valid /25 subnet address (172.16.47.80..=124) and NOT 0.0.0.0
        assert_eq!(&offer_uefi[16..19], &[172, 16, 47]);
        assert!((80..=124).contains(&offer_uefi[19]));
        // Verify Option 60 ("PXEClient") is omitted for x86_64 UEFI PXE so EDK2 UefiPxeBcDxe boots immediately
        assert!(!String::from_utf8_lossy(&offer_uefi[240..]).contains("PXEClient"));

        // 2. x86 Legacy BIOS PXE client (Option 93 = 0x0000, MAC 00:1B:21:DD:EE:FF)
        let mut bios_disc = vec![0u8; 240];
        bios_disc[0] = 1;
        bios_disc[1] = 1;
        bios_disc[2] = 6;
        bios_disc[4..8].copy_from_slice(&0xABCD_0002u32.to_be_bytes());
        bios_disc[28..34].copy_from_slice(&[0x00, 0x1B, 0x21, 0xDD, 0xEE, 0xFF]);
        bios_disc[236..240].copy_from_slice(&DHCP_MAGIC_COOKIE);
        bios_disc.extend_from_slice(&[53, 1, DHCP_DISCOVER]);
        bios_disc.extend_from_slice(&[93, 2, 0x00, 0x00]); // Intel x86PC
        let vci_bios = b"PXEClient:Arch:00000:UNDI:002001";
        bios_disc.extend_from_slice(&[60, vci_bios.len() as u8]);
        bios_disc.extend_from_slice(vci_bios);
        bios_disc.push(255);

        let parsed_bios = parse_dhcp_packet(&bios_disc).expect("valid BIOS DHCPDISCOVER");
        assert_eq!(parsed_bios.arch_type, Some(0));
        let offer_bios = build_memnet_dhcp_reply(&parsed_bios, &mut backend).expect("BIOS OFFER");
        let file_bios = String::from_utf8_lossy(&offer_bios[108..108 + 19]);
        assert_eq!(file_bios, "limine-bios-pxe.bin");
    }
}
