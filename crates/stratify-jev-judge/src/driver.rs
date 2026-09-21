use crate::cache::{cache_key, Cache};
use crate::config::JevConfig;
use crate::context::RepoContext;
use crate::judges::{registry, Judge};
use crate::model::Report;
// Only the test module below constructs a `Finding` directly; production
// code here never names the type.
#[cfg(test)]
use crate::model::Finding;
use crate::verdict::{apply, Judgment, Verdict};
use jev_client::{Answer, Client, Question, SystemOneRequest};
use std::collections::BTreeMap;
use std::sync::Arc;

pub const TOKEN_CEILING: usize = 24_000;

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RunStats {
    pub judged: usize,
    pub from_cache: usize,
    pub requested: usize,
    pub failed: usize,
    pub input_tokens: u64,
    pub by_verdict: BTreeMap<String, usize>,
}

/// Group item indices into batches that respect both the count cap and the
/// state token ceiling. An item larger than the ceiling on its own still
/// gets a batch, since dropping it silently would be worse.
pub fn plan_batches(
    items: usize,
    max_per_batch: usize,
    est_tokens: &[usize],
    token_ceiling: usize,
) -> Vec<Vec<usize>> {
    let mut out: Vec<Vec<usize>> = Vec::new();
    let mut cur: Vec<usize> = Vec::new();
    let mut cur_tokens = 0usize;
    for i in 0..items {
        let t = est_tokens.get(i).copied().unwrap_or(0);
        let full = cur.len() >= max_per_batch.max(1);
        let over = !cur.is_empty() && cur_tokens + t > token_ceiling;
        if full || over {
            out.push(std::mem::take(&mut cur));
            cur_tokens = 0;
        }
        cur.push(i);
        cur_tokens += t;
    }
    if !cur.is_empty() {
        out.push(cur);
    }
    out
}

/// Rough token estimate. Four characters per token is close enough to
/// keep a batch under the state ceiling with headroom to spare.
fn estimate_tokens(state: &serde_json::Value) -> usize {
    serde_json::to_string(state).map(|s| s.len() / 4).unwrap_or(0)
}

fn slot_prefix(i: usize) -> String {
    format!("s{i}__")
}

pub struct Driver {
    client: Option<Arc<Client>>,
    cache: Cache,
    cfg: JevConfig,
}

/// One finding's prepared work: its state, its cache key, and where it
/// sits in the report.
struct Prepared {
    index: usize,
    state: serde_json::Value,
    key: String,
    tokens: usize,
}

impl Driver {
    pub fn new(client: Option<Client>, cache: Cache, cfg: JevConfig) -> Driver {
        Driver {
            client: client.map(Arc::new),
            cache,
            cfg,
        }
    }

