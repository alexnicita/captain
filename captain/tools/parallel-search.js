#!/usr/bin/env node

/**
 * Parallel Search CLI wrapper
 * Usage:
 *   PARALLEL_API_KEY=... node captain/tools/parallel-search.js --query "..." --count 5
 */

const fs = require("fs");
const os = require("os");
const path = require("path");

const API_URL = "https://api.parallel.ai/v1beta/search";
const BETA_HEADER = "search-extract-2025-10-10";

function loadApiKey() {
  if (process.env.PARALLEL_API_KEY) return process.env.PARALLEL_API_KEY.trim();

  const candidates = [
    path.join(os.homedir(), ".openclaw", "credentials", "parallel_api_key"),
    path.join(os.homedir(), ".parallel_api_key"),
  ];

  for (const p of candidates) {
    try {
      if (fs.existsSync(p)) {
        const v = fs.readFileSync(p, "utf8").trim();
        if (v) return v;
      }
    } catch {}
  }

  return "";
}

function parseArgs(argv) {
  const out = { count: 10, mode: "one-shot", maxCharsPerResult: 4000 };
  for (let i = 2; i < argv.length; i++) {
    const a = argv[i];
    const next = argv[i + 1];
    if (a === "--query" || a === "-q") {
      out.query = next; i++;
    } else if (a === "--count" || a === "-n") {
      out.count = Number(next); i++;
    } else if (a === "--mode") {
      out.mode = next; i++;
    } else if (a === "--max-chars") {
      out.maxCharsPerResult = Number(next); i++;
    } else if (a === "--help" || a === "-h") {
      out.help = true;
    }
  }
  return out;
}

function usage() {
  console.log(`parallel-search.js\n\nUsage:\n  PARALLEL_API_KEY=... node captain/tools/parallel-search.js --query "latest on..." [--count 5] [--mode one-shot|agentic|fast] [--max-chars 3000]\n`);
}

async function main() {
  const args = parseArgs(process.argv);
  if (args.help || !args.query) {
    usage();
    process.exit(args.help ? 0 : 1);
  }

  const apiKey = loadApiKey();
  if (!apiKey) {
    console.error(JSON.stringify({ error: "missing_parallel_api_key", message: "Set PARALLEL_API_KEY in environment or ~/.openclaw/credentials/parallel_api_key" }, null, 2));
    process.exit(2);
  }

  const body = {
    mode: args.mode,
    objective: args.query,
    search_queries: [args.query],
    max_results: Number.isFinite(args.count) ? args.count : 10,
    excerpts: {
      max_chars_per_result: Number.isFinite(args.maxCharsPerResult) ? args.maxCharsPerResult : 4000,
    },
  };

  const res = await fetch(API_URL, {
    method: "POST",
    headers: {
      "content-type": "application/json",
      "x-api-key": apiKey,
      "parallel-beta": BETA_HEADER,
    },
    body: JSON.stringify(body),
  });

  let data;
  try {
    data = await res.json();
  } catch {
    data = { raw: await res.text() };
  }

  if (!res.ok) {
    console.error(JSON.stringify({ status: res.status, error: data }, null, 2));
    process.exit(3);
  }

  const normalized = (data.results || []).map((r) => ({
    title: r.title || "",
    url: r.url || "",
    snippet: Array.isArray(r.excerpts) ? (r.excerpts[0] || "") : "",
    publish_date: r.publish_date || null,
  }));

  console.log(JSON.stringify({
    provider: "parallel",
    query: args.query,
    count: normalized.length,
    search_id: data.search_id || null,
    results: normalized,
  }, null, 2));
}

main().catch((err) => {
  console.error(JSON.stringify({ error: "unexpected_error", message: err.message }, null, 2));
  process.exit(9);
});
