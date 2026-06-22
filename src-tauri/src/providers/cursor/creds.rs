//! Cursor credential read: the sign-in JWT stored as a raw string at
//! `ItemTable[cursorAuth/accessToken]` in Cursor's `state.vscdb` SQLite DB.

use crate::providers::ProviderError;
use crate::secrets;

const ACCESS_KEY: &str = "cursorAuth/accessToken";

/// Read the Cursor access token from `state.vscdb` (raw string value).
pub(super) fn read_cursor_token() -> Result<String, ProviderError> {
    let db = secrets::cursor_state_db().ok_or_else(|| {
        ProviderError::NotLoggedIn("Cursor not installed / no state.vscdb".into())
    })?;
    // The DB file is known to exist (cursor_state_db checks), so an open/prepare
    // failure is an IO/lock/corruption condition, NOT "not signed in" — map it to
    // a retryable Network error rather than NotLoggedIn/Parse (B-14).
    let conn = rusqlite::Connection::open_with_flags(
        &db,
        rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY | rusqlite::OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )
    .map_err(|e| ProviderError::Network(format!("open state.vscdb: {e}")))?;
    let mut stmt = conn
        .prepare("SELECT value FROM ItemTable WHERE key = ? LIMIT 1")
        .map_err(|e| ProviderError::Network(format!("query state.vscdb: {e}")))?;
    // Distinguish "no token row" (genuinely not signed in) from a query/step
    // error (transient DB-busy/corruption) instead of swallowing both with .ok().
    let token: Option<String> = match stmt.query_row([ACCESS_KEY], |r| r.get::<_, String>(0)) {
        Ok(v) => Some(v),
        Err(rusqlite::Error::QueryReturnedNoRows) => None,
        Err(e) => return Err(ProviderError::Network(format!("read state.vscdb: {e}"))),
    };
    token.filter(|t| !t.is_empty()).ok_or_else(|| {
        ProviderError::NotLoggedIn(
            "Cursor token not found in state.vscdb (sign in to Cursor)".into(),
        )
    })
}
