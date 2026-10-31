import gleam/io
import gleam/int {sum}

fn check(condition: Bool, name: String) {
  case condition {
    True -> io.println(name <> " ok")
    False -> {
      io.println(name <> " FAILED")
      panic as "check failed"
    }
  }
}

pub fn main() {
  // absolute_value
  check(int.absolute_value(-12) == 12, "absolute_value neg")
  check(int.absolute_value(10) == 10, "absolute_value pos")

  // clamp
  check(int.clamp(40, min: 50, max: 60) == 50, "clamp low")
  check(int.clamp(70, min: 50, max: 60) == 60, "clamp high")
  check(int.clamp(55, min: 50, max: 60) == 55, "clamp mid")

  // compare (returns gleam/order.Order)
  check(
    case int.compare(2, 3) {
      Lt -> True
      _ -> False
    },
    "compare lt"
  )
  check(
    case int.compare(4, 3) {
      Gt -> True
      _ -> False
    },
    "compare gt"
  )
  check(
    case int.compare(3, 3) {
      Eq -> True
      _ -> False
    },
    "compare eq"
  )

  // digits
  check(
    case int.digits(234, 10) {
      Ok(d) -> d == [2, 3, 4]
      _ -> False
    },
    "digits decimal"
  )
  check(
    case int.digits(234, 1) {
      Error(InvalidBase) -> True
      _ -> False
    },
    "digits bad base"
  )
  check(
    case int.digits(4, 2) {
      Ok(d) -> d == [1, 0, 0]
      _ -> False
    },
    "digits binary"
  )
  check(
    case int.digits(0, 10) {
      Ok(d) -> d == [0]
      _ -> False
    },
    "digits zero"
  )

  // is_even / is_odd
  check(int.is_even(2), "is_even true")
  check(!int.is_even(3), "is_even false")
  check(int.is_odd(3), "is_odd true")
  check(!int.is_odd(2), "is_odd false")

  // max / min
  check(int.max(2, 3) == 3, "max")
  check(int.min(2, 3) == 2, "min")

  // negate
  check(int.negate(1) == -1, "negate")
  check(int.negate(-5) == 5, "negate double")

  // parse
  check(
    case int.parse("2") {
      Ok(n) -> n == 2
      _ -> False
    },
    "parse ok"
  )
  check(
    case int.parse("ABC") {
      Error(Nil) -> True
      _ -> False
    },
    "parse letters"
  )
  check(
    case int.parse("-42") {
      Ok(n) -> n == -42
      _ -> False
    },
    "parse negative"
  )
  check(
    case int.parse("+42") {
      Ok(n) -> n == 42
      _ -> False
    },
    "parse plus"
  )
  check(
    case int.parse("") {
      Error(Nil) -> True
      _ -> False
    },
    "parse empty"
  )
  check(
    case int.parse("12x") {
      Error(Nil) -> True
      _ -> False
    },
    "parse trailing junk"
  )
  check(
    case int.parse("-") {
      Error(Nil) -> True
      _ -> False
    },
    "parse sign only"
  )

  // product / sum
  check(int.product([2, 3, 4]) == 24, "product")
  check(int.product([]) == 1, "product empty")
  check(sum([1, 2, 3]) == 6, "sum unqualified")
  check(sum([]) == 0, "sum empty")

  // to_base2 / to_base8 / to_base16 / to_base36
  check(int.to_base2(2) == "10", "to_base2")
  check(int.to_base8(15) == "17", "to_base8")
  check(int.to_base16(48) == "30", "to_base16")
  check(int.to_base36(48) == "1C", "to_base36")
  check(int.to_base2(-2) == "-10", "to_base2 negative")
  check(int.to_base16(0) == "0", "to_base16 zero")

  // to_base_string
  check(
    case int.to_base_string(2, 2) {
      Ok(s) -> s == "10"
      _ -> False
    },
    "to_base_string binary"
  )
  check(
    case int.to_base_string(48, 16) {
      Ok(s) -> s == "30"
      _ -> False
    },
    "to_base_string hex"
  )
  check(
    case int.to_base_string(48, 36) {
      Ok(s) -> s == "1C"
      _ -> False
    },
    "to_base_string base36"
  )
  check(
    case int.to_base_string(48, 1) {
      Error(InvalidBase) -> True
      _ -> False
    },
    "to_base_string too low"
  )
  check(
    case int.to_base_string(48, 37) {
      Error(InvalidBase) -> True
      _ -> False
    },
    "to_base_string too high"
  )

  // to_float / to_string
  check(int.to_float(5) == 5.0, "to_float")
  check(int.to_float(-3) == -3.0, "to_float negative")
  check(int.to_string(2) == "2", "to_string")
  check(int.to_string(-13) == "-13", "to_string negative")

  // undigits
  check(
    case int.undigits([2, 3, 4], 10) {
      Ok(n) -> n == 234
      _ -> False
    },
    "undigits decimal"
  )
  check(
    case int.undigits([2, 3, 4], 1) {
      Error(InvalidBase) -> True
      _ -> False
    },
    "undigits bad base"
  )
  check(
    case int.undigits([2, 3, 4], 2) {
      Error(InvalidBase) -> True
      _ -> False
    },
    "undigits digit out of range"
  )
  check(
    case int.undigits([1, 0, 1, 0], 2) {
      Ok(n) -> n == 10
      _ -> False
    },
    "undigits binary"
  )

  // random: the result must lie within the given boundaries.
  check(int.random(10, 20) >= 10 && int.random(10, 20) <= 20, "random in range")
  check(int.random(7, 7) == 7, "random single point")

  // digits/undigits round trip.
  check(
    case int.digits(12345, 36) {
      Ok(d) -> case int.undigits(d, 36) {
        Ok(n) -> n == 12345
        _ -> False
      }
      _ -> False
    },
    "digits round trip"
  )

  io.println("all int tests passed")
}
