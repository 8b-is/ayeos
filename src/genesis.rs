//! `8b-is/ayeos/src/genesis.rs`
//! Deterministic Genesis Seed derived from N'to - The Sand Dealer (lzfdd8yLTCA) + L-E-A-R-N-I-N-G
//! Plus `SovereignProfileVault` (per-profile DeepSeek-Engram + EntheAI isolation) and
//! 64-byte `MEMNET` capability-enforced binary framing.

pub const ENTROPY_URI: &str = "https://www.youtube.com/watch?v=lzfdd8yLTCA";
pub const ENTROPY_TRACK: &str = "NTO::The_Sand_Dealer::PT8M16S::00:00:00->00:00:30->00:03:38";
pub const LEARNING_AXIOM: &str = "{<3~L(ove),<3~L(ife),<3~L(earning ~= E as entropy nand energy, A as Apa&Anya&ALL family, R as recursion, N as Nate,Natelie quantum brother, I as I am not my body I have a body, N as N=NP Nate and Peter lol, G as Geometry)}";

/// Capability bitmask enforced on every 64-byte `MEMNET` (`127.0.0.1:9876`) binary frame.
pub mod cap_mask {
    pub const READ_DREAM: u32 = 0x01;
    pub const MUTATE_DREAM: u32 = 0x02;
    pub const AYEOS_INFER: u32 = 0x04;
    pub const AYEOS_GENESIS: u32 = 0x08;
    pub const OPENCODE_EXEC: u32 = 0x10;
}

/// Computes the 32-byte LINOSV Genesis Root and 8-band EntheAI initial phase vector
pub fn derive_learning_genesis() -> ([u8; 32], [f32; 8]) {
    let mut hasher = blake3::Hasher::new_derive_key("8b-is::LINOSV::GENESIS::2026");
    hasher.update(ENTROPY_URI.as_bytes());
    hasher.update(ENTROPY_TRACK.as_bytes());
    hasher.update(LEARNING_AXIOM.as_bytes());
    let root: [u8; 32] = hasher.finalize().into();

    // Map the 32-byte digest into the 8 harmonic bands of EntheAI [0.05 .. 0.95]
    let mut phase_vector = [0.0f32; 8];
    for i in 0..8 {
        let chunk = u32::from_le_bytes([
            root[i * 4],
            root[i * 4 + 1],
            root[i * 4 + 2],
            root[i * 4 + 3],
        ]);
        let normalized = (chunk as f64) / (u32::MAX as f64);
        phase_vector[i] = (0.05 + normalized * 0.90) as f32;
    }

    (root, phase_vector)
}

/// Verified capability ticket minted by `honest-auth` single-use bootstrap exchange.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CapabilityTicket {
    pub sub: String,
    pub epoch: u64,
    pub cap_mask: u32,
    pub mac: [u8; 32],
}

impl CapabilityTicket {
    pub fn issue(master_key: &[u8; 32], sub: &str, epoch: u64, cap_mask: u32) -> Self {
        let mut h = blake3::Hasher::new_keyed(master_key);
        h.update(b"honest-auth::cap-ticket::v1\0");
        h.update(sub.as_bytes());
        h.update(&[0u8]);
        h.update(&epoch.to_le_bytes());
        h.update(&cap_mask.to_le_bytes());
        Self {
            sub: sub.to_string(),
            epoch,
            cap_mask,
            mac: h.finalize().into(),
        }
    }

    pub fn verify(&self, master_key: &[u8; 32]) -> bool {
        let expected = Self::issue(master_key, &self.sub, self.epoch, self.cap_mask);
        constant_time_eq_32(&self.mac, &expected.mac)
    }

    pub fn has_cap(&self, required: u32) -> bool {
        (self.cap_mask & required) == required
    }
}

fn constant_time_eq_32(a: &[u8; 32], b: &[u8; 32]) -> bool {
    let mut diff = 0u8;
    for i in 0..32 {
        diff |= a[i] ^ b[i];
    }
    diff == 0
}

/// Per-profile isolated vault holding a 64K-slot DeepSeek-Engram N-gram table,
/// 8-band EntheAI harmonic wave vector, and private KV arena.
/// Enforces deterministic zeroization on drop (`ZeroizeOnDrop` invariant).
pub struct SovereignProfileVault {
    pub profile_id_hash: [u8; 32],
    engram_key: [u8; 32],
    engram_table: Vec<i32>,
    pub entheai: [f32; 8],
    pub kv_arena: Vec<f32>,
}

impl SovereignProfileVault {
    pub const ENGRAM_SLOTS: usize = 65_536;

