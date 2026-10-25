use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// MEMNET protocol — contextual routing for distributed ternary matrices & adaptive streams.
/// Replaces static IP addressing with intent-based resolution (`ayeos_memnet_protocol.md`).
///
/// Address = Context (not numerical IPs):
/// - geo: GPS coordinates or naval grid
/// - role: e.g., "aircraft.f18.ground_ai" or "matrix.ternary.gpu_node"
/// - tags: e.g., "radar_diagnostics", "bctl_v3", "cold_start"
/// - vector: optional hash of active MEM|8 context
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct MemnetAddress {
    pub geo: Option<String>,
    pub role: String,
    pub tags: Vec<String>,
    pub vector_hash: Option<String>,
}

impl MemnetAddress {
    /// Parses a canonical `geo:... + role:... + tags:a,b,c + vector:...` context address string.
    pub fn parse(expr: &str) -> Self {
        let mut geo = None;
        let mut role = String::new();
        let mut tags = Vec::new();
        let mut vector_hash = None;

        for part in expr.split('+').map(str::trim).filter(|s| !s.is_empty()) {
            if let Some(g) = part.strip_prefix("geo:") {
                geo = Some(g.trim().to_string());
            } else if let Some(r) = part.strip_prefix("role:") {
                role = r.trim().to_string();
            } else if let Some(ts) = part.strip_prefix("tags:") {
                for t in ts.split(',').map(str::trim).filter(|s| !s.is_empty()) {
                    tags.push(t.to_string());
                }
            } else if let Some(t) = part.strip_prefix("tag:") {
                for sub in t.split(',').map(str::trim).filter(|s| !s.is_empty()) {
                    tags.push(sub.to_string());
                }
            } else if let Some(v) = part.strip_prefix("vector:") {
                vector_hash = Some(v.trim().to_string());
            }
        }

        Self {
            geo,
            role,
            tags,
            vector_hash,
        }
    }
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
    pub payload_type: String, // "ternary_matrix", "scale_array", etc.
    pub payload_b64: String,  // base64-encoded binary
    pub relevance_score: f32,
    pub timestamp: u64,
}

/// Pipe capacity metrics influencing MEMNET adaptive delivery (`ayeos_memnet_protocol.md`).
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct PipeCapacity {
    pub bandwidth_kbps: u32,
    pub jitter_ms: u16,
    pub loss_rate: f32,
}

impl PipeCapacity {
    pub const fn abundant() -> Self {
        Self {
            bandwidth_kbps: 50_000,
            jitter_ms: 4,
            loss_rate: 0.001,
        }
    }

    pub const fn constrained() -> Self {
        Self {
            bandwidth_kbps: 1_200,
            jitter_ms: 45,
            loss_rate: 0.04,
        }
    }

    pub const fn severe() -> Self {
        Self {
            bandwidth_kbps: 64,
            jitter_ms: 180,
            loss_rate: 0.15,
        }
    }

    pub fn health_score(&self) -> f32 {
        let bw = (self.bandwidth_kbps as f32 / 10_000.0).min(1.0);
        let jitter_pen = (self.jitter_ms as f32 / 250.0).min(0.5);
        let loss_pen = (self.loss_rate * 2.0).min(0.6);
        (bw * (1.0 - 0.5 * jitter_pen) * (1.0 - loss_pen)).clamp(0.0, 1.0)
    }
}

/// Origin of dynamic interest feedback (`ayeos_memnet_protocol.md`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum FeedbackSource {
    HumanInteraction,
    AiInference,
    MissionObjective,
}

/// Mid-stream priority vote from an end node.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct PriorityFeedback {
    pub stream_id: String,
    pub channel: String,
    pub tile_id: Option<String>,
    pub importance_adjust: f32,
    pub source: FeedbackSource,
}

/// End-node interest profile for Adaptive Relevance-Aware Streaming.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct InterestProfile {
    pub node_address: MemnetAddress,
    pub prioritized_channels: Vec<String>,
    pub deprioritized_channels: Vec<String>,
    pub adjustments: BTreeMap<String, f32>,
}

