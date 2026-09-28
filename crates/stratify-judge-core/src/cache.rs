use crate::backend::Backend;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::io;
use std::path::PathBuf;
use systemone_client::{Answer, Question, Usage};

/// One cached finding's answers. Raw answers only, never verdicts, so a
/// threshold change re-scores from disk with no network call.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Entry {
    pub key: String,
    pub judge: String,
    pub judge_version: u32,
    pub model: String,
    pub answers: BTreeMap<String, Answer>,
    pub usage: Usage,
}

/// SHA-256 over every input that could change an answer. Per finding, not
/// per request: editing one function must not invalidate its batch mates.
pub fn cache_key(
    judge: &str,
    version: u32,
    backend: &Backend,
    state: &serde_json::Value,
    questions: &BTreeMap<String, Question>,
) -> String {
    let mut h = Sha256::new();
    feed(&mut h, judge.as_bytes());
    h.update(version.to_le_bytes()); // fixed width, needs no framing

    // The backend identifies the model that answered, and the endpoint it
    // answered from. Without both, a committed cache would serve one
    // model's verdicts to another's run, or one endpoint's answers under
    // a name that no longer names where they came from.
    feed(&mut h, backend.name.as_bytes());
    feed(&mut h, backend.url.as_bytes());
    // Tagged, so an absent model and an empty one are different inputs.
    match &backend.model {
        Some(m) => {
            h.update([1u8]);
            feed(&mut h, m.as_bytes());
        }
        None => h.update([0u8]),
    }
    feed(&mut h, &serde_json::to_vec(state).unwrap_or_default());
    feed(&mut h, &serde_json::to_vec(questions).unwrap_or_default());
    format!("{:x}", h.finalize())
}

/// Hash one variable-length field prefixed by its length, so field
/// boundaries are unambiguous and no two different inputs can produce the
/// same byte stream. A single-byte delimiter would be weaker: a field
/// whose content includes that byte could realign the stream. `model`
/// comes from user TOML and is not under this code's control.
///
/// This costs nothing now and cannot be changed cheaply later: the cache
/// is meant to be committed to git, so altering the hash input format
/// invalidates every entry anyone has already committed.
fn feed(h: &mut Sha256, bytes: &[u8]) {
    h.update((bytes.len() as u64).to_le_bytes());
    h.update(bytes);
}

pub struct Cache {
    dir: PathBuf,
    enabled: bool,
}

impl Cache {
    pub fn new(dir: PathBuf, enabled: bool) -> Cache {
        Cache { dir, enabled }
    }

    fn path_for(&self, key: &str) -> PathBuf {
        let shard = key.get(..2).unwrap_or("00");
        self.dir.join(shard).join(format!("{key}.json"))
    }

    /// A miss for any reason, including a corrupt or unreadable entry.
    /// A bad cache file costs one request, never a crash. Returns the whole
    /// entry so a cache-hit judgment records the model that actually
    /// produced the answers, not the config default.
    pub fn get(&self, key: &str) -> Option<Entry> {
        if !self.enabled {
            return None;
        }
        let text = std::fs::read_to_string(self.path_for(key)).ok()?;
        serde_json::from_str(&text).ok()
    }

