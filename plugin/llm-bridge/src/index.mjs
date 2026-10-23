#!/usr/bin/env node
/**
 * SDI llm-bridge sidecar (PRD-v2 D37).
 *
 * sdid (Rust) cannot call the Node provider stack directly, so this sidecar
 * wraps the two agent-devtools providers and exposes a single SSE route:
 *
 *   POST /v1/agent/stream   — body.provider ∈ {"acp","sdk"} selects the runtime.
 *     - acp: spawns the `claude` CLI as a child (stdio JSON-RPC). The child
 *            inherits `~/.claude` OAuth (subscription) from the environment.
 *     - sdk: in-process `@anthropic-ai/claude-agent-sdk`. Falls back to the
 *            same `~/.claude` OAuth when ANTHROPIC_API_KEY / ANTHROPIC_AUTH_TOKEN
 *            are unset.
 *
 * Both paths reuse the local Claude subscription → API cost 0 (PRD §5a).
 *
 * We deliberately build the app with `createApp` (NOT `startAgentDevtoolsServer`)
 * so no pairing token is minted: the bridge is loopback-bound and the ONLY
 * client is the sdid proxy on the same host, which would otherwise have to carry
 * a memory-only token the browser never sees. Auth is the loopback bind itself.
 *
 * Widget / framework-picker / handoff surfaces are not wired here — only the
 * agent-stream contract sdid proxies.
 *
 * Env:
 *   SDI_LLM_BRIDGE_PORT  bind port (default 19501)
 *   SDI_LLM_BRIDGE_HOST  bind host (default 127.0.0.1; forced loopback)
 */
import {
  createApp,
  startServer,
  PROVIDER_IDS,
} from '@agent-devtools/core/server';
import {
  createAcpProvider,
  createSdkProvider,
  createDefaultAcpRuntime,
  createDefaultAcpSessionStore,
} from '@agent-devtools/core/providers';

const DEFAULT_PORT = 19501;

function resolvePort() {
  const raw = process.env.SDI_LLM_BRIDGE_PORT;
  if (!raw) return DEFAULT_PORT;
  const n = Number.parseInt(raw, 10);
  if (!Number.isInteger(n) || n < 0 || n > 65535) {
    throw new Error(`SDI_LLM_BRIDGE_PORT must be a valid port, got: ${raw}`);
  }
  return n;
}

async function main() {
  // Shared ACP runtime so the (cwd, clientSessionId) → acpSessionId mapping
  // survives across turns within the process lifetime (stateful conversation
  // mode). The SDK provider is stateless per request.
  const acpSessionStore = createDefaultAcpSessionStore();
  const acpRuntime = createDefaultAcpRuntime({ sessionStore: acpSessionStore });

  const providers = {
    acp: createAcpProvider({ runtime: acpRuntime }),
    sdk: createSdkProvider(),
  };

  // No `pairingToken` → no Authorization gate (loopback-only sidecar).
  // No `workspace` → the agent operates with its own default cwd; this bridge
  //   is a pure LLM transport, not a file-editing surface.
  const handler = createApp({
    providers,
    acpSessionStore,
    // The natural alignment is batch↔sdk / conversational↔acp, but the client
    // chooses per request via body.provider; we keep acp as the omitted-field
    // default to match agent-devtools' own baseline.
    defaultProvider: 'acp',
  });

  const port = resolvePort();
  const host = process.env.SDI_LLM_BRIDGE_HOST || '127.0.0.1';

  const started = await startServer(handler, { port, host });

  // One structured line on stdout so sdid / supervisors can confirm readiness
  // and learn the actually-bound port (startServer falls back on EADDRINUSE).
  process.stdout.write(
    `${JSON.stringify({
      service: 'sdi-llm-bridge',
      url: started.url,
      port: started.port,
      providers: PROVIDER_IDS,
    })}\n`,
  );

  const shutdown = () => {
    started
      .close()
      .then(() => process.exit(0))
      .catch(() => process.exit(1));
  };
  process.on('SIGINT', shutdown);
  process.on('SIGTERM', shutdown);
}

main().catch((err) => {
  process.stderr.write(`sdi-llm-bridge failed to start: ${err?.stack ?? String(err)}\n`);
  process.exit(1);
});
