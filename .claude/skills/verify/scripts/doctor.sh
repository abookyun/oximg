#!/usr/bin/env bash
# Read-only verification doctor for oximg. Does not spawn, bind, or kill.
set -euo pipefail

script_dir=$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)
# scripts/ -> verify -> skills -> .claude -> repo root
root=$(git -C "$script_dir" rev-parse --show-toplevel 2>/dev/null || true)
if [[ -z "${root}" ]]; then
  root=$(cd "$script_dir/../../../.." && pwd)
fi
cd "$root"

# JSON string. python3 if present; otherwise a bash escape sufficient for
# paths, versions, and the short error strings this script emits.
json_str() {
  if command -v python3 >/dev/null 2>&1; then
    python3 -c 'import json,sys; print(json.dumps(sys.argv[1]))' "$1"
    return
  fi
  local s=$1
  s=${s//\\/\\\\}
  s=${s//\"/\\\"}
  s=${s//$'\n'/\\n}
  s=${s//$'\r'/\\r}
  s=${s//$'\t'/\\t}
  printf '"%s"' "$s"
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

# The cargo profile whose binaries to check: `agent` for the edit loop,
# `release` when the proof must come from the shipped codegen.
profile=${OXIMG_PROFILE:-agent}
if [[ "$profile" == release ]]; then
  build_cmd="cargo build --release"
else
  build_cmd="cargo build --profile $profile"
fi
oximg="$root/target/$profile/oximg"
ctl="$root/target/$profile/oximg-ctl"
oximg_version=""
ctl_version=""

# The [package] version: the first top-level `version =` in Cargo.toml.
pkg_version=$(sed -n 's/^version = "\(.*\)"$/\1/p' "$root/Cargo.toml" | head -n1)

# A binary older than a source file is proof for other code. Version
# alone cannot tell: every build between two releases reports the same
# one. Only inputs whose newer mtime makes cargo relink belong here:
# cargo rebuilds on a newer src/ or build.rs, but a touched Cargo.toml
# or Cargo.lock with no real change leaves the binary as it was, and
# would read as stale forever. The version check covers the manifest.
stale_input() {
  find "$root/src" "$root/build.rs" -type f -newer "$1" 2>/dev/null | head -n1
}

if [[ -x "$oximg" ]]; then
  oximg_version=$("$oximg" --version 2>/dev/null | head -n1 | tr -d '\r')
  newer=$(stale_input "$oximg")
  if [[ ! "$oximg_version" =~ ^oximg\ [0-9] ]]; then
    ok=false
    errors+=("unexpected oximg --version: ${oximg_version:-empty}")
  elif [[ "$oximg_version" != "oximg $pkg_version" ]]; then
    ok=false
    errors+=("$oximg is ${oximg_version#oximg }, Cargo.toml is $pkg_version — run: $build_cmd")
  elif [[ -n "$newer" ]]; then
    ok=false
    errors+=("$oximg is older than ${newer#"$root"/} — run: $build_cmd")
  fi
else
  ok=false
  errors+=("missing executable $oximg — run: $build_cmd")
fi

if [[ -x "$ctl" ]]; then
  ctl_version=$("$ctl" --version 2>/dev/null | head -n1 | tr -d '\r')
  newer=$(stale_input "$ctl")
  if [[ ! "$ctl_version" =~ ^oximg-ctl\ [0-9] ]]; then
    ok=false
    errors+=("unexpected oximg-ctl --version: ${ctl_version:-empty}")
  elif [[ "$ctl_version" != "oximg-ctl $pkg_version" ]]; then
    ok=false
    errors+=("$ctl is ${ctl_version#oximg-ctl }, Cargo.toml is $pkg_version — run: $build_cmd")
  elif [[ -n "$newer" ]]; then
    ok=false
    errors+=("$ctl is older than ${newer#"$root"/} — run: $build_cmd")
  fi
else
  ok=false
  errors+=("missing executable $ctl — run: $build_cmd")
fi

fixtures="$root/tests/fixtures"
have_fixtures=false
if [[ -d "$fixtures" ]]; then
  have_fixtures=true
else
  ok=false
  errors+=("missing $fixtures")
fi

# Unix pid check via kill/ps (Linux and macOS). Not /proc — that is
# Linux-only and would mark a live macOS serve pid as dead.
spawned="none"
if [[ -n "${OXIMG_VERIFY_PID:-}" ]]; then
  pid="$OXIMG_VERIFY_PID"
  if [[ ! "$pid" =~ ^[0-9]+$ ]]; then
    ok=false
    errors+=("OXIMG_VERIFY_PID is not a pid: $pid")
    spawned="invalid"
  elif ! ps -p "$pid" >/dev/null 2>&1; then
    ok=false
    errors+=("OXIMG_VERIFY_PID=$pid is not running")
    spawned="dead"
  else
    uid=$(id -u)
    proc_uid=$(ps -o uid= -p "$pid" 2>/dev/null | tr -d '[:space:]')
    comm=$(ps -o comm= -p "$pid" 2>/dev/null | tr -d '[:space:]')
    base=$(basename "$comm")
    if [[ -n "$proc_uid" && "$proc_uid" != "$uid" ]]; then
      ok=false
      errors+=("OXIMG_VERIFY_PID=$pid is not owned by uid $uid")
      spawned="foreign"
    elif [[ "$base" != "oximg" && "$base" != "oximg-ctl" ]]; then
      ok=false
      errors+=("OXIMG_VERIFY_PID=$pid comm is ${comm:-unknown}, expected oximg or oximg-ctl")
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
  "profile": %s,
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
  "$(json_str "$profile")" \
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
