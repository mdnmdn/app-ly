// Minimal host-protocol runtime for sidecar plugins (protocol v1).
//
// Framing: one JSON object per line on stdio. stdout carries protocol traffic
// only — anything a plugin wants to log goes to stderr, or it corrupts the
// stream. See _docs/plugins.md for the wire format.

import { createInterface } from "node:readline";

const PROTOCOL = 1;

const write = (message) => {
  process.stdout.write(`${JSON.stringify({ v: PROTOCOL, ...message })}\n`);
};

/**
 * Starts the read/dispatch loop.
 *
 * @param {object} manifest  { name, apiVersion, methods: [{ name, description, params }] }
 * @param {object} handlers  method name -> async (params, ctx) => result
 */
export function serve(manifest, handlers) {
  const inflight = new Map();
  let active = 0;        // requests being handled, including their reply write
  let hostGone = false;  // stdin closed: leave once the last reply is out

  // A reply can still be in flight when stdin closes, so exiting on close alone
  // truncates it. Wait for the last one, and give stdout a moment to drain.
  const leaveWhenIdle = () => {
    if (hostGone && active === 0) setTimeout(() => process.exit(0), 10);
  };

  const describe = () => ({
    name: manifest.name,
    apiVersion: manifest.apiVersion ?? PROTOCOL,
    methods: manifest.methods ?? Object.keys(handlers).map((name) => ({ name })),
  });

  const dispatch = async (request) => {
    if (request.method === "$describe") return describe();
    if (request.method === "$shutdown") {
      // Let the reply reach the host before the process goes away.
      hostGone = true;
      return { ok: true };
    }
    if (request.method === "$cancel") {
      const target = inflight.get(request.params?.id);
      if (target) target.abort();
      return { cancelled: Boolean(target) };
    }

    const handler = handlers[request.method];
    if (!handler) throw new Error(`unknown method "${request.method}"`);

    const controller = new AbortController();
    inflight.set(request.id, controller);
    try {
      return await handler(request.params ?? {}, {
        signal: controller.signal,
        emit: (event, data) => write({ event, data }),
        log: (message) => process.stderr.write(`${message}\n`),
      });
    } finally {
      inflight.delete(request.id);
    }
  };

  createInterface({ input: process.stdin }).on("line", async (line) => {
    if (!line.trim()) return;

    let request;
    try {
      request = JSON.parse(line);
    } catch (error) {
      write({ event: "protocol-error", data: { message: String(error) } });
      return;
    }

    active += 1;
    try {
      write({ id: request.id, ok: true, result: await dispatch(request) });
    } catch (error) {
      write({
        id: request.id,
        ok: false,
        error: { message: error?.message ?? String(error) },
      });
    } finally {
      active -= 1;
      leaveWhenIdle();
    }
  });

  // A closed stdin means the host is gone; don't linger as an orphan.
  process.stdin.on("close", () => {
    hostGone = true;
    leaveWhenIdle();
  });
  write({ event: "ready", data: describe() });
}
