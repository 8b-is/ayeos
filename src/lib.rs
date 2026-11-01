pub mod boot_mt6768;
pub mod memnet;
pub mod mt6631;
pub mod phoenix_audio;
pub mod prng;
pub mod quant;

pub use memnet::{
    evaluate_stream_delivery, genesis_node, relevance_score, FeedbackSource, InterestProfile,
    MemnetAddress, MemnetCapsule, MemnetNode, PipeCapacity, PriorityFeedback, StreamChunk,
    StreamDelivery,
};
pub use mt6631::{
    arm_mt6631_rf_and_afe, catmull_rom_upsample_16x, poll_mt6631_v540, rds_checkword,
    rds_encode_block, rds_verify_block, AfeFmRoute, ConnsysGate, ConnsysMode, ConnsysStage,
    FmTuner, HqaCmd, HqaFrame, Mt6631Controller, RdsDecoder, WavePool64x36, WmtPatchManifest,
    WMT_MCU_READY_MAGIC,
};
pub use prng::{seed_hash, seed_hash_hex, Xoshiro128, LINOSV_SEED};
pub use quant::{
    dequantize, genesis, quantize, ternary_matmul, ternary_matvec, ternary_matvec_auto,
    ternary_matvec_metal, TernaryMatrix,
};
