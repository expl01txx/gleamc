//// A tour of the gleam/string standard library module.
////
////   gleamc examples/string.gleam -o /tmp/gleam-string
////   cc -std=c11 /tmp/gleam-string/main.c /tmp/gleam-string/gleam_runtime.c \
////      -o /tmp/gleam-string/prog
import gleam/io

/// Render a Bool as a string (this compiler has no gleam/bool module).
fn bool_to_string(b: Bool) -> String {
  case b {
    True -> "True"
    False -> "False"
  }
}

/// Render an Order comparison result.
fn order_to_string(order: order.Order) -> String {
  case order {
    Eq -> "Eq"
    Lt -> "Lt"
    Gt -> "Gt"
  }
}

pub fn main() {
  // Concatenation.
  io.println("gl" <> "e" <> "am")
  io.println(string.concat(["a", "b", "c"]))

  // Case and length.
  io.println("lower: " <> string.lowercase("GLEAM"))
  io.println("upper: " <> string.uppercase("gleam"))
  io.println("capitalise: " <> string.capitalise("hello world"))
  io.println("length: " <> int.to_string(string.length("hello")))
  io.println("byte_size: " <> int.to_string(string.byte_size("hello")))

  // Whitespace.
  io.println("[" <> string.trim("  padded  ") <> "]")
  io.println("[" <> string.trim_start("  left") <> "]")
  io.println("[" <> string.trim_end("right  ") <> "]")

  // Predicates.
  io.println("empty: " <> bool_to_string(string.is_empty("")))
  io.println("starts_with: " <> bool_to_string(string.starts_with("hello", "he")))
  io.println("ends_with: " <> bool_to_string(string.ends_with("hello", "lo")))
  io.println("contains: " <> bool_to_string(string.contains("hello world", "world")))

  // Splitting and joining.
  let words = string.split("one,two,three", ",")
  io.println("split: " <> string.inspect(words))
  io.println("join: " <> string.join(["one", "two", "three"], with: ", "))

  // Replacing and repeating.
  io.println("replace: " <> string.replace(in: "lollipops", each: "l", with: "L"))
  io.println("repeat: " <> string.repeat("ha", times: 3))

  // Slicing and cropping.
  io.println("slice: " <> string.slice(from: "hello world", at_index: 6, length: 5))
  io.println("crop: " <> string.crop(from: "hello world", before: "world"))
  io.println("remove_prefix: " <> string.remove_prefix(from: "hello world", matching: "hello "))
  io.println("remove_suffix: " <> string.remove_suffix(from: "hello world", matching: " world"))

  // Ends of a string, as Option values.
  let first = string.first("hello")
  let last = string.last("hello")
  io.println("first/last: " <> string.inspect(first) <> " / " <> string.inspect(last))

  // Comparison and conversion to Option.
  io.println(order_to_string(string.compare("apple", "banana")))
  io.println("to_option(\"\"): " <> string.inspect(string.to_option("")))
}
