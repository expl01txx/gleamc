// Expose POSIX.1-2008 declarations (getline, ssize_t) even when compiled
// in strict ANSI mode (-std=c11 defines __STRICT_ANSI__).
#define _POSIX_C_SOURCE 200809L

#include "gleam_runtime.h"

#include <dirent.h>
#include <errno.h>
#include <stdarg.h>
#include <stdlib.h>
#include <string.h>
#include <sys/stat.h>
#include <time.h>

static void *allocate(size_t size) {
  void *memory = calloc(1, size);
  if (!memory) {
    fputs("gleam: out of memory\n", stderr);
    abort();
  }
  return memory;
}

GValue g_int(int64_t value) {
  GValue result = {.tag = G_INT};
  result.as.integer = value;
  return result;
}

GValue g_float(double value) {
  GValue result = {.tag = G_FLOAT};
  result.as.floating = value;
  return result;
}

GValue g_bool(int value) {
  GValue result = {.tag = G_BOOL};
  result.as.boolean = !!value;
  return result;
}

GValue g_nil(void) {
  GValue result = {.tag = G_NIL};
  result.as.pointer = NULL;
  return result;
}

GValue g_string_n(const char *value, size_t length) {
  GString *string = allocate(sizeof(GString) + length + 1);
  string->header.refs = 1;
  string->length = length;
  memcpy(string->data, value, length);
  string->data[length] = '\0';
  GValue result = {.tag = G_STRING};
  result.as.pointer = string;
  return result;
}

GValue g_string(const char *value) { return g_string_n(value, strlen(value)); }

GValue g_list_empty(void) { return g_nil(); }

GValue g_list_prepend(GValue head, GValue tail) {
  if (tail.tag != G_NIL && tail.tag != G_LIST) {
    g_type_error("List", tail, __FILE__, __LINE__);
  }
  GList *list = allocate(sizeof(GList));
  list->header.refs = 1;
  list->head = head;
  list->tail = tail;
  GValue result = {.tag = G_LIST};
  result.as.pointer = list;
  return result;
}

GValue g_tuple(size_t length, GValue *items) {
  GTuple *tuple = allocate(sizeof(GTuple) + length * sizeof(GValue));
  tuple->header.refs = 1;
  tuple->length = length;
  for (size_t i = 0; i < length; ++i) tuple->items[i] = items[i];
  GValue result = {.tag = G_TUPLE};
  result.as.pointer = tuple;
  return result;
}

GValue g_record(const char *tag, size_t length, GValue *fields) {
  GRecord *record = allocate(sizeof(GRecord) + length * sizeof(GValue));
  record->header.refs = 1;
  record->tag = tag;
  record->length = length;
  for (size_t i = 0; i < length; ++i) record->fields[i] = fields[i];
  GValue result = {.tag = G_RECORD};
  result.as.pointer = record;
  return result;
}

GValue g_bytes(size_t length, const unsigned char *data) {
  GBytes *bytes = allocate(sizeof(GBytes) + length);
  bytes->header.refs = 1;
  bytes->length = length;
  memcpy(bytes->data, data, length);
  GValue result = {.tag = G_BYTES};
  result.as.pointer = bytes;
  return result;
}

GValue g_closure_new(GFunction function, size_t arity, size_t capture_count,
                     GValue *captures) {
  GClosure *closure =
      allocate(sizeof(GClosure) + capture_count * sizeof(GValue));
  closure->header.refs = 1;
  closure->function = function;
  closure->arity = arity;
  closure->capture_count = capture_count;
  for (size_t i = 0; i < capture_count; ++i) closure->captures[i] = captures[i];
  GValue result = {.tag = G_CLOSURE};
  result.as.pointer = closure;
  return result;
}

GValue g_function(GFunction function, size_t arity) {
  return g_closure_new(function, arity, 0, NULL);
}

static GHeader *header(GValue value) {
  return (GHeader *)value.as.pointer;
}

GValue g_retain(GValue value) {
  switch (value.tag) {
  case G_STRING:
  case G_LIST:
  case G_TUPLE:
  case G_RECORD:
  case G_CLOSURE:
  case G_BYTES:
    if (value.as.pointer) ++header(value)->refs;
    break;
  default:
    break;
  }
  return value;
}

void g_release(GValue value) {
  switch (value.tag) {
  case G_STRING:
  case G_LIST:
  case G_TUPLE:
  case G_RECORD:
  case G_CLOSURE:
  case G_BYTES:
    break;
  default:
    return;
  }
  if (!value.as.pointer || --header(value)->refs != 0) return;

  switch (value.tag) {
  case G_LIST: {
    GList *list = value.as.pointer;
    g_release(list->head);
    g_release(list->tail);
    break;
  }
  case G_TUPLE: {
    GTuple *tuple = value.as.pointer;
    for (size_t i = 0; i < tuple->length; ++i) g_release(tuple->items[i]);
    break;
  }
  case G_RECORD: {
    GRecord *record = value.as.pointer;
    for (size_t i = 0; i < record->length; ++i) g_release(record->fields[i]);
    break;
  }
  case G_CLOSURE: {
    GClosure *closure = value.as.pointer;
    for (size_t i = 0; i < closure->capture_count; ++i)
      g_release(closure->captures[i]);
    break;
  }
  default:
    break;
  }
  free(value.as.pointer);
}

int g_arity(GValue function) {
  if (function.tag != G_CLOSURE) g_type_error("Function", function, __FILE__, __LINE__);
  return (int)((GClosure *)function.as.pointer)->arity;
}

GValue g_call(GValue function, GValue *args, size_t argc) {
  if (function.tag != G_CLOSURE) g_type_error("Function", function, __FILE__, __LINE__);
  GClosure *closure = function.as.pointer;
  if (closure->arity != argc) {
    fprintf(stderr, "gleam: function expected %zu arguments, got %zu\n",
            closure->arity, argc);
    exit(1);
  }
  return closure->function(closure, args, argc);
}

static int string_equal(const GString *left, const GString *right) {
  return left->length == right->length &&
         memcmp(left->data, right->data, left->length) == 0;
}

int g_equal(GValue left, GValue right) {
  if (left.tag != right.tag) return 0;
  switch (left.tag) {
  case G_INT:
    return left.as.integer == right.as.integer;
  case G_FLOAT:
    return left.as.floating == right.as.floating;
  case G_BOOL:
    return left.as.boolean == right.as.boolean;
  case G_NIL:
    return 1;
  case G_STRING:
    return string_equal(left.as.pointer, right.as.pointer);
  case G_LIST: {
    while (left.tag == G_LIST && right.tag == G_LIST) {
      GList *a = left.as.pointer;
      GList *b = right.as.pointer;
      if (!g_equal(a->head, b->head)) return 0;
      left = a->tail;
      right = b->tail;
    }
    return left.tag == G_NIL && right.tag == G_NIL;
  }
  case G_TUPLE: {
    GTuple *a = left.as.pointer;
    GTuple *b = right.as.pointer;
    if (a->length != b->length) return 0;
    for (size_t i = 0; i < a->length; ++i)
      if (!g_equal(a->items[i], b->items[i])) return 0;
    return 1;
  }
  case G_RECORD: {
    GRecord *a = left.as.pointer;
    GRecord *b = right.as.pointer;
    if (strcmp(a->tag, b->tag) != 0 || a->length != b->length) return 0;
    for (size_t i = 0; i < a->length; ++i)
      if (!g_equal(a->fields[i], b->fields[i])) return 0;
    return 1;
  }
  case G_BYTES: {
    GBytes *a = left.as.pointer;
    GBytes *b = right.as.pointer;
    return a->length == b->length &&
           memcmp(a->data, b->data, a->length) == 0;
  }
  case G_CLOSURE:
    return left.as.pointer == right.as.pointer;
  }
  return 0;
}

void g_type_error(const char *expected, GValue actual, const char *file,
                  int line) {
  static const char *names[] = {"Int", "Float", "Bool", "Nil", "String",
                                "List", "Tuple", "Record", "Function", "BitArray"};
  fprintf(stderr, "%s:%d: expected %s, got %s\n", file, line, expected,
          names[actual.tag]);
  exit(1);
}

GValue g_string_concat(GValue left, GValue right) {
  if (left.tag != G_STRING) g_type_error("String", left, __FILE__, __LINE__);
  if (right.tag != G_STRING) g_type_error("String", right, __FILE__, __LINE__);
  GString *a = left.as.pointer;
  GString *b = right.as.pointer;
  size_t total = a->length + b->length;
  char *buffer = malloc(total + 1);
  memcpy(buffer, a->data, a->length);
  memcpy(buffer + a->length, b->data, b->length);
  buffer[total] = '\0';
  GValue result = g_string_n(buffer, total);
  free(buffer);
  return result;
}

int g_string_equals(GValue value, const char *expected) {
  if (value.tag != G_STRING) return 0;
  GString *string = value.as.pointer;
  size_t length = strlen(expected);
  return string->length == length &&
         memcmp(string->data, expected, length) == 0;
}

int g_string_starts_with(GValue value, const char *prefix) {
  if (value.tag != G_STRING) return 0;
  GString *string = value.as.pointer;
  size_t length = strlen(prefix);
  return string->length >= length &&
         memcmp(string->data, prefix, length) == 0;
}

GValue g_string_drop_prefix(GValue value, size_t bytes) {
  GString *string = value.as.pointer;
  return g_string_n(string->data + bytes, string->length - bytes);
}

GValue g_list_spread(GValue elements, GValue rest) {
  if (rest.tag != G_NIL && rest.tag != G_LIST)
    g_type_error("List", rest, __FILE__, __LINE__);
  size_t count = g_list_length(elements);
  GValue *items = allocate(count * sizeof(GValue));
  GValue cursor = elements;
  for (size_t i = 0; i < count; ++i) {
    GList *node = cursor.as.pointer;
    items[count - i - 1] = g_retain(node->head);
    cursor = node->tail;
  }
  GValue result = g_retain(rest);
  for (size_t i = 0; i < count; ++i) result = g_list_prepend(items[i], result);
  free(items);
  return result;
}

GValue g_list_head(GValue list) {
  if (list.tag != G_LIST) g_type_error("non-empty List", list, __FILE__, __LINE__);
  return ((GList *)list.as.pointer)->head;
}

GValue g_list_tail(GValue list) {
  if (list.tag != G_LIST) g_type_error("non-empty List", list, __FILE__, __LINE__);
  return ((GList *)list.as.pointer)->tail;
}

size_t g_list_length(GValue list) {
  size_t length = 0;
  while (list.tag == G_LIST) {
    ++length;
    list = ((GList *)list.as.pointer)->tail;
  }
  if (list.tag != G_NIL) g_type_error("List", list, __FILE__, __LINE__);
  return length;
}

GValue g_tuple_get(GValue tuple, size_t index) {
  if (tuple.tag != G_TUPLE) g_type_error("Tuple", tuple, __FILE__, __LINE__);
  GTuple *value = tuple.as.pointer;
  if (index >= value->length) g_panic("tuple index out of bounds", __FILE__, __LINE__);
  return value->items[index];
}

GValue g_record_get(GValue record, size_t index) {
  if (record.tag != G_RECORD) g_type_error("Record", record, __FILE__, __LINE__);
  GRecord *value = record.as.pointer;
  if (index >= value->length) g_panic("record field out of bounds", __FILE__, __LINE__);
  return value->fields[index];
}

int g_record_is(GValue record, const char *tag) {
  return record.tag == G_RECORD && strcmp(((GRecord *)record.as.pointer)->tag, tag) == 0;
}

GValue g_int_divide(GValue left, GValue right) {
  if (left.tag != G_INT) g_type_error("Int", left, __FILE__, __LINE__);
  if (right.tag != G_INT) g_type_error("Int", right, __FILE__, __LINE__);
  if (right.as.integer == 0) g_panic("division by zero", __FILE__, __LINE__);
  return g_int(left.as.integer / right.as.integer);
}

