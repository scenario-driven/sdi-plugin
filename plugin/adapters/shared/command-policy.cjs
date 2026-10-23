'use strict';

const fs = require('fs');
const path = require('path');
const { _internals: { lex, classifySimple } } = require('./bash-classifier.cjs');

const FILES = ['settings.json', 'settings.local.json'];
const ASSIGNMENT = /^[A-Za-z_][A-Za-z0-9_]*\+?=/;

function realPath(value) {
  if (typeof value !== 'string' || !value) return null;
  try { return fs.realpathSync(value); } catch { return null; }
}

function contains(root, cwd) {
  const relative = path.relative(root, cwd);
  return relative === '' || (relative !== '..' && !relative.startsWith(`..${path.sep}`) && !path.isAbsolute(relative));
}

function resolvePolicyRoot(cwd, project) {
  const invocation = realPath(cwd);
  if (!invocation || !project) return null;
  const roots = Array.isArray(project.cwds) ? project.cwds : [];
  const candidates = [...roots, project.cwd]
    .map((entry) => realPath(typeof entry === 'string' ? entry : entry && entry.cwd))
    .filter((root) => root && contains(root, invocation));
  return candidates.sort((a, b) => b.length - a.length)[0] || null;
}

function isObject(value) {
  return value !== null && typeof value === 'object' && !Array.isArray(value);
}

function isStatic(word) {
  return !word.dynamic && !word.glob && !word.expanding;
}

function parseRule(rule, source) {
  if (typeof rule !== 'string' || !rule.trim()) throw new Error('expected a nonempty command string');
  const parsed = lex(rule);
  if (parsed.error) throw new Error('unsupported shell syntax');
  if (Object.values(parsed.syntax).some(Boolean) || parsed.segments.length !== 1) {
    throw new Error('expected a simple command without shell operators, comments or redirects');
  }
  const words = parsed.segments[0].words;
  const last = words[words.length - 1];
  const prefix = words.length > 1 && last.raw === '*' && /\s\*\s*$/.test(rule);
  const fixed = prefix ? words.slice(0, -1) : words;
  if (!fixed.length || fixed.some((word) => !isStatic(word))) {
    throw new Error('only a terminal unquoted space-star wildcard is supported');
  }
  if (ASSIGNMENT.test(fixed[0].raw) || fixed[0].value === '!') {
    throw new Error('expected an executable followed by arguments');
  }
  return { rule, source, words: fixed.map((word) => word.value), prefix };
}

function loadCommandPolicy({ cwd, project }) {
  const root = resolvePolicyRoot(cwd, project);
  const result = { root, rules: [], diagnostics: [] };
  if (!root) return result;
  const seen = new Set();
  for (const filename of FILES) {
    const source = path.join(root, '.sdi', filename);
    let text;
    try { text = fs.readFileSync(source, 'utf8'); } catch (error) {
      if (error.code !== 'ENOENT') result.diagnostics.push({ source, reason: `cannot read settings (${error.code || 'I/O error'})` });
      continue;
    }
    let settings;
    try { settings = JSON.parse(text); } catch {
      result.diagnostics.push({ source, reason: 'invalid JSON' });
      continue;
    }
    if (!isObject(settings)) {
      result.diagnostics.push({ source, reason: 'settings must be an object' });
      continue;
    }
    if (!Object.prototype.hasOwnProperty.call(settings, 'permissions')) continue;
    if (!isObject(settings.permissions)) {
      result.diagnostics.push({ source, reason: 'permissions must be an object' });
      continue;
    }
    if (!Object.prototype.hasOwnProperty.call(settings.permissions, 'allow')) continue;
    if (!Array.isArray(settings.permissions.allow)) {
      result.diagnostics.push({ source, reason: 'permissions.allow must be an array' });
      continue;
    }
    settings.permissions.allow.forEach((rule, index) => {
      try {
        const parsed = parseRule(rule, source);
        if (!seen.has(rule)) { result.rules.push(parsed); seen.add(rule); }
      } catch (error) {
        result.diagnostics.push({ source, reason: `permissions.allow[${index}]: ${error.message}` });
      }
    });
  }
  // A partially written file must never leave grants from the other file active.
  if (result.diagnostics.length) result.rules = [];
  return result;
}

function matchesRule(words, rule) {
  if (words.length < rule.words.length || (!rule.prefix && words.length !== rule.words.length)) return false;
  return rule.words.every((value, index) => isStatic(words[index]) && words[index].value === value);
}

function directoryCommand(words) {
  if (words.some((word) => /^CDPATH\+?=/.test(word.raw))) return [];
  let index = 0;
  while (index < words.length && ASSIGNMENT.test(words[index].raw)) index += 1;
  while (words[index] && words[index].value === '!') index += 1;
  return words[index] && ['cd', 'pushd', 'popd', 'eval', 'source', '.', 'command', 'builtin', 'time'].includes(words[index].value)
    ? words.slice(index) : null;
}

function advanceDirectories(words, directories, root, project) {
  const command = directoryCommand(words);
  if (!command) return directories;
  if (!command.length) return [];
  if (command[0].value !== 'cd') return [];
  const args = command.slice(1);
  if (args[0] && args[0].value === '--') args.shift();
  if (args.length !== 1 || !isStatic(args[0]) || args[0].value.startsWith('-') || !args[0].value) return [];
  if (process.env.CDPATH && !args[0].value.includes('/') && !['.', '..'].includes(args[0].value)) return [];
  const candidates = new Set(directories);
  for (const cwd of directories) {
    const target = realPath(path.resolve(cwd, args[0].value));
    if (!target || resolvePolicyRoot(target, project) !== root) return [];
    try { if (!fs.statSync(target).isDirectory()) return []; } catch { return []; }
    candidates.add(target);
  }
  // Preserve failure and branch possibilities instead of assuming cd succeeds.
  return candidates.size <= 16 ? [...candidates] : [];
}

function evaluateCommandPolicy(command, options) {
  const policy = loadCommandPolicy(options);
  const result = { allowedByPolicy: false, matches: [], diagnostics: policy.diagnostics };
  if (!policy.rules.length || typeof command !== 'string' || !command.trim()) return result;
  const parsed = lex(command);
  if (parsed.error || parsed.syntax.redirects || parsed.syntax.compounds || parsed.syntax.invalidChain) return result;
  const ctx = { pathModified: false };
  let directories = [realPath(options.cwd)];
  for (const segment of parsed.segments) {
    const verdict = classifySimple(segment.words, ctx);
    if (!verdict.ok) {
      const match = directories.length > 0 && policy.rules.find((rule) => matchesRule(segment.words, rule));
      if (!match) { result.matches = []; return result; }
      if (!result.matches.some((entry) => entry.rule === match.rule && entry.source === match.source)) {
        result.matches.push({ rule: match.rule, source: match.source });
      }
    }
    directories = advanceDirectories(segment.words, directories, policy.root, options.project);
  }
  result.allowedByPolicy = result.matches.length > 0;
  return result;
}

module.exports = { evaluateCommandPolicy, loadCommandPolicy, resolvePolicyRoot };