    /// Judge every finding a registered judge claims. Findings the model
    /// never reaches, for any reason, are left exactly as the engine
    /// reported them.
    pub async fn run(&self, report: &mut Report, ctx: &RepoContext) -> RunStats {
        let mut stats = RunStats::default();
        let Some(client) = self.client.clone() else {
            return stats;
        };

        for judge in registry() {
            let judge: Arc<dyn Judge> = Arc::from(judge);
            let questions = judge.questions();

            // Prepare every finding this judge claims.
            let mut prepared: Vec<Prepared> = Vec::new();
            for (index, f) in report.findings.iter().enumerate() {
                if f.rule != judge.rule() {
                    continue;
                }
                let state = judge.state_for(f, ctx);
                let key = cache_key(
                    judge.rule(),
                    judge.version(),
                    &self.cfg.model,
                    &state,
                    &questions,
                );
                let tokens = estimate_tokens(&state);
                prepared.push(Prepared { index, state, key, tokens });
            }
            if prepared.is_empty() {
                continue;
            }

            // Cache split. Hits are applied now and never batched.
            let mut misses: Vec<Prepared> = Vec::new();
            for p in prepared {
                match self.cache.get(&p.key) {
                    Some(entry) => {
                        stats.from_cache += 1;
                        self.apply_one(
                            report,
                            p.index,
                            &*judge,
                            &entry.answers,
                            &entry.model,
                            &mut stats,
                        );
                    }
                    None => misses.push(p),
                }
            }
            if misses.is_empty() {
                continue;
            }

            // Batch the misses and fire them concurrently.
            let est: Vec<usize> = misses.iter().map(|p| p.tokens).collect();
            let batches = plan_batches(
                misses.len(),
                self.cfg.batch_findings,
                &est,
                TOKEN_CEILING,
            );

            let sem = Arc::new(tokio::sync::Semaphore::new(self.cfg.concurrency.max(1)));
            let mut tasks = Vec::new();
            for batch in batches {
                let mut state = serde_json::Map::new();
                let mut qs: BTreeMap<String, Question> = BTreeMap::new();
                for (slot, &mi) in batch.iter().enumerate() {
                    state.insert(format!("finding_{slot}"), misses[mi].state.clone());
                    for (name, q) in &questions {
                        qs.insert(format!("{}{}", slot_prefix(slot), name), q.clone());
                    }
                }
                let req = SystemOneRequest {
                    state: serde_json::Value::Object(state),
                    model: self.cfg.model.clone(),
                    questions: qs,
                };
                let client = client.clone();
                let sem = sem.clone();
                tasks.push(async move {
                    let _permit = sem.acquire().await.expect("semaphore is never closed");
                    (batch, client.ask(&req).await)
                });
            }

            let results = futures::future::join_all(tasks).await;

            for (batch, result) in results {
                let resp = match result {
                    Ok(r) => r,
                    Err(_) => {
                        stats.failed += 1;
                        continue;
                    }
                };
                stats.requested += 1;
                stats.input_tokens += resp.usage.input_tokens;

                for (slot, &mi) in batch.iter().enumerate() {
                    let prefix = slot_prefix(slot);
                    // Strip the slot prefix so a cache entry is independent
                    // of the batch position this finding happened to get.
                    let answers: BTreeMap<String, Answer> = resp
                        .answers
                        .iter()
                        .filter_map(|(k, v)| {
                            k.strip_prefix(&prefix).map(|n| (n.to_string(), v.clone()))
                        })
                        .collect();
                    if answers.is_empty() {
                        continue;
                    }
                    let _ = self.cache.put(
                        &misses[mi].key,
                        judge.rule(),
                        judge.version(),
                        &resp.model,
                        &answers,
                        resp.usage,
                    );
                    self.apply_one(
                        report,
                        misses[mi].index,
                        &*judge,
                        &answers,
                        &resp.model,
                        &mut stats,
                    );
                }
            }
        }

        stats
    }

