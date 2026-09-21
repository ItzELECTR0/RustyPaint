#!/usr/bin/env bash
set -euo pipefail

runtime_dir="${1:?usage: build-cutout-runtime.sh SOURCE_DIRECTORY}"
runtime_revision=da9b5e364c465de65c49d91e696cd6485270757f
if [[ ! -e "$runtime_dir" ]]; then
    git clone --depth 1 --branch v1.28.0 --recursive --shallow-submodules \
        https://github.com/microsoft/onnxruntime.git "$runtime_dir"
fi
if [[ "$(git -C "$runtime_dir" rev-parse HEAD)" != "$runtime_revision" ]]; then
    echo 'Unexpected ONNX Runtime source revision; use a separate build directory.' >&2
    exit 1
fi

cd "$runtime_dir"
parallel="${CMAKE_BUILD_PARALLEL_LEVEL:-4}"
python3.12 tools/ci_build/build.py \
    --build_dir build/Linux --config Release --update --build \
    --parallel "$parallel" --skip_tests --allow_running_as_root \
    --compile_no_warning_as_error \
    --cmake_extra_defines onnxruntime_ENABLE_CPUINFO=ON onnxruntime_BUILD_UNIT_TESTS=OFF

# The providers reference re2, which nothing in the default target ever builds.
cmake --build build/Linux/Release --target re2 --parallel "$parallel"

# ort links one archive when it finds one and otherwise enumerates each component itself, which
# already misses what this version splits out. Hand it the single archive instead.
rm -rf lib
mkdir -p lib
{
    echo "create $PWD/lib/libonnxruntime.a"
    find "$PWD/build/Linux/Release" -name '*.a' -printf 'addlib %p\n'
    echo save
    echo end
} | ar -M
