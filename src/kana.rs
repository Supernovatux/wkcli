//! Romaji → kana conversion for reading answers (wanakana / IME style).

const TABLE: &[(&str, &str)] = &[
    ("a", "あ"), ("i", "い"), ("u", "う"), ("e", "え"), ("o", "お"),
    ("ka", "か"), ("ki", "き"), ("ku", "く"), ("ke", "け"), ("ko", "こ"),
    ("sa", "さ"), ("si", "し"), ("shi", "し"), ("su", "す"), ("se", "せ"), ("so", "そ"),
    ("ta", "た"), ("ti", "ち"), ("chi", "ち"), ("tu", "つ"), ("tsu", "つ"), ("te", "て"), ("to", "と"),
    ("na", "な"), ("ni", "に"), ("nu", "ぬ"), ("ne", "ね"), ("no", "の"),
    ("ha", "は"), ("hi", "ひ"), ("hu", "ふ"), ("fu", "ふ"), ("he", "へ"), ("ho", "ほ"),
    ("ma", "ま"), ("mi", "み"), ("mu", "む"), ("me", "め"), ("mo", "も"),
    ("ya", "や"), ("yu", "ゆ"), ("yo", "よ"),
    ("ra", "ら"), ("ri", "り"), ("ru", "る"), ("re", "れ"), ("ro", "ろ"),
    ("wa", "わ"), ("wi", "うぃ"), ("we", "うぇ"), ("wo", "を"),
    ("ga", "が"), ("gi", "ぎ"), ("gu", "ぐ"), ("ge", "げ"), ("go", "ご"),
    ("za", "ざ"), ("zi", "じ"), ("ji", "じ"), ("zu", "ず"), ("ze", "ぜ"), ("zo", "ぞ"),
    ("da", "だ"), ("di", "ぢ"), ("du", "づ"), ("dzu", "づ"), ("de", "で"), ("do", "ど"),
    ("ba", "ば"), ("bi", "び"), ("bu", "ぶ"), ("be", "べ"), ("bo", "ぼ"),
    ("pa", "ぱ"), ("pi", "ぴ"), ("pu", "ぷ"), ("pe", "ぺ"), ("po", "ぽ"),
    ("kya", "きゃ"), ("kyu", "きゅ"), ("kyo", "きょ"),
    ("gya", "ぎゃ"), ("gyu", "ぎゅ"), ("gyo", "ぎょ"),
    ("sya", "しゃ"), ("syu", "しゅ"), ("syo", "しょ"),
    ("sha", "しゃ"), ("shu", "しゅ"), ("she", "しぇ"), ("sho", "しょ"),
    ("zya", "じゃ"), ("zyu", "じゅ"), ("zyo", "じょ"),
    ("ja", "じゃ"), ("ju", "じゅ"), ("je", "じぇ"), ("jo", "じょ"),
    ("jya", "じゃ"), ("jyu", "じゅ"), ("jyo", "じょ"),
    ("tya", "ちゃ"), ("tyu", "ちゅ"), ("tyo", "ちょ"),
    ("cha", "ちゃ"), ("chu", "ちゅ"), ("che", "ちぇ"), ("cho", "ちょ"),
    ("cya", "ちゃ"), ("cyu", "ちゅ"), ("cyo", "ちょ"),
    ("dya", "ぢゃ"), ("dyu", "ぢゅ"), ("dyo", "ぢょ"),
    ("nya", "にゃ"), ("nyu", "にゅ"), ("nyo", "にょ"),
    ("hya", "ひゃ"), ("hyu", "ひゅ"), ("hyo", "ひょ"),
    ("bya", "びゃ"), ("byu", "びゅ"), ("byo", "びょ"),
    ("pya", "ぴゃ"), ("pyu", "ぴゅ"), ("pyo", "ぴょ"),
    ("mya", "みゃ"), ("myu", "みゅ"), ("myo", "みょ"),
    ("rya", "りゃ"), ("ryu", "りゅ"), ("ryo", "りょ"),
    ("fa", "ふぁ"), ("fi", "ふぃ"), ("fe", "ふぇ"), ("fo", "ふぉ"),
    ("va", "ゔぁ"), ("vi", "ゔぃ"), ("vu", "ゔ"), ("ve", "ゔぇ"), ("vo", "ゔぉ"),
    ("thi", "てぃ"), ("thu", "てゅ"), ("dhi", "でぃ"), ("dhu", "でゅ"),
    ("twu", "とぅ"), ("dwu", "どぅ"),
    ("xa", "ぁ"), ("xi", "ぃ"), ("xu", "ぅ"), ("xe", "ぇ"), ("xo", "ぉ"),
    ("la", "ぁ"), ("li", "ぃ"), ("lu", "ぅ"), ("le", "ぇ"), ("lo", "ぉ"),
    ("xya", "ゃ"), ("xyu", "ゅ"), ("xyo", "ょ"),
    ("lya", "ゃ"), ("lyu", "ゅ"), ("lyo", "ょ"),
    ("xtu", "っ"), ("xtsu", "っ"), ("ltu", "っ"), ("ltsu", "っ"), ("xwa", "ゎ"),
];

fn lookup(key: &str) -> Option<&'static str> {
    TABLE.iter().find(|(k, _)| *k == key).map(|(_, v)| *v)
}

fn is_prefix_of_key(s: &str) -> bool {
    TABLE.iter().any(|(k, _)| k.len() > s.len() && k.starts_with(s))
}

fn is_vowel(c: char) -> bool {
    matches!(c, 'a' | 'i' | 'u' | 'e' | 'o')
}

pub fn is_kana(c: char) -> bool {
    matches!(c, '\u{3041}'..='\u{3096}' | '\u{30A1}'..='\u{30FA}' | 'ー')
}

