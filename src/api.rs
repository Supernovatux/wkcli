//! WaniKani API v2 client (revision 20170710).

use crate::config;
use crate::model::{Kind, Meaning, Pronunciation, Reading, ReviewItem, Subject, Submission};
use anyhow::{Context, Result, bail};
use serde::Deserialize;
use serde::de::DeserializeOwned;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::mpsc::{self, Receiver, Sender};
use std::thread::JoinHandle;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

const BASE: &str = "https://api.wanikani.com/v2";
const REVISION: &str = "20170710";
/// Subject ids per request; keeps URLs comfortably short.
const ID_CHUNK: usize = 300;
const BURNED_STAGE: u64 = 9;

fn radical_cache_path(id: u64) -> Option<PathBuf> {
    config::cache_dir().map(|d| d.join("radicals").join(format!("{id}.svg")))
}

/// Every file that may have been cached for a subject (radical image, audio).
pub fn cache_files(s: &Subject) -> Vec<PathBuf> {
    let mut files: Vec<PathBuf> = s.audios.iter().map(|a| crate::audio::cache_path(&a.url)).collect();
    if s.image_svg.is_some() {
        files.extend(radical_cache_path(s.id));
    }
    files
}

pub struct Client {
    agent: ureq::Agent,
    token: String,
}

#[derive(Deserialize)]
struct Resource<T> {
    id: u64,
    object: String,
    data: T,
}

#[derive(Deserialize)]
struct Collection<T> {
    pages: Pages,
    data: Vec<Resource<T>>,
}

#[derive(Deserialize)]
struct Pages {
    next_url: Option<String>,
}

#[derive(Deserialize)]
struct Report<T> {
    data: T,
}

#[derive(Deserialize)]
struct AssignmentData {
    subject_id: u64,
}

#[derive(Deserialize)]
struct SubjectData {
    level: u32,
    slug: String,
    characters: Option<String>,
    #[serde(default)]
    meanings: Vec<ApiMeaning>,
    #[serde(default)]
    auxiliary_meanings: Vec<ApiAuxMeaning>,
    #[serde(default)]
    readings: Vec<ApiReading>,
    #[serde(default)]
    character_images: Vec<ApiImage>,
    #[serde(default)]
    pronunciation_audios: Vec<ApiAudio>,
    meaning_mnemonic: Option<String>,
    reading_mnemonic: Option<String>,
    hidden_at: Option<String>,
}

#[derive(Deserialize)]
struct ApiMeaning {
    meaning: String,
    primary: bool,
    accepted_answer: bool,
}

#[derive(Deserialize)]
struct ApiAuxMeaning {
    meaning: String,
    #[serde(rename = "type")]
    kind: String,
}

#[derive(Deserialize)]
struct ApiReading {
    reading: String,
    primary: bool,
    accepted_answer: bool,
    #[serde(rename = "type")]
    kind: Option<String>,
}

#[derive(Deserialize)]
struct ApiImage {
    url: String,
    content_type: String,
    #[serde(default)]
    metadata: serde_json::Value,
}

#[derive(Deserialize)]
struct ApiAudio {
    url: String,
    content_type: String,
    #[serde(default)]
    metadata: serde_json::Value,
}

#[derive(Deserialize)]
struct StudyMaterialData {
    subject_id: u64,
    #[serde(default)]
    meaning_synonyms: Vec<String>,
}

#[derive(Deserialize)]
pub struct User {
    pub username: String,
    pub level: u32,
    pub subscription: Subscription,
}

#[derive(Deserialize)]
pub struct Subscription {
    pub max_level_granted: u32,
}

#[derive(Deserialize)]
pub struct Summary {
    pub lessons: Vec<SummarySlot>,
    pub reviews: Vec<SummarySlot>,
    pub next_reviews_at: Option<String>,
}

#[derive(Deserialize)]
pub struct SummarySlot {
    pub available_at: String,
    pub subject_ids: Vec<u64>,
}

impl Client {
    pub fn new(token: String) -> Self {
        let config = ureq::Agent::config_builder()
            .timeout_global(Some(Duration::from_secs(30)))
            .http_status_as_error(false)
            .build();
        Self { agent: ureq::Agent::new_with_config(config), token }
    }

    fn auth(&self) -> String {
        format!("Bearer {}", self.token)
    }

