//// A tour of the gleam/list standard library module.
////
////   gleamc examples/list.gleam -o /tmp/gleam-list
////   cc -std=c11 /tmp/gleam-list/main.c /tmp/gleam-list/gleam_runtime.c \
////      -o /tmp/gleam-list/prog
import gleam/io

/// Render a Bool as a string (this compiler has no gleam/bool module).
fn bool_to_string(b: Bool) -> String {
  case b {
    True -> "True"
    False -> "False"
  }
}

pub fn main() {
  let numbers = [1, 2, 3, 4, 5]

  // Basics.
  io.println(string.inspect(numbers))
  io.println("length: " <> int.to_string(list.length(numbers)))
  io.println("is_empty: " <> bool_to_string(list.is_empty(numbers)))
  io.println("reversed: " <> string.inspect(list.reverse(numbers)))

  // Combining and slicing.
  io.println("spread: " <> string.inspect([0, ..numbers]))
  io.println("append: " <> string.inspect(list.append([1, 2], [3, 4])))
  io.println("drop: " <> string.inspect(list.drop(numbers, up_to: 2)))
  io.println("take: " <> string.inspect(list.take(numbers, up_to: 3)))

  // Mapping and filtering.
  let doubled = list.map(numbers, with: fn(n) { n * 2 })
  io.println("doubled: " <> string.inspect(doubled))
  let evens = list.filter(numbers, keeping: fn(n) { int.is_even(n) })
  io.println("evens: " <> string.inspect(evens))

  // Folding.
  let total = list.fold(over: numbers, from: 0, with: fn(acc, n) { acc + n })
  io.println("sum: " <> int.to_string(total))

  // Zipping and unzipping.
  let pairs = list.zip(numbers, [10, 20, 30])
  io.println("zip: " <> string.inspect(pairs))
  let #(left, right) = list.unzip(pairs)
  io.println("unzip: " <> string.inspect(left) <> " / " <> string.inspect(right))

  // Searching.
  io.println("contains: " <> bool_to_string(list.contains(numbers, 3)))
  case list.find(in: numbers, one_that: fn(n) { n > 2 }) {
    Ok(found) -> io.println("first above 2: " <> int.to_string(found))
    Error(_) -> io.println("not found")
  }

  // Sorting and uniqueness.
  let shuffled = [3, 1, 2]
  let sorted = list.sort(shuffled, by: fn(a, b) { int.compare(a, with: b) })
  io.println("sorted: " <> string.inspect(sorted))
  io.println("unique: " <> string.inspect(list.unique([1, 2, 1, 3, 2])))

  // Chunking and interspersing.
  io.println("chunked: " <> string.inspect(list.sized_chunk(numbers, into: 2)))
  io.println("interspersed: " <> string.inspect(list.intersperse([1, 2, 3], with: 0)))

  // Ends of a list.
  let first = list.first(numbers)
  let last = list.last(numbers)
  let rest = list.rest(numbers)
  io.println("first/last: " <> string.inspect(first) <> " / " <> string.inspect(last))
  io.println("rest: " <> string.inspect(rest))
}
