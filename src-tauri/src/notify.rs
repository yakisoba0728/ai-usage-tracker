//! Pure text builder for the anchor OS-native notification (FEAT-3, spec §6.4).
//!
//! The notification *content* is a pure function of (provider, account label,
//! outcome, automatic-vs-manual) so it is unit-testable in isolation. The actual
//! `tauri-plugin-notification` `.show()` call stays thin and untested — only this
//! string-building logic is the testable core (spec §8 "logic-in-lib").

use crate::model::Provider;

/// Canonical display name for a provider — mirrors the frontend `PROVIDER_LABEL`
/// and `lib.rs::provider_label`. Kept here so the pure builder has no Tauri deps.
pub fn provider_label(provider: Provider) -> &'static str {
    match provider {
        Provider::Claude => "Claude",
        Provider::Codex => "Codex",
        Provider::Gemini => "Gemini",
        Provider::Copilot => "GitHub Copilot",
        Provider::Cursor => "Cursor",
        Provider::Zai => "z.ai",
    }
}

/// The rendered OS-notification text: a short title and a one-line body. Both are
/// already localized to English (the OS notification is intentionally not routed
/// through the in-app i18next catalog — it fires from Rust while the webview may
/// be closed).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NotificationText {
    pub title: String,
    pub body: String,
}

/// Build the anchor-result OS notification text.
///
/// Inputs:
/// - `provider`  — which service the anchor targeted.
/// - `account`   — the account display string (email / workspace / label). When
///   `None`/blank, the body names only the provider (finding
///   `notif-missing-account-identity`: never leak a raw `stored:<uuid>` id, and
///   never imply a specific account we couldn't identify).
/// - `ok`        — whether the send succeeded.
/// - `is_auto`   — an AUTOMATIC (auto-anchor) send vs a manual "Send anchor now".
///   Automatic failures read differently from a manual one so a background
///   failure isn't mistaken for a button the user just pressed.
pub fn anchor_notification(
    provider: Provider,
    account: Option<&str>,
    ok: bool,
    is_auto: bool,
) -> NotificationText {
    let provider_name = provider_label(provider);
    // A blank account string is treated as absent so we never render an empty
    // "()" or a bare separator.
    let account = account.map(str::trim).filter(|a| !a.is_empty());

    // "Claude" or "Claude (person@example.com)".
    let subject = match account {
        Some(acc) => format!("{provider_name} ({acc})"),
        None => provider_name.to_string(),
    };

    let title = if ok {
        "Anchor sent".to_string()
    } else {
        "Anchor failed".to_string()
    };

    let body = match (ok, is_auto) {
        (true, true) => format!("Auto-anchored {subject}."),
        (true, false) => format!("Anchor message sent for {subject}."),
        (false, true) => format!("Auto-anchor failed for {subject}."),
        (false, false) => format!("Couldn't send anchor for {subject}."),
    };

    NotificationText { title, body }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn manual_success_names_provider_and_account() {
        let n = anchor_notification(Provider::Claude, Some("person@example.invalid"), true, false);
        assert_eq!(n.title, "Anchor sent");
        assert_eq!(
            n.body,
            "Anchor message sent for Claude (person@example.invalid)."
        );
    }

    #[test]
    fn auto_success_reads_as_automatic() {
        let n = anchor_notification(Provider::Zai, Some("z.ai workspace"), true, true);
        assert_eq!(n.title, "Anchor sent");
        // Auto sends read differently from a manual button press.
        assert_eq!(n.body, "Auto-anchored z.ai (z.ai workspace).");
    }

    #[test]
    fn manual_failure_differs_from_auto_failure() {
        let manual = anchor_notification(Provider::Codex, Some("acct"), false, false);
        let auto = anchor_notification(Provider::Codex, Some("acct"), false, true);
        assert_eq!(manual.title, "Anchor failed");
        assert_eq!(auto.title, "Anchor failed");
        assert_eq!(manual.body, "Couldn't send anchor for Codex (acct).");
        assert_eq!(auto.body, "Auto-anchor failed for Codex (acct).");
        assert_ne!(
            manual.body, auto.body,
            "an automatic failure must read differently from a manual one"
        );
    }

    #[test]
    fn missing_account_names_provider_only_never_a_raw_id() {
        // No account identity → name the provider only, never a `stored:<uuid>`
        // id (finding notif-missing-account-identity).
        let none = anchor_notification(Provider::Claude, None, true, false);
        assert_eq!(none.body, "Anchor message sent for Claude.");
        // A blank/whitespace account is treated as absent (no empty parens).
        let blank = anchor_notification(Provider::Claude, Some("   "), true, true);
        assert_eq!(blank.body, "Auto-anchored Claude.");
        assert!(!blank.body.contains("("), "no empty parentheses for a blank account");
    }

    #[test]
    fn provider_label_covers_every_provider() {
        assert_eq!(provider_label(Provider::Claude), "Claude");
        assert_eq!(provider_label(Provider::Codex), "Codex");
        assert_eq!(provider_label(Provider::Gemini), "Gemini");
        assert_eq!(provider_label(Provider::Copilot), "GitHub Copilot");
        assert_eq!(provider_label(Provider::Cursor), "Cursor");
        assert_eq!(provider_label(Provider::Zai), "z.ai");
    }
}
