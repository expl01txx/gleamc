//// A tour of the gleam/int standard library module.
////
////   gleamc examples/int.gleam -o /tmp/gleam-int
////   cc -std=c11 /tmp/gleam-int/main.c /tmp/gleam-int/gleam_runtime.c \
////      -o /tmp/gleam-int/prog
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
  // Conversions.
  io.println(int.to_string(123))
  case int.parse("42") {
    Ok(n) -> io.println("parsed: " <> int.to_string(n))
    Error(_) -> io.println("parse failed")
  }
  io.println("to_float: " <> string.inspect(int.to_float(7)))

  // Parity and sign.
  io.println("is_even(4): " <> bool_to_string(int.is_even(4)))
  io.println("is_odd(7): " <> bool_to_string(int.is_odd(7)))
  io.println("negate(5): " <> int.to_string(int.negate(5)))
  io.println("abs(-9): " <> int.to_string(int.absolute_value(-9)))

  // Extrema and clamping.
  io.println("max: " <> int.to_string(int.max(3, 9)))
  io.println("min: " <> int.to_string(int.min(3, 9)))
  io.println("clamp: " <> int.to_string(int.clamp(15, min: 0, max: 10)))

  // Comparison.
  io.println(order_to_string(int.compare(3, with: 9)))
  io.println(order_to_string(int.compare(9, with: 3)))
  io.println(order_to_string(int.compare(4, with: 4)))

  // Sums and products over lists.
  let numbers = [1, 2, 3, 4]
  io.println("sum: " <> int.to_string(int.sum(numbers)))
  io.println("product: " <> int.to_string(int.product(numbers)))

  // Numbers in other bases.
  io.println("binary(8): " <> int.to_base2(8))
  io.println("octal(64): " <> int.to_base8(64))
  io.println("hex(255): " <> int.to_base16(255))
  io.println("base36(35): " <> int.to_base36(35))
  case int.to_base_string(45, base: 16) {
    Ok(hex) -> io.println("base16(45): " <> hex)
    Error(_) -> io.println("bad base")
  }

  // Digits as a list, and back.
  case int.digits(456, base: 10) {
    Ok(digits) -> io.println("digits: " <> string.inspect(digits))
    Error(_) -> io.println("bad base")
  }
  case int.undigits([4, 5, 6], base: 10) {
    Ok(value) -> io.println("undigits: " <> int.to_string(value))
    Error(_) -> io.println("bad base")
  }

  // A random integer in a range (the last line, so the output above it is
  // deterministic).
  let roll = int.random(1, 6)
  io.println("roll: " <> int.to_string(roll))
}
