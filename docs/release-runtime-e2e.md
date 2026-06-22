# Release and Runtime Verification

Use these checks before cutting or smoke-testing a build:

```bash
pnpm verify:runtime
pnpm verify:release
```

`verify:runtime` runs frontend lint, TypeScript, Vitest, and Rust unit tests. `verify:release` runs the frontend build and a debug `tauri build --no-bundle` smoke through the cross-platform `pnpm tauri` wrapper. It does not sign, notarize, or produce a release installer.

Manual runtime smoke:

1. Run `pnpm tauri dev`.
2. Open Add account and confirm pasted credentials use a password field.
3. Start and cancel an OAuth login; the browser callback page must not retain the query string and must send no-store/no-referrer headers.
4. Refresh a stored account and confirm error toasts do not show tokens, cookies, or email addresses.

Windows note: live tray and credential reads are still hardware-smoke items. The plaintext `accounts.json` store relies on the user profile ACL inherited from `%APPDATA%`; POSIX-only `0600`/`0700` permissions do not translate through Rust `std`.
