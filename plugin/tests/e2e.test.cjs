// Plugin-shell composition e2e.
//
// This is the one composition the CLI-level d3 test does not exercise:
//   1. Run the SessionStart shim as a real Claude Code install would —
//      stdin JSON in, banner JSON on stdout, ensureInstalled spawns the daemon.
//   2. Drive the full SDI workflow (project → plan → confirmed scenario →
//      D8 approve → R1 round → task → §6.6 evidence → round result →
//      round complete → plan complete) via the `sdi` CLI against THAT daemon.
//   3. Fire the PreToolUse / PostToolUse / SubagentStart / SubagentStop shims
//      with the in-flight task pinned via SDI_ACTIVE_TASK, exactly the way
//      Claude Code would when a hook fires.
//   4. Assert two independent truth sources agree:
//        - Daemon (HTTP /projects/by-cwd, /tasks/:id) — the SQLite SSoT.
//        - hook.log audit trail under XDG state — the LM-8 single-channel
//          write path from plugin code.
//
// Everything runs under a per-test temp SDI_HOME so the dev machine's
// ~/.local/share/sdi is untouched. The daemon is torn down at the end.
//
// Run: `node --test plugin/tests/e2e.test.cjs`

'use strict';

const test = require('node:test');
const assert = require('node:assert/strict');
const fs = require('node:fs');
const os = require('node:os');
const path = require('node:path');
const http = require('node:http');
const { spawnSync } = require('node:child_process');

const PLUGIN_ROOT = path.resolve(__dirname, '..');
const WORKSPACE_ROOT = path.resolve(PLUGIN_ROOT, '..');
const SHIM_DIR = path.join(PLUGIN_ROOT, 'adapters/claude');
const SDI_BIN = path.join(WORKSPACE_ROOT, 'target/debug/sdi');

function mkTempHome() {
  return fs.mkdtempSync(path.join(os.tmpdir(), 'sdi-plugin-e2e-'));
}

function shimEnv(home, extra) {
  return {
    ...process.env,
    SDI_HOME: home,
    CLAUDE_PLUGIN_ROOT: PLUGIN_ROOT,
    ...(extra || {}),
  };
}

function runShim(name, env, stdin) {
  const shim = path.join(SHIM_DIR, name);
  return spawnSync('node', [shim], {
    env,
    input: stdin || '',
    encoding: 'utf8',
    timeout: 15000,
  });
}

function runSdi(home, args, extraEnv) {
  return spawnSync(SDI_BIN, args, {
    env: {
      ...process.env,
      SDI_HOME: home,
      ...(extraEnv || {}),
    },
    encoding: 'utf8',
    timeout: 15000,
  });
}

function runSdiOk(home, args) {
  const r = runSdi(home, args);
  assert.equal(r.status, 0, `sdi ${args.join(' ')} exit=${r.status} stderr=${r.stderr}`);
  return JSON.parse(r.stdout);
}

function readDaemonPort(home) {
  const portFile = path.join(home, '.cache/sdi/sdid.port');
  return parseInt(fs.readFileSync(portFile, 'utf8').trim(), 10);
}

function httpGetJson(port, urlPath) {
  return new Promise((resolve, reject) => {
    const req = http.request(
      { host: '127.0.0.1', port, path: urlPath, method: 'GET', timeout: 3000 },
      (res) => {
        let body = '';
        res.setEncoding('utf8');
        res.on('data', (chunk) => (body += chunk));
        res.on('end', () => {
          if (res.statusCode === 404) return resolve(null);
          if (res.statusCode >= 400) return reject(new Error(`HTTP ${res.statusCode}: ${body}`));
          try {
            resolve(JSON.parse(body));
          } catch (e) {
            reject(new Error(`bad JSON from ${urlPath}: ${e.message}\n${body}`));
          }
        });
      },
    );
    req.on('error', reject);
    req.on('timeout', () => req.destroy(new Error('timeout')));
    req.end();
  });
}

function tailHookLog(home) {
  const log = path.join(home, '.local/state/sdi/hook.log');
  if (!fs.existsSync(log)) return [];
  return fs
    .readFileSync(log, 'utf8')
    .trim()
    .split('\n')
    .filter(Boolean)
    .map((line) => JSON.parse(line));
}

function killDaemon(home) {
  const pidFile = path.join(home, '.cache/sdi/sdid.pid');
  if (!fs.existsSync(pidFile)) return;
  const pid = parseInt(fs.readFileSync(pidFile, 'utf8').trim(), 10);
  if (!Number.isFinite(pid)) return;
  try {
    process.kill(pid, 'SIGTERM');
  } catch {}
}

// Some assertions need a tiny window for the daemon to flush state to SQLite
// after the CLI returns. Poll briefly rather than sleep blindly.
async function waitFor(predicate, { tries = 20, intervalMs = 50 } = {}) {
  for (let i = 0; i < tries; i++) {
    const r = await predicate();
    if (r) return r;
    await new Promise((res) => setTimeout(res, intervalMs));
  }
  return null;
}

// ─────────────────────────────────────────────────────────────────────────────

