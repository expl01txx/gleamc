#ifndef GLEAM_RUNTIME_H
#define GLEAM_RUNTIME_H

#include <stdbool.h>
#include <stddef.h>
#include <stdint.h>
#include <stdio.h>

#ifdef __cplusplus
extern "C" {
#endif

typedef enum {
  G_INT,
  G_FLOAT,
  G_BOOL,
  G_NIL,
  G_STRING,
  G_LIST,
  G_TUPLE,
  G_RECORD,
  G_CLOSURE,
  G_BYTES
} GTag;

typedef struct GClosure GClosure;

typedef struct {
  GTag tag;
  union {
    int64_t integer;
    double floating;
    int boolean;
    void *pointer;
  } as;
} GValue;

typedef GValue (*GFunction)(GClosure *, GValue *, size_t);

typedef struct {
  size_t refs;
} GHeader;

typedef struct {
  GHeader header;
  size_t length;
  char data[];
} GString;

typedef struct {
  GHeader header;
  GValue head;
  GValue tail;
} GList;

typedef struct {
  GHeader header;
  size_t length;
  GValue items[];
} GTuple;

typedef struct {
  GHeader header;
  const char *tag;
  size_t length;
  GValue fields[];
} GRecord;

struct GClosure {
  GHeader header;
  GFunction function;
  size_t arity;
  size_t capture_count;
  GValue captures[];
};

typedef struct {
  GHeader header;
  size_t length;
  unsigned char data[];
} GBytes;

GValue g_int(int64_t value);
GValue g_float(double value);
GValue g_bool(int value);
GValue g_nil(void);
GValue g_string(const char *value);
GValue g_string_n(const char *value, size_t length);
GValue g_list_empty(void);
GValue g_list_prepend(GValue head, GValue tail);
GValue g_tuple(size_t length, GValue *items);
GValue g_record(const char *tag, size_t length, GValue *fields);
GValue g_bytes(size_t length, const unsigned char *data);
GValue g_closure_new(GFunction function, size_t arity, size_t capture_count,
                     GValue *captures);
GValue g_function(GFunction function, size_t arity);

GValue g_retain(GValue value);
void g_release(GValue value);
GValue g_call(GValue function, GValue *args, size_t argc);
int g_arity(GValue function);

int g_equal(GValue left, GValue right);
GValue g_string_concat(GValue left, GValue right);
int g_string_equals(GValue value, const char *expected);
int g_string_starts_with(GValue value, const char *prefix);
GValue g_string_drop_prefix(GValue value, size_t bytes);
GValue g_list_spread(GValue elements, GValue rest);
GValue g_list_head(GValue list);
GValue g_list_tail(GValue list);
size_t g_list_length(GValue list);
GValue g_tuple_get(GValue tuple, size_t index);
GValue g_record_get(GValue record, size_t index);
int g_record_is(GValue record, const char *tag);

GValue g_int_divide(GValue left, GValue right);
GValue g_int_modulo(GValue left, GValue right);
void g_echo(GValue value, const char *file, int line);
_Noreturn void g_panic(const char *message, const char *file, int line);
void g_type_error(const char *expected, GValue actual, const char *file,
                  int line);

GValue gleam_string_append(GValue *args, size_t argc);
GValue gleam_string_byte_size(GValue *args, size_t argc);
GValue gleam_string_capitalise(GValue *args, size_t argc);
GValue gleam_string_compare(GValue *args, size_t argc);
GValue gleam_string_concat(GValue *args, size_t argc);
GValue gleam_string_contains(GValue *args, size_t argc);
GValue gleam_string_crop(GValue *args, size_t argc);
GValue gleam_string_ends_with(GValue *args, size_t argc);
GValue gleam_string_first(GValue *args, size_t argc);
GValue gleam_string_inspect(GValue *args, size_t argc);
GValue gleam_string_is_empty(GValue *args, size_t argc);
GValue gleam_string_join(GValue *args, size_t argc);
GValue gleam_string_last(GValue *args, size_t argc);
GValue gleam_string_length(GValue *args, size_t argc);
GValue gleam_string_lowercase(GValue *args, size_t argc);
GValue gleam_string_pad_end(GValue *args, size_t argc);
GValue gleam_string_pad_start(GValue *args, size_t argc);
GValue gleam_string_remove_prefix(GValue *args, size_t argc);
GValue gleam_string_remove_suffix(GValue *args, size_t argc);
GValue gleam_string_repeat(GValue *args, size_t argc);
GValue gleam_string_replace(GValue *args, size_t argc);
GValue gleam_string_reverse(GValue *args, size_t argc);
GValue gleam_string_slice(GValue *args, size_t argc);
GValue gleam_string_split(GValue *args, size_t argc);
GValue gleam_string_split_once(GValue *args, size_t argc);
GValue gleam_string_starts_with(GValue *args, size_t argc);
GValue gleam_string_to_option(GValue *args, size_t argc);
GValue gleam_string_trim(GValue *args, size_t argc);
GValue gleam_string_trim_end(GValue *args, size_t argc);
GValue gleam_string_trim_start(GValue *args, size_t argc);
GValue gleam_string_uppercase(GValue *args, size_t argc);

GValue gleam_int_absolute_value(GValue *args, size_t argc);
GValue gleam_int_clamp(GValue *args, size_t argc);
GValue gleam_int_compare(GValue *args, size_t argc);
GValue gleam_int_digits(GValue *args, size_t argc);
GValue gleam_int_is_even(GValue *args, size_t argc);
GValue gleam_int_is_odd(GValue *args, size_t argc);
GValue gleam_int_max(GValue *args, size_t argc);
GValue gleam_int_min(GValue *args, size_t argc);
GValue gleam_int_negate(GValue *args, size_t argc);
GValue gleam_int_parse(GValue *args, size_t argc);
GValue gleam_int_product(GValue *args, size_t argc);
GValue gleam_int_random(GValue *args, size_t argc);
GValue gleam_int_sum(GValue *args, size_t argc);
GValue gleam_int_to_base16(GValue *args, size_t argc);
GValue gleam_int_to_base2(GValue *args, size_t argc);
GValue gleam_int_to_base36(GValue *args, size_t argc);
GValue gleam_int_to_base8(GValue *args, size_t argc);
GValue gleam_int_to_base_string(GValue *args, size_t argc);
GValue gleam_int_to_float(GValue *args, size_t argc);
GValue gleam_int_to_string(GValue *args, size_t argc);
GValue gleam_int_undigits(GValue *args, size_t argc);

GValue gleam_io_print(GValue *args, size_t argc);
GValue gleam_io_println(GValue *args, size_t argc);
GValue gleam_io_debug(GValue *args, size_t argc);
GValue gleam_io_read_line(GValue *args, size_t argc);
GValue gleam_fs_read(GValue *args, size_t argc);
GValue gleam_fs_write(GValue *args, size_t argc);
GValue gleam_fs_append(GValue *args, size_t argc);
GValue gleam_fs_remove(GValue *args, size_t argc);
GValue gleam_fs_is_file(GValue *args, size_t argc);
GValue gleam_fs_is_directory(GValue *args, size_t argc);
GValue gleam_fs_create_directory(GValue *args, size_t argc);
GValue gleam_fs_list_directory(GValue *args, size_t argc);

#ifdef __cplusplus
}
#endif

#endif
