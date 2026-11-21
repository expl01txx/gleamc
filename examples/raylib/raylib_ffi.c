#include "gleam_runtime.h"
#include <raylib.h>

GValue raylib_ffi_open_window(GValue *args, size_t argc) {
  (void)argc;
  const GString *title = args[2].as.pointer;
  InitWindow((int)args[0].as.integer, (int)args[1].as.integer, title->data);
  if (!IsWindowReady()) return g_bool(false);
  SetTargetFPS(60);
  return g_bool(true);
}

GValue raylib_ffi_should_close(GValue *args, size_t argc) {
  (void)args;
  (void)argc;
  return g_bool(WindowShouldClose());
}

GValue raylib_ffi_draw_frame(GValue *args, size_t argc) {
  (void)argc;
  BeginDrawing();
  ClearBackground(RAYWHITE);
  DrawRectangle((int)args[0].as.integer, (int)args[1].as.integer,
                (int)args[2].as.integer, (int)args[3].as.integer, BLUE);
  EndDrawing();
  return g_nil();
}

GValue raylib_ffi_close_window(GValue *args, size_t argc) {
  (void)args;
  (void)argc;
  CloseWindow();
  return g_nil();
}
