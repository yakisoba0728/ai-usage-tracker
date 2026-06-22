const EMAIL_RE = /\b[\w.+-]+@[\w.-]+\.[A-Za-z]{2,}\b/g;
const CLAUDE_TOKEN_RE = /\bsk-ant-[A-Za-z0-9._-]+\b/g;
const GITHUB_TOKEN_RE = /\b(?:gho|ghu|ghp|github_pat)_[A-Za-z0-9_]+\b/g;
const BEARER_TOKEN_RE = /\bBearer\s+[A-Za-z0-9._~+/-]+=*/gi;
const ASSIGNMENT_TOKEN_RE =
  /\b(access_token|refresh_token|sessionKey|session_key|token)=([^;\s]+)/gi;

export function scrubErrorText(value: string): string {
  return value
    .replace(ASSIGNMENT_TOKEN_RE, "$1=[redacted]")
    .replace(EMAIL_RE, "[redacted]")
    .replace(BEARER_TOKEN_RE, "Bearer [redacted]")
    .replace(CLAUDE_TOKEN_RE, "[redacted]")
    .replace(GITHUB_TOKEN_RE, "[redacted]");
}