pub fn is_japanese(c: char) -> bool {
    is_kana(c) || matches!(c, '\u{4E00}'..='\u{9FFF}' | '\u{3400}'..='\u{4DBF}' | '々')
}

/// Katakana → hiragana, everything else unchanged.
pub fn to_hiragana(s: &str) -> String {
    s.chars()
        .map(|c| match c {
            '\u{30A1}'..='\u{30F6}' => char::from_u32(c as u32 - 0x60).unwrap_or(c),
            _ => c,
        })
        .collect()
}

fn to_katakana(s: &str) -> String {
    s.chars()
        .map(|c| match c {
            '\u{3041}'..='\u{3096}' => char::from_u32(c as u32 + 0x60).unwrap_or(c),
            _ => c,
        })
        .collect()
}

/// Convert romaji in `s` to kana. Non-romaji characters pass through.
/// With `ime` set, an incomplete trailing syllable ("k", "ny", "n") is left as typed
/// so the next keystroke can complete it. Uppercase romaji gives katakana.
pub fn romaji_to_kana(s: &str, ime: bool) -> String {
    let chars: Vec<char> = s.chars().collect();
    let n = chars.len();
    let mut out = String::new();
    let mut i = 0;
    while i < n {
        let orig = chars[i];
        let c = orig.to_ascii_lowercase();
        if c == '-' {
            out.push('ー');
            i += 1;
            continue;
        }
        if !c.is_ascii_alphabetic() {
            out.push(orig);
            i += 1;
            continue;
        }
        let kata = orig.is_ascii_uppercase();
        let emit = |out: &mut String, kana: &str| {
            if kata {
                out.push_str(&to_katakana(kana));
            } else {
                out.push_str(kana);
            }
        };
        let next = chars.get(i + 1).map(|c| c.to_ascii_lowercase());

        if c == 'n' {
            match next {
                None => {
                    if ime {
                        out.push(orig);
                    } else {
                        emit(&mut out, "ん");
                    }
                    i += 1;
                    continue;
                }
                Some('n') | Some('\'') => {
                    emit(&mut out, "ん");
                    i += 2;
                    continue;
                }
                Some(x) if is_vowel(x) || x == 'y' => {}
                Some(_) => {
                    emit(&mut out, "ん");
                    i += 1;
                    continue;
                }
            }
        } else if !is_vowel(c) && (next == Some(c) || (c == 't' && next == Some('c'))) {
            emit(&mut out, "っ");
            i += 1;
            continue;
        }

        let mut matched = false;
        for len in (1..=4.min(n - i)).rev() {
            let slice = &chars[i..i + len];
            if !slice.iter().all(|c| c.is_ascii_alphabetic()) {
                continue;
            }
            let key: String = slice.iter().map(|c| c.to_ascii_lowercase()).collect();
            if let Some(kana) = lookup(&key) {
                emit(&mut out, kana);
                i += len;
                matched = true;
                break;
            }
        }
        if matched {
            continue;
        }
        let rest: String = chars[i..].iter().map(|c| c.to_ascii_lowercase()).collect();
        if ime && rest.chars().all(|c| c.is_ascii_alphabetic()) && is_prefix_of_key(&rest) {
            out.extend(&chars[i..]);
            break;
        }
        out.push(orig);
        i += 1;
    }
    out
}

/// Live conversion: re-convert the trailing romaji run of the input buffer in place.
pub fn convert_tail(buf: &mut String) {
    let start = buf
        .char_indices()
        .rev()
        .take_while(|(_, c)| c.is_ascii_alphabetic() || *c == '-' || *c == '\'')
        .last()
        .map(|(i, _)| i);
    if let Some(start) = start {
        let converted = romaji_to_kana(&buf[start..], true);
        buf.truncate(start);
        buf.push_str(&converted);
    }
}

/// Final conversion at submit time (a trailing "n" becomes ん).
pub fn finalize(s: &str) -> String {
    romaji_to_kana(s.trim(), false)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn typed(s: &str) -> String {
        let mut buf = String::new();
        for c in s.chars() {
            buf.push(c);
            convert_tail(&mut buf);
        }
        buf
    }

    #[test]
    fn basic() {
        assert_eq!(finalize("otona"), "おとな");
        assert_eq!(finalize("kyou"), "きょう");
        assert_eq!(finalize("gakkou"), "がっこう");
        assert_eq!(finalize("matcha"), "まっちゃ");
        assert_eq!(finalize("shinbun"), "しんぶん");
        assert_eq!(finalize("kan"), "かん");
        assert_eq!(finalize("kan'i"), "かんい");
        assert_eq!(finalize("ra-men"), "らーめん");
        assert_eq!(finalize("BI-RU"), "ビール");
        assert_eq!(finalize("jyuu"), "じゅう");
        assert_eq!(finalize("tsudzuku"), "つづく");
    }

    #[test]
    fn live_typing() {
        assert_eq!(typed("k"), "k");
        assert_eq!(typed("ky"), "ky");
        assert_eq!(typed("kyo"), "きょ");
        assert_eq!(typed("kan"), "かn");
        assert_eq!(typed("kann"), "かん");
        assert_eq!(typed("kanji"), "かんじ");
        assert_eq!(typed("sonnna"), "そんな");
        assert_eq!(typed("nyu"), "にゅ");
        assert_eq!(typed("tt"), "っt");
        assert_eq!(typed("kitte"), "きって");
        assert_eq!(typed("shi"), "し");
        assert_eq!(typed("ts"), "ts");
        assert_eq!(finalize(&typed("hon")), "ほん");
    }

    #[test]
    fn kana_passthrough() {
        assert_eq!(typed("おとna"), "おとな");
        assert_eq!(to_hiragana("ビール"), "びーる");
    }
}
