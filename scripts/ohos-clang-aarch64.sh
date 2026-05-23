#!/usr/bin/env bash
# Cargo linker wrapper — aarch64-unknown-linux-ohos
# Requires: export OHOS_NDK_HOME=/path/to/ohos-sdk/openharmony/native
set -euo pipefail
NDK="${OHOS_NDK_HOME:?OHOS_NDK_HOME must be set to the OHOS NDK native/ directory}"
exec "${NDK}/llvm/bin/clang" --target=aarch64-linux-ohos --sysroot="${NDK}/sysroot" "$@"
