#include "tree_sitter/parser.h"
#include <stdint.h>
#include <stdbool.h>
#include <string.h>

/* ─────────────────────────────────────────────────────────────────
 * Token types – must match the order of externals in grammar.js.
 * ───────────────────────────────────────────────────────────────── */
typedef enum {
  INDENT,        // 0
  DEDENT,        // 1
  CASE_INDENT,   // 2
  BLOCK_COMMENT, // 3
  CALL_OPEN,     // 4
} TokenType;

/* ─────────────────────────────────────────────────────────────────
 * Scanner state
 * ───────────────────────────────────────────────────────────────── */
/* The serialized state is one byte plus two uint32_t fields and the stack.
 * Keep the worst-case output below Tree-sitter's 1024-byte buffer. */
#define MAX_INDENT ((TREE_SITTER_SERIALIZATION_BUFFER_SIZE - 1 - 2 * sizeof(uint32_t)) / sizeof(uint32_t))

/* The high bit tags the frame opened by a case header. */
#define CASE_FRAME (UINT32_C(1) << 31)

typedef struct {
  uint32_t indent_stack[MAX_INDENT];
  uint32_t stack_size;
  /* pending DEDENT count: emit one DEDENT per call until 0 */
  uint32_t pending_dedents;
  /* bracket depth: () [] {}; INDENT/DEDENT suppressed when > 0 */
  uint32_t bracket_depth;
  /* column of the last non-blank, non-comment content line found during a
     DEDENT scan.  Used to detect "phantom INDENT" when the opener statement
     (e.g. `scene_event := interface:`) lives at a higher column than the
     current indent-stack top, so the next sibling at the same column is NOT
     mistaken for a genuine child of the empty body. */
  uint32_t last_stmt_col;
} Scanner;

/* ─────────────────────────────────────────────────────────────────
 * Helpers
 * ───────────────────────────────────────────────────────────────── */

static inline uint32_t indent_column(uint32_t frame) {
  return frame & ~CASE_FRAME;
}

static inline uint32_t current_indent(const Scanner *s) {
  return s->stack_size > 0 ? indent_column(s->indent_stack[s->stack_size - 1]) : 0;
}

/* Advance past one character (consuming it). */
static inline void advance(TSLexer *lexer) { lexer->advance(lexer, false); }

/* Skip a character as whitespace (not part of any token). */
static inline void skip(TSLexer *lexer) { lexer->advance(lexer, true); }

/* ─────────────────────────────────────────────────────────────────
 * Block comment scanner: <# ... #> with arbitrary nesting.
 * ───────────────────────────────────────────────────────────────── */
/* A comment that starts at the lookahead. `<#>` runs to the end of the line
   (the UEFN digest form); any other `<#` opens a nested `<# ... #>` block
   that ends at its matching `#>` (or at EOF, so the lexer stage reports the
   unterminated comment). Whitespace skipped before the call stays outside
   the token. Returns false when the lookahead is not `<#`; the caller must
   then return false too, so Tree-sitter restores the lexer position. */
static bool scan_comment_here(TSLexer *lexer) {
  if (lexer->lookahead != '<') return false;
  lexer->mark_end(lexer);
  advance(lexer); /* '<' */
  if (lexer->lookahead != '#') return false;
  advance(lexer); /* '#' */
  if (lexer->lookahead == '>') {
    advance(lexer); /* '>' */
    while (lexer->lookahead != '\n' && lexer->lookahead != '\r' &&
           lexer->lookahead != 0) {
      advance(lexer);
    }
    lexer->mark_end(lexer);
    lexer->result_symbol = BLOCK_COMMENT;
    return true;
  }
  int depth = 1;
  while (lexer->lookahead != 0) {
    if (lexer->lookahead == '<') {
      advance(lexer);
      if (lexer->lookahead == '#') { advance(lexer); depth++; }
    } else if (lexer->lookahead == '#') {
      advance(lexer);
      if (lexer->lookahead == '>') { advance(lexer); depth--; if (depth == 0) break; }
    } else {
      advance(lexer);
    }
  }
  lexer->mark_end(lexer); /* token ends at closing #> or EOF */
  lexer->result_symbol = BLOCK_COMMENT;
  return true;
}

/* ─────────────────────────────────────────────────────────────────
 * find_next_content_col: scan forward (consuming only whitespace/blank
 * lines via skip()) and return the column of the first non-blank line.
 *
 * Skips:
 *   - blank lines (lines with only whitespace)
 *
 * Does NOT skip `#` line comments or `<#>` block comments; they are
 * treated as regular content so that the grammar can parse them as
 * `line_comment` / `hash_gt_comment` tokens.
 *
 * On entry  : lexer->lookahead is the first char of the first line to
 *             examine (newlines before the loop must be consumed by
 *             the caller).
 * On return : lexer->lookahead is the first non-whitespace char of the
 *             returned content line (or 0 at EOF).
 *             Returns UINT32_MAX on EOF.
 * ───────────────────────────────────────────────────────────────── */
