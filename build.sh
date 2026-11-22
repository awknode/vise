#!/bin/sh
# Build Vise for Windows (VST3) inside Docker.
# Output: target/bundled/Vise.vst3
set -e
cd "$(dirname "$0")"
docker build -q -t kicknull-build -f Dockerfile.build . >/dev/null
mkdir -p "$HOME/.cache/kicknull-cargo"
docker run --rm -u "$(id -u):$(id -g)" \
  -e CARGO_HOME=/cargo -v "$HOME/.cache/kicknull-cargo:/cargo" \
  -v "${NICE_PLUG_AWK:-$(cd .. && pwd)/nice-plug-awk}:/nice-plug-awk:ro" -v "$PWD:/src" -w /src -e HOME=/tmp kicknull-build \
  sh -c 'cargo xtask bundle vise --release --target x86_64-pc-windows-gnu'
ls -la target/bundled
