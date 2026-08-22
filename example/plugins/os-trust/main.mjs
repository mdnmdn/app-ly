// OS bridge — a sidecar plugin that reaches for things the shell has no
// Rust code for: platform APIs, a CLI tool, and files outside dataPath.

import os from "node:os";
import { readFile } from "node:fs/promises";
import { execFile } from "node:child_process";
import { X509Certificate } from "node:crypto";
import { promisify } from "node:util";
import { serve } from "../sidecar-sdk.mjs";

const run = promisify(execFile);

/** Where each platform keeps its trust anchors, and how to get at them. */
const TRUST_SOURCES = {
  darwin: async () => {
    const { stdout } = await run(
      "security",
      ["find-certificate", "-a", "-p", "/System/Library/Keychains/SystemRootCertificates.keychain"],
      { maxBuffer: 32 * 1024 * 1024 },
    );
    return { source: "security(1) + SystemRootCertificates.keychain", pem: stdout };
  },
  linux: async () => {
    const candidates = [
      "/etc/ssl/certs/ca-certificates.crt",
      "/etc/pki/tls/certs/ca-bundle.crt",
      "/etc/ssl/cert.pem",
    ];
    for (const path of candidates) {
      try {
        return { source: path, pem: await readFile(path, "utf8") };
      } catch {
        continue;
      }
    }
    throw new Error(`no CA bundle found (looked in ${candidates.join(", ")})`);
  },
  win32: async () => {
    const { stdout } = await run("certutil", ["-store", "-user", "Root"], {
      maxBuffer: 32 * 1024 * 1024,
    });
    return { source: "certutil -store -user Root", pem: stdout, parsed: false };
  },
};

const PEM_BLOCK = /-----BEGIN CERTIFICATE-----[\s\S]*?-----END CERTIFICATE-----/g;

function parseAnchors(pem, limit) {
  const anchors = [];
  for (const block of pem.match(PEM_BLOCK) ?? []) {
    if (anchors.length >= limit) break;
    try {
      const cert = new X509Certificate(block);
      anchors.push({
        subject: cert.subject.replace(/\n/g, ", "),
        validTo: cert.validTo,
        fingerprint256: cert.fingerprint256,
        ca: cert.ca,
      });
    } catch {
      // A block certutil printed as text, or a malformed entry — skip it.
    }
  }
  return anchors;
}

serve(
  {
    name: "os",
    apiVersion: 1,
    methods: [
      { name: "platform", description: "Machine and OS facts from the Node os module" },
      { name: "env", description: "Selected environment variables", params: { keys: "string[]" } },
      {
        name: "trustAnchors",
        description: "Certificates in the OS trust store",
        params: { limit: "number?" },
      },
    ],
  },
  {
    platform: () => ({
      platform: os.platform(),
      release: os.release(),
      arch: os.arch(),
      hostname: os.hostname(),
      uptimeSeconds: Math.round(os.uptime()),
      cpus: os.cpus().length,
      totalMemMb: Math.round(os.totalmem() / 1024 / 1024),
      freeMemMb: Math.round(os.freemem() / 1024 / 1024),
      user: os.userInfo().username,
      homedir: os.homedir(),
    }),

    env: ({ keys = [] }) =>
      Object.fromEntries(keys.map((key) => [key, process.env[key] ?? null])),

    trustAnchors: async ({ limit = 25 }) => {
      const source = TRUST_SOURCES[os.platform()];
      if (!source) throw new Error(`no trust store reader for ${os.platform()}`);

      const { source: label, pem, parsed = true } = await source();
      const anchors = parsed ? parseAnchors(pem, limit) : [];
      const total = (pem.match(PEM_BLOCK) ?? []).length;
      return {
        platform: os.platform(),
        source: label,
        total,
        shown: anchors.length,
        anchors,
        note: parsed ? null : "certutil output is not PEM; anchors are not parsed on Windows",
      };
    },
  },
);
