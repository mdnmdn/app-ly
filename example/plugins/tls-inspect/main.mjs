// TLS certificate inspector — a sidecar plugin.
//
// Everything here is plain Node: the shell knows nothing about TLS, and this
// file can change without rebuilding the Rust binary.

import tls from "node:tls";
import { serve } from "../sidecar-sdk.mjs";

const DAY_MS = 24 * 60 * 60 * 1000;

const days = (until) => Math.floor((Date.parse(until) - Date.now()) / DAY_MS);

const names = (subject) =>
  Object.entries(subject ?? {})
    .map(([key, value]) => `${key}=${Array.isArray(value) ? value.join(",") : value}`)
    .join(", ");

/** Walks `issuerCertificate` up to the root, guarding the self-signed cycle. */
function chainOf(leaf) {
  const chain = [];
  const seen = new Set();
  for (let cert = leaf; cert && cert.fingerprint256; cert = cert.issuerCertificate) {
    if (seen.has(cert.fingerprint256)) break;
    seen.add(cert.fingerprint256);
    chain.push({
      subject: names(cert.subject),
      issuer: names(cert.issuer),
      commonName: cert.subject?.CN ?? null,
      altNames: cert.subjectaltname ?? null,
      serialNumber: cert.serialNumber ?? null,
      validFrom: cert.valid_from ?? null,
      validTo: cert.valid_to ?? null,
      daysRemaining: cert.valid_to ? days(cert.valid_to) : null,
      fingerprint256: cert.fingerprint256,
      keyType: cert.asn1Curve ?? (cert.modulus ? `RSA-${cert.bits ?? "?"}` : null),
      selfSigned: names(cert.subject) === names(cert.issuer),
    });
  }
  return chain;
}

function connect({ host, port = 443, servername, insecure = false, timeoutMs = 10000 }, signal) {
  if (!host) throw new Error("host is required");

  return new Promise((resolve, reject) => {
    const options = { host, port, servername: servername ?? host, rejectUnauthorized: !insecure };
    // With `insecure`, hostname mismatches are reported rather than fatal, so an
    // expired or misissued certificate is still inspectable — that is the point.
    if (insecure) options.checkServerIdentity = () => undefined;

    const socket = tls.connect(options, () => {
      const peer = socket.getPeerCertificate(true);
      const result = {
        host,
        port,
        authorized: socket.authorized,
        authorizationError: socket.authorizationError ? String(socket.authorizationError) : null,
        protocol: socket.getProtocol(),
        cipher: socket.getCipher()?.name ?? null,
        chain: chainOf(peer),
      };
      socket.end();
      resolve(result);
    });

    const fail = (error) => {
      socket.destroy();
      reject(error instanceof Error ? error : new Error(String(error)));
    };

    socket.setTimeout(timeoutMs, () => fail(new Error(`timed out after ${timeoutMs}ms`)));
    socket.on("error", fail);
    signal?.addEventListener("abort", () => fail(new Error("cancelled")), { once: true });
  });
}

serve(
  {
    name: "tls",
    apiVersion: 1,
    methods: [
      {
        name: "inspect",
        description: "Full certificate chain for a host",
        params: { host: "string", port: "number?", insecure: "boolean?" },
      },
      {
        name: "expiry",
        description: "Days remaining on the leaf certificate",
        params: { host: "string", port: "number?" },
      },
      {
        name: "check",
        description: "Expiry for several hosts at once, emits progress events",
        params: { hosts: "string[]", warnDays: "number?" },
      },
    ],
  },
  {
    inspect: (params, ctx) => connect(params, ctx.signal),

    expiry: async (params, ctx) => {
      const { chain, authorized, authorizationError } = await connect(params, ctx.signal);
      const leaf = chain[0] ?? {};
      return {
        host: params.host,
        commonName: leaf.commonName ?? null,
        validTo: leaf.validTo ?? null,
        daysRemaining: leaf.daysRemaining ?? null,
        authorized,
        authorizationError,
      };
    },

    check: async ({ hosts = [], warnDays = 30 }, ctx) => {
      const results = [];
      for (const [index, host] of hosts.entries()) {
        ctx.emit("progress", { done: index, total: hosts.length, host });
        try {
          const { chain } = await connect({ host }, ctx.signal);
          const leaf = chain[0] ?? {};
          results.push({
            host,
            daysRemaining: leaf.daysRemaining ?? null,
            validTo: leaf.validTo ?? null,
            warn: (leaf.daysRemaining ?? Infinity) <= warnDays,
          });
        } catch (error) {
          results.push({ host, error: error.message });
        }
      }
      ctx.emit("progress", { done: hosts.length, total: hosts.length });
      return { warnDays, results };
    },
  },
);
