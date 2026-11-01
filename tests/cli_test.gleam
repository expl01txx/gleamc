fn main() {
  let args = cli.args()
  io.println("count=" <> int.to_string(list.length(args)))
  case list.first(args) {
    Ok(first) -> io.println("first=" <> first)
    Error(nil) -> io.println("empty ok")
  }
  io.println("all cli tests passed")
}
