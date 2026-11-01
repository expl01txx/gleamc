import gleam/io
import gleam/int
import gleam/list
import gleam/string

pub type Step {
  Continue(line: Int, acc: Int, mem: List(Int))
  End(acc: Int, mem: List(Int))
}

fn to_int(s: String) -> Int {
  let assert Ok(n) = int.parse(s)
  n
}

fn list_get(lst: List(a), idx: Int) -> Result(a, Nil) {
  case idx < 0 {
    True -> Error(Nil)
    False -> list.first(list.drop(lst, idx))
  }
}

fn replace_at(lst: List(a), idx: Int, val: a) -> List(a) {
  list.index_map(lst, fn(item, i) {
    case i == idx {
      True -> val
      False -> item
    }
  })
}

fn check_line_bounds(
  line_num: Int,
  max_line: Int,
  line: Int,
  acc: Int,
  mem: List(Int),
) -> Step {
  case line_num >= max_line || line_num < 0 {
    True -> {
      io.println("Line number out of range. Line: " <> int.to_string(line))
      End(acc, mem)
    }
    False -> Continue(line_num, acc, mem)
  }
}

pub fn parse_command(
  head: String,
  line: Int,
  acc: Int,
  mem: List(Int),
  max_line: Int,
) -> Step {
  case string.split(head, " ") {
    ["cmp", num, line_num] -> {
      let line_num = to_int(line_num)
      case acc == to_int(num) {
        True -> check_line_bounds(line_num, max_line, line, acc, mem)
        False -> Continue(line + 1, acc, mem)
      }
    }

    ["jge", num, line_num] -> {
      let line_num = to_int(line_num)
      case acc >= to_int(num) {
        True -> check_line_bounds(line_num, max_line, line, acc, mem)
        False -> Continue(line + 1, acc, mem)
      }
    }

    ["jle", num, line_num] -> {
      let line_num = to_int(line_num)
      case acc <= to_int(num) {
        True -> check_line_bounds(line_num, max_line, line, acc, mem)
        False -> Continue(line + 1, acc, mem)
      }
    }

    ["jmp", line_num] -> {
      let line_num = to_int(line_num)
      case line_num >= max_line {
        True -> {
          io.println("Line number out of range. Line: " <> int.to_string(line))
          End(acc, mem)
        }
        False -> Continue(line_num, acc, mem)
      }
    }

    ["grow", len] -> {
      let len = to_int(len)
      case len < 2 {
        True -> {
          io.println(
            "Grow size must be at least 2, got "
            <> int.to_string(len)
            <> ". Line: "
            <> int.to_string(line),
          )
          End(acc, mem)
        }
        False -> Continue(line + 1, acc, list.append(mem, list.repeat(0, len)))
      }
    }

    ["get", idx] -> {
      let idx = to_int(idx)
      case list_get(mem, idx) {
        Ok(value) -> Continue(line + 1, value, mem)
        Error(_) -> {
          io.println("Memory index out of range. Line: " <> int.to_string(line))
          End(acc, mem)
        }
      }
    }

    ["set", idx] -> {
      let idx = to_int(idx)
      case list_get(mem, idx) {
        Ok(_) -> Continue(line + 1, acc, replace_at(mem, idx, acc))
        Error(_) -> {
          io.println("Memory index out of range. Line: " <> int.to_string(line))
          End(acc, mem)
        }
      }
    }

    ["loadx", addr] -> {
      let addr = to_int(addr)
      case list_get(mem, addr) {
        Error(_) -> {
          io.println(
            "Loadx address cell out of range: "
            <> int.to_string(addr)
            <> ". Line: "
            <> int.to_string(line),
          )
          End(acc, mem)
        }
        Ok(target) -> {
          case list_get(mem, target) {
            Ok(value) -> Continue(line + 1, value, mem)
            Error(_) -> {
              io.println(
                "Loadx target index out of range: "
                <> int.to_string(target)
                <> ". Line: "
                <> int.to_string(line),
              )
              End(acc, mem)
            }
          }
        }
      }
    }

    ["storex", addr] -> {
      let addr = to_int(addr)
      case list_get(mem, addr) {
        Error(_) -> {
          io.println(
            "Storex address cell out of range: "
            <> int.to_string(addr)
            <> ". Line: "
            <> int.to_string(line),
          )
          End(acc, mem)
        }
        Ok(target) -> {
          case list_get(mem, target) {
            Ok(_) -> Continue(line + 1, acc, replace_at(mem, target, acc))
            Error(_) -> {
              io.println(
                "Storex target index out of range: "
                <> int.to_string(target)
                <> ". Line: "
                <> int.to_string(line),
              )
              End(acc, mem)
            }
          }
        }
      }
    }

    ["seta", num] -> Continue(line + 1, to_int(num), mem)
    ["add", num] -> Continue(line + 1, acc + to_int(num), mem)
    ["sub", num] -> Continue(line + 1, acc - to_int(num), mem)
    ["inc"] -> Continue(line + 1, acc + 1, mem)
    ["dec"] -> Continue(line + 1, acc - 1, mem)
    ["mul", num] -> Continue(line + 1, acc * to_int(num), mem)
    ["div", num] -> Continue(line + 1, acc / to_int(num), mem)

    ["mod", num] -> {
      let num = to_int(num)
      case num == 0 {
        True -> {
          io.println("Mod division by zero. Line: " <> int.to_string(line))
          End(acc, mem)
        }
        False -> Continue(line + 1, acc % num, mem)
      }
    }

    ["neg"] -> Continue(line + 1, -acc, mem)
    ["abs"] -> Continue(line + 1, int.absolute_value(acc), mem)

    ["read"] -> {
      case io.read_line() {
        Ok(s) -> Continue(line + 1, to_int(string.trim(s)), mem)
        Error(_) -> {
          io.println("Read failed. Line: " <> int.to_string(line))
          End(acc, mem)
        }
      }
    }

    ["puti"] -> {
      io.println(int.to_string(acc))
      Continue(line + 1, acc, mem)
    }

    ["puts", ..strings] -> {
      io.println(string.join(strings, " "))
      Continue(line + 1, acc, mem)
    }

    ["end"] -> End(acc, mem)

    _ -> {
      io.println("Unknown command: " <> head <> ", line: " <> int.to_string(line))
      Continue(line + 1, acc, mem)
    }
  }
}

pub fn run(program: List(String), line: Int, acc: Int, mem: List(Int)) -> Int {
  case list_get(program, line) {
    Error(_) -> acc
    Ok(head) -> {
      case parse_command(head, line, acc, mem, list.length(program)) {
        End(acc, _mem) -> acc
        Continue(next_line, next_acc, next_mem) ->
          run(program, next_line, next_acc, next_mem)
      }
    }
  }
}

pub fn main() -> Nil {
  let args = cli.args()
  case args {
    [path] -> {
      let code = case fs.read(path) {
        Ok(v) -> v
        Error(_) -> panic
      }

      let program = code
         |> string.trim()
         |> string.split("\n")
         |> list.map(fn (v) {string.trim(v)})

       run(program, 0, 0, list.repeat(0, 64))
       Nil

    }
    _ -> io.println("Usage: program <script-file>")
  }
  Nil
}
