#include "gleam_runtime.h"

static int64_t clamp_byte(int64_t value) {
  if (value < 0) return 0;
  if (value > 255) return 255;
  return value;
}

GValue example_clamp_byte(GValue *args, size_t argc) {
  (void)argc;
  return g_int(clamp_byte(args[0].as.integer));
}

GValue example_keep_string(GValue *args, size_t argc) {
  (void)argc;
  return g_retain(args[0]);
}
