import { strict as assert } from "node:assert";
import { test } from "node:test";

test("replace rereads and skips PUT when the value changed", async () => {
  // Given a signed-in session and a server value different from the editor's snapshot.
  Object.assign(globalThis, {
    location: new URL("http://localhost/management.html"),
    document: { documentElement: { lang: "" } },
    localStorage: { getItem: () => null },
  });
  Object.defineProperty(globalThis.navigator, "languages", { configurable: true, value: ["en"] });
  const { connect, disconnect } = await import("./api.ts");
  const { replace } = await import("./queries.ts");
  const methods: string[] = [];
  const originalFetch = globalThis.fetch;
  globalThis.fetch = async (_input, init) => {
    methods.push(init?.method || "GET");
    return new Response(JSON.stringify(["server"]), { status: 200 });
  };
  connect("test-key", { unauthorized() {}, missing() {}, meta() {} });
  try {
    // When replacing a stale value.
    await assert.rejects(replace("access/api-keys", ["editor"], ["new"]), /changed/i);
    // Then a GET happened, but no PUT.
    assert.deepEqual(methods, ["GET"]);
  } finally {
    disconnect();
    globalThis.fetch = originalFetch;
  }
});