GValue g_int_modulo(GValue left, GValue right) {
  if (left.tag != G_INT) g_type_error("Int", left, __FILE__, __LINE__);
  if (right.tag != G_INT) g_type_error("Int", right, __FILE__, __LINE__);
  if (right.as.integer == 0) g_panic("modulo by zero", __FILE__, __LINE__);
  return g_int(left.as.integer % right.as.integer);
}

static void inspect(FILE *stream, GValue value) {
  switch (value.tag) {
  case G_INT:
    fprintf(stream, "%lld", (long long)value.as.integer);
    break;
  case G_FLOAT:
    fprintf(stream, "%g", value.as.floating);
    break;
  case G_BOOL:
    fputs(value.as.boolean ? "True" : "False", stream);
    break;
  case G_NIL:
    fputs("Nil", stream);
    break;
  case G_STRING: {
    GString *string = value.as.pointer;
    fputc('"', stream);
    for (size_t i = 0; i < string->length; ++i) {
      unsigned char c = (unsigned char)string->data[i];
      if (c == '\n') fputs("\\n", stream);
      else if (c == '\r') fputs("\\r", stream);
      else if (c == '\t') fputs("\\t", stream);
      else if (c == '"' || c == '\\') fprintf(stream, "\\%c", c);
      else if (c < 32 || c == 127) fprintf(stream, "\\x%02x", c);
      else fputc(c, stream);
    }
    fputc('"', stream);
    break;
  }
  case G_LIST: {
    fputc('[', stream);
    GValue cursor = value;
    int first = 1;
    while (cursor.tag == G_LIST) {
      GList *list = cursor.as.pointer;
      if (!first) fputs(", ", stream);
      inspect(stream, list->head);
      first = 0;
      cursor = list->tail;
    }
    fputc(']', stream);
    break;
  }
  case G_TUPLE: {
    GTuple *tuple = value.as.pointer;
    fputs("#(", stream);
    for (size_t i = 0; i < tuple->length; ++i) {
      if (i) fputs(", ", stream);
      inspect(stream, tuple->items[i]);
    }
    fputc(')', stream);
    break;
  }
  case G_RECORD: {
    GRecord *record = value.as.pointer;
    fputs(record->tag, stream);
    fputc('(', stream);
    for (size_t i = 0; i < record->length; ++i) {
      if (i) fputs(", ", stream);
      inspect(stream, record->fields[i]);
    }
    fputc(')', stream);
    break;
  }
  case G_CLOSURE:
    fputs("<function>", stream);
    break;
  case G_BYTES: {
    GBytes *bytes = value.as.pointer;
    fputs("<<", stream);
    for (size_t i = 0; i < bytes->length; ++i) {
      if (i) fputs(", ", stream);
      fprintf(stream, "%u", bytes->data[i]);
    }
    fputs(">>", stream);
    break;
  }
  }
}

void g_echo(GValue value, const char *file, int line) {
  fprintf(stderr, "%s:%d\n", file, line);
  inspect(stderr, value);
  fputc('\n', stderr);
}

_Noreturn void g_panic(const char *message, const char *file, int line) {
  fprintf(stderr, "%s:%d: %s\n", file, line, message);
  exit(1);
}

static void require_args(const char *name, size_t expected, size_t actual) {
  if (expected != actual) {
    fprintf(stderr, "gleam: %s expected %zu arguments, got %zu\n", name,
            expected, actual);
    exit(1);
  }
}

static GString *require_string(GValue value) {
  if (value.tag != G_STRING) g_type_error("String", value, __FILE__, __LINE__);
  return value.as.pointer;
}

GValue gleam_io_print(GValue *args, size_t argc) {
  require_args("io.print", 1, argc);
  GString *string = require_string(args[0]);
  fwrite(string->data, 1, string->length, stdout);
  fflush(stdout);
  return g_nil();
}

GValue gleam_io_println(GValue *args, size_t argc) {
  require_args("io.println", 1, argc);
  GString *string = require_string(args[0]);
  fwrite(string->data, 1, string->length, stdout);
  fputc('\n', stdout);
  return g_nil();
}

GValue gleam_io_debug(GValue *args, size_t argc) {
  require_args("io.debug", 1, argc);
  inspect(stdout, args[0]);
  fputc('\n', stdout);
  return g_retain(args[0]);
}

GValue gleam_io_read_line(GValue *args, size_t argc) {
  (void)args;
  require_args("io.read_line", 0, argc);
  char *line = NULL;
  size_t capacity = 0;
  ssize_t length = getline(&line, &capacity, stdin);
  if (length < 0) {
    free(line);
    // Matches the declared Result(String, Nil): EOF carries no payload.
    return g_record("Error", 1, (GValue[]){g_nil()});
  }
  if (length > 0 && line[length - 1] == '\n') --length;
  if (length > 0 && line[length - 1] == '\r') --length;
  GValue result = g_record("Ok", 1, (GValue[]){g_string_n(line, length)});
  free(line);
  return result;
}

// Command line arguments, built once by the generated main() before any
// Gleam code runs. The list owns one reference per element.
static GValue cli_args;

void gleam_init_cli_args(int argc, char **argv) {
  // argv[0] is the program name; everything after it is user input.
  GValue list = g_list_empty();
  for (int i = argc - 1; i >= 1; --i) {
    list = g_list_prepend(g_string(argv[i]), list);
  }
  cli_args = list;
}

GValue gleam_cli_args(GValue *args, size_t argc) {
  (void)args;
  require_args("cli.args", 0, argc);
  return g_retain(cli_args);
}

static GValue fs_error(const char *operation, const char *path) {
  const char *reason = strerror(errno);
  size_t size = strlen(operation) + strlen(path) + strlen(reason) + 3;
  char *message = allocate(size);
  snprintf(message, size, "%s %s: %s", operation, path, reason);
  GValue result = g_record("Error", 1, (GValue[]){g_string(message)});
  free(message);
  return result;
}

GValue gleam_fs_read(GValue *args, size_t argc) {
  require_args("fs.read", 1, argc);
  GString *path = require_string(args[0]);
  FILE *file = fopen(path->data, "rb");
  if (!file) return fs_error("read", path->data);
  if (fseek(file, 0, SEEK_END) != 0) {
    fclose(file);
    return fs_error("read", path->data);
  }
  long size = ftell(file);
  if (size < 0 || fseek(file, 0, SEEK_SET) != 0) {
    fclose(file);
    return fs_error("read", path->data);
  }
  char *data = allocate((size_t)size + 1);
  size_t read = fread(data, 1, (size_t)size, file);
  int failed = ferror(file);
  fclose(file);
  if (failed) {
    free(data);
    return fs_error("read", path->data);
  }
  GValue result = g_record("Ok", 1, (GValue[]){g_string_n(data, read)});
  free(data);
  return result;
}

static GValue write_file(GValue *args, size_t argc, const char *mode,
                         const char *name) {
  require_args(name, 2, argc);
  GString *path = require_string(args[0]);
  GString *contents = require_string(args[1]);
  FILE *file = fopen(path->data, mode);
  if (!file) return fs_error("write", path->data);
  size_t written = fwrite(contents->data, 1, contents->length, file);
  int failed = written != contents->length || fclose(file) != 0;
  if (failed) return fs_error("write", path->data);
  return g_record("Ok", 1, (GValue[]){g_nil()});
}

GValue gleam_fs_write(GValue *args, size_t argc) {
  return write_file(args, argc, "wb", "fs.write");
}

GValue gleam_fs_append(GValue *args, size_t argc) {
  return write_file(args, argc, "ab", "fs.append");
}

GValue gleam_fs_remove(GValue *args, size_t argc) {
  require_args("fs.remove", 1, argc);
  GString *path = require_string(args[0]);
  if (remove(path->data) != 0) return fs_error("remove", path->data);
  return g_record("Ok", 1, (GValue[]){g_nil()});
}

GValue gleam_fs_is_file(GValue *args, size_t argc) {
  require_args("fs.is_file", 1, argc);
  GString *path = require_string(args[0]);
  struct stat info;
  return g_bool(stat(path->data, &info) == 0 && S_ISREG(info.st_mode));
}

GValue gleam_fs_is_directory(GValue *args, size_t argc) {
  require_args("fs.is_directory", 1, argc);
  GString *path = require_string(args[0]);
  struct stat info;
  return g_bool(stat(path->data, &info) == 0 && S_ISDIR(info.st_mode));
}

GValue gleam_fs_create_directory(GValue *args, size_t argc) {
  require_args("fs.create_directory", 1, argc);
  GString *path = require_string(args[0]);
  if (mkdir(path->data, 0777) != 0 && errno != EEXIST)
    return fs_error("create directory", path->data);
  return g_record("Ok", 1, (GValue[]){g_nil()});
}

GValue gleam_fs_list_directory(GValue *args, size_t argc) {
  require_args("fs.list_directory", 1, argc);
  GString *path = require_string(args[0]);
  DIR *directory = opendir(path->data);
  if (!directory) return fs_error("list directory", path->data);
  GValue result = g_list_empty();
  errno = 0;
  for (struct dirent *entry = readdir(directory); entry;
       entry = readdir(directory)) {
    if (strcmp(entry->d_name, ".") == 0 || strcmp(entry->d_name, "..") == 0)
      continue;
    result = g_list_prepend(g_string(entry->d_name), result);
    errno = 0;
  }
  int failed = errno != 0;
  closedir(directory);
  if (failed) {
    g_release(result);
    return fs_error("list directory", path->data);
  }
  return g_record("Ok", 1, (GValue[]){result});
}

// ---------------------------------------------------------------------------
// gleam/string
//
// Strings are UTF-8 byte sequences. Operations that the Gleam standard
// library defines in terms of graphemes or Unicode code points are
// implemented at code point granularity with a small table of combining
// marks so common diacritic cases behave like the real stdlib.
// ---------------------------------------------------------------------------

static int64_t require_int(GValue value) {
  if (value.tag != G_INT) g_type_error("Int", value, __FILE__, __LINE__);
  return value.as.integer;
}

// Decode the UTF-8 code point starting at data[index]. Returns the number of
// bytes consumed; invalid input decodes as a single replacement code point.
static size_t utf8_decode(const unsigned char *data, size_t length, size_t index,
                          uint32_t *codepoint) {
  if (index >= length) return 0;
  unsigned char lead = data[index];
  if (lead < 0x80) {
    *codepoint = lead;
    return 1;
  }
  int extra;
  uint32_t value;
  if ((lead & 0xE0) == 0xC0) {
    extra = 1;
    value = lead & 0x1Fu;
  } else if ((lead & 0xF0) == 0xE0) {
    extra = 2;
    value = lead & 0x0Fu;
  } else if ((lead & 0xF8) == 0xF0) {
    extra = 3;
    value = lead & 0x07u;
  } else {
    *codepoint = 0xFFFD;
    return 1;
  }
  for (int i = 1; i <= extra; ++i) {
    size_t at = index + (size_t)i;
    if (at >= length || (data[at] & 0xC0) != 0x80) {
      *codepoint = 0xFFFD;
      return 1;
    }
    value = (value << 6) | (uint32_t)(data[at] & 0x3Fu);
  }
  *codepoint = value;
  return (size_t)(extra + 1);
}

// A practical subset of Unicode combining marks (Mn/Mc/Me) plus the zero
// width joiner, used to approximate grapheme clusters.
static int is_combining(uint32_t cp) {
  return (cp >= 0x0300 && cp <= 0x036F) ||   // combining diacritical marks
         (cp >= 0x0483 && cp <= 0x0489) ||   // Cyrillic
         (cp >= 0x0591 && cp <= 0x05BD) || cp == 0x05BF ||
         (cp >= 0x05C1 && cp <= 0x05C2) || (cp >= 0x05C4 && cp <= 0x05C5) ||
         (cp >= 0x0610 && cp <= 0x061A) || (cp >= 0x064B && cp <= 0x065F) ||
         cp == 0x0670 || (cp >= 0x06D6 && cp <= 0x06ED) ||
         (cp >= 0x06EE && cp <= 0x06EF) || (cp >= 0x06F0 && cp <= 0x06F9) ||
         cp == 0x200D ||                       // zero width joiner
         (cp >= 0x1AB0 && cp <= 0x1AFF) ||     // combining diacritical marks extended
         (cp >= 0x1DC0 && cp <= 0x1DFF) ||     // supplementary combining
         (cp >= 0x20D0 && cp <= 0x20FF) ||     // combining marks for symbols
         (cp >= 0xFE20 && cp <= 0xFE2F);       // combining half marks
}

