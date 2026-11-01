//! Word wrapping for the build log box.

/// Characters per line and lines shown in the build log box.
pub const LOG_COLS: usize = 38;
pub const LOG_ROWS: usize = 8;

/// Breaks `line` into pieces of at most `cols` characters, at spaces when it can.
pub fn wrap(line: &str, cols: usize) -> Vec<String> {
    let mut out = Vec::new();
    let mut cur = String::new();
    for word in line.split(' ') {
        let mut word = word;
        while word.chars().count() > cols {
            let cut = word.char_indices().nth(cols).map_or(word.len(), |(i, _)| i);
            if !cur.is_empty() {
                out.push(std::mem::take(&mut cur));
            }
            out.push(word[..cut].to_string());
            word = &word[cut..];
        }
        let extra = usize::from(!cur.is_empty());
        if cur.chars().count() + extra + word.chars().count() > cols {
            out.push(std::mem::take(&mut cur));
        }
        if !cur.is_empty() {
            cur.push(' ');
        }
        cur.push_str(word);
    }
    out.push(cur);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn long_lines_wrap_within_the_width() {
        let lines = wrap(
            "Bake: 131 lit faces, 63 ms. Build finished in 76 ms",
            LOG_COLS,
        );
        assert!(lines.len() >= 2);
        assert!(
            lines.iter().all(|l| l.chars().count() <= LOG_COLS),
            "{lines:?}"
        );
        assert_eq!(
            lines.join(" "),
            "Bake: 131 lit faces, 63 ms. Build finished in 76 ms"
        );
        let long = wrap(&"x".repeat(100), 10);
        assert!(long.iter().all(|l| l.len() <= 10));
    }
}
