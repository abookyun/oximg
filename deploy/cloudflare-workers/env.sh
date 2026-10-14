# Toolchain env for building oximg's C/C++ codecs to wasm. Source it
# with WASI_SDK_PATH pointing at an unpacked wasi-sdk (verified with
# wasi-sdk 34, 2026-10-14).
#
#   WASI_SDK_PATH=~/wasi-sdk-34.0 . deploy/cloudflare-workers/env.sh

: "${WASI_SDK_PATH:?set WASI_SDK_PATH to an unpacked wasi-sdk}"
_here=$(cd "$(dirname "${BASH_SOURCE[0]:-$0}")" && pwd)
_sysroot=$WASI_SDK_PATH/share/wasi-sysroot
_lib=$_sysroot/lib/wasm32-wasip1
_rt=$(echo "$WASI_SDK_PATH"/lib/clang/*/lib/wasm32-unknown-wasip1)

for _t in wasm32_wasip1 wasm32_unknown_unknown; do
  export CC_$_t="$WASI_SDK_PATH/bin/clang" CXX_$_t="$WASI_SDK_PATH/bin/clang++" AR_$_t="$WASI_SDK_PATH/bin/llvm-ar"
done
export CMAKE_TOOLCHAIN_FILE_wasm32_wasip1=$WASI_SDK_PATH/share/cmake/wasi-sdk-p1.cmake

# jpegli's error.cc includes <setjmp.h> without calling it, and wasi-libc
# refuses that header unless wasm exception handling is on.
export CXXFLAGS_wasm32_wasip1="-isystem $_here/shim-include"

# Path A (wasm32-unknown-unknown): no libc exists for that target, so the
# C code is compiled as wasip1 objects and linked with wasi-libc, libc++
# and compiler-rt; rust-worker/src/wasi_stubs.rs defines the WASI calls.
export CFLAGS_wasm32_unknown_unknown="--sysroot=$_sysroot --target=wasm32-wasip1"
export CXXFLAGS_wasm32_unknown_unknown="--sysroot=$_sysroot --target=wasm32-wasip1 -isystem $_here/shim-include"
export CMAKE_TOOLCHAIN_FILE_wasm32_unknown_unknown=$WASI_SDK_PATH/share/cmake/wasi-sdk-p1.cmake
export CARGO_TARGET_WASM32_UNKNOWN_UNKNOWN_RUSTFLAGS="-L $_lib -L $_lib/noeh -L $_rt -l static=c++abi -l static=c -l static=clang_rt.builtins"
export CARGO_TARGET_WASM32_WASIP1_RUSTFLAGS="-L $_lib/noeh -l static=c++abi"

# WASM_SIMD=1: wasm simd128 for Rust and C (V8 has shipped it since
# 9.1, so Workers runs it).
if [ "${WASM_SIMD:-0}" = 1 ]; then
  for _v in CFLAGS_wasm32_unknown_unknown CXXFLAGS_wasm32_unknown_unknown CXXFLAGS_wasm32_wasip1; do
    export $_v="${!_v} -msimd128"
  done
  export CFLAGS_wasm32_wasip1="-msimd128"
  export CARGO_TARGET_WASM32_UNKNOWN_UNKNOWN_RUSTFLAGS="$CARGO_TARGET_WASM32_UNKNOWN_UNKNOWN_RUSTFLAGS -C target-feature=+simd128"
  export CARGO_TARGET_WASM32_WASIP1_RUSTFLAGS="$CARGO_TARGET_WASM32_WASIP1_RUSTFLAGS -C target-feature=+simd128"
fi
unset _here _sysroot _lib _rt _t _v