    /// Turn a response into JSON, or `Ok(None)` when the request should be retried (rate limit).
    fn handle<T: DeserializeOwned>(
        &self,
        mut resp: ureq::http::Response<ureq::Body>,
    ) -> Result<Option<T>> {
        let status = resp.status().as_u16();
        match status {
            200..=299 => Ok(Some(resp.body_mut().read_json().context("unexpected response from WaniKani")?)),
            429 => {
                let reset = resp
                    .headers()
                    .get("RateLimit-Reset")
                    .and_then(|v| v.to_str().ok())
                    .and_then(|v| v.parse::<u64>().ok());
                let now = SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0);
                let wait = reset.map(|r| r.saturating_sub(now)).unwrap_or(5).clamp(1, 60);
                std::thread::sleep(Duration::from_secs(wait));
                Ok(None)
            }
            401 => bail!("WaniKani rejected the API token (401). Run `wkcli login` to set a new one."),
            _ => {
                let body = resp.body_mut().read_to_string().unwrap_or_default();
                let detail = serde_json::from_str::<serde_json::Value>(&body)
                    .ok()
                    .and_then(|v| v.get("error").and_then(|e| e.as_str()).map(str::to_string))
                    .unwrap_or(body);
                if status == 403 {
                    bail!("WaniKani refused the request (403: {detail}). The token probably lacks the required permission.");
                }
                bail!("WaniKani API error {status}: {detail}")
            }
        }
    }

    fn get<T: DeserializeOwned>(&self, url: &str) -> Result<T> {
        for _ in 0..4 {
            let resp = self
                .agent
                .get(url)
                .header("Authorization", self.auth())
                .header("Wanikani-Revision", REVISION)
                .call()
                .context("could not reach WaniKani")?;
            if let Some(v) = self.handle(resp)? {
                return Ok(v);
            }
        }
        bail!("WaniKani rate limit: gave up after several retries")
    }

    fn post<T: DeserializeOwned>(&self, url: &str, body: &serde_json::Value) -> Result<T> {
        for _ in 0..4 {
            let resp = self
                .agent
                .post(url)
                .header("Authorization", self.auth())
                .header("Wanikani-Revision", REVISION)
                .send_json(body)
                .context("could not reach WaniKani")?;
            if let Some(v) = self.handle(resp)? {
                return Ok(v);
            }
        }
        bail!("WaniKani rate limit: gave up after several retries")
    }

    fn get_all<T: DeserializeOwned>(&self, first_url: String) -> Result<Vec<Resource<T>>> {
        let mut out = Vec::new();
        let mut url = Some(first_url);
        while let Some(u) = url {
            let page: Collection<T> = self.get(&u)?;
            out.extend(page.data);
            url = page.pages.next_url;
        }
        Ok(out)
    }

    pub fn user(&self) -> Result<User> {
        Ok(self.get::<Report<User>>(&format!("{BASE}/user"))?.data)
    }

    pub fn summary(&self) -> Result<Summary> {
        Ok(self.get::<Report<Summary>>(&format!("{BASE}/summary"))?.data)
    }

    /// Submit a review. Returns whether it burned the item.
    pub fn submit(&self, s: &Submission) -> Result<bool> {
        let body = serde_json::json!({
            "review": {
                "assignment_id": s.assignment_id,
                "incorrect_meaning_answers": s.incorrect_meaning,
                "incorrect_reading_answers": s.incorrect_reading,
            }
        });
        let resp: serde_json::Value = self.post(&format!("{BASE}/reviews"), &body)?;
        let burned_at = resp.pointer("/resources_updated/assignment/data/burned_at");
        let stage = resp.pointer("/data/ending_srs_stage").and_then(|v| v.as_u64());
        Ok(burned_at.is_some_and(|v| !v.is_null()) || stage == Some(BURNED_STAGE))
    }

    /// Fetch everything needed to review what is available right now.
    pub fn load_reviews(&self, progress: impl Fn(&str)) -> Result<Vec<ReviewItem>> {
        progress("Fetching user…");
        let user = self.user()?;

        progress("Fetching available reviews…");
        let assignments: Vec<Resource<AssignmentData>> =
            self.get_all(format!("{BASE}/assignments?immediately_available_for_review"))?;
        if assignments.is_empty() {
            return Ok(Vec::new());
        }
        let ids: Vec<u64> = assignments.iter().map(|a| a.data.subject_id).collect();

        progress(&format!("Fetching {} subjects…", ids.len()));
        let mut subjects: HashMap<u64, Resource<SubjectData>> = HashMap::new();
        let mut synonyms: HashMap<u64, Vec<String>> = HashMap::new();
        for chunk in ids.chunks(ID_CHUNK) {
            let list = chunk.iter().map(u64::to_string).collect::<Vec<_>>().join(",");
            for s in self.get_all::<SubjectData>(format!("{BASE}/subjects?ids={list}"))? {
                subjects.insert(s.id, s);
            }
            for m in self.get_all::<StudyMaterialData>(format!("{BASE}/study_materials?subject_ids={list}"))? {
                synonyms.insert(m.data.subject_id, m.data.meaning_synonyms);
            }
        }

        let mut items = Vec::new();
        for a in assignments {
            let Some(res) = subjects.remove(&a.data.subject_id) else { continue };
            let Some(kind) = Kind::from_api(&res.object) else { continue };
            let d = res.data;
            if d.hidden_at.is_some() || d.level > user.subscription.max_level_granted {
                continue;
            }
            let image_svg = if d.characters.is_none() {
                progress(&format!("Fetching image for radical “{}”…", d.slug));
                self.radical_svg(res.id, &d.character_images)
            } else {
                None
            };
            let (mut whitelist, mut blacklist) = (Vec::new(), Vec::new());
            for aux in d.auxiliary_meanings {
                match aux.kind.as_str() {
                    "whitelist" => whitelist.push(aux.meaning),
                    "blacklist" => blacklist.push(aux.meaning),
                    _ => {}
                }
            }
            items.push(ReviewItem {
                assignment_id: a.id,
                subject: Subject {
                    id: res.id,
                    kind,
                    characters: d.characters,
                    slug: d.slug,
                    meanings: d
                        .meanings
                        .into_iter()
                        .map(|m| Meaning { text: m.meaning, primary: m.primary, accepted: m.accepted_answer })
                        .collect(),
                    whitelist,
                    blacklist,
                    synonyms: synonyms.remove(&res.id).unwrap_or_default(),
                    readings: d
                        .readings
                        .into_iter()
                        .map(|r| Reading { text: r.reading, primary: r.primary, accepted: r.accepted_answer, kind: r.kind })
                        .collect(),
                    meaning_mnemonic: d.meaning_mnemonic.unwrap_or_default(),
                    reading_mnemonic: d.reading_mnemonic.unwrap_or_default(),
                    image_svg,
                    audios: d
                        .pronunciation_audios
                        .into_iter()
                        .filter(|a| a.content_type == "audio/mpeg")
                        .map(|a| Pronunciation {
                            reading: a.metadata.get("pronunciation").and_then(|v| v.as_str()).unwrap_or_default().to_string(),
                            voice_actor_id: a.metadata.get("voice_actor_id").and_then(|v| v.as_u64()).unwrap_or(0),
                            url: a.url,
                        })
                        .collect(),
                },
            });
        }
        Ok(items)
    }

    /// SVG for an image-only radical, cached on disk. `None` if unavailable.
    fn radical_svg(&self, id: u64, images: &[ApiImage]) -> Option<Vec<u8>> {
        let cache = radical_cache_path(id);
        if let Some(bytes) = cache.as_ref().and_then(|p| std::fs::read(p).ok()) {
            return Some(bytes);
        }
        let svgs: Vec<&ApiImage> = images.iter().filter(|i| i.content_type == "image/svg+xml").collect();
        let image = svgs
            .iter()
            .find(|i| i.metadata.get("inline_styles").and_then(|v| v.as_bool()) == Some(true))
            .or(svgs.first())?;
        let mut resp = self.agent.get(&image.url).call().ok()?;
        if !resp.status().is_success() {
            return None;
        }
        let bytes = resp.body_mut().read_to_vec().ok()?;
        if let Some(path) = cache {
            if let Some(dir) = path.parent() {
                let _ = std::fs::create_dir_all(dir);
            }
            let _ = std::fs::write(path, &bytes);
        }
        Some(bytes)
    }
}

