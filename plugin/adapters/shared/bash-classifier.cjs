// Read-only shell command classifier for the D21 delegation gate.
//
// `classifyBash(cmd)` answers one question: can the main session run this
// command without delegating, because it neither writes files nor executes
// code chosen at run time? The answer is fail-closed: anything the lexer or a
// command rule cannot prove read-only is reported as mutating, together with
// the reason and the chain segment that failed.
//
// Pipeline:
//   1. `lex` — a POSIX-ish lexer. It resolves quoting and escapes, splits
//      chains on `;` `&&` `||` `|` `|&` newline and carriage return, strips
//      comments, and parses redirects. It rejects command / process
//      substitution, heredocs, background `&`, subshells, brace groups and
//      `case` syntax outright — their effect cannot be judged statically.
//   2. `classifySimple` — judges one simple command: leading assignments
//      (execution-altering variables are rejected), wrapper commands
//      (`command`, `env`, `nice`, `timeout`, `time`, `stdbuf`, `xargs`,
//      `builtin`) recurse into the wrapped command, and every other command
//      is looked up in `COMMANDS`, the single per-command rule table.
//
// A command name alone never whitelists anything: each table entry states
// which arguments keep that command read-only.

'use strict';

const OK = Object.freeze({ ok: true });
function no(reason) {
  return { ok: false, reason };
}

// ────────────────────────────────────────────────────────────────────────────
// Lexer

const NAME_RE = /^[A-Za-z_][A-Za-z0-9_]*$/;
const ASSIGN_RE = /^([A-Za-z_][A-Za-z0-9_]*)\+?=/;
const REDIRECT_RE = /^(\d+|&(?=>))?(>>|>\||>&|<<<|<<-|<<|<>|<&|>|<)/;

class LexError extends Error {}

// Reserved words recognised in command position. Each opens, continues, or
// closes a construct on the lexer stack; the commands between them are judged
// as ordinary segments, so a compound command is read-only iff every part is.
const COMPOUND_OPENERS = {
  if: { push: 'if' },
  then: { requires: 'if' },
  elif: { requires: 'if' },
  else: { requires: 'if' },
  fi: { requires: 'if', pop: true },
  while: { push: 'loop' },
  until: { push: 'loop' },
  for: { push: 'loop' },
  do: { requires: 'loop' },
  done: { requires: 'loop', pop: true },
  '{': { push: '{' },
  '}': { requires: '{', pop: true },
};

