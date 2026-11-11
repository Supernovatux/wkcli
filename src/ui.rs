//! Full-screen review interface.

use crate::answer::{self, Verdict};
use crate::api::Submitter;
use crate::audio::Audio;
use crate::config::{Config, QuestionOrder};
use crate::gfx::{Gfx, Rgb, Slot};
use crate::kana;
use crate::model::{Kind, QType, Subject};
use crate::session::Session;
use anyhow::Result;
use ratatui::crossterm::event::{self, Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers};
use ratatui::layout::{Alignment, Constraint, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Clear, Paragraph, Wrap};
use ratatui::{DefaultTerminal, Frame};
use rand::prelude::*;
use std::time::{Duration, Instant};

const RADICAL: Rgb = [0, 147, 221];
const KANJI: Rgb = [221, 0, 147];
const VOCAB: Rgb = [147, 0, 221];
const GREEN: Color = Color::Rgb(102, 170, 0);
const RED: Color = Color::Rgb(221, 0, 44);
const SHAKE: Duration = Duration::from_millis(350);
const DELAY_STEPS: &[u64] = &[0, 500, 1000, 1500, 2000, 3000];
const SETTINGS: usize = 7;

fn rgb(c: Rgb) -> Color {
    Color::Rgb(c[0], c[1], c[2])
}

fn kind_colour(kind: Kind) -> Rgb {
    match kind {
        Kind::Radical => RADICAL,
        Kind::Kanji => KANJI,
        Kind::Vocabulary | Kind::KanaVocabulary => VOCAB,
    }
}

enum Phase {
    Asking,
    Answered { correct: bool, at: Instant },
}

pub struct Report {
    pub session: Session,
    /// Submissions still in flight when the UI closed.
    pub pending: usize,
    pub failures: Vec<String>,
}

struct App<'a> {
    session: Session,
    cfg: &'a mut Config,
    submitter: Option<&'a Submitter>,
    gfx: Gfx,
    input: String,
    phase: Phase,
    message: Option<String>,
    shake_at: Option<Instant>,
    /// Result of the previous question, shown discreetly (useful in lightning mode).
    last: Option<(bool, String)>,
    show_info: bool,
    /// Selected row of the settings popup when open.
    popup: Option<usize>,
    ended: bool,
    quit: bool,
    pending: usize,
    failures: Vec<String>,
    /// Image the last frame wants on screen, with its colours.
    image_want: Option<Slot>,
    audio: Audio,
    /// Voice picked for the current question: (subject id, voice actor id).
    voice: Option<(u64, u64)>,
    /// Last recording played, for replay with `p`.
    last_audio: Option<String>,
}

pub fn run(items: Vec<crate::model::ReviewItem>, cfg: &mut Config, submitter: Option<&Submitter>) -> Result<Report> {
    let session = Session::new(items, &cfg.review);
    let mut terminal = ratatui::init();
    let gfx = Gfx::init(&cfg.image_protocol, cfg.big_text, &cfg.font);
    let mut app = App {
        session,
        cfg,
        submitter,
        gfx,
        input: String::new(),
        phase: Phase::Asking,
        message: None,
        shake_at: None,
        last: None,
        show_info: false,
        popup: None,
        ended: false,
        quit: false,
        pending: 0,
        failures: Vec::new(),
        image_want: None,
        audio: Audio::spawn(),
        voice: None,
        last_audio: None,
    };
    app.prepare_audio();
    let result = app.event_loop(&mut terminal);
    let _ = app.gfx.invalidate();
    ratatui::restore();
    result?;
    Ok(Report { session: app.session, pending: app.pending, failures: app.failures })
}

