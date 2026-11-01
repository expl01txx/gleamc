//// A tour of basic Gleam language features.
////
////   gleamc examples/basic.gleam -o /tmp/gleam-basic
////   cc -std=c11 /tmp/gleam-basic/main.c /tmp/gleam-basic/gleam_runtime.c \
////      -o /tmp/gleam-basic/prog
import gleam/io

/// A custom type with two constructors.
pub type Shape {
  Circle(radius: Float)
  Rectangle(width: Float, height: Float)
}

pub const answer = 42
pub const greeting = "Hello from Gleam!"

/// Area of a shape, using pattern matching on records.
pub fn area(shape: Shape) -> Float {
  case shape {
    Circle(radius) -> 3.14159 *. radius *. radius
    Rectangle(width, height) -> width *. height
  }
}

/// A function with a labeled argument.
pub fn describe(number: Int, adjective: String) -> String {
  adjective <> " " <> int.to_string(number)
}

pub fn main() {
  // Let bindings and arithmetic.
  let x = 40 + 2
  io.println(greeting)
  io.println("answer: " <> int.to_string(x))

  // Tuples and destructuring.
  let point = #(3, 4)
  let #(a, b) = point
  io.println("point sum: " <> int.to_string(a + b))

  // Lists and spread.
  let numbers = [1, 2, 3]
  let with_zero = [0, ..numbers]
  io.println("list length: " <> int.to_string(list.length(with_zero)))

  // Case expressions with guards.
  let label = case x {
    n if n < 0 -> "negative"
    0 -> "zero"
    _ -> "positive"
  }
  io.println("label: " <> label)

  // Records, closures, and pipelines.
  let shapes = [Circle(radius: 1.0), Rectangle(width: 2.0, height: 3.0)]
  let total_area =
    shapes
    |> list.map(area)
    |> list.fold(from: 0.0, with: fn(acc, s) { acc +. s })
  io.println("total area: " <> string.inspect(total_area))

  // Labeled arguments.
  io.println(describe(number: x, adjective: "the answer is"))

  // Result values from the standard library.
  case int.parse("123") {
    Ok(n) -> io.println("parsed: " <> int.to_string(n))
    Error(_) -> io.println("parse failed")
  }

  io.println("done!")
}
