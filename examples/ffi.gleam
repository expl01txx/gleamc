import gleam/io

@external(c, "./ffi_native.c", "example_clamp_byte")
fn clamp_byte(value: Int) -> Int

@external(c, "ffi_native.c", "example_keep_string")
fn keep_string(value: String) -> String

pub fn main() {
  assert clamp_byte(-3) == 0
  assert clamp_byte(42) == 42
  assert clamp_byte(300) == 255

  let kept = keep_string("C " <> "FFI works")
  io.println(kept)
}