// Approximate grapheme count: code points that are not combining marks.
static size_t grapheme_count(const unsigned char *data, size_t length) {
  size_t count = 0;
  size_t index = 0;
  uint32_t cp;
  while (index < length) {
    size_t step = utf8_decode(data, length, index, &cp);
    if (!is_combining(cp)) ++count;
    index += step;
  }
  return count;
}

static int is_whitespace(uint32_t cp) {
  return cp == ' ' || cp == '\t' || cp == '\n' || cp == '\v' || cp == '\f' ||
         cp == '\r';
}

static uint32_t ascii_case(uint32_t cp, int to_upper) {
  if (to_upper && cp >= 'a' && cp <= 'z') return cp - 'a' + 'A';
  if (!to_upper && cp >= 'A' && cp <= 'Z') return cp - 'A' + 'a';
  return cp;
}

GValue gleam_string_append(GValue *args, size_t argc) {
  require_args("string.append", 2, argc);
  return g_string_concat(args[0], args[1]);
}

GValue gleam_string_byte_size(GValue *args, size_t argc) {
  require_args("string.byte_size", 1, argc);
  return g_int((int64_t)require_string(args[0])->length);
}

static GValue case_map(const GString *string, int first_only, int to_upper) {
  GValue result = g_string_n(string->data, string->length);
  GString *out = result.as.pointer;
  size_t index = 0;
  uint32_t cp;
  while (index < out->length) {
    size_t step = utf8_decode((const unsigned char *)out->data, out->length,
                              index, &cp);
    if (cp < 0x80) out->data[index] = (char)ascii_case(cp, to_upper);
    if (first_only) break;
    index += step;
  }
  return result;
}

GValue gleam_string_uppercase(GValue *args, size_t argc) {
  require_args("string.uppercase", 1, argc);
  return case_map(require_string(args[0]), 0, 1);
}

GValue gleam_string_lowercase(GValue *args, size_t argc) {
  require_args("string.lowercase", 1, argc);
  return case_map(require_string(args[0]), 0, 0);
}

GValue gleam_string_capitalise(GValue *args, size_t argc) {
  require_args("string.capitalise", 1, argc);
  GString *string = require_string(args[0]);
  if (string->length == 0) return g_retain(args[0]);
  return case_map(string, 1, 1);
}

GValue gleam_string_compare(GValue *args, size_t argc) {
  require_args("string.compare", 2, argc);
  GString *a = require_string(args[0]);
  GString *b = require_string(args[1]);
  size_t shared = a->length < b->length ? a->length : b->length;
  int result = memcmp(a->data, b->data, shared);
  if (result == 0 && a->length != b->length)
    result = a->length < b->length ? -1 : 1;
  return g_record(result < 0 ? "Lt" : result > 0 ? "Gt" : "Eq", 0, NULL);
}

GValue gleam_string_concat(GValue *args, size_t argc) {
  require_args("string.concat", 1, argc);
  GValue cursor = args[0];
  size_t total = 0;
  while (cursor.tag == G_LIST) {
    GList *list = cursor.as.pointer;
    total += require_string(list->head)->length;
    cursor = list->tail;
  }
  if (cursor.tag != G_NIL) g_type_error("List", cursor, __FILE__, __LINE__);
  GString *out = allocate(sizeof(GString) + total + 1);
  out->header.refs = 1;
  out->length = total;
  size_t offset = 0;
  cursor = args[0];
  while (cursor.tag == G_LIST) {
    GList *list = cursor.as.pointer;
    GString *element = require_string(list->head);
    memcpy(out->data + offset, element->data, element->length);
    offset += element->length;
    cursor = list->tail;
  }
  out->data[total] = '\0';
  GValue result = {.tag = G_STRING};
  result.as.pointer = out;
  return result;
}

GValue gleam_string_contains(GValue *args, size_t argc) {
  require_args("string.contains", 2, argc);
  GString *haystack = require_string(args[0]);
  GString *needle = require_string(args[1]);
  return g_bool(strstr(haystack->data, needle->data) != NULL);
}

GValue gleam_string_starts_with(GValue *args, size_t argc) {
  require_args("string.starts_with", 2, argc);
  GString *string = require_string(args[0]);
  GString *prefix = require_string(args[1]);
  return g_bool(string->length >= prefix->length &&
                memcmp(string->data, prefix->data, prefix->length) == 0);
}

GValue gleam_string_ends_with(GValue *args, size_t argc) {
  require_args("string.ends_with", 2, argc);
  GString *string = require_string(args[0]);
  GString *suffix = require_string(args[1]);
  return g_bool(string->length >= suffix->length &&
                memcmp(string->data + string->length - suffix->length,
                       suffix->data, suffix->length) == 0);
}

GValue gleam_string_crop(GValue *args, size_t argc) {
  require_args("string.crop", 2, argc);
  GString *string = require_string(args[0]);
  GString *substring = require_string(args[1]);
  if (substring->length == 0) return g_retain(args[0]);
  const char *found = strstr(string->data, substring->data);
  if (!found) return g_retain(args[0]);
  size_t offset = (size_t)(found - string->data);
  return g_string_n(found, string->length - offset);
}

GValue gleam_string_first(GValue *args, size_t argc) {
  require_args("string.first", 1, argc);
  GString *string = require_string(args[0]);
  if (string->length == 0) return g_record("Error", 1, (GValue[]){g_nil()});
  uint32_t cp;
  size_t end = utf8_decode((const unsigned char *)string->data, string->length,
                           0, &cp);
  // Keep following combining marks attached to the first code point.
  while (end < string->length) {
    uint32_t next;
    size_t step = utf8_decode((const unsigned char *)string->data,
                              string->length, end, &next);
    if (!is_combining(next)) break;
    end += step;
  }
  return g_record("Ok", 1, (GValue[]){g_string_n(string->data, end)});
}

GValue gleam_string_last(GValue *args, size_t argc) {
  require_args("string.last", 1, argc);
  GString *string = require_string(args[0]);
  if (string->length == 0) return g_record("Error", 1, (GValue[]){g_nil()});
  // Find the lead byte of the last code point: walk back over continuation
  // bytes, which stops right after the lead byte.
  size_t end = string->length;
  size_t lead = end;
  while (lead > 0 && ((unsigned char)string->data[lead - 1] & 0xC0) == 0x80)
    --lead;
  uint32_t cp;
  utf8_decode((const unsigned char *)string->data, string->length, lead - 1,
              &cp);
  size_t start = lead - 1;
  // Include the preceding base character when the last code point is a
  // combining mark.
  if (is_combining(cp) && start > 0) {
    size_t previous = start;
    while (previous > 0 &&
           ((unsigned char)string->data[previous - 1] & 0xC0) == 0x80)
      --previous;
    start = previous - 1;
  }
  return g_record("Ok", 1,
                  (GValue[]){g_string_n(string->data + start, end - start)});
}

GValue gleam_string_inspect(GValue *args, size_t argc) {
  require_args("string.inspect", 1, argc);
  char *buffer = NULL;
  size_t size = 0;
  FILE *stream = open_memstream(&buffer, &size);
  if (!stream) return g_string("");
  inspect(stream, args[0]);
  fclose(stream);
  GValue result = g_string(buffer);
  free(buffer);
  return result;
}

GValue gleam_string_is_empty(GValue *args, size_t argc) {
  require_args("string.is_empty", 1, argc);
  return g_bool(require_string(args[0])->length == 0);
}

GValue gleam_string_join(GValue *args, size_t argc) {
  require_args("string.join", 2, argc);
  GString *separator = require_string(args[1]);
  GValue cursor = args[0];
  size_t count = 0;
  size_t total = 0;
  while (cursor.tag == G_LIST) {
    GList *list = cursor.as.pointer;
    ++count;
    total += require_string(list->head)->length;
    cursor = list->tail;
  }
  if (cursor.tag != G_NIL) g_type_error("List", cursor, __FILE__, __LINE__);
  if (count > 1) total += (size_t)(count - 1) * separator->length;
  GString *out = allocate(sizeof(GString) + total + 1);
  out->header.refs = 1;
  out->length = total;
  size_t offset = 0;
  int first = 1;
  cursor = args[0];
  while (cursor.tag == G_LIST) {
    GList *list = cursor.as.pointer;
    GString *element = require_string(list->head);
    if (!first) {
      memcpy(out->data + offset, separator->data, separator->length);
      offset += separator->length;
    }
    memcpy(out->data + offset, element->data, element->length);
    offset += element->length;
    first = 0;
    cursor = list->tail;
  }
  out->data[total] = '\0';
  GValue result = {.tag = G_STRING};
  result.as.pointer = out;
  return result;
}

GValue gleam_string_length(GValue *args, size_t argc) {
  require_args("string.length", 1, argc);
  GString *string = require_string(args[0]);
  return g_int((int64_t)grapheme_count((const unsigned char *)string->data,
                                       string->length));
}

static GValue pad(const GString *string, int64_t desired, const GString *pad,
                  int at_start) {
  size_t current = grapheme_count((const unsigned char *)string->data,
                                  string->length);
  if (desired <= (int64_t)current || pad->length == 0) return g_string_n(string->data, string->length);
  size_t needed = (size_t)desired - current;
  GString *out = allocate(sizeof(GString) + string->length + needed + 1);
  out->header.refs = 1;
  out->length = string->length + needed;
  if (at_start) {
    for (size_t i = 0; i < needed; ++i)
      out->data[i] = pad->data[i % pad->length];
    memcpy(out->data + needed, string->data, string->length);
  } else {
    memcpy(out->data, string->data, string->length);
    for (size_t i = 0; i < needed; ++i)
      out->data[string->length + i] = pad->data[i % pad->length];
  }
  out->data[out->length] = '\0';
  GValue result = {.tag = G_STRING};
  result.as.pointer = out;
  return result;
}

GValue gleam_string_pad_end(GValue *args, size_t argc) {
  require_args("string.pad_end", 3, argc);
  return pad(require_string(args[0]), require_int(args[1]),
             require_string(args[2]), 0);
}

GValue gleam_string_pad_start(GValue *args, size_t argc) {
  require_args("string.pad_start", 3, argc);
  return pad(require_string(args[0]), require_int(args[1]),
             require_string(args[2]), 1);
}

GValue gleam_string_remove_prefix(GValue *args, size_t argc) {
  require_args("string.remove_prefix", 2, argc);
  GString *string = require_string(args[0]);
  GString *prefix = require_string(args[1]);
  if (string->length < prefix->length ||
      memcmp(string->data, prefix->data, prefix->length) != 0)
    return g_retain(args[0]);
  return g_string_n(string->data + prefix->length,
                    string->length - prefix->length);
}

GValue gleam_string_remove_suffix(GValue *args, size_t argc) {
  require_args("string.remove_suffix", 2, argc);
  GString *string = require_string(args[0]);
  GString *suffix = require_string(args[1]);
  if (string->length < suffix->length ||
      memcmp(string->data + string->length - suffix->length, suffix->data,
             suffix->length) != 0)
    return g_retain(args[0]);
  return g_string_n(string->data, string->length - suffix->length);
}

