use serde::{Deserialize, Serialize};

/// MEMNET protocol — contextual routing for distributed ternary matrices.
/// Replaces static IP addressing with intent-based resolution.
///
/// Address = Context (not numerical IPs):
/// - geo: GPS coordinates or naval grid
/// - role: e.g., "matrix.ternary.gpu_node"
/// - tags: e.g., "linosv", "genesis", "group_size=64"
/// - vector: optional hash of active MEM|8 context
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct MemnetAddress {
    pub geo: Option<String>,
    pub role: String,
    pub tags: Vec<String>,
    pub vector_hash: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct MemnetNode {
    pub address: MemnetAddress,
    pub host: String,
    pub port: u16,
    pub alive: bool,
}

/// A MEMNET capsule — an .m8 knowledge capsule containing a ternary matrix.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct MemnetCapsule {
    pub capsule_id: String,
    pub address: MemnetAddress,
    pub payload_type: String,  // "ternary_matrix", "scale_array", etc.
    pub payload_b64: String,   // base64-encoded binary
    pub relevance_score: f32,
    pub timestamp: u64,
}

/// The LINOSV genesis node — the root of the ayeOS ternary mesh.
pub fn genesis_node(host: &str, port: u16) -> MemnetNode {
    MemnetNode {
        address: MemnetAddress {
            geo: None,
            role: "matrix.ternary.genesis".into(),
            tags: vec!["linosv".into(), "genesis".into(), "ayeos".into()],
            vector_hash: Some("LINOSV-GENESIS-0".into()),
        },
        host: host.into(),
        port,
        alive: true,
    }
}

/// Score a capsule's relevance to a node's declared interests.
pub fn relevance_score(capsule: &MemnetCapsule, node: &MemnetNode) -> f32 {
    let mut score = 0.0f32;
    // Tag overlap
    for tag in &capsule.address.tags {
        if node.address.tags.contains(tag) {
            score += 1.0;
        }
    }
    // Role match
    if capsule.address.role == node.address.role {
        score += 2.0;
    }
    // Vector similarity (simplified: hash prefix match)
    if let (Some(cv), Some(nv)) = (&capsule.address.vector_hash, &node.address.vector_hash) {
        let prefix_len = cv.chars().zip(nv.chars()).take_while(|(a, b)| a == b).count();
        score += prefix_len as f32 * 0.5;
    }
    score.max(0.1) // never zero — everything has some relevance
}
