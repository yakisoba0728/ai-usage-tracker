import js from "@eslint/js";
import globals from "globals";
import reactHooks from "eslint-plugin-react-hooks";
import reactRefresh from "eslint-plugin-react-refresh";
import tseslint from "typescript-eslint";

// Flat config. The load-bearing value is `react-hooks` (rules-of-hooks +
// exhaustive-deps) — `tsc` doesn't catch hook-dependency or rules-of-hooks
// defects (X-5). Scoped to `src/` only; the Rust app and build configs are out
// of scope. exhaustive-deps is a warning so pre-existing nits don't block CI.
export default tseslint.config(
  { ignores: ["dist", "node_modules", "src-tauri"] },
  {
    files: ["src/**/*.{ts,tsx}"],
    extends: [js.configs.recommended, ...tseslint.configs.recommended],
    languageOptions: {
      ecmaVersion: 2022,
      sourceType: "module",
      globals: { ...globals.browser },
    },
    plugins: {
      "react-hooks": reactHooks,
      "react-refresh": reactRefresh,
    },
    rules: {
      "react-hooks/rules-of-hooks": "error",
      "react-hooks/exhaustive-deps": "warn",
      "react-refresh/only-export-components": [
        "warn",
        { allowConstantExport: true },
      ],
    },
  },
);