GValue gleam_string_repeat(GValue *args, size_t argc) {
  require_args("string.repeat", 2, argc);
  GString *string = require_string(args[0]);
  int64_t times = require_int(args[1]);
  if (times <= 0 || string->length == 0) return g_string("");
  size_t total = string->length * (size_t)times;
  GString *out = allocate(sizeof(GString) + total + 1);
  out->header.refs = 1;
  out->length = total;
  for (int64_t i = 0; i < times; ++i)
    memcpy(out->data + (size_t)i * string->length, string->data,
           string->length);
  out->data[total] = '\0';
  GValue result = {.tag = G_STRING};
  result.as.pointer = out;
  return result;
}

GValue gleam_string_replace(GValue *args, size_t argc) {
  require_args("string.replace", 3, argc);
  GString *string = require_string(args[0]);
  GString *pattern = require_string(args[1]);
  GString *substitute = require_string(args[2]);
  if (pattern->length == 0) return g_retain(args[0]);
  const char *end = string->data + string->length;
  size_t occurrences = 0;
  for (const char *cursor = string->data; cursor < end;) {
    const char *found = strstr(cursor, pattern->data);
    if (!found) break;
    ++occurrences;
    cursor = found + pattern->length;
  }
  size_t total = string->length - occurrences * pattern->length +
                 occurrences * substitute->length;
  GString *out = allocate(sizeof(GString) + total + 1);
  out->header.refs = 1;
  out->length = total;
  size_t offset = 0;
  for (const char *cursor = string->data; cursor < end;) {
    const char *found = strstr(cursor, pattern->data);
    if (!found) {
      memcpy(out->data + offset, cursor, (size_t)(end - cursor));
      break;
    }
    size_t segment = (size_t)(found - cursor);
    memcpy(out->data + offset, cursor, segment);
    offset += segment;
    memcpy(out->data + offset, substitute->data, substitute->length);
    offset += substitute->length;
    cursor = found + pattern->length;
  }
  out->data[total] = '\0';
  GValue result = {.tag = G_STRING};
  result.as.pointer = out;
  return result;
}

GValue gleam_string_reverse(GValue *args, size_t argc) {
  require_args("string.reverse", 1, argc);
  GString *string = require_string(args[0]);
  // Reversing code points preserves the total byte length.
  GValue result = g_string_n(string->data, string->length);
  GString *out = result.as.pointer;
  size_t index = 0;
  size_t out_index = string->length;
  uint32_t cp;
  while (index < string->length) {
    size_t step = utf8_decode((const unsigned char *)string->data,
                              string->length, index, &cp);
    out_index -= step;
    memcpy(out->data + out_index, string->data + index, step);
    index += step;
  }
  return result;
}

GValue gleam_string_slice(GValue *args, size_t argc) {
  require_args("string.slice", 3, argc);
  GString *string = require_string(args[0]);
  int64_t index = require_int(args[1]);
  int64_t length = require_int(args[2]);
  size_t total = 0;
  {
    size_t position = 0;
    uint32_t cp;
    while (position < string->length) {
      size_t step = utf8_decode((const unsigned char *)string->data,
                                string->length, position, &cp);
      ++total;
      position += step;
    }
  }
  int64_t start = index < 0 ? (int64_t)total + index : index;
  // A negative index before the start of the string yields no slice.
  if (start < 0 || start >= (int64_t)total || length <= 0) return g_string("");
  int64_t stop = start + length;
  if (stop > (int64_t)total) stop = (int64_t)total;
  size_t position = 0;
  size_t seen = 0;
  size_t start_byte = 0;
  size_t stop_byte = string->length;
  uint32_t cp;
  while (position < string->length) {
    size_t step = utf8_decode((const unsigned char *)string->data,
                              string->length, position, &cp);
    if (seen == (size_t)start) start_byte = position;
    if (seen + 1 == (size_t)stop) {
      stop_byte = position + step;
      break;
    }
    ++seen;
    position += step;
  }
  return g_string_n(string->data + start_byte, stop_byte - start_byte);
}

GValue gleam_string_split(GValue *args, size_t argc) {
  require_args("string.split", 2, argc);
  GString *string = require_string(args[0]);
  GString *separator = require_string(args[1]);
  if (separator->length == 0) return g_retain(args[0]);
  const char *end = string->data + string->length;
  size_t count = 0;
  for (const char *cursor = string->data;;) {
    const char *found = strstr(cursor, separator->data);
    ++count;
    if (!found) break;
    cursor = found + separator->length;
  }
  // Record each segment's start, then build the list right-to-left so the
  // first segment ends up at the head.
  const char **starts = allocate(count * sizeof(const char *));
  size_t index = 0;
  for (const char *cursor = string->data;;) {
    starts[index++] = cursor;
    const char *found = strstr(cursor, separator->data);
    if (!found) break;
    cursor = found + separator->length;
  }
  GValue result = g_list_empty();
  for (size_t i = count; i-- > 0;) {
    // The segment ends where the next separator begins.
    const char *stop =
        i + 1 < count ? starts[i + 1] - separator->length : end;
    result = g_list_prepend(g_string_n(starts[i], (size_t)(stop - starts[i])),
                             result);
  }
  free(starts);
  return result;
}

GValue gleam_string_split_once(GValue *args, size_t argc) {
  require_args("string.split_once", 2, argc);
  GString *string = require_string(args[0]);
  GString *separator = require_string(args[1]);
  if (separator->length == 0)
    return g_record("Error", 1, (GValue[]){g_nil()});
  const char *found = strstr(string->data, separator->data);
  if (!found) return g_record("Error", 1, (GValue[]){g_nil()});
  size_t before = (size_t)(found - string->data);
  GValue pair = g_tuple(
      2, (GValue[]){g_string_n(string->data, before),
                    g_string_n(found + separator->length,
                               string->length - before - separator->length)});
  return g_record("Ok", 1, (GValue[]){pair});
}

GValue gleam_string_to_option(GValue *args, size_t argc) {
  require_args("string.to_option", 1, argc);
  GString *string = require_string(args[0]);
  if (string->length == 0) return g_record("None", 0, NULL);
  return g_record("Some", 1, (GValue[]){g_retain(args[0])});
}

static GValue trim_edges(const GString *string, int at_start, int at_end) {
  size_t start = 0;
  size_t end = string->length;
  uint32_t cp;
  if (at_start) {
    while (start < end) {
      size_t step = utf8_decode((const unsigned char *)string->data,
                                string->length, start, &cp);
      if (!is_whitespace(cp)) break;
      start += step;
    }
  }
  if (at_end) {
    while (end > start) {
      // Walk back over continuation bytes: stops right after the lead byte.
      size_t index = end;
      while (index > 0 &&
             ((unsigned char)string->data[index - 1] & 0xC0) == 0x80)
        --index;
      utf8_decode((const unsigned char *)string->data, string->length,
                  index - 1, &cp);
      if (!is_whitespace(cp)) break;
      end = index - 1;
    }
  }
  return g_string_n(string->data + start, end - start);
}

GValue gleam_string_trim(GValue *args, size_t argc) {
  require_args("string.trim", 1, argc);
  return trim_edges(require_string(args[0]), 1, 1);
}

GValue gleam_string_trim_start(GValue *args, size_t argc) {
  require_args("string.trim_start", 1, argc);
  return trim_edges(require_string(args[0]), 1, 0);
}

GValue gleam_string_trim_end(GValue *args, size_t argc) {
  require_args("string.trim_end", 1, argc);
  return trim_edges(require_string(args[0]), 0, 1);
}

// ---------------------------------------------------------------------
// gleam/int
// ---------------------------------------------------------------------

GValue gleam_int_absolute_value(GValue *args, size_t argc) {
  require_args("int.absolute_value", 1, argc);
  int64_t x = require_int(args[0]);
  return g_int(x < 0 ? -x : x);
}

GValue gleam_int_clamp(GValue *args, size_t argc) {
  require_args("int.clamp", 3, argc);
  int64_t n = require_int(args[0]);
  int64_t lo = require_int(args[1]);
  int64_t hi = require_int(args[2]);
  if (n < lo) return g_int(lo);
  if (n > hi) return g_int(hi);
  return g_int(n);
}

GValue gleam_int_compare(GValue *args, size_t argc) {
  require_args("int.compare", 2, argc);
  int64_t a = require_int(args[0]);
  int64_t b = require_int(args[1]);
  return g_record(a < b ? "Lt" : a > b ? "Gt" : "Eq", 0, NULL);
}

// The `Error(InvalidBase)` value shared by the base conversion functions.
static GValue invalid_base(void) {
  return g_record("Error", 1, (GValue[]){g_record("InvalidBase", 0, NULL)});
}

GValue gleam_int_digits(GValue *args, size_t argc) {
  require_args("int.digits", 2, argc);
  int64_t number = require_int(args[0]);
  int64_t base = require_int(args[1]);
  if (base < 2) return invalid_base();
  // Digits of the absolute value, least significant first, then reversed.
  uint64_t n = number < 0 ? (uint64_t)(-(number + 1)) + 1 : (uint64_t)number;
  int64_t digits[64];
  size_t count = 0;
  do {
    digits[count++] = (int64_t)(n % (uint64_t)base);
    n /= (uint64_t)base;
  } while (n != 0);
  // Prepend least-significant-first so the list reads most-significant
  // first.
  GValue result = g_list_empty();
  for (size_t i = 0; i < count; ++i)
    result = g_list_prepend(g_int(digits[i]), result);
  return g_record("Ok", 1, (GValue[]){result});
}

GValue gleam_int_is_even(GValue *args, size_t argc) {
  require_args("int.is_even", 1, argc);
  return g_bool(require_int(args[0]) % 2 == 0);
}

GValue gleam_int_is_odd(GValue *args, size_t argc) {
  require_args("int.is_odd", 1, argc);
  return g_bool(require_int(args[0]) % 2 != 0);
}

GValue gleam_int_max(GValue *args, size_t argc) {
  require_args("int.max", 2, argc);
  int64_t a = require_int(args[0]);
  int64_t b = require_int(args[1]);
  return g_int(a > b ? a : b);
}

GValue gleam_int_min(GValue *args, size_t argc) {
  require_args("int.min", 2, argc);
  int64_t a = require_int(args[0]);
  int64_t b = require_int(args[1]);
  return g_int(a < b ? a : b);
}

GValue gleam_int_negate(GValue *args, size_t argc) {
  require_args("int.negate", 1, argc);
  return g_int(-require_int(args[0]));
}

GValue gleam_int_parse(GValue *args, size_t argc) {
  require_args("int.parse", 1, argc);
  GString *string = require_string(args[0]);
  const char *data = string->data;
  size_t length = string->length;
  size_t index = 0;
  int negative = 0;
  if (length > 0 && (data[0] == '-' || data[0] == '+')) {
    negative = data[0] == '-';
    index = 1;
  }
  if (index >= length) return g_record("Error", 1, (GValue[]){g_nil()});
  // Accumulate in unsigned to detect overflow.
  uint64_t value = 0;
  for (; index < length; ++index) {
    char c = data[index];
    if (c < '0' || c > '9') return g_record("Error", 1, (GValue[]){g_nil()});
    uint64_t digit = (uint64_t)(c - '0');
    if (value > (UINT64_MAX - digit) / 10)
      return g_record("Error", 1, (GValue[]){g_nil()});
    value = value * 10 + digit;
  }
  // The largest representable magnitude is INT64_MAX + 1.
  if (negative ? value > (uint64_t)INT64_MAX + 1 : value > (uint64_t)INT64_MAX)
    return g_record("Error", 1, (GValue[]){g_nil()});
  // Two's-complement negation through unsigned arithmetic handles
  // INT64_MIN exactly.
  int64_t result = negative ? (int64_t)(0 - value) : (int64_t)value;
  return g_record("Ok", 1, (GValue[]){g_int(result)});
}

GValue gleam_int_product(GValue *args, size_t argc) {
  require_args("int.product", 1, argc);
  int64_t product = 1;
  GValue item = args[0];
  while (item.tag == G_LIST) {
    product *= ((GList*)item.as.pointer)->head.as.integer;
    item = ((GList*)item.as.pointer)->tail;
  }
  return g_int(product);
}