pub struct SubmitResult {
    pub label: String,
    pub error: Option<String>,
}

/// Background review submission so the UI never waits on the network.
pub struct Submitter {
    pub tx: Sender<Submission>,
    pub results: Receiver<SubmitResult>,
    pub handle: JoinHandle<()>,
}

pub fn spawn_submitter(client: Arc<Client>) -> Submitter {
    let (tx, rx) = mpsc::channel::<Submission>();
    let (res_tx, results) = mpsc::channel();
    let handle = std::thread::spawn(move || {
        for s in rx {
            let mut error = None;
            for attempt in 0..3u64 {
                match client.submit(&s) {
                    Ok(burned) => {
                        if burned {
                            // Burned items never come back for review: drop their cache.
                            for file in &s.cache_files {
                                let _ = std::fs::remove_file(file);
                            }
                        }
                        error = None;
                        break;
                    }
                    Err(e) => {
                        let msg = format!("{e:#}");
                        // Client errors will not succeed on retry.
                        let permanent = msg.contains("(401)") || msg.contains("(403") || msg.contains("error 4");
                        error = Some(msg);
                        if permanent {
                            break;
                        }
                        std::thread::sleep(Duration::from_secs(2 * (attempt + 1)));
                    }
                }
            }
            let _ = res_tx.send(SubmitResult { label: s.label.clone(), error });
        }
    });
    Submitter { tx, results, handle }
}
