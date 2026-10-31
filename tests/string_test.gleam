import gleam/io
import gleam/string {join}

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
  // append
  check(string.append(to: "butter", suffix: "fly") == "butterfly", "append")

  // byte_size
  check(string.byte_size("abc") == 3, "byte_size ascii")
  check(string.byte_size("ß↑e̊") == 8, "byte_size utf8")
  check(string.byte_size("🏳️‍⚧️🏳️‍🌈👩🏾‍❤️‍👨🏻") == 58, "byte_size emoji")

  // capitalise
  check(string.capitalise("mamouna") == "Mamouna", "capitalise")
  check(string.capitalise("") == "", "capitalise empty")

  // compare (returns gleam/order.Order)
  check(
    case string.compare("Anthony", "Anthony") {
      Eq -> True
      _ -> False
    },
    "compare eq"
  )
  check(
    case string.compare("A", "B") {
      Lt -> True
      _ -> False
    },
    "compare lt"
  )
  check(
    case string.compare("B", "A") {
      Gt -> True
      _ -> False
    },
    "compare gt"
  )

  // concat
  check(string.concat(["never", "the", "less"]) == "nevertheless", "concat")
  check(string.concat([]) == "", "concat empty")

  // contains
  check(string.contains(does: "theory", contain: "ory"), "contains true")
  check(string.contains(does: "theory", contain: "the"), "contains start")
  check(!string.contains(does: "theory", contain: "THE"), "contains false")

  // crop
  check(
    string.crop(from: "The Lone Gunmen", before: "Lone") == "Lone Gunmen",
    "crop"
  )
  check(string.crop(from: "abc", before: "z") == "abc", "crop missing")

  // ends_with
  check(string.ends_with("theory", "ory"), "ends_with true")
  check(!string.ends_with("theory", "oryx"), "ends_with false")

  // first
  check(
    case string.first("") {
      Error(Nil) -> True
      _ -> False
    },
    "first empty"
  )
  check(
    case string.first("icecream") {
      Ok(f) -> f == "i"
      _ -> False
    },
    "first"
  )
  check(
    case string.first("e̊x") {
      Ok(f) -> f == "e̊"
      _ -> False
    },
    "first combining"
  )

  // inspect
  check(string.inspect(1) == "1", "inspect int")
  check(string.inspect(True) == "True", "inspect bool")
  check(string.inspect("a\"b") == "\"a\\\"b\"", "inspect string")
  check(string.inspect([1, 2]) == "[1, 2]", "inspect list")
  check(string.inspect(#(1, "x")) == "#(1, \"x\")", "inspect tuple")

  // is_empty
  check(string.is_empty(""), "is_empty true")
  check(!string.is_empty("the world"), "is_empty false")

  // join (unqualified import)
  check(
    join(["home", "evan", "Desktop"], with: "/") == "home/evan/Desktop",
    "join"
  )
  check(join([], with: "/") == "", "join empty")
  check(join(["only"], with: "/") == "only", "join single")

  // last
  check(
    case string.last("") {
      Error(Nil) -> True
      _ -> False
    },
    "last empty"
  )
  check(
    case string.last("icecream") {
      Ok(l) -> l == "m"
      _ -> False
    },
    "last"
  )
  check(
    case string.last("xe̊") {
      Ok(l) -> l == "e̊"
      _ -> False
    },
    "last combining"
  )

  // length (graphemes)
  check(string.length("Gleam") == 5, "length ascii")
  check(string.length("ß↑e̊") == 3, "length utf8")
  check(string.length("") == 0, "length empty")

  // lowercase / uppercase
  check(string.lowercase("X-FILES") == "x-files", "lowercase")
  check(string.uppercase("skinner") == "SKINNER", "uppercase")

  // pad_end / pad_start
  check(string.pad_end("123", to: 5, with: ".") == "123..", "pad_end")
  check(string.pad_end("123", to: 3, with: ".") == "123", "pad_end exact")
  check(string.pad_end("123", to: 2, with: ".") == "123", "pad_end short")
  check(string.pad_start("121", to: 5, with: ".") == "..121", "pad_start")
  check(string.pad_start("121", to: 2, with: ".") == "121", "pad_start short")

  // remove_prefix / remove_suffix
  check(string.remove_prefix("@lpil", "@") == "lpil", "remove_prefix")
  check(string.remove_prefix("hello!", "@") == "hello!", "remove_prefix miss")
  check(string.remove_suffix("Hello!", "!") == "Hello", "remove_suffix")
  check(string.remove_suffix("Hello!?", "!") == "Hello!?", "remove_suffix miss")

  // repeat
  check(string.repeat("ha", times: 3) == "hahaha", "repeat")
  check(string.repeat("ha", times: 0) == "", "repeat zero")

  // replace
  check(
    string.replace("www.example.com", each: ".", with: "-") == "www-example-com",
    "replace"
  )
  check(
    string.replace("a,b,c,d,e", each: ",", with: "/") == "a/b/c/d/e",
    "replace comma"
  )
  check(string.replace("abc", each: "z", with: "-") == "abc", "replace miss")

  // reverse (code point order)
  check(string.reverse("stressed") == "desserts", "reverse")
  check(string.reverse("") == "", "reverse empty")

  // slice
  check(string.slice(from: "gleam", at_index: 1, length: 2) == "le", "slice")
  check(
    string.slice(from: "gleam", at_index: 1, length: 10) == "leam",
    "slice long"
  )
  check(string.slice(from: "gleam", at_index: 10, length: 3) == "", "slice oob")
  check(
    string.slice(from: "gleam", at_index: -2, length: 2) == "am",
    "slice negative"
  )
  check(
    string.slice(from: "gleam", at_index: -12, length: 2) == "",
    "slice negative oob"
  )

  // split
  check(
    string.split("home/gleam/desktop/", on: "/") == ["home", "gleam", "desktop", ""],
    "split"
  )
  check(string.split("abc", on: "z") == ["abc"], "split miss")

  // split_once
  check(
    case string.split_once("home/gleam/desktop/", on: "/") {
      Ok(#(a, b)) -> a == "home" && b == "gleam/desktop/"
      _ -> False
    },
    "split_once"
  )
  check(
    case string.split_once("home/gleam/desktop/", on: "?") {
      Error(Nil) -> True
      _ -> False
    },
    "split_once miss"
  )

  // starts_with
  check(!string.starts_with("theory", "ory"), "starts_with false")
  check(string.starts_with("theory", "the"), "starts_with true")

  // to_option
  check(
    case string.to_option("") {
      None -> True
      _ -> False
    },
    "to_option none"
  )
  check(
    case string.to_option("hats") {
      Some(s) -> s == "hats"
      _ -> False
    },
    "to_option some"
  )

  // trim / trim_start / trim_end
  check(string.trim("  hats  \n") == "hats", "trim")
  check(string.trim_end("  hats  \n") == "  hats", "trim_end")
  check(string.trim_start("  hats  \n") == "hats  \n", "trim_start")

  // pipeline usage
  check(("abc" |> string.uppercase) == "ABC", "pipeline")

  io.println("all string tests passed")
}
