//! Answer grading, modelled on WaniKani's own checker plus Double-Check style near-miss handling.

use crate::kana;
use crate::model::{Kind, QType, Subject};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Verdict {
    Correct,
    Incorrect,
    /// Not graded: shake, show the message and let the user try again.
    Retry(String),
}

pub fn check(subject: &Subject, qtype: QType, input: &str, near_miss: bool) -> Verdict {
    match qtype {
        QType::Meaning => check_meaning(subject, input, near_miss),
        QType::Reading => check_reading(subject, input),
    }
}

fn normalize_meaning(s: &str) -> String {
    let lowered = s.to_lowercase().replace('-', " ");
    let cleaned: String = lowered
        .chars()
        .filter(|c| !matches!(c, '.' | ',' | '\'' | '’' | '"' | '/' | ':' | ';' | '!' | '?' | '(' | ')'))
        .collect();
    cleaned.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn levenshtein(a: &str, b: &str) -> usize {
    let a: Vec<char> = a.chars().collect();
    let b: Vec<char> = b.chars().collect();
    let mut prev: Vec<usize> = (0..=b.len()).collect();
    for (i, ca) in a.iter().enumerate() {
        let mut cur = vec![i + 1];
        for (j, cb) in b.iter().enumerate() {
            let cost = if ca == cb { 0 } else { 1 };
            cur.push((prev[j] + cost).min(prev[j + 1] + 1).min(cur[j] + 1));
        }
        prev = cur;
    }
    prev[b.len()]
}

/// WaniKani's typo allowance, by length of the expected answer.
fn tolerance(len: usize) -> usize {
    match len {
        0..=3 => 0,
        4..=5 => 1,
        6..=7 => 2,
        _ => 2 + len / 7,
    }
}

fn digits(s: &str) -> String {
    s.chars().filter(|c| c.is_ascii_digit()).collect()
}

fn check_meaning(subject: &Subject, input: &str, near_miss: bool) -> Verdict {
    if input.chars().any(kana::is_japanese) {
        return Verdict::Retry("The meaning is wanted here — answer in English.".into());
    }
    let answer = normalize_meaning(input);
    if answer.is_empty() {
        return Verdict::Retry(String::new());
    }
    if subject.blacklist.iter().any(|b| normalize_meaning(b) == answer) {
        return Verdict::Incorrect;
    }

    let accepted = subject
        .meanings
        .iter()
        .filter(|m| m.accepted)
        .map(|m| m.text.as_str())
        .chain(subject.whitelist.iter().map(String::as_str))
        .chain(subject.synonyms.iter().map(String::as_str));

    // Smallest amount by which the answer misses the allowed typo distance.
    let mut closest_miss = usize::MAX;
    for expected in accepted {
        let expected = normalize_meaning(expected);
        if expected.is_empty() || digits(&expected) != digits(&answer) {
            continue;
        }
        let dist = levenshtein(&answer, &expected);
        let tol = tolerance(expected.chars().count());
        if dist <= tol {
            return Verdict::Correct;
        }
        if expected.chars().count() >= 5 {
            closest_miss = closest_miss.min(dist - tol);
        }
    }

    if near_miss {
        if subject.kind.has_reading() {
            let as_kana = kana::to_hiragana(&kana::finalize(&input.to_lowercase()));
            if !as_kana.is_empty()
                && as_kana.chars().all(kana::is_kana)
                && subject
                    .readings
                    .iter()
                    .any(|r| kana::to_hiragana(&r.text) == as_kana)
            {
                return Verdict::Retry("That is the reading — the meaning is wanted here.".into());
            }
        }
        if closest_miss == 1 {
            return Verdict::Retry("Very close — check your spelling.".into());
        }
    }
    Verdict::Incorrect
}

fn check_reading(subject: &Subject, input: &str) -> Verdict {
    let answer = kana::finalize(input);
    if answer.is_empty() {
        return Verdict::Retry(String::new());
    }
    if !answer.chars().all(kana::is_kana) {
        return Verdict::Retry("The reading is wanted here — answer in kana.".into());
    }
    let answer = kana::to_hiragana(&answer);
    let matches = |r: &&crate::model::Reading| kana::to_hiragana(&r.text) == answer;

    if subject.readings.iter().filter(|r| r.accepted).any(|r| matches(&r)) {
        return Verdict::Correct;
    }
    if subject.kind == Kind::Kanji && subject.readings.iter().any(|r| matches(&r)) {
        let wanted = subject
            .readings
            .iter()
            .find(|r| r.accepted)
            .and_then(|r| r.kind.as_deref())
            .map(|k| match k {
                "onyomi" => "on'yomi",
                "kunyomi" => "kun'yomi",
                other => other,
            })
            .unwrap_or("other");
        return Verdict::Retry(format!("WaniKani is looking for the {wanted} reading."));
    }
    Verdict::Incorrect
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::demo;

    fn subject(slug: &str) -> Subject {
        demo::items()
            .into_iter()
            .map(|i| i.subject)
            .find(|s| s.slug == slug)
            .unwrap()
    }

    #[test]
    fn meaning_exact_and_typos() {
        let s = subject("大人");
        assert_eq!(check(&s, QType::Meaning, "Adult", true), Verdict::Correct);
        assert_eq!(check(&s, QType::Meaning, " adlt ", true), Verdict::Correct);
        assert_eq!(check(&s, QType::Meaning, "grown-up", true), Verdict::Correct);
        assert_eq!(check(&s, QType::Meaning, "child", true), Verdict::Incorrect);
    }

    #[test]
    fn meaning_short_words_are_strict() {
        let s = subject("大");
        assert_eq!(check(&s, QType::Meaning, "big", true), Verdict::Correct);
        assert_eq!(check(&s, QType::Meaning, "bag", true), Verdict::Incorrect);
    }

    #[test]
    fn meaning_blacklist_and_synonyms() {
        let s = subject("大");
        assert_eq!(check(&s, QType::Meaning, "fat", true), Verdict::Incorrect);
        assert_eq!(check(&s, QType::Meaning, "huge", true), Verdict::Correct);
    }

    #[test]
    fn meaning_digits_must_match() {
        let s = subject("二十日");
        assert_eq!(check(&s, QType::Meaning, "20th day", true), Verdict::Correct);
        assert_eq!(check(&s, QType::Meaning, "21th day", true), Verdict::Incorrect);
    }

    #[test]
    fn meaning_near_misses() {
        let s = subject("大人");
        assert!(matches!(check(&s, QType::Meaning, "おとな", false), Verdict::Retry(_)));
        assert!(matches!(check(&s, QType::Meaning, "otona", true), Verdict::Retry(_)));
        assert_eq!(check(&s, QType::Meaning, "otona", false), Verdict::Incorrect);
        // "adult" allows one typo; two is a near miss.
        assert!(matches!(check(&s, QType::Meaning, "adlut", true), Verdict::Retry(_)));
        assert_eq!(check(&s, QType::Meaning, "adlut", false), Verdict::Incorrect);
    }

    #[test]
    fn readings() {
        let s = subject("大人");
        assert_eq!(check(&s, QType::Reading, "おとな", true), Verdict::Correct);
        assert_eq!(check(&s, QType::Reading, "オトナ", true), Verdict::Correct);
        assert_eq!(check(&s, QType::Reading, "おとn", true), Verdict::Incorrect);
        assert!(matches!(check(&s, QType::Reading, "おtoな", true), Verdict::Correct));
        assert!(matches!(check(&s, QType::Reading, "adult", true), Verdict::Retry(_)));
    }

    #[test]
    fn kanji_wrong_reading_type_shakes() {
        let s = subject("大");
        assert_eq!(check(&s, QType::Reading, "たい", true), Verdict::Correct);
        assert_eq!(
            check(&s, QType::Reading, "おお", false),
            Verdict::Retry("WaniKani is looking for the on'yomi reading.".into())
        );
        assert_eq!(check(&s, QType::Reading, "ひと", true), Verdict::Incorrect);
    }
}