impl App<'_> {
    fn event_loop(&mut self, terminal: &mut DefaultTerminal) -> Result<()> {
        while !self.quit {
            self.drain_results();
            terminal.draw(|f| self.draw(f))?;
            self.sync_image(terminal)?;
            if event::poll(Duration::from_millis(50))? {
                match event::read()? {
                    Event::Key(key) if key.kind != KeyEventKind::Release => self.on_key(key),
                    Event::Resize(..) => self.gfx.invalidate()?,
                    _ => {}
                }
            }
        }
        Ok(())
    }

    fn drain_results(&mut self) {
        let Some(sub) = self.submitter else { return };
        while let Ok(res) = sub.results.try_recv() {
            self.pending = self.pending.saturating_sub(1);
            if let Some(err) = res.error {
                self.failures.push(format!("{}: {err}", res.label));
            }
        }
    }

    fn done(&self) -> bool {
        self.ended || self.session.finished()
    }

    /// Kitty graphics live outside ratatui's buffer; keep them in step with the frame.
    fn sync_image(&mut self, terminal: &mut DefaultTerminal) -> Result<()> {
        let want = self.image_want;
        if !self.gfx.kitty_stale(want) {
            return Ok(());
        }
        if self.gfx.kitty_has_image() {
            // Remove the old image, then repaint every cell so nothing of it survives in
            // terminals that only drop an image when its cells are overwritten.
            self.gfx.kitty_clear()?;
            terminal.swap_buffers();
            terminal.draw(|f| self.draw(f))?;
        }
        if let Some(slot) = want {
            let art = self
                .session
                .current_item()
                .and_then(|(it, _)| self.artwork(&it.item.subject).map(|a| (a, kind_colour(it.item.subject.kind))));
            if let Some(((svg, _), colour)) = art {
                self.gfx.kitty_place(slot, &svg, [255, 255, 255], colour)?;
            }
        }
        Ok(())
    }

    /// What to draw as an image for a subject: (SVG, width:height ratio).
    /// `None` means the characters are shown as ordinary terminal text.
    fn artwork(&self, s: &Subject) -> Option<(Vec<u8>, u32)> {
        match (&s.characters, &s.image_svg) {
            (Some(chars), _) => {
                let n = chars.chars().count().max(1) as u32;
                self.gfx.text_svg(chars).map(|svg| (svg, n))
            }
            (None, Some(svg)) => Some((svg.clone(), 1)),
            (None, None) => None,
        }
    }

    // ---------------------------------------------------------------- input

    fn on_key(&mut self, key: KeyEvent) {
        let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
        if ctrl && key.code == KeyCode::Char('c') {
            self.quit = true;
            return;
        }
        if self.popup.is_some() {
            self.on_popup_key(key);
            return;
        }
        if self.done() {
            if matches!(key.code, KeyCode::Enter | KeyCode::Esc | KeyCode::Char('q')) {
                self.quit = true;
            }
            return;
        }
        if key.code == KeyCode::Esc {
            self.popup = Some(0);
            return;
        }
        match self.phase {
            Phase::Asking => match key.code {
                KeyCode::Enter => self.submit_answer(),
                KeyCode::Backspace => {
                    self.input.pop();
                    self.message = None;
                }
                KeyCode::Char('u') | KeyCode::Char('w') if ctrl => self.input.clear(),
                KeyCode::Char(c) if !ctrl && !key.modifiers.contains(KeyModifiers::ALT) => {
                    self.input.push(c);
                    if matches!(self.session.current, Some((_, QType::Reading))) {
                        kana::convert_tail(&mut self.input);
                    }
                    self.message = None;
                }
                _ => {}
            },
            Phase::Answered { correct, at } => match key.code {
                KeyCode::Enter => {
                    let delay = Duration::from_millis(self.cfg.review.wrong_delay_ms);
                    if correct || at.elapsed() >= delay {
                        self.advance(correct);
                    }
                }
                KeyCode::Backspace if self.cfg.review.retype => {
                    // Double-check: throw the result away and answer again.
                    self.input.clear();
                    self.phase = Phase::Asking;
                    self.show_info = false;
                    self.message = Some("Answer discarded — try again.".into());
                }
                KeyCode::Char('f') | KeyCode::Char(' ') => self.show_info = !self.show_info,
                KeyCode::Char('p') => {
                    if let Some(url) = &self.last_audio {
                        self.audio.play(url);
                    }
                }
                _ => {}
            },
        }
    }

    /// For a vocabulary reading question: pick a random voice and download its
    /// recordings now, so they play without delay once the answer is in.
    fn prepare_audio(&mut self) {
        self.voice = None;
        if !self.cfg.review.audio {
            return;
        }
        let Some((it, QType::Reading)) = self.session.current_item() else { return };
        let s = &it.item.subject;
        let Some(pick) = s.audios.choose(&mut rand::rng()) else { return };
        let voice = pick.voice_actor_id;
        for a in s.audios.iter().filter(|a| a.voice_actor_id == voice) {
            self.audio.prefetch(&a.url);
        }
        self.voice = Some((s.id, voice));
    }

    /// Play the recording for `reading` (or the primary reading) in the chosen voice.
    fn play_audio(&mut self, reading: Option<&str>) {
        if !self.cfg.review.audio {
            return;
        }
        let Some((it, QType::Reading)) = self.session.current_item() else { return };
        let s = &it.item.subject;
        let voice = match self.voice {
            Some((id, v)) if id == s.id => v,
            _ => match s.audios.choose(&mut rand::rng()) {
                Some(a) => a.voice_actor_id,
                None => return,
            },
        };
        let same_voice = || s.audios.iter().filter(move |a| a.voice_actor_id == voice);
        let primary = s.readings.iter().find(|r| r.primary).map(|r| r.text.as_str());
        let url = reading
            .and_then(|r| same_voice().find(|a| kana::to_hiragana(&a.reading) == kana::to_hiragana(r)))
            .or_else(|| same_voice().find(|a| Some(a.reading.as_str()) == primary))
            .or_else(|| same_voice().next())
            .map(|a| a.url.clone());
        if let Some(url) = url {
            self.audio.play(&url);
            self.last_audio = Some(url);
        }
    }

    fn submit_answer(&mut self) {
        let Some((it, q)) = self.session.current_item() else { return };
        let verdict = answer::check(&it.item.subject, q, &self.input, self.cfg.review.shake_near_miss);
        match verdict {
            Verdict::Retry(msg) => {
                if !msg.is_empty() {
                    self.message = Some(msg);
                    self.shake_at = Some(Instant::now());
                }
            }
            Verdict::Correct | Verdict::Incorrect => {
                let correct = verdict == Verdict::Correct;
                if q == QType::Reading {
                    self.input = kana::finalize(&self.input);
                }
                self.message = None;
                self.last_audio = None;
                if q == QType::Reading {
                    let said = self.input.clone();
                    self.play_audio(correct.then_some(said.as_str()));
                }
                if correct && self.cfg.review.lightning {
                    self.advance(true);
                } else {
                    self.phase = Phase::Answered { correct, at: Instant::now() };
                }
            }
        }
    }

    fn advance(&mut self, correct: bool) {
        if let Some((it, q)) = self.session.current_item() {
            let s = &it.item.subject;
            self.last = Some((correct, format!("{} — {}", s.display(), primary_answer(s, q))));
        }
        if let Some(submission) = self.session.commit(correct, &self.cfg.review) {
            if let Some(sub) = self.submitter {
                if sub.tx.send(submission).is_ok() {
                    self.pending += 1;
                }
            }
        }
        self.input.clear();
        self.phase = Phase::Asking;
        self.message = None;
        self.show_info = false;
        self.prepare_audio();
    }

    fn on_popup_key(&mut self, key: KeyEvent) {
        let Some(sel) = self.popup else { return };
        let r = &mut self.cfg.review;
        match key.code {
            KeyCode::Esc => {
                self.popup = None;
                if let Err(e) = self.cfg.save() {
                    self.message = Some(format!("Could not save settings: {e:#}"));
                }
            }
            KeyCode::Char('q') => {
                self.popup = None;
                let _ = self.cfg.save();
                self.ended = true;
            }
            KeyCode::Up | KeyCode::Char('k') => self.popup = Some((sel + SETTINGS - 1) % SETTINGS),
            KeyCode::Down | KeyCode::Char('j') => self.popup = Some((sel + 1) % SETTINGS),
            KeyCode::Enter | KeyCode::Char(' ') | KeyCode::Left | KeyCode::Right => {
                let back = key.code == KeyCode::Left;
                match sel {
                    0 => r.back_to_back = !r.back_to_back,
                    1 => {
                        use QuestionOrder::*;
                        r.question_order = match (r.question_order, back) {
                            (ReadingFirst, false) | (Random, true) => MeaningFirst,
                            (MeaningFirst, false) | (ReadingFirst, true) => Random,
                            (Random, false) | (MeaningFirst, true) => ReadingFirst,
                        };
                    }
                    2 => r.lightning = !r.lightning,
                    3 => r.retype = !r.retype,
                    4 => r.shake_near_miss = !r.shake_near_miss,
                    6 => r.audio = !r.audio,
                    _ => {
                        let n = DELAY_STEPS.len();
                        let cur = DELAY_STEPS.iter().position(|&d| d >= r.wrong_delay_ms).unwrap_or(n - 1);
                        let next = if back { (cur + n - 1) % n } else { (cur + 1) % n };
                        r.wrong_delay_ms = DELAY_STEPS[next];
                    }
                }
            }
            _ => {}
        }
    }

    // -------------------------------------------------------------- drawing

    fn draw(&mut self, f: &mut Frame) {
        self.image_want = None;
        if self.done() {
            self.draw_summary(f);
            return;
        }
        let area = f.area();
        let panel_h = area.height.saturating_sub(12).clamp(3, 11);
        let [header, panel, band, input, message, info, footer] = Layout::vertical([
            Constraint::Length(1),
            Constraint::Length(panel_h),
            Constraint::Length(1),
            Constraint::Length(3),
            Constraint::Length(1),
            Constraint::Min(0),
            Constraint::Length(1),
        ])
        .areas(area);

        self.draw_header(f, header);
        let Some((it, q)) = self.session.current_item() else { return };
        let subject = it.item.subject.clone();

        // Character panel.
        let colour = kind_colour(subject.kind);
        let panel_style = Style::new().bg(rgb(colour)).fg(Color::White).add_modifier(Modifier::BOLD);
        f.render_widget(Block::new().style(panel_style), panel);
        let mid = Rect { y: panel.y + panel.height / 2, height: 1, ..panel };
        let plain = subject.characters.as_deref().unwrap_or("(radical image unavailable)");
        // Images sit above text in some terminals, so fall back to text under the popup.
        let art = if self.popup.is_none() { self.artwork(&subject) } else { None };
        let mut drawn = false;
        if let Some((svg, aspect)) = art {
            let slot: Slot = (subject.id, self.gfx.image_area(panel, aspect));
            if self.gfx.is_kitty() {
                self.image_want = Some(slot);
                drawn = true;
            } else if let Some(proto) = self.gfx.widget(slot, &svg, [255, 255, 255], colour) {
                f.render_widget(ratatui_image::Image::new(proto), slot.1);
                drawn = true;
            }
        }
        if !drawn && (self.popup.is_none() || subject.characters.is_some()) {
            f.render_widget(Paragraph::new(plain).alignment(Alignment::Center).style(panel_style), mid);
        }

        // Question band: light for meaning, dark for reading, as on WaniKani.
        let what = match (q, subject.kind) {
            (QType::Meaning, Kind::Radical) => "Name",
            (QType::Meaning, _) => "Meaning",
            (QType::Reading, _) => "Reading",
        };
        let band_style = match q {
            QType::Meaning => Style::new().bg(Color::Rgb(225, 225, 225)).fg(Color::Rgb(40, 40, 40)),
            QType::Reading => Style::new().bg(Color::Rgb(45, 45, 45)).fg(Color::White),
        };
        let band_line = Line::from(vec![
            Span::raw(format!("{} ", subject.kind.label())),
            Span::styled(what, Style::new().add_modifier(Modifier::BOLD)),
        ]);
        f.render_widget(Paragraph::new(band_line).alignment(Alignment::Center).style(band_style), band);

        self.draw_input(f, input, q);
        self.draw_message(f, message);

        if let Phase::Answered { correct, .. } = self.phase {
            if !correct || self.show_info {
                let lines = info_lines(&subject, q, self.show_info);
                let inner = Rect { x: info.x + 2, width: info.width.saturating_sub(4), ..info };
                f.render_widget(Paragraph::new(lines).wrap(Wrap { trim: true }), inner);
            }
        }

        self.draw_footer(f, footer);
        if let Some(sel) = self.popup {
            self.draw_popup(f, area, sel);
        }
    }

    fn draw_header(&self, f: &mut Frame, area: Rect) {
        let s = &self.session;
        let answered = s.answers_correct + s.answers_wrong;
        let pct = if answered == 0 { 100 } else { s.answers_correct * 100 / answered };
        let left = format!(" {}/{} done · {}% correct · {} left", s.completed.len(), s.total(), pct, s.total() - s.completed.len());
        f.render_widget(Paragraph::new(left).style(Style::new().fg(Color::Gray)), area);

        let r = &self.cfg.review;
        let mut flags: Vec<Span> = Vec::new();
        let on = Style::new().fg(Color::Black).bg(Color::Yellow);
        if r.back_to_back {
            flags.push(Span::styled(" B2B ", on));
            flags.push(Span::raw(" "));
        }
        if r.lightning {
            flags.push(Span::styled(" ⚡ ", on));
            flags.push(Span::raw(" "));
        }
        if self.submitter.is_none() {
            flags.push(Span::styled(" not submitting ", Style::new().fg(Color::Black).bg(Color::Cyan)));
            flags.push(Span::raw(" "));
        }
        if !self.failures.is_empty() {
            flags.push(Span::styled(format!(" {} failed ", self.failures.len()), Style::new().fg(Color::White).bg(RED)));
            flags.push(Span::raw(" "));
        }
        if self.pending > 0 {
            flags.push(Span::styled(format!("↑{} ", self.pending), Style::new().fg(Color::DarkGray)));
        }
        f.render_widget(Paragraph::new(Line::from(flags)).alignment(Alignment::Right), area);
    }

    fn draw_input(&self, f: &mut Frame, area: Rect, q: QType) {
        let style = match self.phase {
            Phase::Asking => Style::new(),
            Phase::Answered { correct: true, .. } => Style::new().bg(GREEN).fg(Color::White),
            Phase::Answered { correct: false, .. } => Style::new().bg(RED).fg(Color::White),
        };
        f.render_widget(Block::bordered().style(style), area);
        let inner = Rect { x: area.x + 1, y: area.y + 1, width: area.width.saturating_sub(2), height: 1 };

        let shaking = self.shake_at.is_some_and(|t| t.elapsed() < SHAKE);
        let offset: i32 = if shaking {
            let step = self.shake_at.map(|t| t.elapsed().as_millis() / 50).unwrap_or(0);
            if step % 2 == 0 { 2 } else { -2 }
        } else {
            0
        };

        let placeholder = self.input.is_empty() && matches!(self.phase, Phase::Asking);
        let (text, text_style) = if placeholder {
            let hint = match q {
                QType::Meaning => "Your Response",
                QType::Reading => "答え",
            };
            (hint.to_string(), Style::new().fg(Color::DarkGray))
        } else {
            (self.input.clone(), style.add_modifier(Modifier::BOLD))
        };
        let width = Line::raw(text.as_str()).width() as i32;
        let x = inner.x as i32 + ((inner.width as i32 - width) / 2).max(0) + offset;
        let x = x.clamp(inner.x as i32, (inner.x + inner.width) as i32) as u16;
        let rect = Rect { x, width: (inner.x + inner.width).saturating_sub(x), ..inner };
        f.render_widget(Paragraph::new(text).style(text_style), rect);

        if matches!(self.phase, Phase::Asking) && self.popup.is_none() {
            let cursor = if placeholder { inner.x + inner.width / 2 } else { x + width as u16 };
            f.set_cursor_position((cursor.min(inner.x + inner.width.saturating_sub(1)), inner.y));
        }
    }

    fn draw_message(&self, f: &mut Frame, area: Rect) {
        let line = match (&self.message, &self.phase) {
            (Some(msg), _) => Line::styled(msg.as_str(), Style::new().fg(Color::Yellow)),
            (None, Phase::Answered { correct: false, at }) => {
                let delay = Duration::from_millis(self.cfg.review.wrong_delay_ms);
                if at.elapsed() < delay {
                    Line::styled("Incorrect — take a look before moving on…", Style::new().fg(Color::DarkGray))
                } else {
                    Line::styled("Incorrect", Style::new().fg(RED))
                }
            }
            (None, Phase::Answered { correct: true, .. }) => Line::styled("Correct", Style::new().fg(GREEN)),
            (None, Phase::Asking) => match &self.last {
                Some((ok, text)) => Line::styled(
                    format!("{} {text}", if *ok { "✓" } else { "✗" }),
                    Style::new().fg(if *ok { GREEN } else { RED }).add_modifier(Modifier::DIM),
                ),
                None => Line::raw(""),
            },
        };
        f.render_widget(Paragraph::new(line).alignment(Alignment::Center), area);
    }

    fn draw_footer(&self, f: &mut Frame, area: Rect) {
        let text = match self.phase {
            Phase::Asking => " Enter submit · Esc menu".to_string(),
            Phase::Answered { .. } => {
                let mut t = String::from(" Enter next");
                if self.cfg.review.retype {
                    t.push_str(" · Backspace retype");
                }
                t.push_str(" · f item info");
                if self.last_audio.is_some() {
                    t.push_str(" · p replay audio");
                }
                t.push_str(" · Esc menu");
                t
            }
        };
        f.render_widget(Paragraph::new(text).style(Style::new().fg(Color::DarkGray)), area);
    }

    fn draw_popup(&self, f: &mut Frame, area: Rect, sel: usize) {
        let r = &self.cfg.review;
        let onoff = |b: bool| if b { "on" } else { "off" };
        let order = match r.question_order {
            QuestionOrder::ReadingFirst => "reading first",
            QuestionOrder::MeaningFirst => "meaning first",
            QuestionOrder::Random => "random",
        };
        let delay = if r.wrong_delay_ms == 0 { "off".to_string() } else { format!("{:.1}s", r.wrong_delay_ms as f64 / 1000.0) };
        let rows: [(&str, String); SETTINGS] = [
            ("Back to back", onoff(r.back_to_back).into()),
            ("Question order", order.into()),
            ("Lightning mode", onoff(r.lightning).into()),
            ("Retype with Backspace", onoff(r.retype).into()),
            ("Shake on near-miss", onoff(r.shake_near_miss).into()),
            ("Delay after wrong answer", delay),
            ("Vocabulary audio", onoff(r.audio).into()),
        ];
        let width = 46.min(area.width);
        let height = (SETTINGS as u16 + 6).min(area.height);
        let rect = Rect {
            x: area.x + (area.width - width) / 2,
            y: area.y + (area.height - height) / 2,
            width,
            height,
        };
        let mut lines: Vec<Line> = vec![Line::raw("")];
        for (i, (name, value)) in rows.iter().enumerate() {
            let style = if i == sel { Style::new().add_modifier(Modifier::REVERSED) } else { Style::new() };
            let pad = (width as usize).saturating_sub(6 + name.len() + value.len());
            lines.push(Line::styled(format!("  {name}{}{value}  ", " ".repeat(pad)), style));
        }
        lines.push(Line::raw(""));
        lines.push(Line::styled("  ↑↓ select · Space change · Esc close", Style::new().fg(Color::DarkGray)));
        lines.push(Line::styled("  q end session", Style::new().fg(Color::DarkGray)));
        f.render_widget(Clear, rect);
        f.render_widget(Paragraph::new(lines).block(Block::bordered().title(" Settings ")), rect);
    }

    fn draw_summary(&self, f: &mut Frame) {
        let s = &self.session;
        let done = s.completed.len();
        let wrong: Vec<String> = s
            .completed
            .iter()
            .map(|&i| &s.items[i])
            .filter(|it| it.had_mistakes())
            .map(|it| it.item.subject.display())
            .collect();
        let mut lines = vec![
            Line::raw(""),
            Line::styled("Session complete", Style::new().add_modifier(Modifier::BOLD)),
            Line::raw(""),
            Line::raw(format!("{done} of {} items reviewed", s.total())),
        ];
        if done > 0 {
            lines.push(Line::styled(
                format!("{} correct ({}%)", done - wrong.len(), (done - wrong.len()) * 100 / done),
                Style::new().fg(GREEN),
            ));
        }
        if !wrong.is_empty() {
            lines.push(Line::styled(format!("{} incorrect: {}", wrong.len(), wrong.join("  ")), Style::new().fg(RED)));
        }
        lines.push(Line::raw(""));
        if self.submitter.is_none() {
            lines.push(Line::styled("Nothing was submitted to WaniKani.", Style::new().fg(Color::Cyan)));
        } else if self.pending > 0 {
            lines.push(Line::raw(format!("Submitting {} review(s)…", self.pending)));
        } else if self.failures.is_empty() {
            lines.push(Line::raw("All reviews submitted."));
        }
        for fail in &self.failures {
            lines.push(Line::styled(format!("Failed to submit {fail}"), Style::new().fg(RED)));
        }
        lines.push(Line::raw(""));
        lines.push(Line::styled("Press Enter to exit", Style::new().fg(Color::DarkGray)));
        f.render_widget(Paragraph::new(lines).alignment(Alignment::Center).wrap(Wrap { trim: true }), f.area());
    }
}