static uint32_t find_next_content_col(TSLexer *lexer, void (*step)(TSLexer *)) {
  while (true) {
    /* ── Skip leading newlines ──────────────────────────────────── */
    while (lexer->lookahead == '\n' || lexer->lookahead == '\r')
      step(lexer);
    if (lexer->lookahead == 0) return UINT32_MAX;

    /* ── Count column of this line ──────────────────────────────── */
    uint32_t col = 0;
    while (lexer->lookahead == ' ' || lexer->lookahead == '\t') {
      if (lexer->lookahead == '\t') col = (col + 8) & ~7u;
      else col++;
      step(lexer);
    }

    /* ── Blank line ─────────────────────────────────────────────── */
    if (lexer->lookahead == '\n' || lexer->lookahead == '\r')
      continue;
    if (lexer->lookahead == 0) return UINT32_MAX;

    /* ── Any content (including `#` comments and `<#>`) ───────── */
    return col;
  }
}

/* Look past a plain, empty wildcard without consuming it as part of a
 * dedent. The next content line must leave the case; comments at this
 * boundary are left to the ordinary scanner. */
static bool ends_case(TSLexer *lexer, uint32_t outer_column) {
  lexer->mark_end(lexer);
  advance(lexer); /* '_' */
  while (lexer->lookahead == ' ') advance(lexer);
  if (lexer->lookahead != '=') return false;
  advance(lexer);
  if (lexer->lookahead != '>') return false;
  advance(lexer);
  while (lexer->lookahead == ' ' || lexer->lookahead == '\t') advance(lexer);
  if (lexer->lookahead == 0) return true;
  if (lexer->lookahead != '\n' && lexer->lookahead != '\r') return false;
  uint32_t next = find_next_content_col(lexer, advance);
  return next == UINT32_MAX ||
         (next <= outer_column && lexer->lookahead != '#' && lexer->lookahead != '<');
}

static uint32_t dedents_to(const Scanner *s, TSLexer *lexer, uint32_t col) {
  for (uint32_t i = s->stack_size; i > 1; i--) {
    uint32_t frame = s->indent_stack[i - 1];
    if (col >= indent_column(frame)) return s->stack_size - i;
    uint32_t outer_column = indent_column(s->indent_stack[i - 2]);
    if ((frame & CASE_FRAME) && col > outer_column &&
        lexer->lookahead == '_' && col == lexer->get_column(lexer)) {
      /* Only this case can contain the shallower line. If the lookahead
       * fails, close this frame too and leave its parent open. */
      return s->stack_size - i + (ends_case(lexer, outer_column) ? 0 : 1);
    }
  }
  return s->stack_size - 1;
}

/* ─────────────────────────────────────────────────────────────────
 * Main scan function
 * ───────────────────────────────────────────────────────────────── */
