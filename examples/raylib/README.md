# Raylib rectangle

From the repository root, run:

```sh
./examples/raylib/run.sh
```

The demo needs Rust, a C compiler, `pkg-config`, Raylib development files,
and a desktop session. Check that Raylib is discoverable with
`pkg-config --modversion raylib`.

The window shows a centered blue rectangle. Press Esc or close the window to
exit. Gleam controls the loop and the rectangle dimensions. The C wrappers
open and close the window and draw each frame through Raylib.

The launcher uses `gleamc -c` and links Raylib with the flags from
`pkg-config`. It builds in a temporary directory and removes it on exit.