    pub fn put(
        &self,
        key: &str,
        judge: &str,
        version: u32,
        model: &str,
        answers: &BTreeMap<String, Answer>,
        usage: Usage,
    ) -> io::Result<()> {
        if !self.enabled {
            return Ok(());
        }
        let entry = Entry {
            key: key.to_string(),
            judge: judge.to_string(),
            judge_version: version,
            model: model.to_string(),
            answers: answers.clone(),
            usage,
        };
        let path = self.path_for(key);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let body = serde_json::to_string_pretty(&entry)
            .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
        std::fs::write(path, body)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::backend::Backend;
    use serde_json::json;
    use systemone_client::{Answer, Question, Usage};

    fn questions() -> BTreeMap<String, Question> {
        [("framework_invoked".to_string(), Question::noul("q", None))]
            .into_iter()
            .collect()
    }

    fn answers() -> BTreeMap<String, Answer> {
        [("framework_invoked".to_string(), Answer::Noul { noul: 0.91 })]
            .into_iter()
            .collect()
    }

    #[test]
    fn key_is_stable_for_identical_inputs() {
        let s = json!({ "name": "helper" });
        let a = cache_key("dead_code", 1, &Backend::jev(), &s, &questions());
        let b = cache_key("dead_code", 1, &Backend::jev(), &s, &questions());
        assert_eq!(a, b);
        assert_eq!(a.len(), 64);
    }

    #[test]
    fn key_changes_when_any_input_changes() {
        let s = json!({ "name": "helper" });
        let base = cache_key("dead_code", 1, &Backend::jev(), &s, &questions());
        // A different judge must never collide with this one. Two judges
        // sharing a key would serve one judge's verdict to the other.
        assert_ne!(
            base,
            cache_key("duplication", 1, &Backend::jev(), &s, &questions())
        );
        assert_ne!(
            base,
            cache_key("dead_code", 2, &Backend::jev(), &s, &questions())
        );
        assert_ne!(
            base,
            cache_key(
                "dead_code",
                1,
                &Backend {
                    model: Some("jev-1.13.0".into()),
                    ..Backend::jev()
                },
                &s,
                &questions()
            )
        );
        assert_ne!(
            base,
            cache_key(
                "dead_code",
                1,
                &Backend::jev(),
                &json!({ "name": "other" }),
                &questions()
            )
        );
        let mut q2 = questions();
        q2.insert("test_only".into(), Question::noul("q2", None));
        assert_ne!(base, cache_key("dead_code", 1, &Backend::jev(), &s, &q2));
    }

    #[test]
    fn round_trips_answers_through_disk() {
        let dir = tempfile::tempdir().unwrap();
        let c = Cache::new(dir.path().to_path_buf(), true);
        let key = "a".repeat(64);
        assert!(c.get(&key).is_none());

        c.put(
            &key,
            "dead_code",
            1,
            "jev-1.13.0",
            &answers(),
            Usage {
                input_tokens: 10,
                output_tokens: 0,
            },
        )
        .unwrap();

        let got = c.get(&key).unwrap();
        assert_eq!(got.model, "jev-1.13.0");
        assert_eq!(got.answers["framework_invoked"].as_noul(), Some(0.91));
    }

    #[test]
    fn a_disabled_cache_never_reads_or_writes() {
        let dir = tempfile::tempdir().unwrap();
        let c = Cache::new(dir.path().to_path_buf(), false);
        let key = "b".repeat(64);
        c.put(
            &key,
            "dead_code",
            1,
            "jev-1.13.0",
            &answers(),
            Usage {
                input_tokens: 10,
                output_tokens: 0,
            },
        )
        .unwrap();
        assert!(c.get(&key).is_none());
        assert!(!dir.path().join("bb").exists());
    }

    #[test]
    fn a_corrupt_entry_reads_as_a_miss_rather_than_an_error() {
        let dir = tempfile::tempdir().unwrap();
        let c = Cache::new(dir.path().to_path_buf(), true);
        let key = "c".repeat(64);
        std::fs::create_dir_all(dir.path().join("cc")).unwrap();
        std::fs::write(
            dir.path().join("cc").join(format!("{key}.json")),
            "{ not json",
        )
        .unwrap();
        assert!(c.get(&key).is_none());
    }

    /// Review Focus 4: the cache is designed to be committed, so a
    /// cross-backend hit would serve verdicts from a model the user did
    /// not run.
    #[test]
    fn two_backends_never_share_a_key() {
        let s = json!({ "name": "helper" });
        let q = questions();
        let jev = cache_key("dead_code", 1, &Backend::jev(), &s, &q);
        let laya = cache_key("dead_code", 1, &Backend::laya(), &s, &q);
        assert_ne!(jev, laya);

        // Same name, different model id: still distinct.
        let pinned = Backend {
            model: Some("jev-1.13.0".into()),
            ..Backend::jev()
        };
        assert_ne!(jev, cache_key("dead_code", 1, &pinned, &s, &q));

        // An absent model must not hash the same as an empty one.
        let empty = Backend {
            model: Some(String::new()),
            ..Backend::laya()
        };
        assert_ne!(laya, cache_key("dead_code", 1, &empty, &s, &q));
    }

    /// I2: `--base-url` changes which model answers without changing the
    /// backend's name, so the url has to be part of the key too, or a
    /// committed cache would serve one endpoint's answers to another's run.
    #[test]
    fn two_backends_identical_except_for_url_produce_different_keys() {
        let s = json!({ "name": "helper" });
        let q = questions();
        let a = cache_key("dead_code", 1, &Backend::jev(), &s, &q);
        let b = cache_key(
            "dead_code",
            1,
            &Backend {
                url: "http://127.0.0.1:9999".into(),
                ..Backend::jev()
            },
            &s,
            &q,
        );
        assert_ne!(a, b, "same name and model, different url, must differ");
    }
}