impl InterestProfile {
    pub fn super_bowl_default() -> Self {
        Self {
            node_address: MemnetAddress::parse(
                "geo:missouri + role:subscriber.super_bowl + tags:ball,key_players,commentary + vector:SB2025-MAIN",
            ),
            prioritized_channels: vec!["video.main".into(), "audio.commentary".into()],
            deprioritized_channels: vec!["video.alt_angle".into(), "audio.crowd_noise".into()],
            adjustments: BTreeMap::new(),
        }
    }

    pub fn apply_feedback(&mut self, fb: &PriorityFeedback) {
        let key = match &fb.tile_id {
            Some(tile) => format!("{}#{}", fb.channel, tile),
            None => fb.channel.clone(),
        };
        let entry = self.adjustments.entry(key).or_insert(0.0);
        *entry = (*entry + fb.importance_adjust).clamp(-1.0, 1.0);

        if *entry >= 0.15 {
            self.deprioritized_channels
                .retain(|c| !c.eq_ignore_ascii_case(&fb.channel));
            if !self
                .prioritized_channels
                .iter()
                .any(|c| c.eq_ignore_ascii_case(&fb.channel))
            {
                self.prioritized_channels.push(fb.channel.clone());
            }
        }
    }
}

/// A multi-modal stream chunk routed over MEMNET.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct StreamChunk {
    pub stream_id: String,
    pub channel: String,
    pub tile_id: Option<String>,
    pub importance: f32,
    pub refinement_layer: u8,
    pub address: MemnetAddress,
    pub summary_fallback: Option<String>,
}

/// Delivery action decided by a MEMNET router for a `StreamChunk`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum StreamDelivery {
    DeliverEnhanced {
        effective_importance: f32,
        context_augmentation: String,
    },
    DeliverFull {
        effective_importance: f32,
    },
    ContextDecayDropLayer {
        effective_importance: f32,
        dropped_layer: u8,
    },
    CollapseToSummary {
        effective_importance: f32,
        summary_capsule: String,
    },
    PrunedByInterest {
        effective_importance: f32,
        channel: String,
    },
}