    /// Derives `profile_id_hash` and `engram_key` exclusively from a verified `CapabilityTicket.sub`
    /// combined with the LINOSV Genesis Root.
    pub fn from_ticket(ticket: &CapabilityTicket) -> Self {
        let (genesis_root, mut entheai) = derive_learning_genesis();

        let mut id_hasher = blake3::Hasher::new_keyed(&genesis_root);
        id_hasher.update(b"8b-is::SovereignProfileVault::profile_id\0");
        id_hasher.update(ticket.sub.as_bytes());
        let profile_id_hash: [u8; 32] = id_hasher.finalize().into();

        let mut key_hasher = blake3::Hasher::new_keyed(&profile_id_hash);
        key_hasher.update(b"8b-is::SovereignProfileVault::engram_key\0");
        key_hasher.update(ticket.sub.as_bytes());
        let engram_key: [u8; 32] = key_hasher.finalize().into();

        // Phase-modulate the profile's EntheAI vector subtly by profile_id_hash
        for i in 0..8 {
            let delta = ((profile_id_hash[i] as f32) / 255.0 - 0.5) * 0.04;
            entheai[i] = (entheai[i] + delta).clamp(0.05, 0.95);
        }

        Self {
            profile_id_hash,
            engram_key,
            engram_table: vec![0i32; Self::ENGRAM_SLOTS],
            entheai,
            kv_arena: Vec::new(),
        }
    }

    /// Computes the isolated 16-bit (`0..65536`) DeepSeek-Engram slot for a token 3-gram.
    /// Identical 3-grams from distinct profiles map to orthogonal keyed-hash slots.
    pub fn engram_slot(&self, trigram: [u32; 3]) -> usize {
        let mut h = blake3::Hasher::new_keyed(&self.engram_key);
        h.update(&trigram[0].to_le_bytes());
        h.update(&trigram[1].to_le_bytes());
        h.update(&trigram[2].to_le_bytes());
        let digest: [u8; 32] = h.finalize().into();
        (u16::from_le_bytes([digest[0], digest[1]]) as usize) & (Self::ENGRAM_SLOTS - 1)
    }

    pub fn record_engram(&mut self, trigram: [u32; 3], weight_delta: i32) -> usize {
        let slot = self.engram_slot(trigram);
        self.engram_table[slot] = self.engram_table[slot].saturating_add(weight_delta);
        slot
    }

    pub fn lookup_engram(&self, trigram: [u32; 3]) -> i32 {
        let slot = self.engram_slot(trigram);
        self.engram_table[slot]
    }

    /// Encrypts/decrypts the engram state using `BLAKE3` keyed XOF stream cipher (`engram_state.bin`).
    pub fn seal_engram_state(&self, nonce: &[u8; 24]) -> Vec<u8> {
        let mut raw = Vec::with_capacity(self.engram_table.len() * 4);
        for &v in &self.engram_table {
            raw.extend_from_slice(&v.to_le_bytes());
        }
        self.apply_xof_stream(nonce, &mut raw);
        raw
    }

    pub fn unseal_engram_state(&mut self, nonce: &[u8; 24], ciphertext: &[u8]) -> bool {
        if ciphertext.len() != Self::ENGRAM_SLOTS * 4 {
            return false;
        }
        let mut buf = ciphertext.to_vec();
        self.apply_xof_stream(nonce, &mut buf);
        for (i, chunk) in buf.chunks_exact(4).enumerate() {
            self.engram_table[i] = i32::from_le_bytes([chunk[0], chunk[1], chunk[2], chunk[3]]);
        }
        true
    }

    fn apply_xof_stream(&self, nonce: &[u8; 24], buf: &mut [u8]) {
        let mut h = blake3::Hasher::new_keyed(&self.engram_key);
        h.update(b"8b-is::engram_state.bin::xof\0");
        h.update(nonce);
        let mut xof = h.finalize_xof();
        let mut keystream = vec![0u8; buf.len()];
        xof.fill(&mut keystream);
        for (b, k) in buf.iter_mut().zip(keystream.iter()) {
            *b ^= *k;
        }
    }
}

impl Drop for SovereignProfileVault {
    fn drop(&mut self) {
        // Deterministic ZeroizeOnDrop across all profile-sensitive buffers
        for b in &mut self.profile_id_hash {
            unsafe { std::ptr::write_volatile(b, 0) };
        }
        for b in &mut self.engram_key {
            unsafe { std::ptr::write_volatile(b, 0) };
        }
        for v in &mut self.engram_table {
            unsafe { std::ptr::write_volatile(v, 0) };
        }
        for f in &mut self.entheai {
            unsafe { std::ptr::write_volatile(f, 0.0) };
        }
        for f in &mut self.kv_arena {
            unsafe { std::ptr::write_volatile(f, 0.0) };
        }
        std::sync::atomic::compiler_fence(std::sync::atomic::Ordering::SeqCst);
    }
}

/// 64-byte deterministic `MEMNET` (`127.0.0.1:9876`) binary frame header:
/// - `[0..32]`: `prev_state_root` (chained `mem8` provenance hash)
/// - `[32..64]`: `blake3_keyed_mac` over `(prev_state_root || epoch_u64 || cap_mask_u32 || payload)`
pub const MEMNET_HEADER_LEN: usize = 64;