static bool scan(Scanner *s, TSLexer *lexer, const bool *valid_symbols) {
  /* ── Emit pending DEDENTs first ──────────────────────────────── */
  if (s->pending_dedents > 0 && valid_symbols[DEDENT]) {
    s->pending_dedents--;
    if (s->stack_size > 0) s->stack_size--;
    lexer->result_symbol = DEDENT;
    return true;
  }

  /* ── BLOCK_COMMENT: try '<#' at any point ─────────────────────  */
  /* (tree-sitter extras mechanism will skip whitespace before here,
     but we need to detect '<#' before '<' is consumed by grammar.)  */
  if (valid_symbols[BLOCK_COMMENT] && lexer->lookahead == '<') {
    /* Not a comment: the '<' was consumed, so return false and let the
       grammar lex it. */
    return scan_comment_here(lexer);
  }

  /* ── Skip horizontal whitespace (not newlines). ──────────────── */
  while (lexer->lookahead == ' ' || lexer->lookahead == '\t' ||
         lexer->lookahead == '\r') {
    skip(lexer);
  }

  /* ── CALL_OPEN: a '(' on the same line as the callee ────────────
     Verse ends an expression at a newline before '(', so only an opener
     reached without crossing a newline continues a call or a declaration's
     parameter list. The skipped spaces stay outside the token. A carriage
     return was skipped above, so a lone CR never counts as a line break
     here; the lexer stage refuses bare CR before parsing. */
  if (valid_symbols[CALL_OPEN] && lexer->lookahead == '(') {
    advance(lexer);
    lexer->mark_end(lexer);
    lexer->result_symbol = CALL_OPEN;
    return true;
  }

  /* ── Track bracket depth so we suppress INDENT/DEDENT inside
        (), [], {} ─────────────────────────────────────────────── */
  /* We don't intercept bracket chars here; rely on the fact that
     INDENT/DEDENT are only requested when the grammar is at a
     colon-block or indented-block position, which can't be inside
     an expression's brackets (the grammar rules ensure that).
     Therefore, bracket_depth tracking is unnecessary in this
     approach — we just rely on the grammar constraints. */

  bool case_indent = valid_symbols[CASE_INDENT] && !valid_symbols[INDENT];
  bool can_indent = valid_symbols[INDENT] || case_indent;
  TokenType indent_token = case_indent ? CASE_INDENT : INDENT;

  /* ── EOF: emit phantom INDENT (for empty bodies) or DEDENT ──── */
  if (lexer->lookahead == 0) {
    if (can_indent && !valid_symbols[DEDENT]) {
      /* Empty body at end-of-file: `foo := interface:` with nothing after.
         Push a phantom indent level so the following DEDENT call can close it. */
      uint32_t prev = current_indent(s);
      if (s->stack_size < MAX_INDENT) {
        s->indent_stack[s->stack_size++] = prev + 1;
      }
      lexer->result_symbol = indent_token;
      return true;
    }
    if (valid_symbols[DEDENT] && s->stack_size > 1) {
      /* Close any remaining open indentation levels at EOF. */
      s->stack_size--;
      lexer->result_symbol = DEDENT;
      return true;
    }
    return false;
  }

  /* ── INDENT / DEDENT: only after a newline ────────────────────  */
  if (lexer->lookahead != '\n' && lexer->lookahead != '\r') {
    /* Not at a newline, so neither INDENT nor DEDENT is possible. A comment
       after horizontal space (`X := 1 <# note #>`) is still this scanner's
       token: the grammar's own lexer cannot produce it. */
    if (valid_symbols[BLOCK_COMMENT] && lexer->lookahead == '<') {
      return scan_comment_here(lexer);
    }
    return false;
  }

  if (!can_indent && !valid_symbols[DEDENT]) {
    /* No indentation decision is pending (file scope), so a comment on a
       later line can be scanned directly; the skipped lines stay outside it. */
    if (valid_symbols[BLOCK_COMMENT]) {
      while (lexer->lookahead == '\n' || lexer->lookahead == '\r' ||
             lexer->lookahead == ' ' || lexer->lookahead == '\t') {
        skip(lexer);
      }
      if (lexer->lookahead == '<') return scan_comment_here(lexer);
    }
    return false;
  }

  /* Consume the newline(s). */
  while (lexer->lookahead == '\n' || lexer->lookahead == '\r') {
    skip(lexer);
  }

  /* Find the column of the next non-blank line. */
  uint32_t col = find_next_content_col(lexer, skip);

  /* ── EOF reached after consuming newlines/blanks/comments ─────── */
  if (col == UINT32_MAX || lexer->lookahead == 0) {
    uint32_t prev = current_indent(s);
    /* Prefer closing open bodies at EOF over opening new phantom bodies.
       Only emit phantom INDENT if DEDENT is not also valid (i.e., we're
       strictly waiting for an INDENT to start an empty body). */
    if (can_indent && !valid_symbols[DEDENT]) {
      if (s->stack_size < MAX_INDENT) {
        s->indent_stack[s->stack_size++] = prev + 1;
      }
      lexer->result_symbol = indent_token;
      return true;
    }
    if (valid_symbols[DEDENT] && s->stack_size > 1) {
      s->stack_size--;
      lexer->result_symbol = DEDENT;
      return true;
    }
    return false;
  }

  uint32_t prev = current_indent(s);

  if (col > prev && can_indent) {
    /* col > stack-top.  But is it a genuine child, or a sibling that
       only appears deeper because the indent-stack was popped past the
       opener's column?
       Example:
         component := class:   # col 0, body at col 8
           ...
         scene_event := interface:  # col 4 (sibling, NOT stack-tracked)
         entity := class:           # col 4 — should be a sibling of
                                    # scene_event, NOT a child of its
                                    # empty interface body.
       After the DEDENT from the component body (col 8→0) we set
       last_stmt_col = 4 (the sibling column we landed on).  When
       scene_event's interface needs INDENT, prev=0 but
       last_stmt_col=4, so col 4 > prev but col <= last_stmt_col →
       phantom INDENT. */
    if (col <= s->last_stmt_col) {
      /* Phantom INDENT: the "child" is actually a sibling. */
      if (s->stack_size < MAX_INDENT) {
        s->indent_stack[s->stack_size++] = prev + 1;
      }
      s->pending_dedents = 1;
      lexer->result_symbol = indent_token;
      return true;
    }
    /* Genuine increase in indentation. */
    if (s->stack_size < MAX_INDENT) {
      bool plain_case = case_indent && col == lexer->get_column(lexer) &&
                        lexer->lookahead != '#' && lexer->lookahead != '<';
      s->indent_stack[s->stack_size++] = col | (plain_case ? CASE_FRAME : 0);
    }
    lexer->result_symbol = indent_token;
    return true;
  }

  /* Empty body: grammar expects INDENT but next non-blank content is at
     the same indentation level (or lower). This happens with abstract/native
     classes that have no members (e.g. `abstract_class<native>:` immediately
     followed by a sibling declaration at the same level).
     Emit a phantom INDENT by pushing (prev+1) and schedule an immediate
     DEDENT via pending_dedents=1. This produces an empty block and prevents
     the next sibling from being erroneously nested inside this empty body.
     After INDENT is consumed, the scanner is called again at the same lexer
     position (still pointing at the sibling's first character, not a newline).
     Without pending_dedents, the `lookahead != '\n'` guard would return false
     and DEDENT would never fire, causing infinite nesting. */
  if (col <= prev && can_indent) {
    uint32_t outer_dedents = dedents_to(s, lexer, col);
    if (s->stack_size < MAX_INDENT) {
      s->indent_stack[s->stack_size++] = prev + 1;
    }
    /* The newline has already been consumed. Queue the caller's dedents
       too, before the scanner reaches the next statement at column col. */
    s->pending_dedents = 1 + outer_dedents;
    if (col < prev) s->last_stmt_col = col;
    lexer->result_symbol = indent_token;
    return true;
  }

  if (col < prev) {
    /* Decrease in indentation: may need multiple DEDENTs.
       Record where we landed so future phantom-INDENT detection works. */
    s->last_stmt_col = col;
    uint32_t dedents = dedents_to(s, lexer, col);
    if (dedents > 0 && valid_symbols[DEDENT]) {
      s->pending_dedents = dedents - 1;
      s->stack_size--;
      lexer->result_symbol = DEDENT;
      return true;
    }
  }

  /* No indentation change: the next content line stays in this block. When
     it starts with a comment, scan it here, after the skipped newline and
     indentation, instead of leaving `<#` to the grammar's lexer. */
  if (valid_symbols[BLOCK_COMMENT] && lexer->lookahead == '<') {
    return scan_comment_here(lexer);
  }
  return false;
}

