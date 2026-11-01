#!/usr/bin/env python3
"""Read-only image checks and a local counterexample to signer authentication.

Does not import the OTA hub, flash devices, serve files, or run its main loop.
Optional --online checks the three published HTTPS endpoints, with time/size caps.
"""
import argparse
import ast
import gzip
import hashlib
import json
from pathlib import Path
import struct
import urllib.request


def check(manifest, image):
    size = struct.unpack_from("<I", image, 8)[0]
    kernel = gzip.decompress(image[4096:4096 + size])
    return {
        "version": manifest["version"],
        "bytes": len(image),
        "gzip_bytes": size,
        "kernel_bytes": len(kernel),
        "sha256": hashlib.sha256(image).hexdigest(),
        "size_matches": len(image) == manifest["image_size_bytes"],
        "gzip_size_matches": size == manifest["gzip_kernel_bytes"],
        "sha256_matches": hashlib.sha256(image).hexdigest() == manifest["image_sha256_hex"],
        "blake2s_matches": hashlib.blake2s(image).hexdigest() == manifest["image_blake2s_hex"],
        "android_envelope": image[:8] == b"ANDROID!",
        "arm64_magic": kernel[56:60] == b"ARM\x64",
    }


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--online", action="store_true")
    args = parser.parse_args()
    root = Path(__file__).resolve().parents[1]
    boot = root / "hardware/blu_g75_g1170/boot_b"
    manifest = json.loads((boot / "manifest.json").read_text())
    image = (boot / "ayeos_boot_b_gz.img").read_bytes()
    results = {"local": check(manifest, image)}
    # Extract only the two pure verification functions; no top-level hub effects.
    tree = ast.parse((root / "scripts/wifi_ota_hub.py").read_text())
    names = {"compute_aye_signature", "verify_manifest_and_image"}
    nodes = [n for n in tree.body if isinstance(n, ast.FunctionDef) and n.name in names]
    assert len(nodes) == 2
    scope = {"hashlib": hashlib}
    exec(compile(ast.Module(body=nodes, type_ignores=[]), "hub-pure-functions", "exec"), scope)
    forged = bytearray(image)
    forged[-1] ^= 1
    altered = dict(manifest)
    altered["image_sha256_hex"] = hashlib.sha256(forged).hexdigest()
    altered["image_blake2s_hex"] = hashlib.blake2s(forged).hexdigest()
    altered["signature_hex"] = scope["compute_aye_signature"](
        altered["device_slug"], altered["version"], altered["image_sha256_hex"]
    )
    results["public_hash_counterexample"] = {
        "original": scope["verify_manifest_and_image"](manifest, image),
        "altered_image_without_manifest_change": scope["verify_manifest_and_image"](manifest, forged),
        "altered_image_and_recomputed_public_hashes": scope["verify_manifest_and_image"](altered, forged),
        "signing_key_used": False,
    }
    if args.online:
        results["https"] = {}
        for host in ("g8t.is", "aye.is", "8b.is"):
            try:
                fetched = []
                urls = []
                for name in ("manifest.json", "boot.img"):
                    url = "https://" + host + "/g8t/ota/blu-g75/" + name
                    with urllib.request.urlopen(url, timeout=20) as response:
                        data = response.read(1048577)
                        if len(data) > 1048576:
                            raise ValueError("response exceeds 1 MiB")
                        urls.append(response.url)
                        fetched.append(data)
                entry = check(json.loads(fetched[0]), fetched[1])
                entry["identical_to_local"] = fetched[1] == image
                entry["final_urls"] = urls
                results["https"][host] = entry
            except Exception as exc:
                results["https"][host] = {"error": str(exc)}
    print(json.dumps(results, indent=2))


if __name__ == "__main__":
    main()