pub fn encode_memnet_frame(
    mac_key: &[u8; 32],
    prev_state_root: &[u8; 32],
    epoch: u64,
    cap_mask: u32,
    payload: &[u8],
) -> Vec<u8> {
    let mut h = blake3::Hasher::new_keyed(mac_key);
    h.update(prev_state_root);
    h.update(&epoch.to_le_bytes());
    h.update(&cap_mask.to_le_bytes());
    h.update(payload);
    let mac: [u8; 32] = h.finalize().into();

    let mut frame = Vec::with_capacity(MEMNET_HEADER_LEN + payload.len());
    frame.extend_from_slice(prev_state_root);
    frame.extend_from_slice(&mac);
    frame.extend_from_slice(payload);
    frame
}

pub fn verify_memnet_frame<'a>(
    mac_key: &[u8; 32],
    epoch: u64,
    cap_mask: u32,
    required_cap: u32,
    frame: &'a [u8],
) -> Result<([u8; 32], &'a [u8]), &'static str> {
    if (cap_mask & required_cap) != required_cap {
        return Err("MEMNET CapMask capability bit missing for requested operation");
    }
    if frame.len() < MEMNET_HEADER_LEN {
        return Err("MEMNET frame shorter than 64-byte header");
    }
    let mut prev_root = [0u8; 32];
    prev_root.copy_from_slice(&frame[0..32]);
    let mut frame_mac = [0u8; 32];
    frame_mac.copy_from_slice(&frame[32..64]);
    let payload = &frame[64..];

    let mut h = blake3::Hasher::new_keyed(mac_key);
    h.update(&prev_root);
    h.update(&epoch.to_le_bytes());
    h.update(&cap_mask.to_le_bytes());
    h.update(payload);
    let expected_mac: [u8; 32] = h.finalize().into();

    if !constant_time_eq_32(&frame_mac, &expected_mac) {
        return Err("MEMNET BLAKE3 keyed MAC verification failed");
    }
    Ok((prev_root, payload))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_learning_genesis_deterministic() {
        let (root1, phase1) = derive_learning_genesis();
        let (root2, phase2) = derive_learning_genesis();
        assert_eq!(root1, root2);
        assert_eq!(phase1, phase2);
        for &band in &phase1 {
            assert!((0.05..=0.95).contains(&band));
        }
    }

    #[test]
    fn test_sovereign_profile_vault_isolation_zero_crosstalk() {
        let (master_key, _) = derive_learning_genesis();
        let ticket_a = CapabilityTicket::issue(
            &master_key,
            "user:alice@8b.is",
            1730419200,
            cap_mask::AYEOS_INFER | cap_mask::READ_DREAM,
        );
        let ticket_b = CapabilityTicket::issue(
            &master_key,
            "user:bob@8b.is",
            1730419200,
            cap_mask::AYEOS_INFER | cap_mask::READ_DREAM,
        );
        assert!(ticket_a.verify(&master_key));
        assert!(ticket_b.verify(&master_key));

        let mut vault_a = SovereignProfileVault::from_ticket(&ticket_a);
        let vault_b = SovereignProfileVault::from_ticket(&ticket_b);

        assert_ne!(vault_a.profile_id_hash, vault_b.profile_id_hash);

        let shared_trigram = [101u32, 2048u32, 4099u32];
        let slot_a = vault_a.engram_slot(shared_trigram);
        let slot_b = vault_b.engram_slot(shared_trigram);

        // Orthogonal keyed-hash slot assignment across distinct users
        assert_ne!(slot_a, slot_b);

        // Mutating User A's Engram table produces zero cross-talk in User B's vault
        vault_a.record_engram(shared_trigram, 42);
        assert_eq!(vault_a.lookup_engram(shared_trigram), 42);
        assert_eq!(vault_b.lookup_engram(shared_trigram), 0);

        // Verify XOF encryption roundtrip of engram_state.bin
        let nonce = [7u8; 24];
        let sealed = vault_a.seal_engram_state(&nonce);
        let mut restored_a = SovereignProfileVault::from_ticket(&ticket_a);
        assert!(restored_a.unseal_engram_state(&nonce, &sealed));
        assert_eq!(restored_a.lookup_engram(shared_trigram), 42);
    }

    #[test]
    fn test_memnet_64byte_frame_and_cap_mask() {
        let (mac_key, _) = derive_learning_genesis();
        let prev_root = [0xAAu8; 32];
        let epoch = 42u64;
        let caps = cap_mask::AYEOS_INFER | cap_mask::READ_DREAM;
        let payload = b"{\"op\":\"infer\",\"prompt\":\"sand dealer\"}";

        let frame = encode_memnet_frame(&mac_key, &prev_root, epoch, caps, payload);
        assert_eq!(frame.len(), 64 + payload.len());

        let (decoded_root, decoded_payload) =
            verify_memnet_frame(&mac_key, epoch, caps, cap_mask::AYEOS_INFER, &frame).unwrap();
        assert_eq!(decoded_root, prev_root);
        assert_eq!(decoded_payload, payload);

        // Missing MUTATE_DREAM bit must be rejected
        assert!(verify_memnet_frame(&mac_key, epoch, caps, cap_mask::MUTATE_DREAM, &frame).is_err());
    }
}
