//! Small shared helpers with no provider/domain coupling.

/// Uppercase the first character of `s`, leaving the rest unchanged. Returns an
/// empty string for empty input. Used for plan/tier labels across providers.
pub fn capitalize(s: &str) -> String {
    let mut c = s.chars();
    match c.next() {
        Some(f) => f.to_uppercase().collect::<String>() + c.as_str(),
        None => String::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn capitalizes_first_char_only() {
        assert_eq!(capitalize("pro"), "Pro");
        assert_eq!(capitalize("max 20x"), "Max 20x");
        assert_eq!(capitalize("A"), "A");
        assert_eq!(capitalize(""), "");
    }
}
