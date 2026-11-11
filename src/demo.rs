//! Built-in offline items for `--demo` and for tests. Nothing is sent to WaniKani.

use crate::model::{Kind, Meaning, Reading, ReviewItem, Subject};

const DEMO_RADICAL_SVG: &str = r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 1000 1000"><defs><style>.a{fill:none;stroke:#000;stroke-linecap:square;stroke-miterlimit:2;stroke-width:68px;}</style></defs><path class="a" d="M180 260h640"/><path class="a" d="M500 120v760"/><path class="a" d="M500 420c-80 180-200 300-340 380"/><path class="a" d="M500 420c80 180 200 300 340 380"/></svg>"##;

fn meanings(list: &[&str]) -> Vec<Meaning> {
    list.iter()
        .enumerate()
        .map(|(i, m)| Meaning { text: m.to_string(), primary: i == 0, accepted: true })
        .collect()
}

fn reading(text: &str, kind: Option<&str>, accepted: bool, primary: bool) -> Reading {
    Reading { text: text.into(), primary, accepted, kind: kind.map(str::to_string) }
}

fn subject(id: u64, kind: Kind, chars: Option<&str>, slug: &str, m: &[&str], readings: Vec<Reading>) -> Subject {
    Subject {
        id,
        kind,
        characters: chars.map(str::to_string),
        slug: slug.into(),
        meanings: meanings(m),
        whitelist: vec![],
        blacklist: vec![],
        synonyms: vec![],
        readings,
        meaning_mnemonic: format!("Demo mnemonic for the meaning of {slug}."),
        reading_mnemonic: if kind.has_reading() {
            format!("Demo mnemonic for the reading of {slug}.")
        } else {
            String::new()
        },
        image_svg: None,
        audios: vec![],
    }
}

pub fn items() -> Vec<ReviewItem> {
    let mut big = subject(
        2,
        Kind::Kanji,
        Some("大"),
        "大",
        &["Big", "Large"],
        vec![
            reading("たい", Some("onyomi"), true, true),
            reading("だい", Some("onyomi"), true, false),
            reading("おお", Some("kunyomi"), false, false),
        ],
    );
    big.synonyms = vec!["huge".into()];
    big.blacklist = vec!["fat".into()];

    let mut image_radical = subject(7, Kind::Radical, None, "tree-ish", &["Tree"], vec![]);
    image_radical.image_svg = Some(DEMO_RADICAL_SVG.as_bytes().to_vec());

    let subjects = vec![
        subject(1, Kind::Radical, Some("一"), "ground", &["Ground"], vec![]),
        big,
        subject(
            3,
            Kind::Kanji,
            Some("人"),
            "人",
            &["Person"],
            vec![
                reading("にん", Some("onyomi"), true, true),
                reading("じん", Some("onyomi"), true, false),
                reading("ひと", Some("kunyomi"), false, false),
            ],
        ),
        subject(
            4,
            Kind::Vocabulary,
            Some("大人"),
            "大人",
            &["Adult", "Grown Up"],
            vec![reading("おとな", None, true, true)],
        ),
        subject(
            5,
            Kind::Vocabulary,
            Some("二十日"),
            "二十日",
            &["Twentieth Day", "20th Day", "Twenty Days"],
            vec![reading("はつか", None, true, true)],
        ),
        subject(6, Kind::KanaVocabulary, Some("りんご"), "りんご", &["Apple"], vec![]),
        image_radical,
    ];
    subjects
        .into_iter()
        .map(|subject| ReviewItem { assignment_id: 1000 + subject.id, subject })
        .collect()
}
