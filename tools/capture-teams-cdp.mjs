import fs from "node:fs/promises";

const port = Number(process.env.CDP_PORT ?? process.argv[2] ?? 9223);
const durationMs = Number(process.env.CDP_DURATION_MS ?? process.argv[3] ?? 180000);
const startUrl = process.env.CDP_START_URL ?? "https://teams.microsoft.com/v2/";
const outPath =
  process.env.CDP_OUT ??
  `teams-cdp-${new Date().toISOString().replace(/[:.]/g, "-")}.json`;

const requests = new Map();
const findings = [];
let nextId = 1;
let socket;
const pending = new Map();

function interestingUrl(url) {
  return (
    url.includes("teams.microsoft.com/api/") ||
    url.includes(".ng.msg.teams.microsoft.com/") ||
    url.includes("chatsvc") ||
    url.includes("users/fetch") ||
    url.includes("profilesearch") ||
    url.includes("conversations") ||
    url.includes("roster") ||
    url.includes("members")
  );
}

function candidateScore(url, summary) {
  const text = `${url} ${JSON.stringify(summary)}`.toLowerCase();
  let score = 0;
  for (const needle of [
    "member",
    "members",
    "roster",
    "participant",
    "participants",
    "profile",
    "profiles",
    "users",
    "fetch",
    "conversation",
    "thread",
  ]) {
    if (text.includes(needle)) score += 1;
  }
  return score;
}

function summarizeJson(value, depth = 0) {
  if (depth > 3) return kind(value);
  if (Array.isArray(value)) {
    return {
      type: "array",
      length: value.length,
      first: value.length > 0 ? summarizeJson(value[0], depth + 1) : null,
    };
  }
  if (value && typeof value === "object") {
    const entries = Object.entries(value);
    const summary = { type: "object", keys: entries.map(([key]) => key).slice(0, 40) };
    for (const key of [
      "members",
      "roster",
      "participants",
      "users",
      "value",
      "chats",
      "teams",
      "channels",
      "thread",
      "conversation",
    ]) {
      if (key in value) summary[key] = summarizeJson(value[key], depth + 1);
    }
    return summary;
  }
  return kind(value);
}

function kind(value) {
  if (value === null) return "null";
  if (Array.isArray(value)) return `array(${value.length})`;
  return typeof value;
}

function summarizeBody(body, base64Encoded) {
  if (base64Encoded) return { type: "base64", length: body.length };
  const trimmed = body.trim();
  if (!trimmed) return { type: "empty" };
  try {
    return summarizeJson(JSON.parse(trimmed));
  } catch {
    return { type: "text", length: trimmed.length, sample: trimmed.slice(0, 120) };
  }
}

async function fetchJson(url, options) {
  const response = await fetch(url, options);
  if (!response.ok) {
    throw new Error(`${response.status} ${response.statusText}: ${await response.text()}`);
  }
  return response.json();
}

async function getTarget() {
  const base = `http://127.0.0.1:${port}`;
  const targets = await fetchJson(`${base}/json/list`);
  const existing = targets.find(
    (target) => target.type === "page" && target.url.includes("teams.microsoft.com"),
  );
  if (existing) return existing;
  return fetchJson(`${base}/json/new?${encodeURIComponent(startUrl)}`, { method: "PUT" });
}

function send(method, params = {}) {
  const id = nextId++;
  socket.send(JSON.stringify({ id, method, params }));
  return new Promise((resolve, reject) => {
    pending.set(id, { resolve, reject });
    setTimeout(() => {
      if (pending.delete(id)) reject(new Error(`CDP timeout: ${method}`));
    }, 10000).unref();
  });
}

async function onMessage(raw) {
  const message = JSON.parse(raw.toString());
  if (message.id) {
    const callback = pending.get(message.id);
    if (!callback) return;
    pending.delete(message.id);
    if (message.error) callback.reject(new Error(message.error.message));
    else callback.resolve(message.result);
    return;
  }

  if (message.method === "Network.requestWillBeSent") {
    const { requestId, request } = message.params;
    if (!interestingUrl(request.url)) return;
    requests.set(requestId, {
      url: request.url,
      method: request.method,
      postDataShape: request.postData ? summarizeBody(request.postData, false) : null,
    });
    return;
  }

  if (message.method === "Network.responseReceived") {
    const item = requests.get(message.params.requestId);
    if (!item) return;
    item.status = message.params.response.status;
    item.mimeType = message.params.response.mimeType;
    item.responseUrl = message.params.response.url;
    return;
  }

  if (message.method !== "Network.loadingFinished") return;
  const item = requests.get(message.params.requestId);
  if (!item || item.done) return;
  item.done = true;
  try {
    const body = await send("Network.getResponseBody", { requestId: message.params.requestId });
    item.bodyShape = summarizeBody(body.body, body.base64Encoded);
  } catch (error) {
    item.bodyShape = { type: "unavailable", error: String(error.message ?? error) };
  }

  item.score = candidateScore(item.responseUrl ?? item.url, item.bodyShape);
  if (item.score > 0) {
    findings.push(item);
    console.log(`${item.status ?? "-"} ${item.method} ${item.responseUrl ?? item.url}`);
  }
}

async function main() {
  const target = await getTarget();
  socket = new WebSocket(target.webSocketDebuggerUrl);
  socket.addEventListener("message", (event) => {
    onMessage(event.data).catch((error) => console.error(error));
  });
  await new Promise((resolve, reject) => {
    socket.addEventListener("open", resolve, { once: true });
    socket.addEventListener("error", reject, { once: true });
  });

  await send("Network.enable", {
    maxTotalBufferSize: 100_000_000,
    maxResourceBufferSize: 25_000_000,
  });
  await send("Page.enable");
  await send("Page.navigate", { url: startUrl });
  console.log(`Capturing Teams network for ${Math.round(durationMs / 1000)}s`);
  console.log(`Output: ${outPath}`);

  await new Promise((resolve) => setTimeout(resolve, durationMs));
  findings.sort((a, b) => b.score - a.score);
  await fs.writeFile(outPath, JSON.stringify(findings, null, 2));
  socket.close();
  console.log(`Wrote ${findings.length} findings to ${outPath}`);
}

main().catch((error) => {
  console.error(error);
  process.exit(1);
});
