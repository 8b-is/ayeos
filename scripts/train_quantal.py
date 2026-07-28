#!/usr/bin/env python3
"""
train_quantal.py — train a ternary (BitNet b1.58) model from a HuggingFace
base, using MLX-QUANT's QAT BitLinear layers with straight-through estimator,
then export to ayeOS {n+-1-<△>} ternary matrix format.

Usage:
  python scripts/train_quantal.py --model Qwen/Qwen2.5-0.5B --data your_dataset
  python scripts/train_quantal.py --model Qwen/Qwen2.5-Coder-0.5B --data ./data/code.jsonl

Output:
  - quantal_model.safetensors — trained full-precision weights (for continued training)
  - quantal_model.ayeos.json  — ayeOS TernaryMatrix capsule (for inference via ayeosd)
"""

import argparse
import gzip
import json
import math
import os
import sys
import time
from functools import partial
from pathlib import Path
from typing import Optional

import mlx.core as mx
import mlx.nn as nn
import mlx.optimizers as opt
import numpy as np
from mlx.nn.layers.bitlinear import BitLinear, weight_quant, activation_quant

# ---------------------------------------------------------------------------
# 1.  Model surgery — replace nn.Linear with BitLinear
# ---------------------------------------------------------------------------

def replace_linear_with_bitlinear(model: nn.Module, binary: bool = False):
    """Walk the module tree and replace every ``nn.Linear`` with a
    :class:`BitLinear` that copies the pretrained weight as its initial
    full-precision weight.

    Uses :meth:`mlx.nn.Module.named_modules` to find all Linear layers,
    builds a nested dict matching the module tree, and calls
    :meth:`mlx.nn.Module.update_modules` for a single-pass replacement.
    """

    def _build_path(path: str, new_module: nn.Module) -> dict:
        parts = path.split(".")
        d = {}
        current = d
        for p in parts[:-1]:
            current[p] = {}
            current = current[p]
        current[parts[-1]] = new_module
        return d

    def _merge(base: dict, update: dict):
        for k, v in update.items():
            if k in base and isinstance(base[k], dict) and isinstance(v, dict):
                _merge(base[k], v)
            else:
                base[k] = v

    replacements = {}
    for path, m in model.named_modules():
        if isinstance(m, nn.Linear) and path:
            bl = BitLinear(
                input_dims=m.weight.shape[1],
                output_dims=m.weight.shape[0],
                bias="bias" in m,
                binary=binary,
            )
            bl.weight = m.weight
            if "bias" in m:
                bl.bias = m.bias
            _merge(replacements, _build_path(path, bl))

    model.update_modules(replacements)
    return model


# ---------------------------------------------------------------------------
# 2.  Dataset helpers
# ---------------------------------------------------------------------------

def load_jsonl(path: str, max_samples: Optional[int] = None):
    """Load text samples from a .jsonl or .jsonl.gz file."""
    open_fn = gzip.open if path.endswith(".gz") else open
    samples = []
    with open_fn(path, "rt", encoding="utf-8") as f:
        for i, line in enumerate(f):
            if max_samples and i >= max_samples:
                break
            line = line.strip()
            if not line:
                continue
            try:
                obj = json.loads(line)
                text = obj.get("text", obj.get("content", obj.get("instruction", "")))
                if text:
                    samples.append(text)
            except json.JSONDecodeError:
                if line:
                    samples.append(line)
    return samples


def tokenize_fn(tokenizer, texts: list[str], max_len: int = 512):
    """Tokenize a list of texts into MLX arrays."""
    if tokenizer is None:
        return [
            mx.array([ord(c) for c in t[:max_len]], dtype=mx.uint32)
            for t in texts
        ]
    enc = tokenizer(
        texts,
        truncation=True,
        max_length=max_len,
        padding=False,
        return_tensors=None,
    )
    return [mx.array(ids, dtype=mx.uint32) for ids in enc["input_ids"]]


# ---------------------------------------------------------------------------
# 3.  Loss function (causal LM)
# ---------------------------------------------------------------------------

def ce_loss(logits: mx.array, targets: mx.array) -> mx.array:
    """Cross-entropy for causal LM: predict next token."""
    logits = logits[:, :-1, :]
    targets = targets[:, 1:]
    loss = nn.losses.cross_entropy(logits, targets, reduction="mean")
    return loss


# ---------------------------------------------------------------------------
# 4.  Linear → TernaryMatrix export (ayeOS format)
# ---------------------------------------------------------------------------

