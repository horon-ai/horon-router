import { strict as assert } from "node:assert";
import { test } from "node:test";

test("editor rereads and does not PUT a stale draft", async () => {
  // Given the server has changed after the editor opened.
  Object.assign(globalThis, {
    location: new URL("http://localhost/management.html"),
    document: { documentElement: { lang: "" } },
    localStorage: { getItem: () => null },
  });
  Object.defineProperty(globalThis.navigator, "languages", { configurable: true, value: ["en"] });
  const { connect, disconnect } = await import("./api.ts");
  const { saveEditor } = await import("./editor-save.ts");
  const methods: string[] = [];
  const originalFetch = globalThis.fetch;
  globalThis.fetch = async (_input, init) => {
    methods.push(init?.method || "GET");
    return Response.json({ server: "new" });
  };
  connect("test-key", { unauthorized() {}, missing() {}, meta() {} });
  try {
    // When the reviewed draft is applied.
    await assert.rejects(saveEditor("/config/plugins/configs/example", { server: "old" }, { draft: true }, false, () => true),
      /server copy changed/i);
    // Then rereading took place and PUT was skipped.
    assert.deepEqual(methods, ["GET"]);
  } finally {
    disconnect();
    globalThis.fetch = originalFetch;
  }
});
