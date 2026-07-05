import react from "@vitejs/plugin-react";
import { defineConfig } from "vitest/config";

// Port 1420 is Tauri's convention; strictPort so the shell never attaches to
// a stranger's dev server.
export default defineConfig({
  plugins: [react()],
  clearScreen: false,
  server: {
    port: 1420,
    strictPort: true,
  },
  build: {
    rollupOptions: {
      // Split the React runtime out of the app bundle so the panels don't all
      // ship in one monolithic chunk.
      output: {
        manualChunks: { react: ["react", "react-dom"] },
      },
    },
  },
  test: {
    environment: "jsdom",
    globals: true,
    setupFiles: ["src/test/setup.ts"],
    include: ["src/test/**/*.test.{ts,tsx}"],
  },
});