def export_to_ayeos(
    model: nn.Module,
    output_path: str,
    group_size: int = 64,
    metadata: Optional[dict] = None,
):
    """Export trained BitLinear weights into ayeOS TernaryMatrix format.

    Each BitLinear layer's quantized weight (after ``weight_quant``) is
    packed using MLX's own ``mx.quantize(..., bits=2, mode='ternary')``,
    producing packed uint32 words (2-bit LSB codes {0→-1, 1→0, 2→+1})
    and per-group scales that exactly match ayeOS's ``ternary_matmul``.
    """
    capsule = {
        "capsule_id": f"quantal-{int(time.time())}",
        "address": {
            "role": "matrix.ternary.quantal",
            "tags": ["quantal", "bitnet", "ternary", "ayeos"],
        },
        "payload_type": "ternary_matrix",
        "matrices": [],
        "metadata": metadata or {},
        "timestamp": int(time.time()),
    }

    for path, m in model.named_modules():
        if isinstance(m, BitLinear) and path:
            w = m["weight"]
            wq = weight_quant(w)

            codes, scales = mx.quantize(
                wq, group_size=group_size, bits=2, mode="ternary"
            )
            codes_flat = np.array(codes.reshape(-1), dtype=np.uint8)
            codes_list = codes_flat.tolist()
            scales_flat = np.array(scales.reshape(-1), dtype=np.float32).tolist()

            entry = {
                "name": path,
                "dim": w.shape[0],
                "in_features": w.shape[1],
                "group_size": group_size,
                "codes": codes_list,
                "scales": scales_flat,
                "seed_hash": "quantal-trained",
            }
            capsule["matrices"].append(entry)

    with open(output_path, "w") as f:
        json.dump(capsule, f, indent=2)
    print(f"  exported {len(capsule['matrices'])} matrices -> {output_path}")
    return capsule


# ---------------------------------------------------------------------------
# 5.  Training loop
# ---------------------------------------------------------------------------

def train_step(model, inputs, targets, optimizer):
    """Single training step with loss value for logging."""

    def loss_fn(params):
        model.update(params)
        logits = model(inputs)
        return ce_loss(logits, targets)

    loss, grads = loss_fn(model.trainable_parameters()), mx.grad(loss_fn)(
        model.trainable_parameters()
    )
    optimizer.update(model, grads)
    return loss


def evaluate(model, val_inputs, val_targets):
    logits = model(val_inputs)
    return ce_loss(logits, val_targets)


# ---------------------------------------------------------------------------
# 6.  CLI
# ---------------------------------------------------------------------------

def build_parser():
    p = argparse.ArgumentParser(
        description="Train a ternary (BitNet b1.58) model using MLX-QUANT"
    )
    p.add_argument("--model", default="Qwen/Qwen2.5-0.5B", help="HF model ID")
    p.add_argument("--data", required=True, help="Path to .jsonl training data")
    p.add_argument("--val-data", help="Path to .jsonl validation data (optional)")
    p.add_argument("--lr", type=float, default=1e-4, help="Learning rate")
    p.add_argument("--batch-size", type=int, default=4, help="Per-device batch size")
    p.add_argument("--epochs", type=int, default=3, help="Number of epochs")
    p.add_argument("--max-len", type=int, default=512, help="Max sequence length")
    p.add_argument("--max-samples", type=int, default=None, help="Limit training samples")
    p.add_argument("--binary", action="store_true", help="Use binary (provably 1-bit) instead of ternary")
    p.add_argument("--export", default="quantal_model.ayeos.json", help="ayeOS export path")
    p.add_argument("--save", default="quantal_model.safetensors", help="Save checkpoint path")
    p.add_argument("--group-size", type=int, default=64, help="Quantization group size")
    p.add_argument("--seed", type=int, default=42, help="Random seed")
    return p


