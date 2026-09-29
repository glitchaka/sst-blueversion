//! Pure, repeatable correlation of available evidence. No collection or network I/O.
use std::collections::BTreeMap;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Classification {
    Normal,
    Performance,
    Attention,
    Suspicious,
    Alert,
}

impl Classification {
    pub fn label(self) -> &'static str {
        match self {
            Self::Normal => "NORMAL",
            Self::Performance => "PERFORMANCE",
            Self::Attention => "ATTENTION",
            Self::Suspicious => "SUSPICIOUS",
            Self::Alert => "ALERT",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Family {
    Identity,
    Lineage,
    Execution,
    Persistence,
    Network,
    ResourceUsage,
    History,
    ExternalIntel,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ObservationState {
    Known,
    Unknown,
    Pending,
    AccessDenied,
    Unavailable,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Strength {
    Context,
    Weak,
    Anomaly,
    Strong,
}

#[derive(Debug, Clone)]
pub struct Evidence {
    pub family: Family,
    pub state: ObservationState,
    pub strength: Strength,
    pub reason: String,
}

impl Evidence {
    pub fn known(family: Family, strength: Strength, reason: impl Into<String>) -> Self {
        Self {
            family,
            state: ObservationState::Known,
            strength,
            reason: reason.into(),
        }
    }
}

#[derive(Debug, Clone)]
pub struct Assessment {
    pub classification: Classification,
    pub reasons: Vec<String>,
    pub limitations: Vec<String>,
}

/// Collectors must verify the exact digest and the source's validity before
/// supplying ExactBlockedHash. Filename resemblance never raises priority.
#[derive(Debug, Clone, Copy)]
pub enum Reputation {
    ExactBlockedHash,
    NameResemblance,
    NoMatch,
}

impl Reputation {
    pub fn evidence(self, source: &str) -> Evidence {
        let (strength, reason) = match self {
            Self::ExactBlockedHash => (Strength::Strong, "exact SHA-256 blocklist match"),
            Self::NameResemblance => (Strength::Context, "filename resemblance only"),
            Self::NoMatch => (
                Strength::Context,
                "no reputation match; local evidence remains",
            ),
        };
        Evidence::known(
            Family::ExternalIntel,
            strength,
            format!("{source}: {reason}"),
        )
    }
}

/// Keep only the strongest contribution of each independent family. Trust and
/// negative reputation results are context, never subtractors from current evidence.
/// Exact malicious SHA-256 matches are Strong; name resemblance is Context.
pub fn correlate(evidence: &[Evidence]) -> Assessment {
    let mut families = BTreeMap::new();
    let mut reasons = Vec::new();
    let mut limitations = Vec::new();
    let mut performance = false;
    for item in evidence {
        if item.state != ObservationState::Known {
            let limitation = format!("{}: {:?}", item.reason, item.state);
            if !limitations.contains(&limitation) {
                limitations.push(limitation);
            }
            continue;
        }
        if item.strength == Strength::Context {
            continue;
        }
        if !reasons.contains(&item.reason) {
            reasons.push(item.reason.clone());
        }
        if item.family == Family::ResourceUsage {
            performance = true;
        } else {
            let strength = families.entry(item.family).or_insert(item.strength);
            *strength = (*strength).max(item.strength);
        }
    }
    let anomaly = families.values().any(|s| *s >= Strength::Anomaly);
    let classification = if families.values().any(|s| *s == Strength::Strong) {
        Classification::Alert
    } else if families.len() >= 3 && anomaly {
        Classification::Suspicious
    } else if anomaly || families.len() >= 2 {
        Classification::Attention
    } else if performance {
        Classification::Performance
    } else {
        Classification::Normal
    };
    Assessment {
        classification,
        reasons,
        limitations,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn e(f: Family, s: Strength) -> Evidence {
        Evidence::known(f, s, format!("{f:?} {s:?}"))
    }
    #[test]
    fn related_identity_observations_do_not_escalate() {
        let mut items = vec![e(Family::Identity, Strength::Weak); 3];
        assert_eq!(correlate(&items).classification, Classification::Normal);
        items.push(e(Family::ResourceUsage, Strength::Strong));
        assert_eq!(
            correlate(&items).classification,
            Classification::Performance
        );
    }
    #[test]
    fn independent_families_converge() {
        let mut items = vec![
            e(Family::Lineage, Strength::Anomaly),
            e(Family::Network, Strength::Weak),
        ];
        assert_eq!(correlate(&items).classification, Classification::Attention);
        items.push(e(Family::Persistence, Strength::Anomaly));
        assert_eq!(correlate(&items).classification, Classification::Suspicious);
    }
    #[test]
    fn strong_current_evidence_overrides_trust() {
        let items = [
            e(Family::History, Strength::Context),
            e(Family::Identity, Strength::Context),
            e(Family::ExternalIntel, Strength::Strong),
        ];
        assert_eq!(correlate(&items).classification, Classification::Alert);
        assert_eq!(
            correlate(&[e(Family::ExternalIntel, Strength::Context)]).classification,
            Classification::Normal
        );
    }
    #[test]
    fn reputation_requires_exact_match_and_cannot_clear_local_anomaly() {
        assert_eq!(
            correlate(&[Reputation::ExactBlockedHash.evidence("local")]).classification,
            Classification::Alert
        );
        assert_eq!(
            correlate(&[Reputation::NameResemblance.evidence("local")]).classification,
            Classification::Normal
        );
        let items = [
            Reputation::NoMatch.evidence("cache"),
            e(Family::Lineage, Strength::Anomaly),
        ];
        assert_eq!(correlate(&items).classification, Classification::Attention);
    }
    #[test]
    fn missing_data_never_becomes_evidence_and_enrichment_recalculates() {
        for state in [
            ObservationState::Unknown,
            ObservationState::Pending,
            ObservationState::AccessDenied,
            ObservationState::Unavailable,
        ] {
            let mut item = e(Family::ExternalIntel, Strength::Strong);
            item.state = state;
            let result = correlate(&[item.clone()]);
            assert_eq!(result.classification, Classification::Normal);
            assert_eq!(result.limitations.len(), 1);
            item.state = ObservationState::Known;
            assert_eq!(correlate(&[item]).classification, Classification::Alert);
            assert_eq!(correlate(&[]).classification, Classification::Normal);
        }
    }
}
