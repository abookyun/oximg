#!/usr/bin/env bash
# Read-only verification doctor for oximg. Does not spawn, bind, or kill.
set -euo pipefail

root=$(git rev-parse --show-toplevel 2>/dev/null || true)
if [[ -z "${root}" ]]; then
  root=$(cd "$(dirname "$0")/../../.." && pwd)
fi
cd "$root"

json_str() {
  python3 -c 'import json,sys; print(json.dumps(sys.argv[1]))' "$1"
}

ok=true
errors=()

have_cmake=false
cmake_v=""
if command -v cmake >/dev/null 2>&1; then
  have_cmake=true
  cmake_v=$(cmake --version 2>/dev/null | head -n1 | tr -d '\r')
else
  ok=false
  errors+=("cmake not on PATH (needed to build jpegli)")
fi

have_nasm=false
nasm_v=""
if command -v nasm >/dev/null 2>&1; then
  have_nasm=true
  nasm_v=$(nasm -v 2>/dev/null | head -n1 | tr -d '\r')
else
  ok=false
  errors+=("nasm not on PATH (needed to build mozjpeg SIMD)")
fi

rustc_v=""
if command -v rustc >/dev/null 2>&1; then
  rustc_v=$(rustc --version 2>/dev/null | tr -d '\r')
fi

oximg="$root/target/release/oximg"
ctl="$root/target/release/oximg-ctl"
oximg_version=""
ctl_version=""

if [[ -x "$oximg" ]]; then
  oximg_version=$("$oximg" --version 2>/dev/null | head -n1 | tr -d '\r')
else
  ok=false
  errors+=("missing executable $oximg — run: cargo build --release")
fi

if [[ -x "$ctl" ]]; then
  ctl_version=$("$ctl" --version 2>/dev/null | head -n1 | tr -d '\r')
else
  ok=false
  errors+=("missing executable $ctl — run: cargo build --release")
fi

fixtures="$root/tests/fixtures"
have_fixtures=false
if [[ -d "$fixtures" ]]; then
  have_fixtures=true
else
  ok=false
  errors+=("missing $fixtures")
fi

spawned="none"
if [[ -n "${OXIMG_VERIFY_PID:-}" ]]; then
  pid="$OXIMG_VERIFY_PID"
  if [[ ! "$pid" =~ ^[0-9]+$ ]]; then
    ok=false
    errors+=("OXIMG_VERIFY_PID is not a pid: $pid")
    spawned="invalid"
  elif [[ ! -d "/proc/$pid" ]]; then
    ok=false
    errors+=("OXIMG_VERIFY_PID=$pid is not running")
    spawned="dead"
  else
    uid=$(id -u)
    proc_uid=$(stat -c '%u' "/proc/$pid" 2>/dev/null || echo "")
    exe=$(readlink "/proc/$pid/exe" 2>/dev/null || echo "")
    base=$(basename "$exe")
    if [[ "$proc_uid" != "$uid" ]]; then
      ok=false
      errors+=("OXIMG_VERIFY_PID=$pid is not owned by uid $uid")
      spawned="foreign"
    elif [[ "$base" != "oximg" && "$base" != "oximg-ctl" ]]; then
      ok=false
      errors+=("OXIMG_VERIFY_PID=$pid exe is ${exe:-unknown}, expected oximg or oximg-ctl")
      spawned="wrong-exe"
    else
      spawned="$pid $base"
    fi
  fi
fi

error_json="[]"
if ((${#errors[@]} > 0)); then
  error_json="["
  first=1
  for e in "${errors[@]}"; do
    if [[ $first -eq 1 ]]; then
      first=0
    else
      error_json+=", "
    fi
    error_json+=$(json_str "$e")
  done
  error_json+="]"
fi

ok_json=true
[[ "$ok" == true ]] || ok_json=false

printf '{
  "ok": %s,
  "root": %s,
  "oximg": %s,
  "oximg_version": %s,
  "oximg_ctl": %s,
  "oximg_ctl_version": %s,
  "cmake": %s,
  "cmake_version": %s,
  "nasm": %s,
  "nasm_version": %s,
  "rustc_version": %s,
  "fixtures": %s,
  "spawned": %s,
  "errors": %s
}\n' \
  "$ok_json" \
  "$(json_str "$root")" \
  "$(json_str "$oximg")" \
  "$(json_str "$oximg_version")" \
  "$(json_str "$ctl")" \
  "$(json_str "$ctl_version")" \
  "$have_cmake" \
  "$(json_str "$cmake_v")" \
  "$have_nasm" \
  "$(json_str "$nasm_v")" \
  "$(json_str "$rustc_v")" \
  "$have_fixtures" \
  "$(json_str "$spawned")" \
  "$error_json"

[[ "$ok" == true ]]