def main():
    args = build_parser().parse_args()
    mx.random.seed(args.seed)
    np.random.seed(args.seed)

    print(f"  quantal training")
    print(f"  model:     {args.model}")
    print(f"  data:      {args.data}")
    print(f"  lr:        {args.lr}")
    print(f"  batch:     {args.batch_size}")
    print(f"  epochs:    {args.epochs}")
    print(f"  max_len:   {args.max_len}")
    print(f"  binary:    {args.binary}")
    print(f"  group:     {args.group_size}")
    print(f"  metal:     {mx.metal.is_available()}")
    print()

    # -- Load model
    print("1. loading base model...")
    try:
        from transformers import AutoModelForCausalLM, AutoTokenizer

        hf_model = AutoModelForCausalLM.from_pretrained(args.model, torch_dtype="auto")
        tokenizer = AutoTokenizer.from_pretrained(args.model)
        tokenizer.pad_token = tokenizer.eos_token

        from mlx.nn import from_huggingface

        model = from_huggingface(hf_model)
        del hf_model
        n_params = sum(v.nbytes for v in model.trainable_parameters().values()) / 1e6
        print(f"   loaded {n_params:.1f}M params")
    except ImportError:
        print("   transformers not available; building a tiny test transformer")
        model = nn.Transformer(dims=256, num_heads=4, num_encoder_layers=4)
        tokenizer = None

    # -- Replace Linear -> BitLinear
    print("2. swapping Linear -> BitLinear (QAT)...")
    n_before = sum(1 for m in model.modules() if isinstance(m, nn.Linear))
    model = replace_linear_with_bitlinear(model, binary=args.binary)
    n_after = sum(1 for m in model.modules() if isinstance(m, BitLinear))
    print(f"   replaced {n_before} Linear -> {n_after} BitLinear")

    # -- Load data
    print("3. loading data...")
    samples = load_jsonl(args.data, max_samples=args.max_samples)
    print(f"   {len(samples)} training samples")

    val_samples = []
    if args.val_data:
        val_samples = load_jsonl(args.val_data, max_samples=min(args.max_samples or 100, 100))
        print(f"   {len(val_samples)} validation samples")

    if not val_samples and len(samples) > 100:
        val_samples = samples[-50:]
        samples = samples[:-50]
        print(f"   held out {len(val_samples)} for validation")

    # -- Optimizer
    print("4. setting up optimizer...")
    optimizer = opt.AdamW(learning_rate=args.lr)

    # -- Training loop
    print("5. training...")
    n_batches = len(samples) // args.batch_size
    best_val_loss = float("inf")

    for epoch in range(args.epochs):
        epoch_loss = 0.0
        t0 = time.time()
        idxs = np.random.permutation(len(samples))

        for batch_idx in range(0, len(idxs), args.batch_size):
            batch_idxs = idxs[batch_idx: batch_idx + args.batch_size]
            batch_texts = [samples[i] for i in batch_idxs]

            tokens = tokenize_fn(tokenizer, batch_texts, args.max_len)
            if not tokens:
                continue
            max_t = max(t.shape[0] for t in tokens)
            padded = mx.zeros((len(tokens), max_t), dtype=mx.uint32)
            for i, t in enumerate(tokens):
                padded[i, : t.shape[0]] = t

            inputs = padded[:, :-1]
            targets = padded[:, 1:]

            loss = train_step(model, inputs, targets, optimizer)
            epoch_loss += loss.item()

            if (batch_idx // args.batch_size) % 10 == 0:
                mx.metal.clear_cache()

        avg_loss = epoch_loss / max(1, n_batches)
        elapsed = time.time() - t0

        val_loss = None
        if val_samples:
            val_tokens = tokenize_fn(tokenizer, val_samples, args.max_len)
            if val_tokens:
                max_t = max(t.shape[0] for t in val_tokens)
                vpadded = mx.zeros((len(val_tokens), max_t), dtype=mx.uint32)
                for i, t in enumerate(val_tokens):
                    vpadded[i, : t.shape[0]] = t
                vinputs = vpadded[:, :-1]
                vtargets = vpadded[:, 1:]
                val_loss = evaluate(model, vinputs, vtargets).item()
                if val_loss < best_val_loss:
                    best_val_loss = val_loss

        print(
            f"  epoch {epoch + 1}/{args.epochs} | "
            f"loss {avg_loss:.4f} | "
            + (f"val {val_loss:.4f} | " if val_loss else "")
            + f"{elapsed:.1f}s"
        )

    # -- Save checkpoint
    print("6. saving checkpoint...")
    if args.save:
        model.save_weights(args.save)
        print(f"   weights -> {args.save}")

    # -- Export to ayeOS
    print("7. exporting to ayeOS ternary format...")
    export_to_ayeos(
        model,
        args.export,
        group_size=args.group_size,
        metadata={
            "base_model": args.model,
            "binary": args.binary,
            "group_size": args.group_size,
            "epochs": args.epochs,
            "final_loss": avg_loss,
        },
    )

    print("  done")


if __name__ == "__main__":
    main()
