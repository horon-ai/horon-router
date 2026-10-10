import { defineConfig } from "vite";
import { tanstackRouter } from "@tanstack/router-plugin/vite";
import react from "@vitejs/plugin-react";
import tailwindcss from "@tailwindcss/vite";

// Development only: point the proxy at another backend with CPA_BACKEND=http://127.0.0.1:8318.
const backend = process.env.CPA_BACKEND || "http://127.0.0.1:8317";

export default defineConfig({
  base: "./",
  plugins: [tanstackRouter({ target: "react", autoCodeSplitting: false }), react(), tailwindcss()],
  // cors: false lets OPTIONS capability probes reach the backend instead of Vite.
  server: { port: 5173, cors: false, proxy: { "/v8": backend, "/healthz": backend } },
  // Keep the existing build minifier until the React bundle is measured.
  build: {
    target: "es2022",
    sourcemap: false,
    minify: "terser",
    terserOptions: { compress: { passes: 3 }, format: { comments: false } },
  },
});
