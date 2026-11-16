// SPDX-License-Identifier: GPL-3.0-or-later
import js from "@eslint/js";
import globals from "globals";
import tseslint from "typescript-eslint";

// Modules and globals that must never appear in src/: the package has to run
// on Hermes (React Native) as well as on Node, so no Node-only API is allowed.
const nodeModules = [
  "buffer",
  "crypto",
  "fs",
  "path",
  "os",
  "stream",
  "util",
  "events",
  "child_process",
  "net",
  "http",
  "https",
  "zlib",
];

export default tseslint.config(
  { ignores: ["dist/**", "node_modules/**"] },
  js.configs.recommended,
  ...tseslint.configs.recommendedTypeChecked,
  {
    languageOptions: {
      parserOptions: {
        projectService: {
          allowDefaultProject: ["eslint.config.js"],
        },
        tsconfigRootDir: import.meta.dirname,
      },
    },
  },
  {
    files: ["**/*.js"],
    ...tseslint.configs.disableTypeChecked,
    languageOptions: { globals: { ...globals.node } },
  },
  {
    files: ["src/**/*.ts"],
    rules: {
      "no-restricted-imports": [
        "error",
        {
          patterns: [
            {
              group: ["node:*"],
              message: "Node APIs are forbidden in src/ (must run on Hermes).",
            },
            {
              group: nodeModules,
              message: "Node APIs are forbidden in src/ (must run on Hermes).",
            },
          ],
          paths: nodeModules.map((name) => ({
            name,
            message: "Node APIs are forbidden in src/ (must run on Hermes).",
          })),
        },
      ],
      "no-restricted-globals": [
        "error",
        { name: "Buffer", message: "Use Uint8Array; Buffer is Node-only." },
        { name: "process", message: "process is Node-only." },
        { name: "require", message: "Use ESM imports; require is Node-only." },
        { name: "__dirname", message: "Node-only." },
        { name: "__filename", message: "Node-only." },
      ],
    },
  },
);
