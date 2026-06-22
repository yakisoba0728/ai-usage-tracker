//! The success/error HTML served on the localhost callback, the HTML escaping
//! used for provider error messages, and the security headers attached to the
//! HTML responses.

pub(crate) fn html_security_headers() -> Vec<tiny_http::Header> {
    [
        ("Content-Type", "text/html; charset=utf-8"),
        ("Cache-Control", "no-store"),
        ("Pragma", "no-cache"),
        ("Referrer-Policy", "no-referrer"),
    ]
    .into_iter()
    .map(|(name, value)| tiny_http::Header::from_bytes(name.as_bytes(), value.as_bytes()).unwrap())
    .collect()
}

pub(crate) fn success_html() -> String {
    r#"<!DOCTYPE html><html><head><meta charset="utf-8"><meta name="referrer" content="no-referrer"><title>Logged in</title>
<style>body{font-family:system-ui;display:flex;align-items:center;justify-content:center;height:100vh;margin:0;background:#111827;color:#e5e7eb}
.c{text-align:center}.ck{font-size:48px}</style><script>history.replaceState(null,"",location.pathname);</script></head>
<body><div class="c"><div class="ck">✓</div><h1>Logged in</h1><p>You can close this tab and return to AI Usage Tracker.</p></div></body></html>"#.to_string()
}

pub(crate) fn error_html(message: &str) -> String {
    let escaped = escape_html(message);
    format!(
        r#"<!DOCTYPE html><html><head><meta charset="utf-8"><meta name="referrer" content="no-referrer"><title>Login failed</title>
<style>body{{font-family:system-ui;display:flex;align-items:center;justify-content:center;height:100vh;margin:0;background:#111827;color:#e5e7eb}}.c{{max-width:640px;text-align:center}}</style><script>history.replaceState(null,"",location.pathname);</script></head>
<body><div class="c"><h1>Login failed</h1><p>{escaped}</p></div></body></html>"#
    )
}

fn escape_html(s: &str) -> String {
    s.chars()
        .flat_map(|c| match c {
            '&' => "&amp;".chars().collect::<Vec<_>>(),
            '<' => "&lt;".chars().collect::<Vec<_>>(),
            '>' => "&gt;".chars().collect::<Vec<_>>(),
            '"' => "&quot;".chars().collect::<Vec<_>>(),
            '\'' => "&#39;".chars().collect::<Vec<_>>(),
            _ => vec![c],
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn oauth_success_html_scrubs_query_and_blocks_referrers() {
        let html = success_html();
        assert!(
            html.contains("history.replaceState"),
            "query scrub missing: {html}"
        );
        assert!(html.contains("referrer") && html.contains("no-referrer"));
        assert!(html.contains("Logged in"));
    }

    #[test]
    fn oauth_error_html_escapes_provider_error() {
        let html = error_html(r#"bad <script>alert("x")</script> & retry"#);
        assert!(html.contains("&lt;script&gt;alert(&quot;x&quot;)&lt;/script&gt;"));
        assert!(html.contains("&amp; retry"));
        assert!(!html.contains("<script>alert"));
    }

    #[test]
    fn oauth_html_security_headers_disable_storage_and_referrers() {
        let headers = html_security_headers();
        let joined = headers
            .iter()
            .map(|h| format!("{}: {}", h.field, h.value))
            .collect::<Vec<_>>()
            .join("\n")
            .to_ascii_lowercase();
        assert!(joined.contains("cache-control: no-store"));
        assert!(joined.contains("pragma: no-cache"));
        assert!(joined.contains("referrer-policy: no-referrer"));
        assert!(joined.contains("content-type: text/html; charset=utf-8"));
    }
}