test(
  'plugin-shell e2e: SessionStart → CLI workflow → hook shims → audit log',
  { timeout: 60000 },
  async () => {
    // Skip cleanly if the dev-mode binary isn't built. d3 covers this surface
    // via CARGO_BIN_EXE_sdi; here we depend on `cargo build` having run.
    if (!fs.existsSync(SDI_BIN)) {
      console.error(`[e2e] skip: ${SDI_BIN} not built — run \`cargo build -p sdi\` first`);
      return;
    }

    const home = mkTempHome();
    const projectCwd = path.join(home, 'work');
    fs.mkdirSync(projectCwd, { recursive: true });

    try {
      // 1) SessionStart shim: bootstraps the daemon under temp SDI_HOME and
      //    emits a banner. No project registered yet, so the banner should
      //    say so — proves the daemon is reachable from the shim's POV.
      const session = runShim(
        'session-start.cjs',
        shimEnv(home, {}),
        JSON.stringify({ cwd: projectCwd }),
      );
      assert.equal(session.status, 0, `session-start stderr=${session.stderr}`);
      assert.match(session.stdout, /SessionStart/, 'banner must self-identify');
      assert.match(session.stdout, /No SDI project registered/);

      // Daemon should be live now — pid + port files written, /health OK.
      const port = readDaemonPort(home);
      assert.ok(Number.isFinite(port) && port > 0, `port=${port}`);
      const health = await httpGetJson(port, '/health');
      assert.ok(health, '/health returned null');
      assert.equal(health.ok, true);

      // 2) Drive the canonical SDI workflow through the live CLI.
      const suffix = Date.now().toString(36);
      const key = `E${suffix.slice(-4).toUpperCase()}`;

      const project = runSdiOk(home, [
        'project', 'create', key, 'E2E Plugin Shell',
        '--slug', `e2e-${suffix}`,
        '--cwd', projectCwd,
      ]);
      const projectId = project.id;

      const planCode = `P-${suffix}`;
      const plan = runSdiOk(home, [
        'plan', 'create', projectId, planCode, 'v0.1 — plugin-shell e2e',
        '--body', 'verify the shell composition end-to-end',
      ]);
      const planId = plan.id;

      runSdiOk(home, [
        'req', 'create', planId, `REQ-${suffix}`, 'Audit log truth',
        '--body', 'hook.log records all hook events under XDG state',
      ]);

      const scnCode = `SCN-${suffix}`;
      const scn = runSdiOk(home, [
        'scenario', 'create', planId, scnCode,
        '--given', 'a registered SDI project and an approved plan',
        '--when',  'the LLM activates a round and pins an active task',
        '--then',  'pre/post/subagent hook shims record entries against that task',
        '--confirmed',
      ]);
      const scnId = scn.id;
      assert.equal(scn.status, 'confirmed', 'scenario must be confirmed to satisfy D8');

      // D8 gate: ≥1 confirmed scenario.
      const approved = runSdiOk(home, ['plan', 'approve', planId]);
      assert.equal(approved.status, 'active');

      const roundCode = `R1-${suffix}`;
      const r1 = runSdiOk(home, ['round', 'create', planId, roundCode]);
      assert.equal(r1.status, 'planning');
      assert.equal(r1.mode, 'strict-regression', 'D6 default must be strict-regression');
      const r1Id = r1.id;

      const activation = runSdiOk(home, ['round', 'activate', r1Id]);
      assert.equal(activation.round.status, 'active');

      const taskCode = `T-${suffix}`;
      const task = runSdiOk(home, [
        'task', 'create', r1Id, taskCode,
        'wire the plugin shell',
        '--scenario', scnId,
      ]);
      const taskId = task.id;
      runSdiOk(home, ['task', 'start', taskId]);

      // 3) Fire the four "active-task" shims as Claude Code would.
      //    Pin the active task via SDI_ACTIVE_TASK so readActiveTaskHint()
      //    sees it — that is exactly how a real session would pass it.
      const taskEnv = shimEnv(home, { SDI_ACTIVE_TASK: taskId });

      const filePath = path.join(projectCwd, 'src/lib.rs');
      // D21: Edit must come from a registered specialist sub-agent. Simulate
      // the impl-coder spawning Edit; main-session Edit would be denied
      // before the active-task gate is even reached.
      const subAgentEnv = {
        agent_id: '00000000-0000-0000-0000-0000000000aa',
        agent_type: 'impl-coder',
      };
      const preEdit = runShim(
        'pre-tool-use.cjs',
        taskEnv,
        JSON.stringify({
          tool_name: 'Edit',
          tool_input: { file_path: filePath },
          ...subAgentEnv,
        }),
      );
      assert.equal(preEdit.status, 0, `pre-tool-use stderr=${preEdit.stderr}`);
      // Active task is pinned + delegation gate passes — no deny payload.
      assert.doesNotMatch(preEdit.stdout, /permissionDecision.*deny/);

      const postEdit = runShim(
        'post-tool-use.cjs',
        taskEnv,
        JSON.stringify({ tool_name: 'Edit', tool_input: { file_path: filePath } }),
      );
      assert.equal(postEdit.status, 0, `post-tool-use stderr=${postEdit.stderr}`);

      const subStart = runShim(
        'subagent-start.cjs',
        taskEnv,
        JSON.stringify({ subagent_type: 'Plan' }),
      );
      assert.equal(subStart.status, 0, `subagent-start stderr=${subStart.stderr}`);

      const subStop = runShim(
        'subagent-stop.cjs',
        taskEnv,
        JSON.stringify({ subagent_type: 'Plan', result: 'design complete' }),
      );
      assert.equal(subStop.status, 0, `subagent-stop stderr=${subStop.stderr}`);

      // Sanity: PreToolUse without an active task pinned still denies, even
      // though the daemon and project exist. This proves the gate's source
      // of truth is the env hint, not daemon state — the documented contract.
      // D21: also use a registered sub-agent so the delegation gate passes
      // first, isolating the active-task gate behaviour here.
      const preNoTask = runShim(
        'pre-tool-use.cjs',
        shimEnv(home, {}),
        JSON.stringify({
          tool_name: 'Edit',
          tool_input: { file_path: filePath },
          ...subAgentEnv,
        }),
      );
      assert.equal(preNoTask.status, 0);
      assert.match(preNoTask.stdout, /permissionDecision.*deny/);
      assert.match(preNoTask.stdout, /no active task/);

      // 4) Daemon SSoT must agree with what the CLI told us. The handler
      //    wraps the row under `{ "project": ... }` (see router/project.rs
      //    `by_cwd`), so dereference accordingly.
      const projByCwdWrap = await httpGetJson(
        port,
        `/projects/by-cwd?cwd=${encodeURIComponent(projectCwd)}`,
      );
      assert.ok(projByCwdWrap, '/projects/by-cwd returned null');
      const projByCwd = projByCwdWrap.project;
      assert.ok(projByCwd, '/projects/by-cwd payload missing `project` field');
      assert.equal(projByCwd.id, projectId);
      assert.equal(projByCwd.key, key);

      // 5) Audit log SSoT must reflect every hook we just fired, all
      //    correlated against the same task.
      const entries = await waitFor(() => {
        const ls = tailHookLog(home);
        const hasAll =
          ls.some((e) => e.event === 'session_start') &&
          ls.some((e) => e.event === 'pre_tool_use_allow' && e.task_id === taskId) &&
          ls.some((e) => e.event === 'post_tool_use' && e.task_id === taskId && e.file === filePath) &&
          ls.some((e) => e.event === 'subagent_start' && e.task_id === taskId && e.agent === 'Plan') &&
          ls.some((e) => e.event === 'subagent_stop' && e.task_id === taskId && e.result === 'design complete') &&
          ls.some((e) => e.event === 'pre_tool_use_blocked' && e.reason === 'no-active-task');
        return hasAll ? ls : null;
      });
      assert.ok(entries, `hook.log missing expected entries:\n${JSON.stringify(tailHookLog(home), null, 2)}`);

      // 6) Walk the rest of the round → §6.6 evidence gate, complete round,
      //    complete plan.
      const evidence = `${scnId}=passing@${path.relative(projectCwd, filePath)}:1`;
      const done = runSdiOk(home, [
        'task', 'complete', taskId,
        '--evidence', evidence,
        '--summary', 'plugin shell composition verified',
      ]);
      assert.equal(done.status, 'done');
      assert.ok(done.evidence_at, '§6.6: evidence_at must be set on done transition');

      runSdiOk(home, [
        'round', 'result', r1Id,
        '--scenario', scnId,
        '--result', 'passing',
        '--evidence', 'hook.log + daemon /projects/by-cwd agree',
      ]);
      runSdiOk(home, ['round', 'complete', r1Id]);
      const planDone = runSdiOk(home, ['plan', 'complete', planId]);
      assert.equal(planDone.status, 'completed');

      // 7) D12 — append-only decisions: record a supersession and assert the
      //    superseded decision is reachable + flagged. The plugin-shell test
      //    is the right place for this because the same install gate that
      //    spawned the daemon is also what surfaces decision context back to
      //    the LLM via UserPromptSubmit later.
      const d1 = runSdiOk(home, [
        'decision', 'create', planId, `DEC-${suffix}-A`,
        'Single-channel audit log',
        '--body', 'plugin code writes only via appendHookLog (LM-8)',
      ]);
      assert.equal(d1.status, 'accepted');
      runSdiOk(home, [
        'decision', 'supersede', d1.id,
        '--plan-id', planId,
        '--short-code', `DEC-${suffix}-B`,
        '--title', 'Single-channel audit log — keep until daemon ingest lands',
        '--body', 'append-only chain; the predecessor becomes status=superseded',
      ]);
      const d1After = runSdiOk(home, ['decision', 'view', d1.id]);
      assert.equal(d1After.status, 'superseded', 'D12 supersession must flip predecessor');
    } finally {
      killDaemon(home);
      // Best-effort cleanup. If the daemon hasn't actually exited yet the
      // socket file may linger; the next test gets a fresh temp dir anyway.
      try {
        fs.rmSync(home, { recursive: true, force: true });
      } catch {}
    }
  },
);