// Two 31-bit `rand` draws form a uniform 62-bit value.
static uint64_t random_draw(void) {
  static int seeded = 0;
  if (!seeded) {
    srand((unsigned int)time(NULL));
    seeded = 1;
  }
  return ((uint64_t)rand() << 31) | (uint64_t)(uint32_t)rand();
}

GValue gleam_int_random(GValue *args, size_t argc) {
  require_args("int.random", 2, argc);
  int64_t a = require_int(args[0]);
  int64_t b = require_int(args[1]);
  int64_t lo = a < b ? a : b;
  int64_t hi = a < b ? b : a;
  uint64_t span = (uint64_t)hi - (uint64_t)lo + 1;
  if (span == 0) {
    // The full 64-bit range: use the draw as the offset directly.
    return g_int(lo + (int64_t)random_draw());
  }
  // Rejection sampling to avoid modulo bias.
  uint64_t threshold = ~0ULL - (~0ULL % span);
  for (;;) {
    uint64_t value = random_draw();
    if (value < threshold) return g_int(lo + (int64_t)(value % span));
  }
}

GValue gleam_int_sum(GValue *args, size_t argc) {
  require_args("int.sum", 1, argc);
  int64_t sum = 0;
  GValue item = args[0];
  while (item.tag == G_LIST) {
    sum += ((GList*)item.as.pointer)->head.as.integer;
    item = ((GList*)item.as.pointer)->tail;
  }
  return g_int(sum);
}

// Digits for bases up to 36, as used by the `to_base*` functions.
static const char int_digits[] = "0123456789ABCDEFGHIJKLMNOPQRSTUVWXYZ";

static GValue int_to_base(int64_t number, uint64_t base) {
  uint64_t n = number < 0 ? (uint64_t)(-(number + 1)) + 1 : (uint64_t)number;
  char buffer[70]; // sign + up to 64 base-2 digits + NUL
  size_t length = 0;
  do {
    buffer[length++] = int_digits[n % base];
    n /= base;
  } while (n != 0);
  if (number < 0) buffer[length++] = '-';
  // Reverse in place.
  for (size_t i = 0, j = length; j-- > i;) {
    char tmp = buffer[i];
    buffer[i] = buffer[j];
    buffer[j] = tmp;
    ++i;
  }
  return g_string_n(buffer, length);
}

GValue gleam_int_to_base2(GValue *args, size_t argc) {
  require_args("int.to_base2", 1, argc);
  return int_to_base(require_int(args[0]), 2);
}

GValue gleam_int_to_base8(GValue *args, size_t argc) {
  require_args("int.to_base8", 1, argc);
  return int_to_base(require_int(args[0]), 8);
}

GValue gleam_int_to_base16(GValue *args, size_t argc) {
  require_args("int.to_base16", 1, argc);
  return int_to_base(require_int(args[0]), 16);
}

GValue gleam_int_to_base36(GValue *args, size_t argc) {
  require_args("int.to_base36", 1, argc);
  return int_to_base(require_int(args[0]), 36);
}

GValue gleam_int_to_base_string(GValue *args, size_t argc) {
  require_args("int.to_base_string", 2, argc);
  int64_t number = require_int(args[0]);
  int64_t base = require_int(args[1]);
  if (base < 2 || base > 36) return invalid_base();
  return g_record("Ok", 1, (GValue[]){int_to_base(number, (uint64_t)base)});
}

GValue gleam_int_to_float(GValue *args, size_t argc) {
  require_args("int.to_float", 1, argc);
  return g_float((double)require_int(args[0]));
}

GValue gleam_int_to_string(GValue *args, size_t argc) {
  require_args("int.to_string", 1, argc);
  char buffer[24];
  snprintf(buffer, sizeof buffer, "%lld", (long long)require_int(args[0]));
  return g_string(buffer);
}

GValue gleam_int_undigits(GValue *args, size_t argc) {
  require_args("int.undigits", 2, argc);
  GValue list = args[0];
  int64_t base = require_int(args[1]);
  if (base < 2) return invalid_base();
  int64_t value = 0;
  while (list.tag == G_LIST) {
    GValue head = ((GList*)list.as.pointer)->head;
    if (head.tag != G_INT)
      g_type_error("Int", head, __FILE__, __LINE__);
    int64_t digit = head.as.integer;
    // Digits must be in the range 0..base-1.
    if (digit < 0 || digit >= base) return invalid_base();
    value = value * base + digit;
    list = ((GList*)list.as.pointer)->tail;
  }
  return g_record("Ok", 1, (GValue[]){g_int(value)});
}

// ---------------------------------------------------------------------------
// gleam/list
// ---------------------------------------------------------------------------

static GValue require_list(GValue value) {
  if (value.tag != G_LIST && value.tag != G_NIL)
    g_type_error("List", value, __FILE__, __LINE__);
  return value;
}

// Copy a list into an array of owned elements: `items[i]` points at a
// retained copy of the i-th element. The caller transfers every element
// into a new structure (or leaks it) and then calls list_free.
static GValue **list_collect(GValue list, size_t *count) {
  size_t length = g_list_length(list);
  GValue **items = malloc((length ? length : 1) * sizeof(GValue *));
  size_t index = 0;
  while (list.tag == G_LIST) {
    GList *cell = list.as.pointer;
    GValue *slot = malloc(sizeof(GValue));
    *slot = g_retain(cell->head);
    items[index++] = slot;
    list = cell->tail;
  }
  *count = index;
  return items;
}

// Release every element still owned by the array and free the slots.
static void list_free(GValue **items, size_t count) {
  for (size_t i = 0; i < count; ++i) {
    g_release(*items[i]);
    free(items[i]);
  }
  free(items);
}

// Build a list from owned elements in forward order. Prepending must go
// from the last element to the first so the list reads in index order.
static GValue list_build(GValue **items, size_t count) {
  GValue result = g_list_empty();
  for (size_t i = count; i-- > 0;)
    result = g_list_prepend(g_retain(*items[i]), result);
  return result;
}

// Copy a list in reverse order.
static GValue list_reverse(GValue list) {
  GValue result = g_list_empty();
  while (list.tag == G_LIST) {
    GList *cell = list.as.pointer;
    result = g_list_prepend(g_retain(cell->head), result);
    list = cell->tail;
  }
  return result;
}

// Reverse a locally built list and release the original chain. Every
// element must already hold its own reference (as list_collect copies do).
static GValue list_reverse_own(GValue list) {
  GValue result = list_reverse(list);
  g_release(list);
  return result;
}

// Append two lists. Takes ownership of both: the first is copied cell by
// cell (so a shared prefix is never mutated) and then released, and the
// second becomes the tail of the result.
static GValue list_take_append(GValue first, GValue second) {
  size_t count;
  GValue **items = list_collect(first, &count);
  GValue result = second;
  for (size_t i = count; i-- > 0;)
    result = g_list_prepend(g_retain(*items[i]), result);
  list_free(items, count);
  g_release(first);
  return result;
}

GValue gleam_list_all(GValue *args, size_t argc) {
  require_args("list.all", 2, argc);
  GValue list = require_list(args[0]);
  GValue predicate = args[1];
  while (list.tag == G_LIST) {
    GList *cell = list.as.pointer;
    GValue ok = g_call(predicate, (GValue[]){cell->head}, 1);
    int pass = ok.as.boolean;
    g_release(ok);
    if (!pass) return g_bool(0);
    list = cell->tail;
  }
  return g_bool(1);
}

GValue gleam_list_any(GValue *args, size_t argc) {
  require_args("list.any", 2, argc);
  GValue list = require_list(args[0]);
  GValue predicate = args[1];
  while (list.tag == G_LIST) {
    GList *cell = list.as.pointer;
    GValue ok = g_call(predicate, (GValue[]){cell->head}, 1);
    int pass = ok.as.boolean;
    g_release(ok);
    if (pass) return g_bool(1);
    list = cell->tail;
  }
  return g_bool(0);
}

GValue gleam_list_append(GValue *args, size_t argc) {
  require_args("list.append", 2, argc);
  return list_take_append(g_retain(require_list(args[0])),
                          g_retain(require_list(args[1])));
}

// Group consecutive elements whose `by` key compares equal.
GValue gleam_list_chunk(GValue *args, size_t argc) {
  require_args("list.chunk", 2, argc);
  GValue list = require_list(args[0]);
  GValue by = args[1];
  if (list.tag == G_NIL) return g_list_empty();
  GList *cell = list.as.pointer;
  GValue key = g_call(by, (GValue[]){cell->head}, 1);
  GValue chunk = g_list_prepend(g_retain(cell->head), g_list_empty());
  GValue result = g_list_empty();
  list = cell->tail;
  while (list.tag == G_LIST) {
    GList *next = list.as.pointer;
    GValue next_key = g_call(by, (GValue[]){next->head}, 1);
    if (g_equal(key, next_key)) {
      chunk = g_list_prepend(g_retain(next->head), chunk);
      g_release(next_key);
    } else {
      result = g_list_prepend(chunk, result);
      chunk = g_list_prepend(g_retain(next->head), g_list_empty());
      g_release(key);
      key = next_key;
    }
    list = next->tail;
  }
  // Chunks were collected front-to-back; restore the order.
  return list_reverse_own(g_list_prepend(chunk, result));
}

GValue gleam_list_combination_pairs(GValue *args, size_t argc) {
  require_args("list.combination_pairs", 1, argc);
  size_t count;
  GValue **items = list_collect(require_list(args[0]), &count);
  GValue result = g_list_empty();
  for (size_t i = count; i-- > 0;) {
    for (size_t j = count - 1; j > i; --j) {
      // Each element appears in several pairs, so every tuple gets its
      // own retained references.
      GValue pair = g_tuple(2, (GValue[]){g_retain(*items[i]),
                                         g_retain(*items[j])});
      result = g_list_prepend(pair, result);
    }
  }
  list_free(items, count);
  return result;
}

GValue gleam_list_combinations(GValue *args, size_t argc) {
  require_args("list.combinations", 2, argc);
  GValue list = require_list(args[0]);
  int64_t n = require_int(args[1]);
  size_t count;
  GValue **items = list_collect(list, &count);
  if (n <= 0 || (uint64_t)n > count) {
    list_free(items, count);
    return g_list_empty();
  }
  size_t width = (size_t)n;
  // Index combination in lexicographic order: start at [0..width) and
  // advance the rightmost position that is not yet at its maximum.
  size_t *index = malloc(width * sizeof(size_t));
  for (size_t i = 0; i < width; ++i) index[i] = i;
  GValue result = g_list_empty();
  for (;;) {
    GValue combo = g_list_empty();
    for (size_t i = width; i-- > 0;)
      combo = g_list_prepend(g_retain(*items[index[i]]), combo);
    result = g_list_prepend(combo, result);
    size_t i = width;
    while (i > 0 && index[i - 1] == count - (width - (i - 1))) --i;
    if (i == 0) break;
    ++index[i - 1];
    for (size_t j = i; j < width; ++j) index[j] = index[j - 1] + 1;
  }
  free(index);
  list_free(items, count);
  // Each combination was prepended, so reverse to keep the lexicographic
  // order of the index choices.
  return list_reverse_own(result);
}

GValue gleam_list_contains(GValue *args, size_t argc) {
  require_args("list.contains", 2, argc);
  GValue list = require_list(args[0]);
  GValue elem = args[1];
  while (list.tag == G_LIST) {
    GList *cell = list.as.pointer;
    if (g_equal(cell->head, elem)) return g_bool(1);
    list = cell->tail;
  }
  return g_bool(0);
}

GValue gleam_list_count(GValue *args, size_t argc) {
  require_args("list.count", 2, argc);
  GValue list = require_list(args[0]);
  GValue predicate = args[1];
  int64_t count = 0;
  while (list.tag == G_LIST) {
    GList *cell = list.as.pointer;
    GValue ok = g_call(predicate, (GValue[]){cell->head}, 1);
    if (ok.as.boolean) ++count;
    g_release(ok);
    list = cell->tail;
  }
  return g_int(count);
}

