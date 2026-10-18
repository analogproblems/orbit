//! Small helpers for text Orbit shows or passes on.

/// Text from elsewhere as a sentence, ending in a full stop if it has no
/// ending of its own.
pub fn sentence(text: &str) -> String {
    let text = text.trim();
    if text.ends_with(['.', '!', '?']) {
        text.to_string()
    } else {
        format!("{text}.")
    }
}
/// `text` cut to at most `max` characters, as it is otherwise.
pub fn cut(text: &str, max: usize) -> String {
    match text.char_indices().nth(max) {
        Some((i, _)) => text[..i].to_string(),
        None => text.to_string(),
    }
}
/// `text` trimmed and, when longer than `max` characters, cut to `max` with
/// an ellipsis after it, for text shown in a sentence.
pub fn shorten(text: &str, max: usize) -> String {
    let text = text.trim();
    if text.chars().count() <= max {
        return text.to_string();
    }
    format!("{}…", cut(text, max))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn sentences_cuts_and_shortened_text() {
        assert_eq!(sentence(" It broke "), "It broke.");
        assert_eq!(sentence("Why?"), "Why?");
        assert_eq!(cut(" héllo ", 3), " hé");
        assert_eq!(cut("hi", 3), "hi");
        assert_eq!(shorten(" héllo ", 3), "hél…");
        assert_eq!(shorten(" hi ", 3), "hi");
    }
}
