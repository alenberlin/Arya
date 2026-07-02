//! Word error rate for the speech benchmark: Levenshtein distance over
//! normalized words (lowercased, punctuation stripped) divided by reference
//! length.

pub fn normalize_words(text: &str) -> Vec<String> {
    text.split_whitespace()
        .map(|w| {
            w.chars()
                .filter(|c| c.is_alphanumeric() || *c == '\'')
                .collect::<String>()
                .to_lowercase()
        })
        .filter(|w| !w.is_empty())
        .collect()
}

pub fn word_error_rate(reference: &str, hypothesis: &str) -> f64 {
    let r = normalize_words(reference);
    let h = normalize_words(hypothesis);
    if r.is_empty() {
        return if h.is_empty() { 0.0 } else { 1.0 };
    }
    // Standard DP edit distance, two-row.
    let mut prev: Vec<usize> = (0..=h.len()).collect();
    let mut curr = vec![0usize; h.len() + 1];
    for i in 1..=r.len() {
        curr[0] = i;
        for j in 1..=h.len() {
            let cost = usize::from(r[i - 1] != h[j - 1]);
            curr[j] = (prev[j] + 1).min(curr[j - 1] + 1).min(prev[j - 1] + cost);
        }
        std::mem::swap(&mut prev, &mut curr);
    }
    prev[h.len()] as f64 / r.len() as f64
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn identical_is_zero() {
        assert_eq!(word_error_rate("Hello, world!", "hello world"), 0.0);
    }

    #[test]
    fn one_substitution_in_four_words_is_quarter() {
        let wer = word_error_rate("ask not what your", "ask now what your");
        assert!((wer - 0.25).abs() < 1e-9);
    }

    #[test]
    fn empty_hypothesis_is_full_error() {
        assert_eq!(word_error_rate("a b c", ""), 1.0);
    }

    #[test]
    fn punctuation_and_case_are_ignored() {
        assert_eq!(
            word_error_rate("And so, my fellow Americans:", "and so my fellow americans"),
            0.0
        );
    }
}
