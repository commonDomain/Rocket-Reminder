import { resolve } from "node:path";
import { defineConfig } from "vite";

export default defineConfig({
  clearScreen: false,
  server: { port: 1420, strictPort: true },
  build: {
    rollupOptions: {
      input: {
        reminders: resolve(__dirname, "index.html"),
        addReminder: resolve(__dirname, "add.html"),
        animation: resolve(__dirname, "animation.html"),
      },
    },
  },
});