GValue gleam_list_drop(GValue *args, size_t argc) {
  require_args("list.drop", 2, argc);
  GValue list = require_list(args[0]);
  int64_t n = require_int(args[1]);
  while (list.tag == G_LIST && n > 0) {
    list = ((GList*)list.as.pointer)->tail;
    --n;
  }
  // The tail shares cells with the input list, so it needs its own
  // reference.
  return g_retain(list);
}

GValue gleam_list_drop_while(GValue *args, size_t argc) {
  require_args("list.drop_while", 2, argc);
  GValue list = require_list(args[0]);
  GValue predicate = args[1];
  while (list.tag == G_LIST) {
    GList *cell = list.as.pointer;
    GValue ok = g_call(predicate, (GValue[]){cell->head}, 1);
    int pass = ok.as.boolean;
    g_release(ok);
    if (!pass) break;
    list = cell->tail;
  }
  return g_retain(list);
}

GValue gleam_list_each(GValue *args, size_t argc) {
  require_args("list.each", 2, argc);
  GValue list = require_list(args[0]);
  GValue fun = args[1];
  while (list.tag == G_LIST) {
    GList *cell = list.as.pointer;
    GValue result = g_call(fun, (GValue[]){cell->head}, 1);
    g_release(result);
    list = cell->tail;
  }
  return g_nil();
}

GValue gleam_list_filter(GValue *args, size_t argc) {
  require_args("list.filter", 2, argc);
  GValue list = require_list(args[0]);
  GValue predicate = args[1];
  GValue result = g_list_empty();
  while (list.tag == G_LIST) {
    GList *cell = list.as.pointer;
    GValue ok = g_call(predicate, (GValue[]){cell->head}, 1);
    int pass = ok.as.boolean;
    g_release(ok);
    if (pass) result = g_list_prepend(g_retain(cell->head), result);
    list = cell->tail;
  }
  return list_reverse_own(result);
}

GValue gleam_list_filter_map(GValue *args, size_t argc) {
  require_args("list.filter_map", 2, argc);
  GValue list = require_list(args[0]);
  GValue fun = args[1];
  GValue result = g_list_empty();
  while (list.tag == G_LIST) {
    GList *cell = list.as.pointer;
    GValue mapped = g_call(fun, (GValue[]){cell->head}, 1);
    if (g_record_is(mapped, "Ok"))
      result = g_list_prepend(g_retain(g_record_get(mapped, 0)), result);
    g_release(mapped);
    list = cell->tail;
  }
  return list_reverse_own(result);
}

GValue gleam_list_find(GValue *args, size_t argc) {
  require_args("list.find", 2, argc);
  GValue list = require_list(args[0]);
  GValue predicate = args[1];
  while (list.tag == G_LIST) {
    GList *cell = list.as.pointer;
    GValue ok = g_call(predicate, (GValue[]){cell->head}, 1);
    int pass = ok.as.boolean;
    g_release(ok);
    if (pass) return g_record("Ok", 1, (GValue[]){g_retain(cell->head)});
    list = cell->tail;
  }
  return g_record("Error", 1, (GValue[]){g_nil()});
}

GValue gleam_list_find_map(GValue *args, size_t argc) {
  require_args("list.find_map", 2, argc);
  GValue list = require_list(args[0]);
  GValue fun = args[1];
  while (list.tag == G_LIST) {
    GList *cell = list.as.pointer;
    GValue mapped = g_call(fun, (GValue[]){cell->head}, 1);
    if (g_record_is(mapped, "Ok")) {
      GValue value = g_retain(g_record_get(mapped, 0));
      g_release(mapped);
      return g_record("Ok", 1, (GValue[]){value});
    }
    g_release(mapped);
    list = cell->tail;
  }
  return g_record("Error", 1, (GValue[]){g_nil()});
}

GValue gleam_list_first(GValue *args, size_t argc) {
  require_args("list.first", 1, argc);
  GValue list = require_list(args[0]);
  if (list.tag == G_NIL) return g_record("Error", 1, (GValue[]){g_nil()});
  return g_record("Ok", 1, (GValue[]){g_retain(((GList*)list.as.pointer)->head)});
}

GValue gleam_list_flat_map(GValue *args, size_t argc) {
  require_args("list.flat_map", 2, argc);
  GValue list = require_list(args[0]);
  GValue fun = args[1];
  size_t outer;
  GValue **elements = list_collect(list, &outer);
  // Map every element to a list first, then count the total.
  GValue *mapped = malloc(outer * sizeof(GValue));
  size_t total = 0;
  for (size_t i = 0; i < outer; ++i) {
    mapped[i] = require_list(g_call(fun, (GValue[]){*elements[i]}, 1));
    total += g_list_length(mapped[i]);
  }
  GValue **items = malloc((total ? total : 1) * sizeof(GValue *));
  size_t index = 0;
  for (size_t i = 0; i < outer; ++i) {
    GValue inner = mapped[i];
    while (inner.tag == G_LIST) {
      GList *cell = inner.as.pointer;
      GValue *slot = malloc(sizeof(GValue));
      *slot = g_retain(cell->head);
      items[index++] = slot;
      inner = cell->tail;
    }
  }
  for (size_t i = 0; i < outer; ++i) {
    g_release(mapped[i]);
    g_release(*elements[i]);
    free(elements[i]);
  }
  free(mapped);
  free(elements);
  GValue result = list_build(items, index);
  list_free(items, index);
  return result;
}

GValue gleam_list_flatten(GValue *args, size_t argc) {
  require_args("list.flatten", 1, argc);
  GValue lists = require_list(args[0]);
  size_t outer;
  GValue **rows = list_collect(lists, &outer);
  size_t total = 0;
  for (size_t i = 0; i < outer; ++i)
    total += g_list_length(require_list(*rows[i]));
  GValue **items = malloc((total ? total : 1) * sizeof(GValue *));
  size_t index = 0;
  for (size_t i = 0; i < outer; ++i) {
    GValue inner = require_list(*rows[i]);
    while (inner.tag == G_LIST) {
      GList *cell = inner.as.pointer;
      GValue *slot = malloc(sizeof(GValue));
      *slot = g_retain(cell->head);
      items[index++] = slot;
      inner = cell->tail;
    }
  }
  for (size_t i = 0; i < outer; ++i) {
    g_release(*rows[i]);
    free(rows[i]);
  }
  free(rows);
  GValue result = list_build(items, index);
  list_free(items, index);
  return result;
}

GValue gleam_list_fold(GValue *args, size_t argc) {
  require_args("list.fold", 3, argc);
  GValue list = require_list(args[0]);
  // The arguments are released by the caller after this returns, so take
  // an owned reference to the accumulator before reassigning it.
  GValue acc = g_retain(args[1]);
  GValue fun = args[2];
  while (list.tag == G_LIST) {
    GList *cell = list.as.pointer;
    GValue next = g_call(fun, (GValue[]){acc, cell->head}, 2);
    g_release(acc);
    acc = next;
    list = cell->tail;
  }
  return acc;
}

GValue gleam_list_fold_right(GValue *args, size_t argc) {
  require_args("list.fold_right", 3, argc);
  // Folding right-to-left is a left fold over the reversed list.
  // A local reversed copy for the traversal; released when done.
  GValue reversed = list_reverse(require_list(args[0]));
  GValue list = reversed;
  GValue acc = g_retain(args[1]);
  GValue fun = args[2];
  while (list.tag == G_LIST) {
    GList *cell = list.as.pointer;
    GValue next = g_call(fun, (GValue[]){acc, cell->head}, 2);
    g_release(acc);
    acc = next;
    list = cell->tail;
  }
  g_release(reversed);
  return acc;
}

GValue gleam_list_fold_until(GValue *args, size_t argc) {
  require_args("list.fold_until", 3, argc);
  GValue list = require_list(args[0]);
  GValue acc = g_retain(args[1]);
  GValue fun = args[2];
  while (list.tag == G_LIST) {
    GList *cell = list.as.pointer;
    GValue step = g_call(fun, (GValue[]){acc, cell->head}, 2);
    int stop = !g_record_is(step, "Continue");
    GValue next = g_retain(g_record_get(step, 0));
    g_release(step);
    g_release(acc);
    acc = next;
    if (stop) return acc;
    list = cell->tail;
  }
  return acc;
}

GValue gleam_list_index_fold(GValue *args, size_t argc) {
  require_args("list.index_fold", 3, argc);
  GValue list = require_list(args[0]);
  GValue acc = g_retain(args[1]);
  GValue fun = args[2];
  int64_t index = 0;
  while (list.tag == G_LIST) {
    GList *cell = list.as.pointer;
    GValue next = g_call(fun, (GValue[]){acc, cell->head, g_int(index)}, 3);
    g_release(acc);
    acc = next;
    ++index;
    list = cell->tail;
  }
  return acc;
}

GValue gleam_list_index_map(GValue *args, size_t argc) {
  require_args("list.index_map", 2, argc);
  GValue list = require_list(args[0]);
  GValue fun = args[1];
  GValue result = g_list_empty();
  int64_t index = 0;
  while (list.tag == G_LIST) {
    GList *cell = list.as.pointer;
    result = g_list_prepend(
        g_call(fun, (GValue[]){cell->head, g_int(index)}, 2), result);
    ++index;
    list = cell->tail;
  }
  return list_reverse_own(result);
}

// Take the first element of every inner list, then the second, and so on.
GValue gleam_list_interleave(GValue *args, size_t argc) {
  require_args("list.interleave", 1, argc);
  size_t rows;
  GValue **items = list_collect(require_list(args[0]), &rows);
  GValue result = g_list_empty();
  for (size_t column = 0;; ++column) {
    int found = 0;
    for (size_t i = 0; i < rows; ++i) {
      GValue row = *items[i];
      for (size_t c = 0; c < column && row.tag == G_LIST; ++c)
        row = ((GList*)row.as.pointer)->tail;
      if (row.tag == G_LIST) {
        result = g_list_prepend(g_retain(((GList*)row.as.pointer)->head), result);
        found = 1;
      }
    }
    if (!found) break;
  }
  list_free(items, rows);
  return list_reverse_own(result);
}

GValue gleam_list_intersperse(GValue *args, size_t argc) {
  require_args("list.intersperse", 2, argc);
  GValue elem = args[1];
  size_t count;
  GValue **items = list_collect(require_list(args[0]), &count);
  GValue result = g_list_empty();
  for (size_t i = count; i-- > 0;) {
    result = g_list_prepend(g_retain(*items[i]), result);
    // The separator precedes every element except the first.
    if (i > 0) result = g_list_prepend(g_retain(elem), result);
  }
  list_free(items, count);
  return result;
}

GValue gleam_list_is_empty(GValue *args, size_t argc) {
  require_args("list.is_empty", 1, argc);
  return g_bool(require_list(args[0]).tag == G_NIL);
}

GValue gleam_list_key_filter(GValue *args, size_t argc) {
  require_args("list.key_filter", 2, argc);
  GValue list = require_list(args[0]);
  GValue key = args[1];
  GValue result = g_list_empty();
  while (list.tag == G_LIST) {
    GList *cell = list.as.pointer;
    if (g_equal(g_tuple_get(cell->head, 0), key))
      result = g_list_prepend(g_retain(g_tuple_get(cell->head, 1)), result);
    list = cell->tail;
  }
  return list_reverse_own(result);
}

GValue gleam_list_key_find(GValue *args, size_t argc) {
  require_args("list.key_find", 2, argc);
  GValue list = require_list(args[0]);
  GValue key = args[1];
  while (list.tag == G_LIST) {
    GList *cell = list.as.pointer;
    if (g_equal(g_tuple_get(cell->head, 0), key))
      return g_record("Ok", 1, (GValue[]){g_retain(g_tuple_get(cell->head, 1))});
    list = cell->tail;
  }
  return g_record("Error", 1, (GValue[]){g_nil()});
}