// Returns { segments: [{ words, text }] } or { error, segment }.
// A word is { value, raw, quoted, dynamic, glob }: `value` is the text after
// quote removal (parameter expansions kept verbatim), `dynamic` marks a `$…`
// expansion whose run-time value is unknown.
function lex(src) {
  const segments = [];
  let cur = { words: [], start: 0 };
  let i = 0;
  const n = src.length;
  let atCmdStart = true;
  let dbracket = false;
  // Open compound constructs: '(' subshell, '{' brace group, 'if', 'loop'.
  const stack = [];
  // Set after `)` / `}` / `fi` / `done`: only an operator or redirect may follow.
  let afterClose = false;

  const endSegment = (at, next) => {
    if (cur.words.length > 0) segments.push({ words: cur.words, text: src.slice(cur.start, at).trim() });
    cur = { words: [], start: next };
    atCmdStart = true;
  };
  const endChain = (at, next) => {
    endSegment(at, next);
    afterClose = false;
  };
  const fail = (reason) => {
    const lineEnd = src.slice(i).search(/[;&|\n\r]/);
    const text = src.slice(cur.start, lineEnd < 0 ? n : i + lineEnd).trim();
    return { error: reason, segment: text || src.trim() };
  };

  function readDollar(word) {
    // src[i] === '$'
    const nx = src[i + 1];
    if (nx === '(') throw new LexError('command substitution `$(…)`');
    if (nx === '{') {
      const close = src.indexOf('}', i + 2);
      if (close < 0) throw new LexError('unterminated `${…}` expansion');
      const body = src.slice(i, close + 1);
      if (body.includes('$(') || body.includes('`')) throw new LexError('command substitution inside `${…}`');
      word.value += body;
      word.dynamic = true;
      i = close + 1;
      return;
    }
    if (nx !== undefined && /[A-Za-z_]/.test(nx)) {
      let j = i + 1;
      while (j < n && /[A-Za-z0-9_]/.test(src[j])) j += 1;
      word.value += src.slice(i, j);
      word.dynamic = true;
      i = j;
      return;
    }
    if (nx !== undefined && /[0-9@*#?$!-]/.test(nx)) {
      word.value += src.slice(i, i + 2);
      word.dynamic = true;
      i += 2;
      return;
    }
    word.value += '$';
    i += 1;
  }

  function readDoubleQuoted(word) {
    // src[i] is just past the opening quote.
    word.quoted = true;
    for (;;) {
      if (i >= n) throw new LexError('unbalanced double quote');
      const d = src[i];
      if (d === '"') {
        i += 1;
        return;
      }
      if (d === '\\' && i + 1 < n && '$`"\\\n'.includes(src[i + 1])) {
        if (src[i + 1] !== '\n') word.value += src[i + 1];
        i += 2;
        continue;
      }
      if (d === '`') throw new LexError('command substitution (backtick)');
      if (d === '$') {
        readDollar(word);
        continue;
      }
      word.value += d;
      i += 1;
    }
  }

  function readWord() {
    const start = i;
    const word = { value: '', raw: '', quoted: false, dynamic: false, glob: false };
    while (i < n) {
      const c = src[i];
      if (' \t\n\r;|&<>()'.includes(c)) break;
      if (c === '\\') {
        if (i + 1 >= n) {
          word.value += '\\';
          i += 1;
        } else if (src[i + 1] === '\n') {
          i += 2;
        } else {
          word.value += src[i + 1];
          word.quoted = true;
          i += 2;
        }
        continue;
      }
      if (c === "'") {
        const close = src.indexOf("'", i + 1);
        if (close < 0) throw new LexError('unbalanced single quote');
        word.value += src.slice(i + 1, close);
        word.quoted = true;
        i = close + 1;
        continue;
      }
      if (c === '"') {
        i += 1;
        readDoubleQuoted(word);
        continue;
      }
      if (c === '`') throw new LexError('command substitution (backtick)');
      if (c === '$') {
        if (src[i + 1] === "'") {
          // ANSI-C quoting: literal text with backslash escapes.
          let j = i + 2;
          while (j < n && src[j] !== "'") j += src[j] === '\\' ? 2 : 1;
          if (j >= n) throw new LexError('unbalanced $\'…\' quote');
          word.value += src.slice(i + 2, j);
          word.quoted = true;
          i = j + 1;
          continue;
        }
        if (src[i + 1] === '"') {
          i += 2;
          readDoubleQuoted(word);
          continue;
        }
        readDollar(word);
        continue;
      }
      if ('*?['.includes(c)) word.glob = true;
      word.value += c;
      i += 1;
    }
    word.raw = src.slice(start, i);
    return word;
  }

  try {
    while (i < n) {
      const c = src[i];
      if (c === ' ' || c === '\t') {
        i += 1;
        continue;
      }
      if (c === '\\' && src[i + 1] === '\n') {
        i += 2;
        continue;
      }
      if (c === '\n' || c === '\r') {
        if (dbracket) return fail('unterminated `[[`');
        endChain(i, i + 1);
        i += 1;
        continue;
      }
      if (c === '#') {
        // A word-initial `#` starts a comment the shell never executes.
        while (i < n && src[i] !== '\n' && src[i] !== '\r') i += 1;
        continue;
      }
      if (dbracket) {
        // Inside `[[ … ]]` these are test operators, not shell operators.
        const two = src.slice(i, i + 2);
        if (two === '&&' || two === '||') {
          cur.words.push({ value: two, raw: two, quoted: false, dynamic: false, glob: false });
          i += 2;
          continue;
        }
        if ('()<>'.includes(c)) {
          cur.words.push({ value: c, raw: c, quoted: false, dynamic: false, glob: false });
          i += 1;
          continue;
        }
      } else {
        const two = src.slice(i, i + 2);
        if (two === '&&' || two === '||') {
          endChain(i, i + 2);
          i += 2;
          continue;
        }
        if (two === ';;' || two === ';&') return fail('`case` statement syntax');
        if (c === ';') {
          endChain(i, i + 1);
          i += 1;
          continue;
        }
        if (two === '|&') {
          endChain(i, i + 2);
          i += 2;
          continue;
        }
        if (c === '|') {
          endChain(i, i + 1);
          i += 1;
          continue;
        }
        const rm = REDIRECT_RE.exec(src.slice(i));
        if (rm) {
          const op = rm[2];
          i += rm[0].length;
          if (op === '<<' || op === '<<-') return fail('heredoc');
          if (op === '<>') return fail('read-write redirect `<>`');
          if ((op === '<' || op === '>') && src[i] === '(') return fail('process substitution');
          while (src[i] === ' ' || src[i] === '\t') i += 1;
          if (i >= n || ' \t\n\r;|&<>()'.includes(src[i])) return fail(`redirect \`${op}\` without a target`);
          const target = readWord();
          if (op === '<<<') continue; // here-string feeds a literal to stdin
          if ((op === '>&' || op === '<&') && !target.dynamic && /^(\d+-?|-)$/.test(target.value)) continue;
          if (!target.dynamic && target.value === '/dev/null') continue;
          return fail(`redirect \`${op}\` to \`${target.raw}\` (only /dev/null and fd duplication are read-only)`);
        }
        if (c === '&') return fail('background execution `&`');
        if (c === '(') {
          if (src[i + 1] === '(') return fail('arithmetic command `((…))`');
          // A subshell opens only in command position, separated from any
          // preceding word — `a=(…)` arrays and `f() {…}` definitions fail.
          const prev = i === 0 ? '' : src[i - 1];
          if (!atCmdStart || (prev && !/[\s;&|(!]/.test(prev))) return fail('parentheses outside command position');
          stack.push('(');
          endSegment(i, i + 1);
          i += 1;
          continue;
        }
        if (c === ')') {
          if (stack[stack.length - 1] !== '(') return fail('unbalanced `)`');
          stack.pop();
          endSegment(i, i + 1);
          afterClose = true;
          i += 1;
          continue;
        }
      }
      const word = readWord();
      if (afterClose) return fail(`unexpected word \`${word.raw}\` after a compound command`);
      if (!dbracket && atCmdStart && !word.quoted) {
        const kw = COMPOUND_OPENERS[word.value];
        if (kw) {
          const top = stack[stack.length - 1];
          if (kw.requires && top !== kw.requires) return fail(`\`${word.value}\` without a matching opener`);
          if (kw.pop) stack.pop();
          if (kw.push) stack.push(kw.push);
          if (word.value === 'for') {
            // The loop header stays a segment so its variable name is checked.
            cur.words.push(word);
            atCmdStart = false;
            continue;
          }
          endSegment(i, i);
          if (kw.pop) afterClose = true;
          continue;
        }
        if (word.value === '[[') dbracket = true;
      } else if (dbracket && !word.quoted && word.value === ']]') {
        dbracket = false;
      }
      cur.words.push(word);
      const keepsStart = !word.quoted && (word.value === '!' || ASSIGN_RE.test(word.raw));
      atCmdStart = atCmdStart && keepsStart;
    }
  } catch (e) {
    if (e instanceof LexError) return fail(e.message);
    throw e;
  }
  if (dbracket) return fail('unterminated `[[`');
  if (stack.length > 0) return fail(`unterminated compound command (open: ${stack.join(', ')})`);
  endSegment(n, n);
  return { segments };
}

// ────────────────────────────────────────────────────────────────────────────
// Environment assignments

// Variables that change WHAT a command executes (pager, loader, shell init,
// git hooks/config, interpreter options) or how later words are split.
const DANGEROUS_ENV_RE =
  /^(PAGER|MANPAGER|GH_PAGER|GIT_.*|LD_.*|DYLD_.*|BASH_ENV|ENV|PS4|PROMPT_COMMAND|NODE_OPTIONS|IFS|SHELLOPTS|BASHOPTS|LESSOPEN|LESSCLOSE|BROWSER|GH_BROWSER|EDITOR|VISUAL|PERL5OPT|RUBYOPT|PYTHONSTARTUP|BASH_FUNC_.*)$/;
// A pager variable set to one of these literals cannot run anything new.
const PAGER_VARS = new Set(['PAGER', 'MANPAGER', 'GH_PAGER', 'GIT_PAGER']);
const SAFE_PAGER_VALUES = new Set(['', 'cat', 'less', 'more']);
const SDI_NAMES = new Set(['sdi', 'sdid']);

function parseAssignment(word) {
  const m = ASSIGN_RE.exec(word.raw);
  const name = m[1];
  const eq = word.value.indexOf('=');
  return { name, value: word.value.slice(eq + 1), dynamic: word.dynamic };
}

// `command` is the command the assignment prefixes (null when standalone or
// exported — then it affects every later segment of the chain).
function checkAssignment(a, command, ctx) {
  if (DANGEROUS_ENV_RE.test(a.name)) {
    if (PAGER_VARS.has(a.name) && !a.dynamic && SAFE_PAGER_VALUES.has(a.value)) return OK;
    return no(`\`${a.name}=\` changes what the command executes`);
  }
  if (a.name === 'PATH') {
    // The bundled-binary idiom: `PATH=<plugin>/bin:$PATH sdi …` or
    // `export PATH=…; sdi …`. Any other command after a PATH change could
    // resolve to an arbitrary binary.
    if (command === null) {
      ctx.pathModified = true;
      return OK;
    }
    if (SDI_NAMES.has(command)) return OK;
    return no('`PATH=` prefix changes which binary runs (allowed only for `sdi`)');
  }
  return OK;
}

// ────────────────────────────────────────────────────────────────────────────
// Option scanning

// Splits `args` into options and positionals. `shortArgs` lists short flags
// that take a value (attached `-kVAL` or separate `-k VAL`); `longArgs` lists
// long flags that take a separate value when no `=` is attached.
function scanOptions(args, shortArgs = '', longArgs = []) {
  const opts = [];
  const positionals = [];
  let k = 0;
  for (; k < args.length; k++) {
    const w = args[k];
    const v = w.value;
    if (v === '--') {
      k += 1;
      break;
    }
    if (v.startsWith('--') && v.length > 2) {
      const eq = v.indexOf('=');
      const name = eq < 0 ? v : v.slice(0, eq);
      let value = eq < 0 ? null : v.slice(eq + 1);
      if (value === null && longArgs.includes(name) && k + 1 < args.length) {
        k += 1;
        value = args[k].value;
      }
      opts.push({ name, value, word: w });
      continue;
    }
    if (v.startsWith('-') && v.length > 1) {
      for (let j = 1; j < v.length; j++) {
        const ch = v[j];
        if (shortArgs.includes(ch)) {
          let value = v.slice(j + 1);
          if (!value && k + 1 < args.length) {
            k += 1;
            value = args[k].value;
          }
          opts.push({ name: `-${ch}`, value, word: w });
          break;
        }
        opts.push({ name: `-${ch}`, value: null, word: w });
      }
      continue;
    }
    positionals.push(w);
  }
  for (; k < args.length; k++) positionals.push(args[k]);
  return { opts, positionals };
}

function firstDynamic(args) {
  const d = args.find((w) => w.dynamic);
  return d ? no(`argument \`${d.raw}\` is a variable expansion; its value cannot be checked`) : null;
}

// For commands whose danger lives in OPTIONS (not in a script argument): an
// expansion behind a literal prefix (`"apps/$app/src"`) can never become an
// option, so only words whose value starts with `$` or `-` are rejected.
function firstOptionDynamic(args) {
  const d = args.find((w) => w.dynamic && /^[-$]/.test(w.value));
  return d ? no(`argument \`${d.raw}\` is a variable expansion that could be an option`) : null;
}

// ────────────────────────────────────────────────────────────────────────────
// sed — the script must be print-only.

function sedScriptVerdict(script) {
  let i = 0;
  const n = script.length;
  const skipSpaces = () => {
    while (i < n && (script[i] === ' ' || script[i] === '\t')) i += 1;
  };
  const readDelimited = (delim) => {
    // Reads up to the next unescaped `delim`; returns false when unterminated.
    while (i < n) {
      const c = script[i];
      if (c === '\\') {
        i += 2;
        continue;
      }
      if (c === delim) {
        i += 1;
        return true;
      }
      if (c === '\n') return false;
      i += 1;
    }
    return false;
  };
  const readAddress = () => {
    const c = script[i];
    if (c === undefined) return true;
    if (/[0-9]/.test(c)) {
      while (i < n && /[0-9]/.test(script[i])) i += 1;
      if (script[i] === '~') {
        i += 1;
        while (i < n && /[0-9]/.test(script[i])) i += 1;
      }
      return true;
    }
    if (c === '$') {
      i += 1;
      return true;
    }
    if (c === '/' || c === '\\') {
      let delim = '/';
      if (c === '\\') {
        delim = script[i + 1];
        if (!delim || delim === '\n' || delim === '\\') return false;
        i += 2;
      } else {
        i += 1;
      }
      if (!readDelimited(delim)) return false;
      while (i < n && /[IM]/.test(script[i])) i += 1;
      return true;
    }
    return true;
  };
  const skipToLineEnd = () => {
    while (i < n && script[i] !== '\n') {
      if (script[i] === '\\' && script[i + 1] === '\n') i += 2;
      else i += 1;
    }
  };
  const readLabel = () => {
    while (i < n && script[i] !== '\n' && script[i] !== ';' && script[i] !== '}') i += 1;
  };

  while (i < n) {
    while (i < n && /[\s;]/.test(script[i])) i += 1;
    if (i >= n) break;
    const c0 = script[i];
    if (c0 === '#') {
      skipToLineEnd();
      continue;
    }
    if (c0 === '}') {
      i += 1;
      continue;
    }
    if (!readAddress()) return no('sed address regex is not terminated');
    if (script[i] === ',') {
      i += 1;
      if (script[i] === '+' || script[i] === '~') {
        i += 1;
        while (i < n && /[0-9]/.test(script[i])) i += 1;
      } else if (!readAddress()) {
        return no('sed address regex is not terminated');
      }
    }
    skipSpaces();
    while (script[i] === '!') {
      i += 1;
      skipSpaces();
    }
    const cmd = script[i];
    i += 1;
    switch (cmd) {
      case '{':
      case 'p':
      case 'P':
      case '=':
      case 'd':
      case 'D':
      case 'n':
      case 'N':
      case 'g':
      case 'G':
      case 'h':
      case 'H':
      case 'x':
      case 'z':
      case 'F':
        break;
      case 'l':
      case 'L':
      case 'q':
      case 'Q':
        skipSpaces();
        while (i < n && /[0-9]/.test(script[i])) i += 1;
        break;
      case ':':
      case 'b':
      case 't':
      case 'T':
      case 'v':
        readLabel();
        break;
      case 'a':
      case 'i':
      case 'c':
        // Text commands only print; consume the (possibly continued) text.
        skipToLineEnd();
        break;
      case 's':
      case 'y': {
        const delim = script[i];
        if (!delim || delim === '\n' || delim === '\\') return no(`sed \`${cmd}\` command has an invalid delimiter`);
        i += 1;
        if (!readDelimited(delim) || !readDelimited(delim)) return no(`sed \`${cmd}\` command is not terminated`);
        if (cmd === 's') {
          while (i < n && !/[\s;}#]/.test(script[i])) {
            const f = script[i];
            if (f === 'w' || f === 'W') return no('sed `s///w` flag writes a file');
            if (f === 'e') return no('sed `s///e` flag executes the pattern space as a command');
            if (!/[gpiImM0-9]/.test(f)) return no(`sed \`s\` flag \`${f}\` is not recognised`);
            i += 1;
          }
        }
        break;
      }
      case 'w':
      case 'W':
        return no(`sed \`${cmd}\` command writes a file`);
      case 'r':
      case 'R':
        return no(`sed \`${cmd}\` command reads a file into the output`);
      case 'e':
        return no('sed `e` command executes a shell command');
      default:
        return no(`sed command \`${cmd === undefined ? '<end>' : cmd}\` is not a known print-only command`);
    }
  }
  return OK;
}

function sedVerdict(args) {
  const dyn = firstDynamic(args);
  if (dyn) return dyn;
  const scripts = [];
  const files = [];
  for (let k = 0; k < args.length; k++) {
    const v = args[k].value;
    if (v === '--') {
      files.push(...args.slice(k + 1));
      break;
    }
    if (v.startsWith('--') && v.length > 2) {
      const [name, value] = v.includes('=') ? [v.slice(0, v.indexOf('=')), v.slice(v.indexOf('=') + 1)] : [v, null];
      if (name === '--in-place') return no('`sed --in-place` edits files');
      if (name === '--file') return no('`sed --file` script content cannot be inspected');
      if (name === '--expression') {
        if (value !== null) scripts.push(value);
        else if (k + 1 < args.length) scripts.push(args[++k].value);
        continue;
      }
      if (name === '--line-length') {
        if (value === null) k += 1;
        continue;
      }
      if (['--quiet', '--silent', '--regexp-extended', '--null-data', '--separate', '--unbuffered', '--posix', '--debug', '--sandbox', '--zero-terminated'].includes(name)) continue;
      return no(`sed option \`${name}\` is not known to be read-only`);
    }
    if (v.startsWith('-') && v.length > 1) {
      for (let j = 1; j < v.length; j++) {
        const ch = v[j];
        if (ch === 'i' || ch === 'I') return no('`sed -i` edits files in place');
        if (ch === 'f') return no('`sed -f` script content cannot be inspected');
        if (ch === 'e' || ch === 'l') {
          const rest = v.slice(j + 1);
          const value = rest || (k + 1 < args.length ? args[++k].value : '');
          if (ch === 'e') scripts.push(value);
          break;
        }
        if (!'nErsuz'.includes(ch)) return no(`sed option \`-${ch}\` is not known to be read-only`);
      }
      continue;
    }
    files.push(args[k]);
  }
  if (scripts.length === 0) {
    if (files.length === 0) return no('sed without a script');
    scripts.push(files.shift().value);
  }
  return sedScriptVerdict(scripts.join('\n'));
}

// ────────────────────────────────────────────────────────────────────────────
// awk — the program must not spawn commands or redirect output.

const AWK_REGEX_PRECEDERS = new Set(['', '(', ',', '~', '!', '{', '}', ';', '&', '|', '=', '?', ':', '[', '\n', '+', '-', '*', '%', '<', '>']);

function maskAwkLiterals(prog) {
  let out = '';
  let prev = '';
  let i = 0;
  while (i < prog.length) {
    const c = prog[i];
    if (c === '"') {
      let j = i + 1;
      while (j < prog.length && prog[j] !== '"') j += prog[j] === '\\' ? 2 : 1;
      out += '""';
      i = j + 1;
      prev = '"';
      continue;
    }
    if (c === '/' && (AWK_REGEX_PRECEDERS.has(prev) || /\b(print|printf|return|in)$/.test(out.trimEnd()))) {
      let j = i + 1;
      while (j < prog.length && prog[j] !== '/' && prog[j] !== '\n') j += prog[j] === '\\' ? 2 : 1;
      out += '//';
      i = j + 1;
      prev = '/';
      continue;
    }
    if (c === '#') {
      while (i < prog.length && prog[i] !== '\n') i += 1;
      continue;
    }
    out += c;
    if (!/\s/.test(c) || c === '\n') prev = c;
    i += 1;
  }
  return out;
}

function awkProgramVerdict(prog) {
  const masked = maskAwkLiterals(prog);
  if (/\bsystem\s*\(/.test(masked)) return no('awk `system()` executes a shell command');
  if (/@(load|include)\b/.test(masked)) return no('awk `@load`/`@include` loads external code');
  if (masked.replace(/\|\|/g, '').includes('|')) return no('awk `|` pipes to or from a command');
  if (/\bprintf?\b[^;{}\n]*>/.test(masked)) return no('awk `print >` redirects output to a file');
  return OK;
}

function awkVerdict(args) {
  const dyn = firstDynamic(args);
  if (dyn) return dyn;
  const programs = [];
  const rest = [];
  for (let k = 0; k < args.length; k++) {
    const v = args[k].value;
    if (v === '--') {
      rest.push(...args.slice(k + 1));
      break;
    }
    if (rest.length > 0 || programs.length > 0 || !v.startsWith('-') || v === '-') {
      rest.push(args[k]);
      continue;
    }
    if (v === '-F' || v === '-v') {
      k += 1;
      continue;
    }
    if (v.startsWith('-F') || v.startsWith('-v')) continue;
    if (v === '-e' || v === '--source') {
      if (k + 1 < args.length) programs.push(args[++k].value);
      continue;
    }
    if (v.startsWith('--source=')) {
      programs.push(v.slice('--source='.length));
      continue;
    }
    if (v === '-i' || v.startsWith('--include') || v === '-l' || v.startsWith('--load')) return no('awk `-i`/`-l` loads external code (e.g. `-i inplace` edits files)');
    if (v === '-f' || v.startsWith('--file') || v === '-E' || v.startsWith('--exec')) return no('awk program file content cannot be inspected');
    if (/^-(o|p|d|D)/.test(v) || /^--(pretty-print|profile|dump-variables|debug)/.test(v)) return no(`awk option \`${v}\` writes a file`);
    if (v === '-W') return no('awk `-W` implementation options are not inspected');
    if (['-b', '-c', '-C', '-O', '-P', '-r', '-S', '-t', '-M', '-n', '-N', '-s', '--posix', '--sandbox', '--traditional', '--re-interval', '--characters-as-bytes', '--optimize', '--lint'].includes(v)) continue;
    return no(`awk option \`${v}\` is not known to be read-only`);
  }
  if (programs.length === 0) {
    if (rest.length === 0) return no('awk without a program');
    programs.push(rest.shift().value);
  }
  return awkProgramVerdict(programs.join('\n'));
}

// ────────────────────────────────────────────────────────────────────────────
// sqlite3 — only `-readonly`, only inspectable SQL, no file-touching dot
// commands or functions.

const SQLITE_VALUE_OPTS = new Set(['-cmd', '-separator', '-newline', '-nullvalue', '-vfs', '-mmap', '-maxsize', '-pagecache', '-lookaside', '-heap', '-escape']);
const SQLITE_FLAG_OPTS = new Set(['-readonly', '-batch', '-bail', '-echo', '-header', '-noheader', '-column', '-csv', '-html', '-line', '-list', '-box', '-json', '-markdown', '-table', '-tabs', '-ascii', '-quote', '-safe', '-stats', '-nofollow', '-version', '-help', '-zip', '-readonly']);
const SQLITE_DOT_RE = /(^|[\n;])\s*\.(shell|system|output|once|save|backup|import|load|excel|log|open|read|restore|clone|archive|ar|trace|iotrace|cd)\b/i;

function sqliteVerdict(args) {
  const dyn = firstDynamic(args);
  if (dyn) return dyn;
  let readonly = false;
  let safe = false;
  const texts = [];
  const positionals = [];
  for (let k = 0; k < args.length; k++) {
    const raw = args[k].value;
    if (raw.startsWith('-') && raw.length > 1) {
      const v = raw.startsWith('--') ? raw.slice(1) : raw;
      if (v === '-readonly') readonly = true;
      if (v === '-safe') safe = true;
      if (v === '-init') return no('sqlite3 `-init` runs a command file that cannot be inspected');
      if (v === '-cmd') {
        if (k + 1 < args.length) texts.push(args[++k].value);
        continue;
      }
      if (SQLITE_VALUE_OPTS.has(v)) {
        k += 1;
        continue;
      }
      if (SQLITE_FLAG_OPTS.has(v)) continue;
      return no(`sqlite3 option \`${raw}\` is not known to be read-only`);
    }
    positionals.push(raw);
  }
  if (!readonly) return no('sqlite3 is read-only only with `-readonly`');
  if (positionals.length < 2 && !safe) return no('sqlite3 without SQL reads commands from stdin, which cannot be inspected');
  texts.push(...positionals.slice(1));
  const sql = texts.join('\n');
  const dot = SQLITE_DOT_RE.exec(sql);
  if (dot) return no(`sqlite3 dot command \`.${dot[2]}\` touches files or runs commands`);
  if (/\battach\b/i.test(sql)) return no('sqlite3 `ATTACH` opens another database file');
  if (/\bvacuum\b[\s\S]*\binto\b/i.test(sql)) return no('sqlite3 `VACUUM INTO` writes a file');
  if (/\b(load_extension|writefile|edit)\s*\(/i.test(sql)) return no('sqlite3 function writes files or loads code');
  return OK;
}

// ────────────────────────────────────────────────────────────────────────────
// git

const GIT_READ_SUBCOMMANDS = new Set([
  'status', 'log', 'diff', 'show', 'rev-parse', 'describe', 'ls-files', 'ls-tree', 'ls-remote', 'blame',
  'annotate', 'shortlog', 'cat-file', 'rev-list', 'merge-base', 'grep', 'check-ignore', 'check-attr',
  'check-ref-format', 'show-ref', 'for-each-ref', 'name-rev', 'count-objects', 'whatchanged', 'range-diff',
  'cherry', 'diff-tree', 'diff-files', 'diff-index', 'version', 'help', 'verify-commit', 'verify-tag',
  'show-branch', 'var',
]);
const GIT_COSMETIC_CONFIG_RE = /^(color\.[A-Za-z0-9.-]+|core\.quotepath)=/i;

function gitBranchVerdict(args) {
  const { opts, positionals } = scanOptions(args, '', ['--contains', '--no-contains', '--merged', '--no-merged', '--points-at', '--sort', '--format']);
  let list = false;
  for (const o of opts) {
    if (o.name === '-l' || o.name === '--list') list = true;
    else if (['-a', '-r', '-v', '-q', '--all', '--remotes', '--verbose', '--quiet', '--show-current', '--color', '--no-color', '--column', '--no-column', '--abbrev', '--no-abbrev', '-i', '--ignore-case', '--omit-empty'].includes(o.name)) continue;
    else if (['--contains', '--no-contains', '--merged', '--no-merged', '--points-at', '--sort', '--format'].includes(o.name)) list = true;
    else return no(`\`git branch ${o.name}\` modifies branches`);
  }
  if (positionals.length > 0 && !list) return no('`git branch <name>` creates a branch');
  return OK;
}

function gitTagVerdict(args) {
  const { opts, positionals } = scanOptions(args, '', ['--contains', '--no-contains', '--merged', '--no-merged', '--points-at', '--sort', '--format']);
  let list = false;
  for (const o of opts) {
    if (o.name === '-l' || o.name === '--list' || o.name === '-v' || o.name === '--verify') list = true;
    else if (/^-n\d*$/.test(o.word.value) || o.name === '-n') list = true;
    else if (['--contains', '--no-contains', '--merged', '--no-merged', '--points-at', '--sort', '--format', '--column', '--no-column', '--color', '-i', '--ignore-case', '--omit-empty'].includes(o.name)) list = true;
    else if (/^-\d$/.test(o.name)) continue;
    else return no(`\`git tag ${o.word.value}\` modifies tags`);
  }
  if (positionals.length > 0 && !list) return no('`git tag <name>` creates a tag');
  return OK;
}

function gitRemoteVerdict(args) {
  const vals = args.map((w) => w.value);
  if (vals.length === 0) return OK;
  if (vals.every((v) => v === '-v' || v === '--verbose')) return OK;
  if (vals[0] === 'show' || vals[0] === 'get-url') return OK;
  return no(`\`git remote ${vals[0]}\` modifies remotes`);
}

function gitConfigVerdict(args) {
  const first = args[0] ? args[0].value : '';
  if (first === 'get' || first === 'list') return OK;
  if (['set', 'unset', 'rename-section', 'remove-section', 'edit'].includes(first)) return no(`\`git config ${first}\` writes configuration`);
  const { opts, positionals } = scanOptions(args, 'f', ['--file', '--blob', '--default', '--type', '--get', '--get-all', '--get-regexp', '--get-urlmatch', '--get-color', '--get-colorbool']);
  let get = false;
  for (const o of opts) {
    if (['--get', '--get-all', '--get-regexp', '--get-urlmatch', '--get-color', '--get-colorbool', '-l', '--list'].includes(o.name)) get = true;
    else if (['--global', '--local', '--system', '--worktree', '--file', '-f', '--blob', '--show-origin', '--show-scope', '--name-only', '--null', '-z', '--type', '--bool', '--int', '--bool-or-int', '--path', '--expiry-date', '--default', '--includes', '--no-includes', '--no-type'].includes(o.name)) continue;
    else return no(`\`git config ${o.name}\` writes configuration`);
  }
  const fromGetOpt = opts.some((o) => o.name.startsWith('--get') && o.value !== null);
  if (get || fromGetOpt) return OK;
  if (positionals.length <= 1) return OK; // `git config <key>` reads
  return no('`git config <key> <value>` writes configuration');
}

const GIT_SUBCOMMAND_RULES = {
  branch: gitBranchVerdict,
  tag: gitTagVerdict,
  remote: gitRemoteVerdict,
  config: gitConfigVerdict,
  stash: (a) => (a[0] && ['list', 'show'].includes(a[0].value) ? OK : no('`git stash` without list/show changes the stash')),
  worktree: (a) => (a[0] && a[0].value === 'list' ? OK : no('`git worktree` without list changes worktrees')),
  submodule: (a) => (a[0] && ['status', 'summary'].includes(a[0].value) ? OK : no('`git submodule` without status/summary changes submodules')),
  notes: (a) => (a.length === 0 || ['list', 'show'].includes(a[0].value) ? OK : no('`git notes` without list/show changes notes')),
  reflog: (a) => (a.length === 0 || ['show', 'list', 'exists'].includes(a[0].value) || a[0].value.startsWith('-') ? OK : no(`\`git reflog ${a[0].value}\` rewrites the reflog`)),
  'hash-object': (a) => (a.some((w) => w.value === '-w' || w.value === '--stdin-paths' || /^-[a-z]*w/.test(w.value)) ? no('`git hash-object -w` writes an object') : OK),
  'symbolic-ref': (a) => (a.filter((w) => !w.value.startsWith('-')).length <= 1 ? OK : no('`git symbolic-ref <name> <ref>` writes a ref')),
};

function gitVerdict(args) {
  let k = 0;
  while (k < args.length) {
    const w = args[k];
    const v = w.value;
    if (v === '-C') {
      k += 2; // the path may be a variable — it only selects the repository
      continue;
    }
    if (w.dynamic) return no(`git argument \`${w.raw}\` is a variable expansion; its value cannot be checked`);
    if (v === '-c') {
      const kv = args[k + 1];
      if (!kv || kv.dynamic || !GIT_COSMETIC_CONFIG_RE.test(kv.value)) {
        return no(`\`git -c ${kv ? kv.raw : ''}\` may change what git executes (only color.* and core.quotepath are allowed)`);
      }
      k += 2;
      continue;
    }
    if (v === '--config-env' || v.startsWith('--config-env=')) return no('`git --config-env` injects configuration');
    if (v.startsWith('--exec-path=')) return no('`git --exec-path=` changes which helpers git runs');
    if (v === '--git-dir' || v === '--work-tree' || v === '--namespace') {
      k += 2;
      continue;
    }
    if (/^--(git-dir|work-tree|namespace)=/.test(v)) {
      k += 1;
      continue;
    }
    if (['--no-pager', '-P', '-p', '--paginate', '--no-replace-objects', '--literal-pathspecs', '--glob-pathspecs', '--noglob-pathspecs', '--icase-pathspecs', '--no-optional-locks', '--bare', '--no-advice', '--exec-path', '--version', '--help', '-h'].includes(v)) {
      k += 1;
      continue;
    }
    if (v.startsWith('-')) return no(`git global option \`${v}\` is not known to be read-only`);
    break;
  }
  if (k >= args.length) return OK;
  const subWord = args[k];
  const sub = subWord.value;
  const rest = args.slice(k + 1);
  const dyn = firstOptionDynamic(rest);
  if (dyn) return dyn;
  for (const w of rest) {
    const v = w.value;
    if (v === '--output' || v.startsWith('--output=')) return no(`\`git ${sub} --output\` writes a file`);
    if (v === '--ext-diff') return no(`\`git ${sub} --ext-diff\` runs an external diff program`);
  }
  if (sub === 'grep' && rest.some((w) => /^(-O|--open-files-in-pager)/.test(w.value))) {
    return no('`git grep -O` opens files in an external program');
  }
  if (GIT_READ_SUBCOMMANDS.has(sub)) return OK;
  const rule = GIT_SUBCOMMAND_RULES[sub];
  if (rule) return rule(rest);
  return no(`\`git ${sub}\` is not a read-only git subcommand`);
}

// ────────────────────────────────────────────────────────────────────────────
// gh

const GH_READ_ACTIONS = new Set(['list', 'view', 'status', 'diff', 'checks', 'ls', 'watch']);
const GH_ACTION_GROUPS = new Set(['repo', 'issue', 'pr', 'run', 'release', 'workflow', 'label', 'gist', 'cache', 'org', 'secret', 'variable', 'ruleset']);

function ghApiVerdict(args) {
  const dyn = firstDynamic(args);
  if (dyn) return dyn;
  for (let k = 0; k < args.length; k++) {
    const v = args[k].value;
    let method = null;
    if (v === '-X' || v === '--method') method = args[k + 1] ? args[k + 1].value : '';
    else if (v.startsWith('--method=')) method = v.slice('--method='.length);
    else if (/^-X./.test(v)) method = v.slice(2);
    if (method !== null && !/^(GET|HEAD)$/i.test(method)) return no(`\`gh api -X ${method}\` is not a GET request`);
    if (/^(-f|-F)/.test(v) || /^--(field|raw-field|input)(=|$)/.test(v)) {
      return no(`\`gh api ${v}\` sends a request body (gh defaults to POST)`);
    }
  }
  return OK;
}

function ghVerdict(args) {
  if (args.length === 0) return OK;
  if (args[0].dynamic || (args[1] && args[1].dynamic)) return no('gh subcommand is a variable expansion');
  const sub = args[0].value;
  const action = args[1] ? args[1].value : '';
  if (['--version', 'version', '--help', 'help', '-h', 'status', 'browse', 'search'].includes(sub)) return OK;
  if (sub === 'auth') return /^(status|token)$/.test(action) ? OK : no(`\`gh auth ${action}\` changes authentication`);
  if (sub === 'api') return ghApiVerdict(args.slice(1));
  if (GH_ACTION_GROUPS.has(sub)) {
    if (GH_READ_ACTIONS.has(action) || action === '--help' || action === '-h') return OK;
    if (action === 'download') return no(`\`gh ${sub} download\` writes files`);
    return no(`\`gh ${sub} ${action}\` is not a read-only gh action`);
  }
  return no(`\`gh ${sub}\` is not a read-only gh command`);
}

// ────────────────────────────────────────────────────────────────────────────
// sdi — orchestration authoring belongs to the main session; task lifecycle
// mutation is execution work (D3) and delegates to a specialist.

const SDI_TASK_READ_ACTIONS = new Set(['list', 'view', 'brief', 'stats', 'ancestors', 'descendants', 'subtree', 'relations', 'lease-info', 'preflight', 'help']);

function sdiVerdict(args) {
  if (args.some((w) => !w.dynamic && (w.value === '--help' || w.value === '-h'))) return OK;
  if (args[0] && args[0].dynamic) return no('sdi subcommand is a variable expansion');
  const sub = args[0] ? args[0].value : '';
  if (sub === 'task') {
    if (args[1] && args[1].dynamic) return no('sdi task action is a variable expansion');
    const action = args[1] ? args[1].value : '';
    if (SDI_TASK_READ_ACTIONS.has(action) || action === '') return OK;
    return no(`\`sdi task ${action}\` mutates task lifecycle; delegate it to a specialist sub-agent`);
  }
  return OK;
}

// ────────────────────────────────────────────────────────────────────────────
// Misc per-command rules

function dateVerdict(args) {
  const { opts, positionals } = scanOptions(args, 'rdfvIzt', ['--reference', '--date', '--file', '--rfc-3339', '--iso-8601']);
  let parseOnly = false;
  for (const o of opts) {
    if (o.name === '-s' || o.name === '--set') return no('`date -s` sets the system clock');
    if (o.name === '-t') return no('`date -t` sets the kernel timezone');
    if (o.name === '-j') parseOnly = true;
  }
  for (const p of positionals) {
    if (!p.value.startsWith('+') && !parseOnly) return no('`date <operand>` sets the system clock');
  }
  return OK;
}

const HOSTNAME_READ_OPTS = new Set(['-s', '-f', '-d', '-i', '-I', '-a', '-A', '-y', '--short', '--fqdn', '--long', '--domain', '--ip-address', '--all-ip-addresses', '--all-fqdns', '--alias', '--nis', '--help', '--version']);
function hostnameVerdict(args) {
  for (const w of args) {
    if (!HOSTNAME_READ_OPTS.has(w.value)) return no('`hostname <name>` sets the host name');
  }
  return OK;
}

function forbidOptions(names, why, shortArgs = '', longArgs = []) {
  return (args) => {
    const dyn = firstOptionDynamic(args);
    if (dyn) return dyn;
    const { opts } = scanOptions(args, shortArgs, longArgs);
    for (const o of opts) {
      if (names.includes(o.name)) return no(`\`${o.name}\` ${why}`);
    }
    return OK;
  };
}

function maxPositionals(limit, why, shortArgs = '') {
  return (args) => {
    const { positionals } = scanOptions(args, shortArgs);
    return positionals.length <= limit ? OK : no(why);
  };
}

function testVerdict(args) {
  // `[[ x -eq 'a[$(cmd)]' ]]` evaluates array subscripts arithmetically.
  const bad = args.find((w) => /\[.*[$`]/.test(w.value));
  return bad ? no(`test operand \`${bad.raw}\` can trigger arithmetic command execution`) : OK;
}

function exportVerdict(args, ctx) {
  for (const w of args) {
    const v = w.value;
    if (v === '-p' || v === '-n') continue;
    if (v.startsWith('-')) return no(`\`export ${v}\` is not a plain variable export`);
    if (ASSIGN_RE.test(w.raw)) {
      const r = checkAssignment(parseAssignment(w), null, ctx);
      if (!r.ok) return r;
      continue;
    }
    if (!NAME_RE.test(v)) return no(`\`export ${w.raw}\` is not a variable name`);
    if (DANGEROUS_ENV_RE.test(v)) return no(`\`export ${v}\` exports a variable that changes what commands execute`);
  }
  return OK;
}

function setVerdict(args) {
  for (const w of args) {
    if (/^[-+][A-Za-z]*k/.test(w.value) || w.value === 'keyword') return no('`set -k` exports later assignment words into commands');
  }
  return OK;
}

function printfVerdict(args) {
  return args[0] && args[0].value === '-v' ? no('`printf -v` assigns a shell variable') : OK;
}

function cargoVerdict(args) {
  let k = 0;
  if (args[0] && args[0].value.startsWith('+')) k = 1; // `+toolchain`
  const sub = args[k] ? args[k].value : '';
  const rest = args.slice(k + 1).map((w) => w.value);
  if (['--version', '-V', 'version', 'tree', 'metadata', 'locate-project', 'verify-project', '--help', '-h', 'help'].includes(sub)) return OK;
  if (sub === 'check' || sub === 'clippy') {
    return rest.some((v) => v === '--fix' || v === '--allow-dirty') ? no(`\`cargo ${sub} --fix\` rewrites sources`) : OK;
  }
  if (sub === 'fmt') return rest.includes('--check') ? OK : no('`cargo fmt` without `--check` rewrites sources');
  return no(`\`cargo ${sub}\` is not a read-only cargo command`);
}

function npmVerdict(args) {
  const sub = args[0] ? args[0].value : '';
  return /^(list|ls|view|outdated|--version|-v)$/.test(sub) ? OK : no(`\`${sub}\` may run package scripts or modify node_modules`);
}

function nodeVerdict(args) {
  return args.length === 1 && /^(--version|-v)$/.test(args[0].value) ? OK : no('node evaluates arbitrary JavaScript');
}

function commandsWithPositionalOutput(name) {
  return maxPositionals(1, `\`${name}\` writes its second operand as an output file`, name === 'uniq' ? 'fsw' : 'cglson');
}

// ────────────────────────────────────────────────────────────────────────────
// Wrappers — they run another command, which is classified in turn.

function wrapped(args, ctx, depth) {
  if (args.length === 0) return OK;
  return classifySimple(args, ctx, depth + 1);
}

function commandVerdict(args, ctx, depth) {
  let k = 0;
  let lookup = false;
  for (; k < args.length; k++) {
    const v = args[k].value;
    if (v === '--') {
      k += 1;
      break;
    }
    if (!/^-[pvV]+$/.test(v)) break;
    if (/[vV]/.test(v)) lookup = true;
  }
  const operands = args.slice(k);
  if (lookup) {
    const bad = operands.find((w) => w.dynamic || !/^[A-Za-z0-9_.+/@:-]+$/.test(w.value));
    return bad ? no(`\`command -v\` operand \`${bad.raw}\` is not a plain command name`) : OK;
  }
  return wrapped(operands, ctx, depth);
}

function envVerdict(args, ctx, depth) {
  let k = 0;
  for (; k < args.length; k++) {
    const v = args[k].value;
    if (['-', '-i', '--ignore-environment', '-0', '--null', '-v', '--debug'].includes(v)) continue;
    if (v === '-u' || v === '--unset' || v === '-C' || v === '--chdir') {
      k += 1;
      continue;
    }
    if (/^-u./.test(v) || v.startsWith('--unset=') || v.startsWith('--chdir=')) continue;
    if (v.startsWith('-S') || v.startsWith('--split-string')) return no('`env -S` splits a string into a new command line');
    if (v === '-P') return no('`env -P` changes the command search path');
    if (v === '--') {
      k += 1;
      break;
    }
    if (v.startsWith('-')) return no(`env option \`${v}\` is not known to be read-only`);
    break;
  }
  const rest = args.slice(k);
  if (rest.every((w) => ASSIGN_RE.test(w.raw))) {
    // `env [VAR=…]` with no command prints the environment.
    for (const w of rest) {
      const r = checkAssignment(parseAssignment(w), 'env', ctx);
      if (!r.ok) return r;
    }
    return OK;
  }
  return classifySimple(rest, ctx, depth + 1);
}

function niceVerdict(args, ctx, depth) {
  let k = 0;
  for (; k < args.length; k++) {
    const v = args[k].value;
    if (v === '-n' || v === '--adjustment') k += 1;
    else if (/^-n.|^--adjustment=|^-\d+$/.test(v)) continue;
    else if (v === '--') {
      k += 1;
      break;
    } else break;
  }
  return wrapped(args.slice(k), ctx, depth);
}

function timeoutVerdict(args, ctx, depth) {
  let k = 0;
  for (; k < args.length; k++) {
    const v = args[k].value;
    if (v === '-s' || v === '--signal' || v === '-k' || v === '--kill-after') k += 1;
    else if (/^(-s.|-k.|--signal=|--kill-after=)/.test(v) || ['--preserve-status', '--foreground', '-v', '--verbose', '-f', '-p'].includes(v)) continue;
    else if (v === '--') {
      k += 1;
      break;
    } else break;
  }
  return wrapped(args.slice(k + 1), ctx, depth); // skip DURATION
}

function timeVerdict(args, ctx, depth) {
  let k = 0;
  for (; k < args.length; k++) {
    const v = args[k].value;
    if (v === '-p' || v === '-l' || v === '-v' || v === '--portability' || v === '--verbose') continue;
    if (v === '-o' || v.startsWith('--output') || v === '-a' || v === '--append') return no('`time -o` writes a file');
    break;
  }
  return wrapped(args.slice(k), ctx, depth);
}

function stdbufVerdict(args, ctx, depth) {
  let k = 0;
  for (; k < args.length; k++) {
    const v = args[k].value;
    if (v === '-i' || v === '-o' || v === '-e') k += 1;
    else if (/^(-[ioe].|--(input|output|error)=)/.test(v)) continue;
    else break;
  }
  return wrapped(args.slice(k), ctx, depth);
}

const XARGS_VALUE_OPTS = new Set(['-n', '-L', '-l', '-I', '-d', '-E', '-s', '-P', '-a', '-J', '-R', '-S']);
function xargsVerdict(args, ctx, depth) {
  let k = 0;
  for (; k < args.length; k++) {
    const v = args[k].value;
    if (XARGS_VALUE_OPTS.has(v)) {
      k += 1;
      continue;
    }
    if (/^-[nLlIdEsPaJRSie]./.test(v) || /^--(max-args|max-lines|replace|delimiter|eof|max-chars|max-procs|arg-file|process-slot-var)=/.test(v)) continue;
    if (['-0', '-r', '-t', '-p', '-x', '-o', '-i', '-e', '--null', '--no-run-if-empty', '--verbose', '--interactive', '--exit', '--open-tty'].includes(v)) continue;
    if (v === '--') {
      k += 1;
      break;
    }
    if (v.startsWith('-')) return no(`xargs option \`${v}\` is not known to be read-only`);
    break;
  }
  const inner = args.slice(k);
  if (inner.length === 0) return OK; // runs `echo`
  const r = classifySimple(inner, ctx, depth + 1);
  if (!r.ok) return r;
  const name = commandName(inner.find((w) => !ASSIGN_RE.test(w.raw)) || inner[0]);
  const spec = name && COMMANDS[name];
  if (!spec || spec !== READ) {
    return no(`xargs appends input-derived arguments; \`${name}\` is read-only only for fixed arguments`);
  }
  return OK;
}

// ────────────────────────────────────────────────────────────────────────────
// Command table — the single per-command rule structure. `READ` marks
// commands that stay read-only for ANY argument list (no option of theirs
// writes files or runs programs); everything else is a rule function
// (args, ctx, depth) → { ok, reason }.

const READ = () => OK;

// `for NAME [in WORDS…]` — the header only assigns NAME; the loop body is
// judged as separate segments. NAME goes through the assignment check.
function forHeaderVerdict(args, ctx) {
  const name = args[0];
  if (!name || name.quoted || !NAME_RE.test(name.value)) return no('`for` loop variable is not a plain name');
  if (args.length > 1 && args[1].value !== 'in') return no('`for` header is not `for NAME in WORDS`');
  return checkAssignment({ name: name.value, value: '', dynamic: true }, null, ctx);
}

// `read` assigns variables from stdin; the names are checked like assignments.
function readVerdict(args, ctx) {
  const { positionals } = scanOptions(args, 'adinNptu');
  for (const w of positionals) {
    if (!NAME_RE.test(w.value)) return no(`\`read ${w.raw}\` is not a variable name`);
    const r = checkAssignment({ name: w.value, value: '', dynamic: true }, null, ctx);
    if (!r.ok) return r;
  }
  return OK;
}

// Clawket work-management CLI — reads only; every lifecycle verb mutates.
const CLAWKET_READS = {
  project: new Set(['resolve', 'view', 'show', 'list']),
  task: new Set(['view', 'list', 'search', 'tree', 'ancestors', 'descendants', 'stats']),
  plan: new Set(['view', 'show', 'list']),
  unit: new Set(['view', 'show', 'list']),
  cycle: new Set(['view', 'show', 'list']),
  knowledge: new Set(['view', 'show', 'list', 'search']),
  comment: new Set(['view', 'show', 'list']),
  question: new Set(['view', 'show', 'list']),
  run: new Set(['view', 'show', 'list']),
  daemon: new Set(['status']),
};
function clawketVerdict(args) {
  if (args.some((w) => !w.dynamic && (w.value === '--help' || w.value === '-h'))) return OK;
  if ((args[0] && args[0].dynamic) || (args[1] && args[1].dynamic)) return no('clawket subcommand is a variable expansion');
  const sub = args[0] ? args[0].value : '';
  if (['', 'help', '--version', '-V', 'dashboard', 'doctor', 'replay'].includes(sub)) return OK;
  const reads = CLAWKET_READS[sub];
  const action = args[1] ? args[1].value : '';
  if (reads && (reads.has(action) || action === 'help')) return OK;
  return no(`\`clawket ${sub} ${action}\` changes work-management state`);
}

// cmux terminal multiplexer — inspection verbs only; send / new-* / close-*
// act on terminals and workspaces.
const CMUX_READS = new Set(['identify', 'read-screen', 'current-window', 'top', 'version', 'ping', 'capabilities', '--help', '-h', 'help']);

// curl — a GET/HEAD request whose response goes to stdout or /dev/null.
const CURL_SINKS = new Set(['-o', '--output', '-D', '--dump-header', '-c', '--cookie-jar', '--trace', '--trace-ascii', '--stderr', '--libcurl', '--etag-save', '--hsts', '--alt-svc']);
const CURL_BODY = new Set(['-d', '--data', '--data-raw', '--data-binary', '--data-urlencode', '--data-ascii', '--json', '-F', '--form', '--form-string', '-T', '--upload-file']);
function curlVerdict(args) {
  const dyn = firstOptionDynamic(args);
  if (dyn) return dyn;
  const { opts } = scanOptions(args, 'oDcKXdFTuHbAewmrxEUyYzCQt', [...CURL_SINKS, ...CURL_BODY, '--config', '--request']);
  for (const o of opts) {
    if (CURL_SINKS.has(o.name) && o.value !== '-' && o.value !== '/dev/null') return no(`\`curl ${o.name}\` writes a file`);
    if (CURL_BODY.has(o.name)) return no(`\`curl ${o.name}\` sends a request body`);
    if (['-O', '--remote-name', '--remote-name-all', '-J', '--remote-header-name', '--create-dirs'].includes(o.name)) return no(`\`curl ${o.name}\` writes a file`);
    if (o.name === '-K' || o.name === '--config') return no('`curl --config` options cannot be inspected');
    if ((o.name === '-X' || o.name === '--request') && !/^(GET|HEAD)$/i.test(o.value || '')) return no(`\`curl -X ${o.value}\` is not a GET request`);
  }
  return OK;
}

// Toolchain introspection: version / list / show forms only.
function toolchainVerdict(name, reads, requireFlag) {
  return (args) => {
    const vals = args.map((w) => w.value);
    if (vals.some((v) => v === '--version' || v === '-V' || v === '--help' || v === '-h')) return OK;
    if (requireFlag) return vals.includes(requireFlag) ? OK : no(`\`${name}\` without \`${requireFlag}\` rewrites files`);
    const key = vals.filter((v) => !v.startsWith('-')).slice(0, 2).join(' ');
    return reads.some((r) => key === r || key.startsWith(`${r} `)) ? OK : no(`\`${name} ${key}\` is not a read-only ${name} command`);
  };
}

function securityVerdict(args) {
  const sub = args[0] ? args[0].value : '';
  return ['find-identity', 'find-certificate', 'list-keychains', 'show-keychain-info', 'verify-cert'].includes(sub) ? OK : no(`\`security ${sub}\` is not a read-only keychain query`);
}
function cmuxVerdict(args) {
  let k = 0;
  while (k < args.length && /^--(socket|password|window|workspace|surface)$/.test(args[k].value)) k += 2;
  const head = args[k];
  if (head && head.dynamic) return no('cmux command is a variable expansion');
  const sub = head ? head.value : '';
  if (CMUX_READS.has(sub) || sub.startsWith('list-')) return OK;
  return no(`\`cmux ${sub}\` acts on terminals or workspaces`);
}

const COMMANDS = {
  // Shell state only — no payload, no file.
  cd: READ,
  pushd: READ,
  popd: READ,
  export: exportVerdict,
  unset: READ,
  set: setVerdict,
  true: READ,
  false: READ,
  ':': READ,
  // Lookup / test builtins.
  type: READ,
  which: READ,
  whereis: READ,
  test: testVerdict,
  '[': testVerdict,
  '[[': testVerdict,
  // Plain readers.
  ls: READ,
  cat: READ,
  head: READ,
  tail: READ,
  grep: READ,
  egrep: READ,
  fgrep: READ,
  wc: READ,
  pwd: READ,
  echo: READ,
  printf: printfVerdict,
  stat: READ,
  printenv: READ,
  read: readVerdict,
  uname: READ,
  whoami: READ,
  id: READ,
  groups: READ,
  jq: READ,
  cut: READ,
  comm: READ,
  column: READ,
  tr: READ,
  seq: READ,
  sleep: READ,
  basename: READ,
  dirname: READ,
  realpath: READ,
  readlink: READ,
  diff: READ,
  nl: READ,
  df: READ,
  du: READ,
  ps: READ,
  pgrep: READ,
  shasum: READ,
  sha1sum: READ,
  sha256sum: READ,
  sha512sum: READ,
  md5: READ,
  md5sum: READ,
  cksum: READ,
  cmp: READ,
  od: READ,
  hexdump: READ,
  strings: READ,
  uptime: READ,
  sw_vers: READ,
  arch: READ,
  // Readers with a writing option or operand.
  file: forbidOptions(['-C', '--compile'], 'compiles a magic file', 'emfFP', ['--magic-file', '--separator', '--files-from', '--parameter']),
  uniq: commandsWithPositionalOutput('uniq'),
  xxd: (args) => {
    const values = new Set(['-c', '-cols', '-g', '-groupsize', '-l', '-len', '-s', '-seek', '-o', '-offset', '-n', '-name']);
    let count = 0;
    for (let k = 0; k < args.length; k++) {
      const v = args[k].value;
      if (values.has(v)) k += 1;
      else if (!v.startsWith('-') || v === '-') count += 1;
    }
    return count <= 1 ? OK : no('`xxd` writes its second operand as an output file');
  },
  sort: forbidOptions(['-o', '--output', '--compress-program'], 'writes a file or runs a program', 'kotST', ['--key', '--field-separator', '--buffer-size', '--temporary-directory', '--output', '--compress-program', '--parallel', '--batch-size', '--random-source', '--files0-from']),
  tree: forbidOptions(['-o', '--output', '-R'], 'writes output files', 'LIPHTo', ['--filelimit', '--timefmt', '--sort', '--charset', '--fromfile']),
  find: (args) => {
    const dyn = firstOptionDynamic(args);
    if (dyn) return dyn;
    const bad = args.find((w) => /^-(delete|exec|execdir|ok|okdir|fprint|fprint0|fprintf|fls)$/.test(w.value));
    return bad ? no(`\`find ${bad.value}\` deletes, writes, or executes`) : OK;
  },
  rg: (args) => {
    const dyn = firstOptionDynamic(args);
    if (dyn) return dyn;
    const bad = args.find((w) => /^--pre(-glob)?(=|$)/.test(w.value));
    return bad ? no(`\`rg ${bad.value}\` runs a preprocessor program`) : OK;
  },
  sed: sedVerdict,
  awk: awkVerdict,
  gawk: awkVerdict,
  mawk: awkVerdict,
  nawk: awkVerdict,
  date: dateVerdict,
  hostname: hostnameVerdict,
  sqlite3: sqliteVerdict,
  git: gitVerdict,
  gh: ghVerdict,
  cargo: cargoVerdict,
  npm: npmVerdict,
  pnpm: npmVerdict,
  node: nodeVerdict,
  sdi: sdiVerdict,
  sdid: sdiVerdict,
  clawket: clawketVerdict,
  cmux: cmuxVerdict,
  curl: curlVerdict,
  security: securityVerdict,
  rustup: toolchainVerdict('rustup', ['show', 'toolchain list', 'target list', 'component list', 'which']),
  rustc: (args) => (args.length > 0 && args.every((w) => /^(--version|-V|-vV|--print(=.*)?|[a-z0-9-]+)$/.test(w.value)) && args.some((w) => /^(--version|-V|-vV|--print)/.test(w.value)) ? OK : no('`rustc` compiles; only `--version` / `--print` are read-only')),
  rustfmt: toolchainVerdict('rustfmt', [], '--check'),
  lsof: READ,
  nm: READ,
  otool: READ,
  // Wrappers.
  command: commandVerdict,
  builtin: (args, ctx, depth) => wrapped(args, ctx, depth),
  env: envVerdict,
  nice: niceVerdict,
  timeout: timeoutVerdict,
  gtimeout: timeoutVerdict,
  time: timeVerdict,
  stdbuf: stdbufVerdict,
  xargs: xargsVerdict,
};

const EXPLICIT_REJECTIONS = {
  eval: '`eval` executes a string as a command',
  exec: '`exec` replaces the shell with another program',
  source: '`source` executes a script file',
  '.': '`.` executes a script file',
  bash: 'a nested shell executes arbitrary commands',
  sh: 'a nested shell executes arbitrary commands',
  zsh: 'a nested shell executes arbitrary commands',
  dash: 'a nested shell executes arbitrary commands',
  ksh: 'a nested shell executes arbitrary commands',
  fish: 'a nested shell executes arbitrary commands',
  sudo: 'privilege escalation is never read-only',
  doas: 'privilege escalation is never read-only',
  trap: '`trap` registers a command to run later',
  alias: '`alias` redefines commands',
  hash: '`hash` changes command lookup',
};
const COMPOUND_KEYWORDS = new Set(['case', 'esac', 'select', 'function', 'coproc', '((', 'in']);
// Builtins whose resolution does not consult PATH.
const PATH_INDEPENDENT = new Set(['cd', 'pushd', 'popd', 'export', 'unset', 'set', 'echo', 'printf', 'test', '[', '[[', 'type', 'command', 'builtin', 'true', 'false', ':', 'pwd']);
const SYSTEM_BIN_DIRS = new Set(['/bin', '/usr/bin', '/usr/sbin', '/sbin', '/usr/local/bin', '/opt/homebrew/bin']);

function commandName(word) {
  if (!word) return null;
  const v = word.value;
  if (!v.includes('/')) return v;
  const slash = v.lastIndexOf('/');
  const base = v.slice(slash + 1);
  if (SDI_NAMES.has(base) || SYSTEM_BIN_DIRS.has(v.slice(0, slash))) return base;
  return null;
}

function classifySimple(words, ctx, depth = 0) {
  if (depth > 6) return no('wrapper commands nested too deeply');
  let k = 0;
  const assigns = [];
  while (k < words.length && ASSIGN_RE.test(words[k].raw)) {
    assigns.push(parseAssignment(words[k]));
    k += 1;
  }
  if (k < words.length && !words[k].quoted && words[k].value === '!') {
    return classifySimple([...words.slice(0, k), ...words.slice(k + 1)], ctx, depth);
  }
  if (k >= words.length) {
    for (const a of assigns) {
      const r = checkAssignment(a, null, ctx);
      if (!r.ok) return r;
    }
    return OK;
  }
  const head = words[k];
  if (!head.quoted && head.value === 'for' && k === 0) return forHeaderVerdict(words.slice(1), ctx);
  if (head.dynamic) return no(`command name \`${head.raw}\` is a variable expansion`);
  if (!head.quoted && COMPOUND_KEYWORDS.has(head.value)) return no(`compound command \`${head.value}\` is not analysed`);
  const name = commandName(head);
  if (name === null) return no(`path-qualified command \`${head.value}\` is not a known system binary`);
  for (const a of assigns) {
    const r = checkAssignment(a, name, ctx);
    if (!r.ok) return r;
  }
  if (ctx.pathModified && !SDI_NAMES.has(name) && !PATH_INDEPENDENT.has(name)) {
    return no(`PATH was changed earlier in the command; \`${name}\` could resolve to any binary`);
  }
  if (EXPLICIT_REJECTIONS[name]) return no(EXPLICIT_REJECTIONS[name]);
  const rule = Object.prototype.hasOwnProperty.call(COMMANDS, name) ? COMMANDS[name] : null;
  if (!rule) return no(`\`${name}\` is not a known read-only command`);
  return rule(words.slice(k + 1), ctx, depth);
}

// ────────────────────────────────────────────────────────────────────────────
// Public API

// → { readOnly: boolean, reason: string, failingSegment: string | null }
function classifyBash(cmd) {
  if (typeof cmd !== 'string' || !cmd.trim()) {
    return { readOnly: false, reason: 'empty command', failingSegment: null };
  }
  const lexed = lex(cmd);
  if (lexed.error) return { readOnly: false, reason: lexed.error, failingSegment: lexed.segment };
  const ctx = { pathModified: false };
  for (const seg of lexed.segments) {
    if (seg.words.length === 0) continue;
    const r = classifySimple(seg.words, ctx, 0);
    if (!r.ok) return { readOnly: false, reason: r.reason, failingSegment: seg.text };
  }
  return { readOnly: true, reason: 'every segment is read-only', failingSegment: null };
}

module.exports = { classifyBash, _internals: { lex, sedScriptVerdict, awkProgramVerdict, COMMANDS } };
