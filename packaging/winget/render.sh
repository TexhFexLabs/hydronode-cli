#!/usr/bin/env bash
# Writes the winget manifests of a published release to manifests/t/TexhFexLabs/HydroNode/<version>/
# (the layout of microsoft/winget-pkgs). The hash comes from the release's own .sha256 file.
#   packaging/winget/render.sh 0.1.0
set -euo pipefail
version="${1:?usage: render.sh <version>}"
here="$(cd "$(dirname "$0")" && pwd)"
base="https://github.com/TexhFexLabs/hydronode-cli/releases/download/v${version}"
sha="$(curl -fsSL "${base}/hydronode-cli-x86_64-pc-windows-msvc.zip.sha256" | awk '{print toupper($1)}')"
[ "${#sha}" -eq 64 ] || { echo "no sha256 for v${version}" >&2; exit 1; }
out="${here}/manifests/t/TexhFexLabs/HydroNode/${version}"
mkdir -p "$out"
for f in "$here"/*.yaml.in; do
  name="$(basename "$f" .in)"
  sed -e "s/@VERSION@/${version}/g" -e "s/@SHA256@/${sha}/g" -e "s/@DATE@/$(date -u +%F)/g" "$f" > "${out}/${name}"
done
echo "$out"