/* ─────────────────────────────────────────────────────────────────
 * Tree-sitter interface
 * ───────────────────────────────────────────────────────────────── */

void *tree_sitter_verse_external_scanner_create(void) {
  Scanner *s = calloc(1, sizeof(Scanner));
  s->indent_stack[0] = 0;
  s->stack_size = 1;
  return s;
}

void tree_sitter_verse_external_scanner_destroy(void *payload) {
  free(payload);
}

bool tree_sitter_verse_external_scanner_scan(
    void *payload, TSLexer *lexer, const bool *valid_symbols) {
  Scanner *s = (Scanner *)payload;
  return scan(s, lexer, valid_symbols);
}

unsigned tree_sitter_verse_external_scanner_serialize(void *payload,
                                                       char *buffer) {
  Scanner *s = (Scanner *)payload;
  unsigned n = 0;
  buffer[n++] = (char)s->pending_dedents;
  memcpy(buffer + n, &s->stack_size, sizeof(uint32_t));
  n += sizeof(uint32_t);
  unsigned copy = s->stack_size < MAX_INDENT ? s->stack_size : MAX_INDENT;
  memcpy(buffer + n, s->indent_stack, copy * sizeof(uint32_t));
  n += copy * sizeof(uint32_t);
  memcpy(buffer + n, &s->last_stmt_col, sizeof(uint32_t));
  n += sizeof(uint32_t);
  return n;
}

void tree_sitter_verse_external_scanner_deserialize(void *payload,
                                                     const char *buffer,
                                                     unsigned length) {
  Scanner *s = (Scanner *)payload;
  if (length == 0) return;
  unsigned n = 0;
  s->pending_dedents = (uint8_t)buffer[n++];
  if (n + sizeof(uint32_t) > length) return;
  memcpy(&s->stack_size, buffer + n, sizeof(uint32_t));
  n += sizeof(uint32_t);
  unsigned copy = s->stack_size < MAX_INDENT ? s->stack_size : MAX_INDENT;
  if (n + copy * sizeof(uint32_t) > length) return;
  memcpy(s->indent_stack, buffer + n, copy * sizeof(uint32_t));
  n += copy * sizeof(uint32_t);
  if (n + sizeof(uint32_t) <= length) {
    memcpy(&s->last_stmt_col, buffer + n, sizeof(uint32_t));
  }
}