fn primary_answer(s: &Subject, q: QType) -> String {
    match q {
        QType::Meaning => s.meanings.iter().find(|m| m.primary).or(s.meanings.first()).map(|m| m.text.clone()),
        QType::Reading => s.readings.iter().find(|r| r.primary).or(s.readings.first()).map(|r| r.text.clone()),
    }
    .unwrap_or_default()
}

fn strip_tags(s: &str) -> String {
    let mut out = String::new();
    let mut in_tag = false;
    for c in s.chars() {
        match c {
            '<' => in_tag = true,
            '>' if in_tag => in_tag = false,
            _ if !in_tag => out.push(c),
            _ => {}
        }
    }
    out
}

/// Answers for the question just graded; with `full`, every section of the item
/// (meanings, readings and both mnemonics), starting with the one just asked.
fn info_lines(s: &Subject, q: QType, full: bool) -> Vec<Line<'static>> {
    let label = Style::new().fg(Color::DarkGray);
    let mut lines = Vec::new();
    let meaning = |lines: &mut Vec<Line<'static>>| {
        let list: Vec<&str> = s.meanings.iter().filter(|m| m.accepted).map(|m| m.text.as_str()).collect();
        let mut spans = vec![Span::styled("Meanings  ", label), Span::raw(list.join(", "))];
        if !s.synonyms.is_empty() {
            spans.push(Span::styled(format!("   (yours: {})", s.synonyms.join(", ")), label));
        }
        lines.push(Line::from(spans));
        if full && !s.meaning_mnemonic.is_empty() {
            lines.push(Line::raw(strip_tags(&s.meaning_mnemonic)));
            lines.push(Line::raw(""));
        }
    };
    let reading = |lines: &mut Vec<Line<'static>>| {
        let mut spans = vec![Span::styled("Readings  ", label)];
        for (i, r) in s.readings.iter().enumerate() {
            if i > 0 {
                spans.push(Span::raw(", "));
            }
            let style = if r.accepted { Style::new() } else { label };
            spans.push(Span::styled(r.text.clone(), style));
            if let Some(kind) = &r.kind {
                spans.push(Span::styled(format!(" ({kind})"), label));
            }
        }
        lines.push(Line::from(spans));
        if full && !s.reading_mnemonic.is_empty() {
            lines.push(Line::raw(strip_tags(&s.reading_mnemonic)));
            lines.push(Line::raw(""));
        }
    };
    match q {
        QType::Meaning => {
            meaning(&mut lines);
            if full && s.kind.has_reading() {
                reading(&mut lines);
            }
        }
        QType::Reading => {
            reading(&mut lines);
            if full {
                meaning(&mut lines);
            }
        }
    }
    lines
}