/// Evaluates how a `StreamChunk` should be shaped/delivered given `PipeCapacity` and `InterestProfile`.
pub fn evaluate_stream_delivery(
    chunk: &StreamChunk,
    profile: &InterestProfile,
    pipe: &PipeCapacity,
) -> StreamDelivery {
    let mut eff = chunk.importance;
    if let Some(&adj) = profile.adjustments.get(&chunk.channel) {
        eff += adj;
    }
    if let Some(ref tile) = chunk.tile_id {
        let key = format!("{}#{}", chunk.channel, tile);
        if let Some(&adj) = profile.adjustments.get(&key) {
            eff += adj;
        }
    }

    let is_deprioritized = profile
        .deprioritized_channels
        .iter()
        .any(|c| c.eq_ignore_ascii_case(&chunk.channel));
    let is_prioritized = profile
        .prioritized_channels
        .iter()
        .any(|c| c.eq_ignore_ascii_case(&chunk.channel));

    if is_prioritized {
        eff += 0.15;
    }
    if is_deprioritized {
        eff -= 0.45;
    }
    eff = eff.clamp(0.0, 1.0);

    // Prune deprioritized streams even if bandwidth is plentiful
    if is_deprioritized && eff < 0.65 {
        return StreamDelivery::PrunedByInterest {
            effective_importance: eff,
            channel: chunk.channel.clone(),
        };
    }

    let health = pipe.health_score();
    if health < 0.15 && chunk.channel.starts_with("video.") {
        return StreamDelivery::CollapseToSummary {
            effective_importance: eff,
            summary_capsule: chunk
                .summary_fallback
                .clone()
                .unwrap_or_else(|| format!("summary:{}", chunk.stream_id)),
        };
    }

    if health < 0.55 && chunk.refinement_layer > 0 && eff < 0.90 {
        return StreamDelivery::ContextDecayDropLayer {
            effective_importance: eff,
            dropped_layer: chunk.refinement_layer,
        };
    }

    if eff >= 0.85 {
        return StreamDelivery::DeliverEnhanced {
            effective_importance: eff,
            context_augmentation: format!(
                "m8:enhanced(stream={},tags={})",
                chunk.stream_id,
                chunk.address.tags.join(",")
            ),
        };
    }

    StreamDelivery::DeliverFull {
        effective_importance: eff,
    }
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
    // Geo match
    if let (Some(cg), Some(ng)) = (&capsule.address.geo, &node.address.geo) {
        if cg.eq_ignore_ascii_case(ng) {
            score += 1.5;
        }
    }
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
    // Vector similarity (prefix match)
    if let (Some(cv), Some(nv)) = (&capsule.address.vector_hash, &node.address.vector_hash) {
        let prefix_len = cv
            .chars()
            .zip(nv.chars())
            .take_while(|(a, b)| a == b)
            .count();
        score += prefix_len as f32 * 0.5;
    }
    score.max(0.1) // never zero — everything has some relevance
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn contextual_address_parsing_and_relevance() {
        let addr = MemnetAddress::parse(
            "geo:cvn78 + role:aircraft.f18.ground_ai + tags:radar_diagnostics,bctl_v3,cold_start + vector:LINOSV-GENESIS-1",
        );
        assert_eq!(addr.geo.as_deref(), Some("cvn78"));
        assert_eq!(addr.role, "aircraft.f18.ground_ai");
        assert_eq!(addr.tags, vec!["radar_diagnostics", "bctl_v3", "cold_start"]);
        assert_eq!(addr.vector_hash.as_deref(), Some("LINOSV-GENESIS-1"));
    }

    #[test]
    fn adaptive_relevance_streaming_super_bowl() {
        let mut profile = InterestProfile::super_bowl_default();
        let addr = profile.node_address.clone();

        let main_video = StreamChunk {
            stream_id: "sb_2025_final".into(),
            channel: "video.main".into(),
            tile_id: Some("x08y04".into()),
            importance: 0.90,
            refinement_layer: 1,
            address: addr.clone(),
            summary_fallback: Some("Bray just scored".into()),
        };
        let alt_cam = StreamChunk {
            stream_id: "sb_2025_final".into(),
            channel: "video.alt_angle".into(),
            tile_id: Some("x12y08".into()),
            importance: 0.55,
            refinement_layer: 1,
            address: addr.clone(),
            summary_fallback: None,
        };

        // Abundant bandwidth: main video is enhanced, alt_angle is pruned by interest
        assert!(matches!(
            evaluate_stream_delivery(&main_video, &profile, &PipeCapacity::abundant()),
            StreamDelivery::DeliverEnhanced { .. }
        ));
        assert!(matches!(
            evaluate_stream_delivery(&alt_cam, &profile, &PipeCapacity::abundant()),
            StreamDelivery::PrunedByInterest { .. }
        ));

        // Mid-stream feedback: favorite player appears on alt_angle tile x12y08
        profile.apply_feedback(&PriorityFeedback {
            stream_id: "sb_2025_final".into(),
            channel: "video.alt_angle".into(),
            tile_id: Some("x12y08".into()),
            importance_adjust: 0.35,
            source: FeedbackSource::AiInference,
        });
        assert!(matches!(
            evaluate_stream_delivery(&alt_cam, &profile, &PipeCapacity::abundant()),
            StreamDelivery::DeliverEnhanced { .. }
        ));

        // Severe constraint: video collapses to signed summary capsule
        assert_eq!(
            evaluate_stream_delivery(&main_video, &profile, &PipeCapacity::severe()),
            StreamDelivery::CollapseToSummary {
                effective_importance: 1.0,
                summary_capsule: "Bray just scored".into(),
            }
        );
    }
}