GValue gleam_list_key_pop(GValue *args, size_t argc) {
  require_args("list.key_pop", 2, argc);
  GValue key = args[1];
  size_t count;
  GValue **items = list_collect(require_list(args[0]), &count);
  int64_t found = -1;
  for (size_t i = 0; i < count; ++i) {
    if (g_equal(g_tuple_get(*items[i], 0), key)) {
      found = (int64_t)i;
      break;
    }
  }
  if (found < 0) {
    list_free(items, count);
    return g_record("Error", 1, (GValue[]){g_nil()});
  }
  GValue value = g_retain(g_tuple_get(*items[found], 1));
  GValue rest = g_list_empty();
  for (size_t i = count; i-- > 0;) {
    if ((int64_t)i == found) continue;
    rest = g_list_prepend(g_retain(*items[i]), rest);
  }
  list_free(items, count);
  return g_record("Ok", 1, (GValue[]){g_tuple(2, (GValue[]){value, rest})});
}

GValue gleam_list_key_set(GValue *args, size_t argc) {
  require_args("list.key_set", 3, argc);
  GValue key = args[1];
  GValue value = args[2];
  size_t count;
  GValue **items = list_collect(require_list(args[0]), &count);
  int64_t found = -1;
  for (size_t i = 0; i < count; ++i) {
    if (g_equal(g_tuple_get(*items[i], 0), key)) {
      found = (int64_t)i;
      break;
    }
  }
  GValue pair = g_tuple(2, (GValue[]){g_retain(key), g_retain(value)});
  GValue result = g_list_empty();
  if (found < 0) {
    // A new entry goes to the end of the list, which is prepended first.
    result = g_list_prepend(pair, result);
    for (size_t i = count; i-- > 0;)
      result = g_list_prepend(g_retain(*items[i]), result);
  } else {
    for (size_t i = count; i-- > 0;) {
      if ((int64_t)i == found) result = g_list_prepend(g_retain(pair), result);
      else result = g_list_prepend(g_retain(*items[i]), result);
    }
    g_release(pair);
  }
  list_free(items, count);
  return result;
}

GValue gleam_list_last(GValue *args, size_t argc) {
  require_args("list.last", 1, argc);
  GValue list = require_list(args[0]);
  if (list.tag == G_NIL) return g_record("Error", 1, (GValue[]){g_nil()});
  while (((GList*)list.as.pointer)->tail.tag == G_LIST)
    list = ((GList*)list.as.pointer)->tail;
  return g_record("Ok", 1, (GValue[]){g_retain(((GList*)list.as.pointer)->head)});
}

GValue gleam_list_length(GValue *args, size_t argc) {
  require_args("list.length", 1, argc);
  return g_int((int64_t)g_list_length(require_list(args[0])));
}

GValue gleam_list_map(GValue *args, size_t argc) {
  require_args("list.map", 2, argc);
  GValue list = require_list(args[0]);
  GValue fun = args[1];
  GValue result = g_list_empty();
  while (list.tag == G_LIST) {
    GList *cell = list.as.pointer;
    result = g_list_prepend(g_call(fun, (GValue[]){cell->head}, 1), result);
    list = cell->tail;
  }
  return list_reverse_own(result);
}

GValue gleam_list_map2(GValue *args, size_t argc) {
  require_args("list.map2", 3, argc);
  GValue list1 = require_list(args[0]);
  GValue list2 = require_list(args[1]);
  GValue fun = args[2];
  GValue result = g_list_empty();
  while (list1.tag == G_LIST && list2.tag == G_LIST) {
    GList *cell1 = list1.as.pointer;
    GList *cell2 = list2.as.pointer;
    result = g_list_prepend(
        g_call(fun, (GValue[]){cell1->head, cell2->head}, 2), result);
    list1 = cell1->tail;
    list2 = cell2->tail;
  }
  return list_reverse_own(result);
}

GValue gleam_list_map_fold(GValue *args, size_t argc) {
  require_args("list.map_fold", 3, argc);
  GValue list = require_list(args[0]);
  GValue acc = g_retain(args[1]);
  GValue fun = args[2];
  GValue result = g_list_empty();
  while (list.tag == G_LIST) {
    GList *cell = list.as.pointer;
    GValue pair = g_call(fun, (GValue[]){acc, cell->head}, 2);
    GValue next = g_retain(g_tuple_get(pair, 0));
    result = g_list_prepend(g_retain(g_tuple_get(pair, 1)), result);
    g_release(pair);
    g_release(acc);
    acc = next;
    list = cell->tail;
  }
  return g_tuple(2, (GValue[]){acc, list_reverse_own(result)});
}

GValue gleam_list_max(GValue *args, size_t argc) {
  require_args("list.max", 2, argc);
  GValue list = require_list(args[0]);
  GValue compare = args[1];
  if (list.tag == G_NIL) return g_record("Error", 1, (GValue[]){g_nil()});
  GList *cell = list.as.pointer;
  GValue best = g_retain(cell->head);
  list = cell->tail;
  while (list.tag == G_LIST) {
    GList *next = list.as.pointer;
    GValue order = g_call(compare, (GValue[]){best, next->head}, 2);
    // `Lt` means the candidate is greater than the current best.
    int greater = g_record_is(order, "Lt");
    g_release(order);
    if (greater) {
      g_release(best);
      best = g_retain(next->head);
    }
    list = next->tail;
  }
  return g_record("Ok", 1, (GValue[]){best});
}

GValue gleam_list_new(GValue *args, size_t argc) {
  (void)args;
  require_args("list.new", 0, argc);
  return g_list_empty();
}

GValue gleam_list_partition(GValue *args, size_t argc) {
  require_args("list.partition", 2, argc);
  GValue list = require_list(args[0]);
  GValue categorise = args[1];
  GValue yes = g_list_empty();
  GValue no = g_list_empty();
  while (list.tag == G_LIST) {
    GList *cell = list.as.pointer;
    GValue ok = g_call(categorise, (GValue[]){cell->head}, 1);
    int pass = ok.as.boolean;
    g_release(ok);
    if (pass) yes = g_list_prepend(g_retain(cell->head), yes);
    else no = g_list_prepend(g_retain(cell->head), no);
    list = cell->tail;
  }
  return g_tuple(2, (GValue[]){list_reverse_own(yes), list_reverse_own(no)});
}

// Append every permutation of the rest after fixing each element in turn
// as the first. `out` grows via realloc and owns one list per entry.
static void permutations_rec(GValue **items, size_t length, GValue ***out,
                             size_t *count) {
  if (length == 0) {
    GValue *entry = malloc(sizeof(GValue));
    *entry = g_list_empty();
    *out = realloc(*out, (*count + 1) * sizeof(GValue *));
    (*out)[(*count)++] = entry;
    return;
  }
  for (size_t i = 0; i < length; ++i) {
    size_t start = *count;
    GValue **rest = malloc((length - 1) * sizeof(GValue *));
    size_t written = 0;
    for (size_t j = 0; j < length; ++j)
      if (j != i) rest[written++] = items[j];
    permutations_rec(rest, length - 1, out, count);
    free(rest);
    for (size_t k = start; k < *count; ++k)
      *(*out)[k] = g_list_prepend(g_retain(*items[i]), *(*out)[k]);
  }
}

GValue gleam_list_permutations(GValue *args, size_t argc) {
  require_args("list.permutations", 1, argc);
  size_t count;
  GValue **items = list_collect(require_list(args[0]), &count);
  GValue **result = NULL;
  size_t result_count = 0;
  permutations_rec(items, count, &result, &result_count);
  GValue list = g_list_empty();
  for (size_t i = result_count; i-- > 0;) list = g_list_prepend(g_retain(*result[i]), list);
  for (size_t i = 0; i < result_count; ++i) {
    g_release(*result[i]);
    free(result[i]);
  }
  free(result);
  list_free(items, count);
  return list;
}

GValue gleam_list_prepend(GValue *args, size_t argc) {
  require_args("list.prepend", 2, argc);
  // The caller releases the arguments after this returns, so both the
  // element and the list tail need their own references.
  return g_list_prepend(g_retain(args[1]), g_retain(require_list(args[0])));
}

GValue gleam_list_reduce(GValue *args, size_t argc) {
  require_args("list.reduce", 2, argc);
  GValue list = require_list(args[0]);
  GValue fun = args[1];
  if (list.tag == G_NIL) return g_record("Error", 1, (GValue[]){g_nil()});
  GList *cell = list.as.pointer;
  GValue acc = g_retain(cell->head);
  list = cell->tail;
  while (list.tag == G_LIST) {
    GList *next = list.as.pointer;
    GValue step = g_call(fun, (GValue[]){acc, next->head}, 2);
    g_release(acc);
    acc = step;
    list = next->tail;
  }
  return g_record("Ok", 1, (GValue[]){acc});
}

GValue gleam_list_repeat(GValue *args, size_t argc) {
  require_args("list.repeat", 2, argc);
  GValue item = args[0];
  int64_t times = require_int(args[1]);
  GValue result = g_list_empty();
  for (int64_t i = 0; i < times; ++i)
    result = g_list_prepend(g_retain(item), result);
  return result;
}

GValue gleam_list_rest(GValue *args, size_t argc) {
  require_args("list.rest", 1, argc);
  GValue list = require_list(args[0]);
  if (list.tag == G_NIL) return g_record("Error", 1, (GValue[]){g_nil()});
  return g_record(
      "Ok", 1, (GValue[]){g_retain(((GList*)list.as.pointer)->tail)});
}

GValue gleam_list_reverse(GValue *args, size_t argc) {
  require_args("list.reverse", 1, argc);
  return list_reverse(require_list(args[0]));
}

// Reservoir sampling: keep the first n elements, then replace a random slot
// for each later element with probability n / (i + 1). The reservoir maps
// slots to original indices so the result keeps the input order.
GValue gleam_list_sample(GValue *args, size_t argc) {
  require_args("list.sample", 2, argc);
  GValue list = require_list(args[0]);
  int64_t n = require_int(args[1]);
  size_t count;
  GValue **items = list_collect(list, &count);
  if (n <= 0) {
    list_free(items, count);
    return g_list_empty();
  }
  if ((uint64_t)n >= count) {
    GValue result = list_build(items, count);
    list_free(items, count);
    return result;
  }
  size_t width = (size_t)n;
  size_t *reservoir = malloc(width * sizeof(size_t));
  for (size_t i = 0; i < width; ++i) reservoir[i] = i;
  for (size_t i = width; i < count; ++i) {
    size_t slot = (size_t)(random_draw() % (uint64_t)(i + 1));
    if (slot < width) reservoir[slot] = i;
  }
  uint8_t *selected = calloc(count, 1);
  for (size_t i = 0; i < width; ++i) selected[reservoir[i]] = 1;
  free(reservoir);
  GValue result = g_list_empty();
  for (size_t i = count; i-- > 0;) {
    if (selected[i]) result = g_list_prepend(g_retain(*items[i]), result);
  }
  free(selected);
  list_free(items, count);
  return result;
}

GValue gleam_list_scan(GValue *args, size_t argc) {
  require_args("list.scan", 3, argc);
  GValue list = require_list(args[0]);
  GValue acc = g_retain(args[1]);
  GValue fun = args[2];
  GValue result = g_list_prepend(g_retain(acc), g_list_empty());
  while (list.tag == G_LIST) {
    GList *cell = list.as.pointer;
    GValue next = g_call(fun, (GValue[]){acc, cell->head}, 2);
    result = g_list_prepend(g_retain(next), result);
    g_release(acc);
    acc = next;
    list = cell->tail;
  }
  g_release(acc);
  return list_reverse_own(result);
}

// Fisher-Yates shuffle.
GValue gleam_list_shuffle(GValue *args, size_t argc) {
  require_args("list.shuffle", 1, argc);
  size_t count;
  GValue **items = list_collect(require_list(args[0]), &count);
  for (size_t i = count; i > 1; --i) {
    size_t j = (size_t)(random_draw() % (uint64_t)i);
    GValue *tmp = items[i - 1];
    items[i - 1] = items[j];
    items[j] = tmp;
  }
  GValue result = list_build(items, count);
  list_free(items, count);
  return result;
}

