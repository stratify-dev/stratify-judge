pub mod dead_code;

use crate::config::Thresholds;
use crate::context::RepoContext;
use crate::model::{Finding, Severity};
use crate::verdict::Judgment;
use std::collections::BTreeMap;
use systemone_client::{Answer, Question};

/// Adjusts findings the engine already produced.
pub trait Judge: Send + Sync {
    /// Stable id matching the engine's rule name.
    fn rule(&self) -> &'static str;

    /// Bumped whenever state shape or question text changes. Part of the
    /// cache key, so a bump invalidates every entry for this judge.
    fn version(&self) -> u32;

    /// Severity restored by Verdict::Strengthen.
    fn full_strength(&self) -> Severity;

    /// Nested JSON state for one finding.
    fn state_for(&self, finding: &Finding, ctx: &RepoContext) -> serde_json::Value;

    /// Questions under canonical names. The driver prefixes them per batch slot.
    fn questions(&self) -> BTreeMap<String, Question>;

    /// Map raw answers to a verdict and a reason. The finding is passed so
    /// policy can read the engine's own severity and confidence.
    fn judge(
        &self,
        finding: &Finding,
        answers: &BTreeMap<String, Answer>,
        cfg: &Thresholds,
    ) -> Judgment;
}

/// Every judge registered in this build.
pub fn registry() -> Vec<Box<dyn Judge>> {
    vec![Box::new(dead_code::DeadCodeJudge)]
}
