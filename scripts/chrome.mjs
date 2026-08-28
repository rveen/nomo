// A very small Chrome DevTools Protocol client.
//
// Enough to open a page, wait for it to settle, emulate print media and ask the
// page questions. Node's built-in WebSocket does the transport, so this adds no
// dependency — which matters because everything under scripts/ is meant to be
// readable evidence rather than a stack to trust.
//
// Not a browser automation framework and should not grow into one. If a check
// ever needs more than this, that is the moment to reach for a real one.

import { spawn } from "node:child_process";
import { mkdtemp, rm } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";

const CANDIDATES = [
  "google-chrome",
  "google-chrome-stable",
  "chromium",
  "chromium-browser",
];

/** Launch headless Chrome and connect to it. Returns a session. */
export async function launch({ chrome } = {}) {
  const binaries = chrome ? [chrome] : CANDIDATES;
  const profile = await mkdtemp(join(tmpdir(), "nomo-chrome-"));

  let lastError;
  for (const binary of binaries) {
    try {
      return await start(binary, profile);
    } catch (error) {
      lastError = error;
    }
  }
  await rm(profile, { recursive: true, force: true });
  throw new Error(
    `could not launch a browser (tried ${binaries.join(", ")}): ${lastError?.message}`,
  );
}

async function start(binary, profile) {
  const child = spawn(
    binary,
    [
      "--headless",
      "--no-sandbox",
      "--disable-gpu",
      "--remote-debugging-port=0",
      `--user-data-dir=${profile}`,
      "about:blank",
    ],
    { stdio: ["ignore", "pipe", "pipe"] },
  );

  // Chrome prints the WebSocket endpoint to stderr once it is listening.
  const endpoint = await new Promise((resolve, reject) => {
    let buffered = "";
    const timer = setTimeout(
      () => reject(new Error("chrome did not report a debugging endpoint")),
      15000,
    );
    child.stderr.on("data", (chunk) => {
      buffered += chunk;
      const match = buffered.match(/ws:\/\/[^\s]+/);
      if (match) {
        clearTimeout(timer);
        resolve(match[0]);
      }
    });
    child.on("error", (error) => {
      clearTimeout(timer);
      reject(error);
    });
    child.on("exit", (code) => {
      clearTimeout(timer);
      reject(new Error(`chrome exited ${code}: ${buffered.slice(-300)}`));
    });
  });

  const socket = new WebSocket(endpoint);
  await new Promise((resolve, reject) => {
    socket.addEventListener("open", resolve, { once: true });
    socket.addEventListener("error", () => reject(new Error("cannot connect")), {
      once: true,
    });
  });

  let nextId = 1;
  const pending = new Map();
  const listeners = [];

  socket.addEventListener("message", (event) => {
    const message = JSON.parse(event.data);
    if (message.id && pending.has(message.id)) {
      const { resolve, reject } = pending.get(message.id);
      pending.delete(message.id);
      if (message.error) reject(new Error(message.error.message));
      else resolve(message.result);
    } else if (message.method) {
      for (const listener of listeners) listener(message);
    }
  });

  function send(method, params = {}, sessionId) {
    const id = nextId++;
    return new Promise((resolve, reject) => {
      pending.set(id, { resolve, reject });
      socket.send(JSON.stringify({ id, method, params, sessionId }));
    });
  }

  // Attach to a page target so Page/Runtime commands have somewhere to go.
  const { targetId } = await send("Target.createTarget", { url: "about:blank" });
  const { sessionId } = await send("Target.attachToTarget", {
    targetId,
    flatten: true,
  });

  const call = (method, params) => send(method, params, sessionId);
  await call("Page.enable");
  await call("Runtime.enable");

  return {
    /** Navigate and wait for the load event. */
    async goto(url) {
      const loaded = new Promise((resolve) => {
        const listener = (message) => {
          if (message.method === "Page.loadEventFired") {
            listeners.splice(listeners.indexOf(listener), 1);
            resolve();
          }
        };
        listeners.push(listener);
      });
      await call("Page.navigate", { url });
      await loaded;
    },

    /**
     * Run a script in every page before its own scripts run.
     *
     * The only way to stand in for a browser capability the running browser does
     * not have. Headless Chrome has no File System Access API, so without this
     * there would be no way to check the branch that Chrome and Edge users
     * actually take.
     */
    onNewDocument(source) {
      return call("Page.addScriptToEvaluateOnNewDocument", { source });
    },

    /** Reload, and wait for the load event. */
    async reload() {
      const loaded = new Promise((resolve) => {
        const listener = (message) => {
          if (message.method === "Page.loadEventFired") {
            listeners.splice(listeners.indexOf(listener), 1);
            resolve();
          }
        };
        listeners.push(listener);
      });
      await call("Page.reload");
      await loaded;
    },

    /**
     * Cut the page off from the network.
     *
     * The service worker's whole purpose is to make the application work in this
     * state, so being able to produce it is what turns "offline support" from a
     * claim into a check.
     */
    async setOffline(offline) {
      await call("Network.enable");
      await call("Network.emulateNetworkConditions", {
        offline,
        latency: 0,
        downloadThroughput: offline ? 0 : -1,
        uploadThroughput: offline ? 0 : -1,
      });
    },

    /** Type into whatever has focus, as a user would. */
    type(text) {
      return call("Input.insertText", { text });
    },

    /** Evaluate an expression in the page and return its value. */
    async evaluate(expression) {
      const result = await call("Runtime.evaluate", {
        expression,
        awaitPromise: true,
        returnByValue: true,
      });
      if (result.exceptionDetails) {
        throw new Error(
          result.exceptionDetails.exception?.description ??
            result.exceptionDetails.text,
        );
      }
      return result.result.value;
    },

    /** Render the page as it would be printed, or back as it is on screen. */
    setPrintMedia(on) {
      return call("Emulation.setEmulatedMedia", { media: on ? "print" : "" });
    },

    async close() {
      try {
        socket.close();
      } catch {
        /* already gone */
      }

      // Wait for Chrome to actually exit before removing its profile. It writes
      // its cache on the way out, and deleting the directory underneath it fails
      // with ENOTEMPTY — which then crashes the caller in place of whatever it
      // was really reporting.
      const exited = new Promise((resolve) => child.once("exit", resolve));
      child.kill();
      await Promise.race([
        exited,
        new Promise((resolve) => setTimeout(resolve, 3000)),
      ]);

      // A leftover temporary directory is not worth failing a check over.
      await rm(profile, {
        recursive: true,
        force: true,
        maxRetries: 5,
        retryDelay: 100,
      }).catch(() => {});
    },
  };
}
