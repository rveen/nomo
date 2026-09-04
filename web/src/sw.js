// Service worker: make the application work with the network off.
//
// There is very little to cache, which is the point. The whole application is
// four files and no computation happens anywhere else, so "offline" is not a
// degraded mode with features missing — it is the same application. A worksheet
// that opens on a train gives the same answers as one that opens at a desk,
// because the engine that computes it is one of the four files.
//
// Cache-first, because the shell is versioned by cache name: a new build changes
// CACHE and the old one is deleted on activate. Network-first would put a round
// trip in front of every load to fetch bytes that are already correct.
//
// That reasoning is only sound if the name really does change, and for a long
// while it did not: this was a literal `"nomo-v1"` that no build step rewrote,
// so every rebuild shipped bytes a returning browser never saw. The symptom was
// not subtle once anything visible changed — a worksheet carrying figures
// rendered its base64 as prose, because the tab was still running an engine from
// before images existed — but nothing failed loudly, which is why it lasted.
//
// The name is now stamped by `web/build.mjs` with a digest of the shell it was
// built from, so it cannot drift from the bytes it names.

const CACHE = `nomo-${__SHELL_VERSION__}`;

const SHELL = [
  "./",
  "./index.html",
  "./style.css",
  "./bundle.js",
  "./nomo_wasm.wasm",
  // The math font. Precached rather than left to the runtime cache below,
  // because it is only requested once typesetting is switched on — so a reader
  // who goes offline and *then* turns it on would otherwise get a fraction laid
  // out by a font with no MATH table, which is the case this font exists to end.
  "./fonts/stix-two-math-subset.woff2",
];

self.addEventListener("install", (event) => {
  event.waitUntil(
    (async () => {
      const cache = await caches.open(CACHE);
      await cache.addAll(SHELL);
      // Take over immediately rather than waiting for every tab to close.
      // Nothing here holds state across versions, so there is nothing to
      // migrate and no reason to make the user close the tab.
      await self.skipWaiting();
    })(),
  );
});

self.addEventListener("activate", (event) => {
  event.waitUntil(
    (async () => {
      for (const name of await caches.keys()) {
        if (name !== CACHE) await caches.delete(name);
      }
      await self.clients.claim();
    })(),
  );
});

self.addEventListener("fetch", (event) => {
  const { request } = event;

  // Only GET, and only this origin. A worker that answers for anything else is
  // a worker that can serve a stale version of something it does not own.
  if (request.method !== "GET") return;
  if (new URL(request.url).origin !== self.location.origin) return;

  event.respondWith(
    (async () => {
      const cached = await caches.match(request, { ignoreSearch: true });
      if (cached) return cached;

      try {
        const response = await fetch(request);
        // Cache what we fetch, so a file added to the shell later still ends up
        // available offline without a version bump.
        if (response.ok && response.type === "basic") {
          const cache = await caches.open(CACHE);
          cache.put(request, response.clone());
        }
        return response;
      } catch (error) {
        // Offline and not in the cache. For a navigation, the shell is a better
        // answer than a browser error page: the application can run, and it is
        // the application the user asked for.
        if (request.mode === "navigate") {
          const shell = await caches.match("./index.html");
          if (shell) return shell;
        }
        throw error;
      }
    })(),
  );
});
