//! The judgment: every rule of the pack is a yes/no question Sage answers
//! about the text with a calibrated probability. A rule fires when P(yes)
//! reaches its threshold, and the strictest fired action decides — `block`
//! over `escalate` over `pass`. Nothing in the decision is free text.

use crate::nearai::Client;
use crate::pack::{Action, Loaded};
use serde::{Deserialize, Serialize};
use serde_json::{json, Map, Value};
use std::collections::HashMap;

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Decision {
    Pass,
    Escalate,
    Block,
}

impl Action {
    fn decision(self) -> Decision {
        match self {
            Action::Escalate => Decision::Escalate,
            Action::Block => Decision::Block,
        }
    }
}

/// One rule, as the model answered it.
#[derive(Debug)]
pub struct Outcome {
    pub id: String,
    pub action: Action,
    /// P(yes) in basis points, 0–10000: an integer, so the signed verdict
    /// reads the same in every language that parses it.
    pub p_yes_bp: u16,
    pub fired: bool,
}

#[derive(Debug)]
pub struct Judgment {
    pub decision: Decision,
    pub outcomes: Vec<Outcome>,
}

/// One `noul` (yes/no) answer of the decision model.
#[derive(Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
enum Answer {
    Noul { noul: f64 },
}

#[derive(Deserialize)]
struct Decisions {
    answers: HashMap<String, Answer>,
}

/// The request for a pack: each rule's question, keyed by its id, and the
/// text as the state the questions are about.
fn request(loaded: &Loaded, text: &str) -> Result<Vec<u8>, String> {
    let questions: Map<String, Value> =
        loaded.pack.rules.iter().map(|rule| (rule.id.clone(), json!({"type": "noul", "instructions": rule.question}))).collect();
    serde_json::to_vec(&json!({"model": loaded.pack.model, "state": text, "questions": questions}))
        .map_err(|e| format!("the decision request does not encode: {e}"))
}

pub fn judge(api: &Client, text: &str, loaded: &Loaded) -> Result<Judgment, String> {
    let response = api.decide(&request(loaded, text)?)?;
    let model = &loaded.pack.model;
    let decisions: Decisions =
        serde_json::from_slice(&response).map_err(|e| format!("{model}'s decisions do not read as yes/no answers: {e}"))?;
    let answers: HashMap<String, f64> = decisions.answers.into_iter().map(|(id, Answer::Noul { noul })| (id, noul)).collect();
    let outcomes = outcomes(loaded, &answers)?;
    let decision = outcomes.iter().filter(|o| o.fired).map(|o| o.action.decision()).max().unwrap_or(Decision::Pass);
    Ok(Judgment { decision, outcomes })
}

fn outcomes(loaded: &Loaded, answers: &HashMap<String, f64>) -> Result<Vec<Outcome>, String> {
    let model = &loaded.pack.model;
    loaded
        .pack
        .rules
        .iter()
        .map(|rule| {
            let p = *answers.get(&rule.id).ok_or_else(|| format!("{model} gave no answer for rule {:?}", rule.id))?;
            if !(0.0..=1.0).contains(&p) {
                return Err(format!("{model} answered rule {:?} with {p}, not a probability", rule.id));
            }
            let p_yes_bp = (p * 10_000.0).round() as u16;
            Ok(Outcome {
                id: rule.id.clone(),
                action: rule.action,
                p_yes_bp,
                // The threshold is a whole percent; the comparison is on the
                // basis points the verdict carries, so a reader recomputes it.
                fired: u32::from(p_yes_bp) >= u32::from(rule.threshold) * 100,
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pack::Pack;

    fn loaded() -> Loaded {
        let pack = Pack::parse(
            br#"{"id":"grants/digital-art","version":1,"model":"levanto/sage-2","rules":[
                {"id":"off_theme","question":"Is it off theme?","threshold":60,"action":"block"},
                {"id":"large_budget","question":"Is the budget large?","threshold":70,"action":"escalate"}]}"#,
        )
        .unwrap();
        Loaded { pack, sha256: "s".into(), registry: "r".into(), block_height: 1, publisher: "p".into(), deprecated: false }
    }

    fn decide(off_theme: f64, large: f64) -> (Decision, Vec<u16>) {
        let answers = HashMap::from([("off_theme".to_string(), off_theme), ("large_budget".to_string(), large)]);
        let outcomes = outcomes(&loaded(), &answers).unwrap();
        let decision = outcomes.iter().filter(|o| o.fired).map(|o| o.action.decision()).max().unwrap_or(Decision::Pass);
        (decision, outcomes.iter().map(|o| o.p_yes_bp).collect())
    }

    #[test]
    fn the_strictest_fired_action_decides() {
        assert_eq!(decide(0.01, 0.02).0, Decision::Pass);
        assert_eq!(decide(0.01, 0.95).0, Decision::Escalate);
        assert_eq!(decide(0.99, 0.95).0, Decision::Block, "block wins over escalate");
    }

    #[test]
    fn a_threshold_is_met_on_the_basis_points_the_verdict_carries() {
        assert_eq!(decide(0.6, 0.0), (Decision::Block, vec![6000, 0]), "exactly the threshold fires");
        assert_eq!(decide(0.59996, 0.0), (Decision::Block, vec![6000, 0]), "rounded to 6000 bp, it fires as the reader sees it");
        assert_eq!(decide(0.59949, 0.0).0, Decision::Pass);
    }

    #[test]
    fn every_rule_must_be_answered_with_a_probability() {
        let missing = HashMap::from([("off_theme".to_string(), 0.1)]);
        assert!(outcomes(&loaded(), &missing).unwrap_err().contains("large_budget"));
        let wrong = HashMap::from([("off_theme".to_string(), 1.5), ("large_budget".to_string(), 0.0)]);
        assert!(outcomes(&loaded(), &wrong).unwrap_err().contains("not a probability"));
    }

    #[test]
    fn a_decision_reads_as_a_yes_no_probability_and_nothing_else() {
        let ok: Decisions = serde_json::from_str(r#"{"answers":{"pii":{"type":"noul","noul":0.93}}}"#).unwrap();
        assert!(matches!(ok.answers["pii"], Answer::Noul { noul } if noul == 0.93));
        assert!(serde_json::from_str::<Decisions>(r#"{"answers":{"pii":{"type":"choice","choice":"a"}}}"#).is_err());
    }
}
