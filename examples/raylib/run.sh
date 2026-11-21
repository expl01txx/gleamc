#!/usr/bin/env bash
set -euo pipefail

demo_dir=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)
raylib_flags=$(pkg-config --cflags --libs raylib)
read -r -a raylib_args <<< "$raylib_flags"
build_dir=$(mktemp -d)
trap 'rm -rf -- "$build_dir"' EXIT

cargo run --quiet --locked --manifest-path "$demo_dir/../../Cargo.toml" -- \
  "$demo_dir/main.gleam" -c -o "$build_dir"
cc -std=c11 -Wall -Wextra -pedantic -I "$build_dir" \
  "$build_dir/main.c" "$demo_dir/raylib_ffi.c" \
  "${raylib_args[@]}" -o "$build_dir/raylib-rectangle"
"$build_dir/raylib-rectangle"
