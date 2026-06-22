//! Per-provider OAuth configuration (client ids/secrets/scopes/extra params) and
//! the authorize-URL builder. Codex uses OpenAI PKCE; Gemini uses Google's
//! installed-app loopback flow (Authorization Code + PKCE + client_secret).

use crate::model::Provider;

// Gemini CLI's public installed-app client (same values gemini-cli ships).
// The "secret" is not actually secret for installed apps (RFC 6749 §2.1);
// Google still requires it in the token exchange for this client type.
const GEMINI_CID: &str = "681255809395-oo8ft2oprdrnp9e3aqf6av3hmdib135j.apps.googleusercontent.com";
const GEMINI_CSEC: &str = "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";

#[derive(Clone)]
pub(crate) enum LoginMode {
    /// Localhost callback server; code captured automatically.
    LocalServer,
}

pub(crate) struct OAuthSpec {
    pub(crate) authorize_url: String,
    pub(crate) token_url: String,
    pub(crate) client_id: String,
    /// Optional client_secret for installed-app clients (Google). PKCE alone
    /// also works, but matching gemini-cli exactly (id+secret) is safest.
    pub(crate) client_secret: Option<String>,
    pub(crate) scope: String,
    pub(crate) extra_params: Vec<(&'static str, &'static str)>,
    pub(crate) mode: LoginMode,
}

pub(crate) fn spec_for(p: Provider) -> Option<OAuthSpec> {
    match p {
        Provider::Codex => Some(OAuthSpec {
            authorize_url: "https://auth.openai.com/oauth/authorize".into(),
            token_url: "https://auth.openai.com/oauth/token".into(),
            client_id: "app_EMoamEEZ73f0CkXaXp7hrann".into(),
            client_secret: None,
            scope: "openid profile email offline_access api.connectors.read api.connectors.invoke".into(),
            extra_params: vec![
                ("originator", "codex_cli_rs"),
                ("codex_cli_simplified_flow", "true"),
                ("id_token_add_organizations", "true"),
            ],
            mode: LoginMode::LocalServer,
        }),
        Provider::Gemini => Some(OAuthSpec {
            // Google Authorization Code + loopback redirect (gemini-cli pattern).
            // The Gemini client_id does NOT support the device-code grant —
            // googleapis.com/device/code returns "invalid_client: Invalid
            // client type" — so we use the same loopback flow gemini-cli uses.
            authorize_url: "https://accounts.google.com/o/oauth2/v2/auth".into(),
            token_url: "https://oauth2.googleapis.com/token".into(),
            client_id: GEMINI_CID.into(),
            client_secret: Some(GEMINI_CSEC.into()),
            scope: "https://www.googleapis.com/auth/cloud-platform https://www.googleapis.com/auth/userinfo.email https://www.googleapis.com/auth/userinfo.profile".into(),
            extra_params: vec![
                ("access_type", "offline"),
                ("prompt", "consent"),
            ],
            mode: LoginMode::LocalServer,
        }),
        _ => None,
    }
}

pub(crate) fn build_authorize_url(
    spec: &OAuthSpec,
    redirect_uri: &str,
    challenge: &str,
    state: &str,
) -> String {
    let mut params: Vec<(&str, String)> = vec![
        ("response_type", "code".into()),
        ("client_id", spec.client_id.clone()),
        ("redirect_uri", redirect_uri.into()),
        ("scope", spec.scope.clone()),
        ("code_challenge", challenge.into()),
        ("code_challenge_method", "S256".into()),
        ("state", state.into()),
    ];
    for (k, v) in &spec.extra_params {
        params.push((k, (*v).into()));
    }
    let query = params
        .iter()
        .map(|(k, v)| format!("{}={}", urlencoding::encode(k), urlencoding::encode(v)))
        .collect::<Vec<_>>()
        .join("&");
    format!("{}?{query}", spec.authorize_url)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn codex_authorize_url_has_required_params() {
        let spec = spec_for(Provider::Codex).unwrap();
        let url = build_authorize_url(&spec, "http://localhost:1455/auth/callback", "chk", "st");
        assert!(url.contains("originator=codex_cli_rs"));
        assert!(url.contains("codex_cli_simplified_flow=true"));
        assert!(url.contains("api.connectors.read"));
        assert!(url.contains("api.connectors.invoke"));
    }

    #[test]
    fn gemini_spec_uses_loopback_oauth_with_secret() {
        // Gemini must NOT use device-code (google rejects it with
        // `invalid_client: Invalid client type`); it uses Authorization Code +
        // loopback redirect like gemini-cli.
        let spec = spec_for(Provider::Gemini).expect("gemini must support OAuth");
        assert!(
            spec.client_secret.is_some(),
            "gemini client_secret required"
        );
        assert!(spec.authorize_url.contains("accounts.google.com"));
        assert!(spec.token_url.contains("oauth2.googleapis.com"));
        assert!(spec.scope.contains("cloud-platform"));
        assert!(spec.scope.contains("userinfo.email"));
        // access_type=offline + prompt=consent forces a refresh_token.
        assert!(spec
            .extra_params
            .iter()
            .any(|(k, v)| *k == "access_type" && *v == "offline"));
        assert!(spec
            .extra_params
            .iter()
            .any(|(k, v)| *k == "prompt" && *v == "consent"));
    }
}