GValue gleam_list_sized_chunk(GValue *args, size_t argc) {
  require_args("list.sized_chunk", 2, argc);
  GValue list = require_list(args[0]);
  int64_t size = require_int(args[1]);
  if (size < 1) size = 1;
  size_t count;
  GValue **items = list_collect(list, &count);
  GValue result = g_list_empty();
  // Build the chunks from the end of the list towards the start. The final
  // chunk (built first) may be shorter than `size`.
  size_t start = count % (size_t)size;
  if (start == 0 && count > 0) start = (size_t)size;
  for (size_t end = count; end > 0;) {
    size_t begin = end - start;
    GValue chunk = g_list_empty();
    for (size_t i = end; i-- > begin;)
      chunk = g_list_prepend(g_retain(*items[i]), chunk);
    result = g_list_prepend(chunk, result);
    end = begin;
    start = (size_t)size;
  }
  list_free(items, count);
  return result;
}

// Insertion sort: simple and stable for the small lists this compiler
// targets.
GValue gleam_list_sort(GValue *args, size_t argc) {
  require_args("list.sort", 2, argc);
  GValue compare = args[1];
  size_t count;
  GValue **items = list_collect(require_list(args[0]), &count);
  for (size_t i = 1; i < count; ++i) {
    GValue *key = items[i];
    size_t j = i;
    while (j > 0) {
      GValue order = g_call(compare, (GValue[]){*items[j - 1], *key}, 2);
      int greater = g_record_is(order, "Gt");
      g_release(order);
      if (!greater) break;
      items[j] = items[j - 1];
      --j;
    }
    items[j] = key;
  }
  GValue result = list_build(items, count);
  list_free(items, count);
  return result;
}

GValue gleam_list_split(GValue *args, size_t argc) {
  require_args("list.split", 2, argc);
  GValue list = require_list(args[0]);
  int64_t index = require_int(args[1]);
  size_t count;
  GValue **items = list_collect(list, &count);
  size_t cut =
      index < 0 ? 0 : (uint64_t)index >= count ? count : (size_t)index;
  GValue before = g_list_empty();
  for (size_t i = cut; i-- > 0;) before = g_list_prepend(g_retain(*items[i]), before);
  GValue after = g_list_empty();
  for (size_t i = count; i-- > cut;) after = g_list_prepend(g_retain(*items[i]), after);
  list_free(items, count);
  return g_tuple(2, (GValue[]){before, after});
}

GValue gleam_list_split_while(GValue *args, size_t argc) {
  require_args("list.split_while", 2, argc);
  GValue list = require_list(args[0]);
  GValue predicate = args[1];
  GValue first = g_list_empty();
  while (list.tag == G_LIST) {
    GList *cell = list.as.pointer;
    GValue ok = g_call(predicate, (GValue[]){cell->head}, 1);
    int pass = ok.as.boolean;
    g_release(ok);
    if (!pass) break;
    first = g_list_prepend(g_retain(cell->head), first);
    list = cell->tail;
  }
  // The remainder shares cells with the input list.
  return g_tuple(2, (GValue[]){list_reverse_own(first), g_retain(list)});
}

GValue gleam_list_strict_zip(GValue *args, size_t argc) {
  require_args("list.strict_zip", 2, argc);
  GValue list = require_list(args[0]);
  GValue other = require_list(args[1]);
  if (g_list_length(list) != g_list_length(other))
    return g_record("Error", 1, (GValue[]){g_nil()});
  GValue result = g_list_empty();
  while (list.tag == G_LIST) {
    GList *cell = list.as.pointer;
    GList *other_cell = other.as.pointer;
    GValue pair = g_tuple(2, (GValue[]){g_retain(cell->head),
                                       g_retain(other_cell->head)});
    result = g_list_prepend(pair, result);
    list = cell->tail;
    other = other_cell->tail;
  }
  return g_record("Ok", 1, (GValue[]){list_reverse_own(result)});
}

GValue gleam_list_take(GValue *args, size_t argc) {
  require_args("list.take", 2, argc);
  GValue list = require_list(args[0]);
  int64_t n = require_int(args[1]);
  if (n <= 0) return g_list_empty();
  size_t count;
  GValue **items = list_collect(list, &count);
  size_t taken = (uint64_t)n < count ? (size_t)n : count;
  GValue result = g_list_empty();
  for (size_t i = taken; i-- > 0;) result = g_list_prepend(g_retain(*items[i]), result);
  list_free(items, count);
  return result;
}

GValue gleam_list_take_while(GValue *args, size_t argc) {
  require_args("list.take_while", 2, argc);
  GValue list = require_list(args[0]);
  GValue predicate = args[1];
  GValue result = g_list_empty();
  while (list.tag == G_LIST) {
    GList *cell = list.as.pointer;
    GValue ok = g_call(predicate, (GValue[]){cell->head}, 1);
    int pass = ok.as.boolean;
    g_release(ok);
    if (!pass) break;
    result = g_list_prepend(g_retain(cell->head), result);
    list = cell->tail;
  }
  return list_reverse_own(result);
}

// Column by column: the c-th element of every row that has one.
GValue gleam_list_transpose(GValue *args, size_t argc) {
  require_args("list.transpose", 1, argc);
  size_t rows;
  GValue **items = list_collect(require_list(args[0]), &rows);
  size_t columns = 0;
  for (size_t i = 0; i < rows; ++i)
    columns = g_list_length(*items[i]) > columns ? g_list_length(*items[i]) : columns;
  GValue result = g_list_empty();
  for (size_t column = columns; column-- > 0;) {
    GValue col = g_list_empty();
    int found = 0;
    for (size_t i = rows; i-- > 0;) {
      GValue row = *items[i];
      for (size_t c = 0; c < column && row.tag == G_LIST; ++c)
        row = ((GList*)row.as.pointer)->tail;
      if (row.tag == G_LIST) {
        col = g_list_prepend(g_retain(((GList*)row.as.pointer)->head), col);
        found = 1;
      }
    }
    if (found) result = g_list_prepend(col, result);
  }
  list_free(items, rows);
  return result;
}

GValue gleam_list_try_each(GValue *args, size_t argc) {
  require_args("list.try_each", 2, argc);
  GValue list = require_list(args[0]);
  GValue fun = args[1];
  while (list.tag == G_LIST) {
    GList *cell = list.as.pointer;
    GValue step = g_call(fun, (GValue[]){cell->head}, 1);
    if (!g_record_is(step, "Ok")) {
      GValue error = g_retain(g_record_get(step, 0));
      g_release(step);
      return g_record("Error", 1, (GValue[]){error});
    }
    g_release(step);
    list = cell->tail;
  }
  return g_record("Ok", 1, (GValue[]){g_nil()});
}

GValue gleam_list_try_fold(GValue *args, size_t argc) {
  require_args("list.try_fold", 3, argc);
  GValue list = require_list(args[0]);
  GValue acc = g_retain(args[1]);
  GValue fun = args[2];
  while (list.tag == G_LIST) {
    GList *cell = list.as.pointer;
    GValue step = g_call(fun, (GValue[]){acc, cell->head}, 2);
    if (!g_record_is(step, "Ok")) {
      GValue error = g_retain(g_record_get(step, 0));
      g_release(step);
      g_release(acc);
      return g_record("Error", 1, (GValue[]){error});
    }
    GValue next = g_retain(g_record_get(step, 0));
    g_release(step);
    g_release(acc);
    acc = next;
    list = cell->tail;
  }
  return g_record("Ok", 1, (GValue[]){acc});
}

GValue gleam_list_try_map(GValue *args, size_t argc) {
  require_args("list.try_map", 2, argc);
  GValue list = require_list(args[0]);
  GValue fun = args[1];
  GValue result = g_list_empty();
  while (list.tag == G_LIST) {
    GList *cell = list.as.pointer;
    GValue step = g_call(fun, (GValue[]){cell->head}, 1);
    if (!g_record_is(step, "Ok")) {
      GValue error = g_retain(g_record_get(step, 0));
      g_release(step);
      g_release(result);
      return g_record("Error", 1, (GValue[]){error});
    }
    result = g_list_prepend(g_retain(g_record_get(step, 0)), result);
    g_release(step);
    list = cell->tail;
  }
  return g_record("Ok", 1, (GValue[]){list_reverse_own(result)});
}

// Keep the first occurrence of each element.
GValue gleam_list_unique(GValue *args, size_t argc) {
  require_args("list.unique", 1, argc);
  size_t count;
  GValue **items = list_collect(require_list(args[0]), &count);
  GValue result = g_list_empty();
  for (size_t i = count; i-- > 0;) {
    // Keep an element only when no earlier one compares equal to it.
    int seen = 0;
    for (size_t j = 0; j < i; ++j) {
      if (g_equal(*items[i], *items[j])) {
        seen = 1;
        break;
      }
    }
    if (!seen) result = g_list_prepend(g_retain(*items[i]), result);
  }
  list_free(items, count);
  return result;
}

GValue gleam_list_unzip(GValue *args, size_t argc) {
  require_args("list.unzip", 1, argc);
  GValue list = require_list(args[0]);
  GValue firsts = g_list_empty();
  GValue seconds = g_list_empty();
  while (list.tag == G_LIST) {
    GList *cell = list.as.pointer;
    firsts = g_list_prepend(g_retain(g_tuple_get(cell->head, 0)), firsts);
    seconds = g_list_prepend(g_retain(g_tuple_get(cell->head, 1)), seconds);
    list = cell->tail;
  }
  return g_tuple(2, (GValue[]){list_reverse_own(firsts), list_reverse_own(seconds)});
}

GValue gleam_list_window(GValue *args, size_t argc) {
  require_args("list.window", 2, argc);
  GValue list = require_list(args[0]);
  int64_t n = require_int(args[1]);
  size_t count;
  GValue **items = list_collect(list, &count);
  if (n <= 0 || (uint64_t)n > count) {
    list_free(items, count);
    return g_list_empty();
  }
  size_t width = (size_t)n;
  GValue result = g_list_empty();
  for (size_t start = count - width + 1; start-- > 0;) {
    GValue window = g_list_empty();
    for (size_t i = start + width; i-- > start;)
      window = g_list_prepend(g_retain(*items[i]), window);
    result = g_list_prepend(window, result);
  }
  list_free(items, count);
  return result;
}

GValue gleam_list_window_by_2(GValue *args, size_t argc) {
  require_args("list.window_by_2", 1, argc);
  GValue list = require_list(args[0]);
  if (list.tag == G_NIL) return g_list_empty();
  GList *cell = list.as.pointer;
  GValue previous = cell->head;
  GValue result = g_list_empty();
  list = cell->tail;
  while (list.tag == G_LIST) {
    GList *next = list.as.pointer;
    GValue pair = g_tuple(2, (GValue[]){g_retain(previous),
                                       g_retain(next->head)});
    result = g_list_prepend(pair, result);
    previous = next->head;
    list = next->tail;
  }
  return list_reverse_own(result);
}

GValue gleam_list_wrap(GValue *args, size_t argc) {
  require_args("list.wrap", 1, argc);
  return g_list_prepend(g_retain(args[0]), g_list_empty());
}

GValue gleam_list_zip(GValue *args, size_t argc) {
  require_args("list.zip", 2, argc);
  GValue list = require_list(args[0]);
  GValue other = require_list(args[1]);
  GValue result = g_list_empty();
  while (list.tag == G_LIST && other.tag == G_LIST) {
    GList *cell = list.as.pointer;
    GList *other_cell = other.as.pointer;
    GValue pair = g_tuple(2, (GValue[]){g_retain(cell->head),
                                       g_retain(other_cell->head)});
    result = g_list_prepend(pair, result);
    list = cell->tail;
    other = other_cell->tail;
  }
  return list_reverse_own(result);
}
