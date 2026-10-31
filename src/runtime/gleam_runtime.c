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
  GValue result = g_string_n(a->data, a->length + b->length);
  GString *out = result.as.pointer;
  memcpy(out->data + a->length, b->data, b->length);
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
    return g_record("Error", 1, (GValue[]){g_string("end of input")});
  }
  if (length > 0 && line[length - 1] == '\n') --length;
  if (length > 0 && line[length - 1] == '\r') --length;
  GValue result = g_record("Ok", 1, (GValue[]){g_string_n(line, length)});
  free(line);
  return result;
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
  GValue result = g_string_n("", total);
  GString *out = result.as.pointer;
  size_t offset = 0;
  cursor = args[0];
  while (cursor.tag == G_LIST) {
    GList *list = cursor.as.pointer;
    GString *element = require_string(list->head);
    memcpy(out->data + offset, element->data, element->length);
    offset += element->length;
    cursor = list->tail;
  }
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
  GValue result = g_string_n("", total);
  GString *out = result.as.pointer;
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
  GValue result =
      g_string_n("", string->length + needed);
  GString *out = result.as.pointer;
  if (at_start) {
    for (size_t i = 0; i < needed; ++i)
      out->data[i] = pad->data[i % pad->length];
    memcpy(out->data + needed, string->data, string->length);
  } else {
    memcpy(out->data, string->data, string->length);
    for (size_t i = 0; i < needed; ++i)
      out->data[string->length + i] = pad->data[i % pad->length];
  }
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
  GValue result = g_string_n("", total);
  GString *out = result.as.pointer;
  for (int64_t i = 0; i < times; ++i)
    memcpy(out->data + (size_t)i * string->length, string->data,
           string->length);
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
  GValue result = g_string_n("", total);
  GString *out = result.as.pointer;
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
