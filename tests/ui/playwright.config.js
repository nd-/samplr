// The page is opened from ui/index.html as a file, the Tauri API faked:
// nothing to build nor serve.
const { defineConfig, devices } = require("@playwright/test");

module.exports = defineConfig({
  testDir: ".",
  testMatch: "*.spec.js",
  fullyParallel: true,
  reporter: "list",
  use: {
    ...devices["Desktop Chrome"],
    viewport: { width: 1400, height: 800 },
  },
});