    fn apply_one(
        &self,
        report: &mut Report,
        index: usize,
        judge: &dyn Judge,
        answers: &BTreeMap<String, Answer>,
        model: &str,
        stats: &mut RunStats,
    ) {
        // Read the finding before mutating it: policy reads the engine's own
        // severity and confidence, which apply() is about to overwrite.
        let Some(finding) = report.findings.get(index) else {
            return;
        };
        let mut judgment: Judgment = judge.judge(finding, answers, &self.cfg.thresholds);
        if judgment.model.is_empty() {
            judgment.model = if model.is_empty() {
                self.cfg.model.clone()
            } else {
                model.to_string()
            };
        }
        let label = match judgment.verdict {
            Verdict::Dismiss => "dismiss",
            Verdict::Weaken => "weaken",
            Verdict::Keep => "keep",
            Verdict::Strengthen => "strengthen",
        };
        *stats.by_verdict.entry(label.to_string()).or_insert(0) += 1;
        stats.judged += 1;
        if let Some(f) = report.findings.get_mut(index) {
            apply(f, &judgment, judge.full_strength());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{Confidence, Severity, Span};
    use jev_client::RetryPolicy;
    use serde_json::json;
    use std::path::PathBuf;
    use wiremock::matchers::method;
    use wiremock::{Mock, MockServer, ResponseTemplate};

    fn ctx() -> RepoContext {
        let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../tests/fixtures/sample-repo");
        RepoContext::new(root).unwrap()
    }

    fn report(n: usize) -> Report {
        Report {
            schema_version: 1,
            findings: (0..n)
                .map(|i| Finding {
                    rule: "dead_code".into(),
                    severity: Severity::Info,
                    message: format!("possibly unused function `fn{i}`"),
                    span: Span {
                        file: "src/lib.rs".into(),
                        start_byte: 0,
                        end_byte: 5,
                        start_line: 1,
                    },
                    confidence: Confidence::Likely,
                    extra: serde_json::Map::new(),
                })
                .collect(),
            extra: serde_json::Map::new(),
        }
    }

    #[test]
    fn batches_respect_the_count_cap() {
        let est = vec![100; 25];
        let b = plan_batches(25, 10, &est, 24_000);
        assert_eq!(b.len(), 3);
        assert_eq!(b[0].len(), 10);
        assert_eq!(b[2].len(), 5);
    }

    #[test]
    fn a_batch_splits_before_it_exceeds_the_token_ceiling() {
        let est = vec![10_000; 6];
        let b = plan_batches(6, 10, &est, 24_000);
        assert!(b.iter().all(|batch| batch.len() <= 2), "got {b:?}");
        assert_eq!(b.iter().map(Vec::len).sum::<usize>(), 6);
    }

    #[test]
    fn a_single_oversized_item_still_gets_its_own_batch() {
        let est = vec![90_000];
        let b = plan_batches(1, 10, &est, 24_000);
        assert_eq!(b, vec![vec![0]]);
    }

    #[tokio::test]
    async fn without_a_client_the_report_passes_through_untouched() {
        let dir = tempfile::tempdir().unwrap();
        let d = Driver::new(None, Cache::new(dir.path().into(), true), JevConfig::default());
        let mut r = report(3);
        let before = r.clone();
        let stats = d.run(&mut r, &ctx()).await;
        assert_eq!(r, before);
        assert_eq!(stats.judged, 0);
        assert_eq!(stats.requested, 0);
    }

    #[tokio::test]
    async fn judges_every_finding_and_applies_the_verdict() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "model": "jev-1.13.0",
                "answers": {
                    "s0__framework_invoked": { "noul": 0.95 },
                    "s0__test_only": { "noul": 0.01 },
                    "s0__external_api": { "noul": 0.02 },
                    "s0__explanation": {
                        "choice": "framework_invoked",
                        "probabilities": { "framework_invoked": 0.9 },
                        "confidence": 0.9
                    }
                },
                "usage": { "input_tokens": 300, "output_tokens": 0 }
            })))
            .mount(&server)
            .await;

        let dir = tempfile::tempdir().unwrap();
        let client = Client::new(server.uri(), "k".into());
        let d = Driver::new(Some(client), Cache::new(dir.path().into(), true), JevConfig::default());

        let mut r = report(1);
        let stats = d.run(&mut r, &ctx()).await;

        assert_eq!(stats.judged, 1);
        assert_eq!(stats.requested, 1);
        assert_eq!(r.findings[0].confidence, Confidence::Unknown);
        assert_eq!(r.findings[0].extra["judgment"]["verdict"], "dismiss");
        assert_eq!(r.findings[0].extra["judgment"]["model"], "jev-1.13.0");
    }

    #[tokio::test]
    async fn a_second_run_is_served_entirely_from_cache() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "model": "jev-1.13.0",
                "answers": {
                    "s0__framework_invoked": { "noul": 0.95 },
                    "s0__test_only": { "noul": 0.01 },
                    "s0__external_api": { "noul": 0.02 },
                    "s0__explanation": {
                        "choice": "framework_invoked",
                        "probabilities": {},
                        "confidence": 0.9
                    }
                },
                "usage": { "input_tokens": 300, "output_tokens": 0 }
            })))
            .expect(1)
            .mount(&server)
            .await;

        let dir = tempfile::tempdir().unwrap();
        let cfg = JevConfig::default();
        let make = || {
            Driver::new(
                Some(Client::new(server.uri(), "k".into())),
                Cache::new(dir.path().into(), true),
                cfg.clone(),
            )
        };

        let mut first = report(1);
        make().run(&mut first, &ctx()).await;

        let mut second = report(1);
        let stats = make().run(&mut second, &ctx()).await;

        assert_eq!(stats.requested, 0);
        assert_eq!(stats.from_cache, 1);
        assert_eq!(second.findings[0].confidence, Confidence::Unknown);
    }

    #[tokio::test]
    async fn a_failed_request_leaves_its_findings_untouched() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .respond_with(ResponseTemplate::new(500))
            .mount(&server)
            .await;

        let dir = tempfile::tempdir().unwrap();
        let cfg = JevConfig {
            concurrency: 2,
            ..JevConfig::default()
        };
        let d = Driver::new(
            Some(Client::new(server.uri(), "k".into()).with_retry(RetryPolicy {
                max_attempts: 2,
                base_delay: std::time::Duration::from_millis(1),
            })),
            Cache::new(dir.path().into(), true),
            cfg,
        );

        let mut r = report(2);
        let before = r.clone();
        let stats = d.run(&mut r, &ctx()).await;

        assert_eq!(stats.failed, 1);
        assert_eq!(stats.judged, 0);
        assert_eq!(r, before);
    }
}
