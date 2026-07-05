#!/usr/bin/env bash
# Regenerates assets/manifest.d.ts from the manifest JSON schema.
# The schema-sha256 stamp is verified by
# cli::tests::embedded_manifest_types_are_generated_from_current_schema.
set -euo pipefail
cd "$(dirname "$0")/.."

tmp="$(mktemp -d)"
trap 'rm -rf "$tmp"' EXIT

cargo run --quiet --bin runweaver -- manifest schema --cwd "$tmp"
schema="$tmp/.runweaver/manifest.schema.json"
hash="$(sha256sum "$schema" | cut -d' ' -f1)"

banner="// Generated from the Runweaver manifest JSON schema. Do not edit.
// Regenerate with: runweaver manifest types
// schema-sha256: $hash"

bunx json-schema-to-typescript -i "$schema" -o assets/manifest.d.ts --bannerComment "$banner"
echo "Wrote assets/manifest.d.ts (schema-sha256: $hash)"
