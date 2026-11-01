import gleam/io
import gleam/int
import gleam/list

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
  // new / wrap / prepend / append
  check(list.new() == [], "new")
  check(list.wrap(1) == [1], "wrap")
  check(list.prepend([1, 2], 0) == [0, 1, 2], "prepend")
  check(list.append([], [1]) == [1], "append to empty")
  check(list.append([1], []) == [1], "append empty")
  check(list.append([1, 2], [3, 4]) == [1, 2, 3, 4], "append")

  // is_empty / length
  check(list.is_empty([]), "is_empty true")
  check(!list.is_empty([1]), "is_empty false")
  check(list.length([]) == 0, "length empty")
  check(list.length([1, 2, 3]) == 3, "length")

  // first / last / rest
  check(
    case list.first([1, 2]) {
      Ok(x) -> x == 1
      _ -> False
    },
    "first"
  )
  check(
    case list.first([]) {
      Error(Nil) -> True
      _ -> False
    },
    "first empty"
  )
  check(
    case list.last([1, 2, 3]) {
      Ok(x) -> x == 3
      _ -> False
    },
    "last"
  )
  check(
    case list.last([]) {
      Error(Nil) -> True
      _ -> False
    },
    "last empty"
  )
  check(
    case list.rest([1, 2, 3]) {
      Ok(r) -> r == [2, 3]
      _ -> False
    },
    "rest"
  )
  check(
    case list.rest([]) {
      Error(Nil) -> True
      _ -> False
    },
    "rest empty"
  )

  // reverse / drop / take
  check(list.reverse([1, 2, 3]) == [3, 2, 1], "reverse")
  check(list.reverse([]) == [], "reverse empty")
  check(list.drop([1, 2, 3, 4, 5], 2) == [3, 4, 5], "drop")
  check(list.drop([1, 2], 5) == [], "drop beyond end")
  check(list.drop([1, 2], 0) == [1, 2], "drop zero")
  check(list.take([1, 2, 3, 4], 2) == [1, 2], "take")
  check(list.take([1, 2], 5) == [1, 2], "take beyond end")
  check(list.take([1, 2], 0) == [], "take zero")

  // take_while / drop_while
  check(
    list.take_while([1, 2, 3, 4], fn(x) { x < 3 }) == [1, 2],
    "take_while"
  )
  check(
    list.drop_while([1, 2, 3, 4], fn(x) { x < 3 }) == [3, 4],
    "drop_while"
  )
  check(
    list.take_while([1, 2], fn(x) { x > 0 }) == [1, 2],
    "take_while all"
  )
  check(
    list.drop_while([1, 2], fn(x) { x > 5 }) == [1, 2],
    "drop_while none"
  )

  // split / split_while
  check(
    case list.split([1, 2, 3, 4], 2) {
      #(a, b) -> a == [1, 2] && b == [3, 4]
    },
    "split"
  )
  check(
    case list.split([1, 2], 5) {
      #(a, b) -> a == [1, 2] && b == []
    },
    "split beyond end"
  )
  check(
    case list.split_while([1, 2, 3, 4], fn(x) { x < 3 }) {
      #(a, b) -> a == [1, 2] && b == [3, 4]
    },
    "split_while"
  )

  // contains / all / any / count
  check(list.contains([1, 2, 3], 2), "contains true")
  check(!list.contains([1, 2, 3], 5), "contains false")
  check(list.all([1, 2, 3], fn(x) { x > 0 }), "all true")
  check(!list.all([1, 2, 3], fn(x) { x > 1 }), "all false")
  check(list.any([1, 2, 3], fn(x) { x == 2 }), "any true")
  check(!list.any([1, 2, 3], fn(x) { x == 5 }), "any false")
  check(list.all([], fn(x) { x > 0 }), "all empty")
  check(!list.any([], fn(x) { x > 0 }), "any empty")
  check(list.count([1, 2, 3, 4], fn(x) { x % 2 == 0 }) == 2, "count")

  // filter / map / flat_map / flatten
  check(
    list.filter([1, 2, 3, 4], fn(x) { x % 2 == 0 }) == [2, 4],
    "filter"
  )
  check(list.filter([], fn(x) { True }) == [], "filter empty")
  check(
    list.map([1, 2, 3], fn(x) { x * 2 }) == [2, 4, 6],
    "map"
  )
  check(
    list.flat_map([1, 2], fn(x) { [x, x] }) == [1, 1, 2, 2],
    "flat_map"
  )
  check(
    list.flatten([[1, 2], [], [3]]) == [1, 2, 3],
    "flatten"
  )

  // fold / fold_right / scan
  check(list.fold([1, 2, 3], 0, fn(acc, x) { acc + x }) == 6, "fold sum")
  check(
    list.fold_right([1, 2, 3], "", fn(acc, x) { acc <> int.to_string(x) }) == "321",
    "fold_right"
  )
  check(
    list.scan([1, 2, 3], 0, fn(acc, x) { acc + x }) == [0, 1, 3, 6],
    "scan"
  )

  // fold_until with Continue/Stop
  check(
    case list.fold_until([1, 2, 3, 4], 0, fn(acc, x) {
      case acc + x >= 5 {
        True -> Stop(acc + x)
        False -> Continue(acc + x)
      }
    }) {
      6 -> True
      _ -> False
    },
    "fold_until stop"
  )
  check(
    list.fold_until([1, 2], 0, fn(acc, x) { Continue(acc + x) }) == 3,
    "fold_until continue all"
  )

  // index_fold / index_map
  check(
    list.index_fold([10, 20, 30], 0, fn(acc, x, i) { acc + x * i }) == 80,
    "index_fold"
  )
  check(
    list.index_map(["a", "b"], fn(x, i) { int.to_string(i) <> x }) == ["0a", "1b"],
    "index_map"
  )

  // interleave / intersperse
  check(list.interleave([[1, 2], [3, 4]]) == [1, 3, 2, 4], "interleave")
  check(list.interleave([]) == [], "interleave empty")
  check(list.intersperse([1, 2, 3], 0) == [1, 0, 2, 0, 3], "intersperse")
  check(list.intersperse([], 0) == [], "intersperse empty")
  check(list.intersperse([1], 0) == [1], "intersperse single")

  // key_filter / key_find / key_pop / key_set
  check(
    list.key_filter([#("a", 1), #("b", 2), #("a", 3)], "a") == [1, 3],
    "key_filter"
  )
  check(
    case list.key_find([#("a", 1), #("b", 2)], "b") {
      Ok(x) -> x == 2
      _ -> False
    },
    "key_find"
  )
  check(
    case list.key_find([#("a", 1)], "z") {
      Error(Nil) -> True
      _ -> False
    },
    "key_find missing"
  )
  check(
    case list.key_pop([#("a", 1), #("b", 2)], "a") {
      Ok(#(v, r)) -> v == 1 && r == [#("b", 2)]
      _ -> False
    },
    "key_pop"
  )
  check(
    case list.key_pop([#("a", 1)], "z") {
      Error(Nil) -> True
      _ -> False
    },
    "key_pop missing"
  )
  check(
    list.key_set([#("a", 1), #("b", 2)], "b", 20) == [#("a", 1), #("b", 20)],
    "key_set existing"
  )
  check(
    list.key_set([#("a", 1)], "z", 3) == [#("a", 1), #("z", 3)],
    "key_set new key"
  )

  // map2 / zip / strict_zip / unzip
  check(list.map2([1, 2], [3, 4], fn(a, b) { a + b }) == [4, 6], "map2")
  check(
    list.zip([1, 2, 3], ["a", "b"]) == [#(1, "a"), #(2, "b")],
    "zip"
  )
  check(
    case list.strict_zip([1, 2], ["a", "b"]) {
      Ok(z) -> z == [#(1, "a"), #(2, "b")]
      _ -> False
    },
    "strict_zip equal"
  )
  check(
    case list.strict_zip([1, 2, 3], ["a", "b"]) {
      Error(Nil) -> True
      _ -> False
    },
    "strict_zip unequal"
  )
  check(
    case list.unzip([#(1, "a"), #(2, "b")]) {
      #(xs, ys) -> xs == [1, 2] && ys == ["a", "b"]
    },
    "unzip"
  )

  // reduce / find / find_map
  check(
    case list.reduce([1, 2, 3], fn(a, b) { a + b }) {
      Ok(x) -> x == 6
      _ -> False
    },
    "reduce"
  )
  check(
    case list.reduce([], fn(a, b) { a + b }) {
      Error(Nil) -> True
      _ -> False
    },
    "reduce empty"
  )
  check(
    case list.find([1, 2, 3], fn(x) { x > 1 }) {
      Ok(x) -> x == 2
      _ -> False
    },
    "find"
  )
  check(
    case list.find([], fn(x) { True }) {
      Error(Nil) -> True
      _ -> False
    },
    "find empty"
  )
  check(
    case list.find_map([1, 2, 3], fn(x) {
      case x > 2 {
        True -> Ok(x * 10)
        False -> Error(Nil)
      }
    }) {
      Ok(x) -> x == 30
      _ -> False
    },
    "find_map"
  )

  // filter_map / try_map / try_each / try_fold
  check(
    list.filter_map([1, 2, 3], fn(x) {
      case x % 2 == 0 {
        True -> Ok(x * 10)
        False -> Error(Nil)
      }
    }) == [20],
    "filter_map"
  )
  check(
    case list.try_map([1, 2], fn(x) {
      case x < 3 {
        True -> Ok(x)
        False -> Error("bad")
      }
    }) {
      Ok(xs) -> xs == [1, 2]
      _ -> False
    },
    "try_map"
  )
  check(
    case list.try_map([1, 2, 3], fn(x) {
      case x < 3 {
        True -> Ok(x)
        False -> Error("bad")
      }
    }) {
      Error(e) -> e == "bad"
      _ -> False
    },
    "try_map error"
  )
  check(
    case list.try_each([1, 2], fn(x) {
      case x < 3 {
        True -> Ok(x)
        False -> Error("bad")
      }
    }) {
      Ok(Nil) -> True
      _ -> False
    },
    "try_each"
  )
  check(
    case list.try_fold([1, 2], 0, fn(acc, x) {
      case x < 5 {
        True -> Ok(acc + x)
        False -> Error(x)
      }
    }) {
      Ok(s) -> s == 3
      _ -> False
    },
    "try_fold ok"
  )
  check(
    case list.try_fold([1, 2, 10], 0, fn(acc, x) {
      case x < 5 {
        True -> Ok(acc + x)
        False -> Error(x)
      }
    }) {
      Error(e) -> e == 10
      _ -> False
    },
    "try_fold error"
  )

  // map_fold / partition
  check(
    case list.map_fold([1, 2, 3], 0, fn(acc, x) { #(acc + x, x * 10) }) {
      #(s, xs) -> s == 6 && xs == [10, 20, 30]
    },
    "map_fold"
  )
  check(
    case list.partition([1, 2, 3, 4], fn(x) { x % 2 == 0 }) {
      #(evens, odds) -> evens == [2, 4] && odds == [1, 3]
    },
    "partition"
  )

  // sort / max (with a compare function returning order/Order)
  check(
    list.sort([3, 1, 2], fn(a, b) { int.compare(a, b) }) == [1, 2, 3],
    "sort"
  )
  check(list.sort([], fn(a, b) { int.compare(a, b) }) == [], "sort empty")
  check(
    case list.max([3, 9, 1], fn(a, b) { int.compare(a, b) }) {
      Ok(x) -> x == 9
      _ -> False
    },
    "max"
  )
  check(
    case list.max([], fn(a, b) { int.compare(a, b) }) {
      Error(Nil) -> True
      _ -> False
    },
    "max empty"
  )

  // repeat / unique
  check(list.repeat(7, 3) == [7, 7, 7], "repeat")
  check(list.repeat(7, 0) == [], "repeat zero")
  check(list.unique([1, 2, 1, 3, 2]) == [1, 2, 3], "unique")
  check(list.unique([]) == [], "unique empty")

  // window / window_by_2 / sized_chunk / chunk
  check(list.window([1, 2, 3, 4], 2) == [[1, 2], [2, 3], [3, 4]], "window")
  check(list.window([1, 2], 3) == [], "window too big")
  check(
    list.window_by_2([1, 2, 3]) == [#(1, 2), #(2, 3)],
    "window_by_2"
  )
  check(list.window_by_2([]) == [], "window_by_2 empty")
  check(
    list.sized_chunk([1, 2, 3, 4, 5], 2) == [[1, 2], [3, 4], [5]],
    "sized_chunk"
  )
  check(list.sized_chunk([], 2) == [], "sized_chunk empty")
  check(
    list.chunk([1, 1, 2, 3, 3, 3], fn(x) { x }) == [[1, 1], [2], [3, 3, 3]],
    "chunk"
  )
  check(list.chunk([], fn(x) { x }) == [], "chunk empty")

  // combinations / combination_pairs / permutations
  check(
    list.combinations([1, 2, 3], 2) == [[1, 2], [1, 3], [2, 3]],
    "combinations"
  )
  check(list.combinations([1, 2], 5) == [], "combinations too big")
  check(
    list.combination_pairs([1, 2, 3]) == [#(1, 2), #(1, 3), #(2, 3)],
    "combination_pairs"
  )
  check(list.permutations([1, 2]) == [[1, 2], [2, 1]], "permutations two")
  check(list.permutations([]) == [[]], "permutations empty")
  check(
    list.length(list.permutations([1, 2, 3])) == 6,
    "permutations count"
  )

  // transpose
  check(list.transpose([[1, 2], [3, 4]]) == [[1, 3], [2, 4]], "transpose")
  check(
    list.transpose([[1, 2], [3]]) == [[1, 3], [2]],
    "transpose ragged"
  )

  // each (returns Nil; just ensure it runs)
  list.each([1, 2], fn(x) { x })
  check(True, "each")

  // shuffle: sorting the result must give back the original list.
  check(
    list.sort(list.shuffle([3, 1, 2]), fn(a, b) { int.compare(a, b) }) == [1, 2, 3],
    "shuffle is a permutation"
  )

  // sample: n >= length keeps everything in order; smaller n gives n items.
  check(list.sample([1, 2, 3], 5) == [1, 2, 3], "sample all")
  check(list.sample([], 3) == [], "sample empty")
  check(
    list.length(list.sample([1, 2, 3, 4, 5], 2)) == 2,
    "sample size"
  )

  io.println("all list tests passed")
}
