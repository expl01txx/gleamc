fn main() {
  // First readln gets the line fed on stdin.
  case io.readln() {
    Ok(line) -> io.println("got: " <> line)
    Error(nil) -> io.println("readln FAILED")
  }
  // Second readln hits end of input.
  case io.readln() {
    Ok(_) -> io.println("second readln FAILED")
    Error(nil) -> io.println("eof")
  }
  io.println("all io tests passed")
}
