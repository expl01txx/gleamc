@external(c, "./raylib_ffi.c", "raylib_ffi_open_window")
fn open_window(width: Int, height: Int, title: String) -> Bool

@external(c, "./raylib_ffi.c", "raylib_ffi_should_close")
fn should_close() -> Bool

@external(c, "./raylib_ffi.c", "raylib_ffi_draw_frame")
fn draw_frame(x: Int, y: Int, width: Int, height: Int) -> Nil

@external(c, "./raylib_ffi.c", "raylib_ffi_close_window")
fn close_window() -> Nil

fn draw_loop() -> Nil {
  case should_close() {
    True -> Nil
    False -> {
      draw_frame(210, 140, 220, 120)
      draw_loop()
    }
  }
}

pub fn main() {
  assert open_window(640, 400, "Gleam + Raylib") as "Raylib could not open a window"
  draw_loop()
  close_window()
}
