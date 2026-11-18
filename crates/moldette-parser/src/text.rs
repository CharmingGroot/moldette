pub(crate) fn without_line_break(raw: &str) -> &str {
    raw.strip_suffix("\r\n")
        .or_else(|| raw.strip_suffix('\n'))
        .unwrap_or(raw)
}

pub(crate) fn array_index(token: &str) -> Option<usize> {
    let leading_zero = token.len() > 1 && token.starts_with('0');
    if leading_zero || !token.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    token.parse().ok()
}
