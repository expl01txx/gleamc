import gleam/io
import gleam/int

fn check(condition: Bool, name: String) {
  case condition {
    True -> io.println(name <> " ok")
    False -> {
      io.println(name <> " FAILED")
      panic as "check failed"
    }
  }
}

type Point {
  Point(x: Int, y: Int)
}

type Color {
  Rgb(r: Int, g: Int, b: Int)
}

const origin = Point(x: 1, y: 2)
const red = Rgb(r: 255, g: 0, b: 0)
/// Constants may reference other constants.
const points = [origin, origin]

pub fn main() {
  // Field access on a const constructor.
  check(origin.x == 1, "const field x")
  check(origin.y == 2, "const field y")
  check(red.r + red.g + red.b == 255, "const rgb sum")

  // Consts are usable in expressions and lists.
  check(list.length(points) == 2, "const list length")
  check(
    case list.first(list.map(points, fn(p) { p.x + p.y })) {
      Ok(n) -> n == 3
      Error(nil) -> False
    },
    "const in pipeline"
  )

  // Pattern matching against a const value.
  case red {
    Rgb(r: 255, g: 0, b: 0) -> io.println("const pattern ok")
    _ -> panic as "const pattern failed"
  }

  io.println("all const tests passed")
}
