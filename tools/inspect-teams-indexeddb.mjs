const port = Number(process.env.CDP_PORT ?? process.argv[2] ?? 9223);
const threadId = process.env.TEAMS_THREAD_ID ?? process.argv[3] ?? "";

let nextId = 1;
const pending = new Map();
let socket;

async function fetchJson(url) {
  const response = await fetch(url);
  if (!response.ok) throw new Error(`${response.status} ${response.statusText}`);
  return response.json();
}

async function getTarget() {
  const targets = await fetchJson(`http://127.0.0.1:${port}/json/list`);
  const page = targets.find(
    (target) => target.type === "page" && target.url.includes("teams.microsoft.com"),
  );
  if (!page) throw new Error("No Teams page target found");
  return page;
}

function send(method, params = {}) {
  const id = nextId++;
  socket.send(JSON.stringify({ id, method, params }));
  return new Promise((resolve, reject) => {
    pending.set(id, { resolve, reject });
    setTimeout(() => {
      if (pending.delete(id)) reject(new Error(`CDP timeout: ${method}`));
    }, 20000).unref();
  });
}

function evaluateSource() {
  return `(${async function (threadId) {
    function summarize(value, depth = 0) {
      if (depth > 3) return typeOf(value);
      if (Array.isArray(value)) {
        return {
          type: "array",
          length: value.length,
          first: value.length ? summarize(value[0], depth + 1) : null,
        };
      }
      if (value && typeof value === "object") {
        const keys = Object.keys(value);
        const out = { type: "object", keys: keys.slice(0, 60) };
        for (const key of [
          "id",
          "threadId",
          "conversationId",
          "members",
          "memberIds",
          "roster",
          "participants",
          "users",
          "mris",
          "profiles",
          "displayName",
          "title",
        ]) {
          if (key in value) out[key] = summarize(value[key], depth + 1);
        }
        return out;
      }
      return typeOf(value);
    }

    function typeOf(value) {
      if (value === null) return "null";
      if (Array.isArray(value)) return "array";
      return typeof value;
    }

    function request(req) {
      return new Promise((resolve, reject) => {
        req.onsuccess = () => resolve(req.result);
        req.onerror = () => reject(req.error);
      });
    }

    function openDb(name) {
      return new Promise((resolve, reject) => {
        const req = indexedDB.open(name);
        req.onsuccess = () => resolve(req.result);
        req.onerror = () => reject(req.error);
        req.onblocked = () => reject(new Error("blocked"));
      });
    }

    async function sampleStore(db, storeName) {
      const tx = db.transaction(storeName, "readonly");
      const store = tx.objectStore(storeName);
      const samples = [];
      let count = 0;
      await new Promise((resolve) => {
        const cursorReq = store.openCursor();
        cursorReq.onsuccess = () => {
          const cursor = cursorReq.result;
          if (!cursor || samples.length >= 3) {
            resolve();
            return;
          }
          count += 1;
          const raw = JSON.stringify(cursor.value);
          const matchesThread = threadId && raw.includes(threadId);
          const looksRelevant =
            matchesThread ||
            /member|roster|participant|profile|conversation|thread|chat/i.test(
              `${storeName} ${raw.slice(0, 2000)}`,
            );
          if (looksRelevant) {
            samples.push({
              key: summarize(cursor.key),
              matchesThread,
              shape: summarize(cursor.value),
            });
          }
          cursor.continue();
        };
        cursorReq.onerror = () => resolve();
      });
      return { storeName, countSeen: count, samples };
    }

    const dbs = await indexedDB.databases();
    const result = [];
    for (const dbInfo of dbs) {
      const db = await openDb(dbInfo.name);
      const stores = Array.from(db.objectStoreNames);
      const relevantStoreNames = stores.filter((name) =>
        /chat|conversation|thread|member|roster|participant|people|profile|user/i.test(name),
      );
      const sampled = [];
      for (const storeName of relevantStoreNames.slice(0, 30)) {
        sampled.push(await sampleStore(db, storeName));
      }
      result.push({
        name: db.name,
        version: db.version,
        stores,
        sampled: sampled.filter((store) => store.samples.length > 0),
      });
      db.close();
    }
    return result;
  }})(${JSON.stringify(threadId)})`;
}

async function main() {
  const target = await getTarget();
  socket = new WebSocket(target.webSocketDebuggerUrl);
  socket.addEventListener("message", (event) => {
    const message = JSON.parse(event.data.toString());
    if (!message.id) return;
    const callback = pending.get(message.id);
    if (!callback) return;
    pending.delete(message.id);
    if (message.error) callback.reject(new Error(message.error.message));
    else callback.resolve(message.result);
  });
  await new Promise((resolve, reject) => {
    socket.addEventListener("open", resolve, { once: true });
    socket.addEventListener("error", reject, { once: true });
  });
  const result = await send("Runtime.evaluate", {
    expression: evaluateSource(),
    awaitPromise: true,
    returnByValue: true,
  });
  console.log(JSON.stringify(result.result.value, null, 2));
  socket.close();
}

main().catch((error) => {
  console.error(error);
  process.exit(1);
});
