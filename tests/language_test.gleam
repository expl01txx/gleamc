//// C backend integration coverage for core syntax.

@internal
@deprecated("Use with_pair")
pub fn old() { Nil }

@external(javascript, "./fallback.mjs", "fallback")
@external(erlang, "fallback", "fallback")
fn fallback() -> Int { 4 }

type Pair {
  Pair(left: Int, right: Int)
}

fn with_pair(value: Int, callback: fn(#(Int, Int)) -> Int) -> Int {
  callback(#(value, value + 1))
}

fn run(callback: fn() -> Int) -> Int {
  callback()
}

fn select(first: Int, second: Int) -> Int {
  case first, second {
    0, value | value, 0 if !False && value > -1 -> value
    _, _ -> -1
  }
}

fn tagged(value: Pair) -> Int {
  case value {
    Pair(left: 0, right:) | Pair(left: right, right: 0) -> right
    Pair(left:, right:) -> left + right
  }
}

fn use_callbacks() -> Int {
  let initial = fallback()
  use <- run
  use #(initial, next): #(Int, Int) <- with_pair(initial)
  let sum = fn(a: Int, b: Int,) -> Int { a + b }
  sum(initial, next)
}

const negative = signed_value
const signed_value = -0x2a
const negative_float = -1.25
const header = <<3:3, -1:5, "🐟">>

fn bits() {
  let encoded = <<header:bits, 1.5:float-little-size(32), 513:little-size(16), 5:3>>
  let assert <<width:3, negative:signed-size(5), "🐟":utf8, real:float-little-32, int:little-16, tail:size(width)>> = encoded
  assert #(width, negative, real, int, tail) == #(3, -1, 1.5, 513, 5)
  assert <<1:1>> != <<128>>
  assert <<1:1, 3:2>> == <<7:3>>
  assert <<1.5:float-16, 1.0:float-little-16>> == <<0x3e, 0, 0, 0x3c>>
  let assert <<half:float-16>> = <<0, 1>>
  assert half == 0.000000059604644775390625
  assert <<"🐟":utf16-little>> == <<0x3d, 0xd8, 0x1f, 0xdc>>
  assert <<"🐟":utf32>> == <<0, 1, 0xf4, 0x1f>>
  let assert <<cp:utf8_codepoint>> = <<"🐟">>
  let assert <<other:utf16_codepoint-little>> = <<cp:utf16_codepoint-little>>
  assert <<other:utf32_codepoint>> == <<"🐟":utf32>>
  assert case <<7:3>> {
    <<_:bytes>> -> False
    <<remaining:bits>> -> remaining == <<7:3>>
  }
  assert case <<0xc0, 0x80>> {
    <<_:utf8_codepoint>> -> False
    _ -> True
  }
}

fn pair_with(first x: Int, second y: String) { #(x, y) }
fn curry(y: String) { fn(x: Int) { #(x, y) } }
const reversed = Pair(right: 3, left: 5)

type Offset { Offset(other: String, left: Int) }
type Box(a) { Box(value: a) }

fn calling_and_records() {
  let captured = pair_with(second: "fixed", first: _)
  assert captured(7) == #(7, "fixed")
  assert { 8 |> pair_with(second: "direct") } == #(8, "direct")
  assert { 9 |> curry("factory")() } == #(9, "factory")
  let indirect = fn(y: String) { fn(x: Int) { #(x, y) } }
  assert { 10 |> indirect("indirect") } == #(10, "indirect")
  let offset = Offset(other: "other", left: 6)
  assert #(reversed.left, offset.left) == #(5, 6)
  let updated = Box(..Box(1), value: "updated")
  assert updated.value == "updated"
  assert #(1 / 0, 1 % 0, 1.0 /. 0.0) == #(0, 0, 0.0)
  assert -0x8000000000000000 < -1
  let nested = #(#(1.))
  assert nested.0.0 == 1.0
  assert case "a\u{0}b" {
    "a\u{0}" as prefix <> rest -> prefix == "a\u{0}" && rest == "b"
    _ -> False
  }
}

pub fn main() {
  assert select(0, 8) == 8
  assert select(9, 0) == 9
  assert select(-1, 0) == -1
  assert tagged(Pair(left: 7, right: 0)) == 7
  assert use_callbacks() == 9
  let assert _: Int = negative as "signed constant"
  assert case negative_float {
    -1.25 -> True
    _ -> False
  }
  let _numbers: List(_) = [1, 2, 3]
  assert True as {"this message must stay unevaluated" <> { panic }}
  bits()
  calling_and_records()
  True |> echo
}
