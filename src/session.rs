//! Review queue logic: which question comes next, and when an item is finished.

use crate::config::{QuestionOrder, ReviewSettings};
use crate::model::{QType, ReviewItem, Submission};
use rand::prelude::*;

/// Number of items in rotation at once, as on WaniKani.
const ACTIVE_SIZE: usize = 10;

#[derive(Debug)]
pub struct ItemState {
    pub item: ReviewItem,
    pub meaning_done: bool,
    pub reading_done: bool,
    pub wrong_meaning: u32,
    pub wrong_reading: u32,
}

impl ItemState {
    fn new(item: ReviewItem) -> Self {
        let reading_done = !item.subject.kind.has_reading();
        Self { item, meaning_done: false, reading_done, wrong_meaning: 0, wrong_reading: 0 }
    }

    pub fn complete(&self) -> bool {
        self.meaning_done && self.reading_done
    }

    pub fn done(&self, q: QType) -> bool {
        match q {
            QType::Meaning => self.meaning_done,
            QType::Reading => self.reading_done,
        }
    }

    pub fn had_mistakes(&self) -> bool {
        self.wrong_meaning + self.wrong_reading > 0
    }
}

pub struct Session {
    pub items: Vec<ItemState>,
    backlog: Vec<usize>,
    active: Vec<usize>,
    pub current: Option<(usize, QType)>,
    /// Finished items, in order of completion.
    pub completed: Vec<usize>,
    pub answers_correct: u32,
    pub answers_wrong: u32,
}

impl Session {
    pub fn new(items: Vec<ReviewItem>, settings: &ReviewSettings) -> Self {
        let items: Vec<ItemState> = items.into_iter().map(ItemState::new).collect();
        let mut backlog: Vec<usize> = (0..items.len()).collect();
        backlog.shuffle(&mut rand::rng());
        let mut s = Self {
            items,
            backlog,
            active: Vec::new(),
            current: None,
            completed: Vec::new(),
            answers_correct: 0,
            answers_wrong: 0,
        };
        s.refill();
        s.pick(None, settings);
        s
    }

    pub fn total(&self) -> usize {
        self.items.len()
    }

    pub fn finished(&self) -> bool {
        self.current.is_none()
    }

    pub fn current_item(&self) -> Option<(&ItemState, QType)> {
        self.current.map(|(i, q)| (&self.items[i], q))
    }

    fn refill(&mut self) {
        while self.active.len() < ACTIVE_SIZE {
            match self.backlog.pop() {
                Some(i) => self.active.push(i),
                None => break,
            }
        }
    }

    fn question_for(&self, idx: usize, settings: &ReviewSettings) -> QType {
        let it = &self.items[idx];
        match (it.meaning_done, it.reading_done) {
            (true, _) => QType::Reading,
            (_, true) => QType::Meaning,
            _ => match settings.question_order {
                QuestionOrder::ReadingFirst => QType::Reading,
                QuestionOrder::MeaningFirst => QType::Meaning,
                QuestionOrder::Random => {
                    if rand::rng().random_bool(0.5) {
                        QType::Meaning
                    } else {
                        QType::Reading
                    }
                }
            },
        }
    }

    /// Choose the next question, avoiding `avoid` (the item just asked) when possible.
    fn pick(&mut self, avoid: Option<usize>, settings: &ReviewSettings) {
        let candidates: Vec<usize> = self.active.iter().copied().filter(|i| Some(*i) != avoid).collect();
        let pool = if candidates.is_empty() { &self.active } else { &candidates };
        self.current = pool
            .choose(&mut rand::rng())
            .map(|&idx| (idx, self.question_for(idx, settings)));
    }

    /// Record the result of the current question and move on.
    /// Returns a submission when the item is fully answered.
    pub fn commit(&mut self, correct: bool, settings: &ReviewSettings) -> Option<Submission> {
        let (idx, q) = self.current?;
        let it = &mut self.items[idx];
        if correct {
            self.answers_correct += 1;
            match q {
                QType::Meaning => it.meaning_done = true,
                QType::Reading => it.reading_done = true,
            }
        } else {
            self.answers_wrong += 1;
            match q {
                QType::Meaning => it.wrong_meaning += 1,
                QType::Reading => it.wrong_reading += 1,
            }
        }

        if it.complete() {
            let submission = Submission {
                assignment_id: it.item.assignment_id,
                label: it.item.subject.display(),
                incorrect_meaning: it.wrong_meaning,
                incorrect_reading: it.wrong_reading,
                cache_files: crate::api::cache_files(&it.item.subject),
            };
            self.active.retain(|&i| i != idx);
            self.completed.push(idx);
            self.refill();
            self.pick(None, settings);
            return Some(submission);
        }

        let other = match q {
            QType::Meaning => QType::Reading,
            QType::Reading => QType::Meaning,
        };
        if settings.back_to_back && !self.items[idx].done(other) {
            // Stay on the item: ask its other question straight away.
            self.current = Some((idx, other));
        } else {
            self.pick(Some(idx), settings);
        }
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::demo;

    fn run(settings: &ReviewSettings, mut answer: impl FnMut(usize, QType, u32) -> bool) -> (Session, Vec<(usize, QType)>) {
        let mut s = Session::new(demo::items(), settings);
        let mut asked = Vec::new();
        let mut n = 0;
        while let Some((idx, q)) = s.current {
            asked.push((idx, q));
            n += 1;
            assert!(n < 1000, "session does not terminate");
            s.commit(answer(idx, q, n), settings);
        }
        (s, asked)
    }

    #[test]
    fn all_correct_asks_every_question_once() {
        let settings = ReviewSettings::default();
        let (s, asked) = run(&settings, |_, _, _| true);
        // 4 items with readings (2 questions) + 3 meaning-only items.
        assert_eq!(asked.len(), 11);
        assert_eq!(s.completed.len(), s.total());
        assert!(s.items.iter().all(|i| !i.had_mistakes()));
    }

    #[test]
    fn back_to_back_pairs_questions() {
        let settings = ReviewSettings { back_to_back: true, ..Default::default() };
        let (_, asked) = run(&settings, |_, _, _| true);
        let mut i = 0;
        while i < asked.len() {
            let (idx, q) = asked[i];
            let s = Session::new(demo::items(), &settings);
            if s.items[idx].item.subject.kind.has_reading() {
                assert_eq!(q, QType::Reading, "reading first by default");
                assert_eq!(asked[i + 1], (idx, QType::Meaning));
                i += 2;
            } else {
                i += 1;
            }
        }
    }

    #[test]
    fn wrong_answers_are_counted_and_reasked() {
        let settings = ReviewSettings { back_to_back: true, ..Default::default() };
        let mut failed = std::collections::HashSet::new();
        let (s, _) = run(&settings, |idx, q, _| {
            // Fail every reading question exactly once.
            q != QType::Reading || !failed.insert(idx)
        });
        assert_eq!(s.completed.len(), s.total());
        for it in &s.items {
            let expected = if it.item.subject.kind.has_reading() { 1 } else { 0 };
            assert_eq!(it.wrong_reading, expected);
            assert_eq!(it.wrong_meaning, 0);
        }
    }
}
