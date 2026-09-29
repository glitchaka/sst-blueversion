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

#[allow(dead_code)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ObservationState {
    Known,
    Unknown,
    Pending,
    AccessDenied,
    Unavailable,
}

impl ObservationState {
    pub fn label(self) -> &'static str {
        match self {
            Self::Known => "KNOWN",
            Self::Unknown => "UNKNOWN",
            Self::Pending => "PENDING",
            Self::AccessDenied => "ACCESS_DENIED",
            Self::Unavailable => "UNAVAILABLE",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Strength {
    Context,
    Weak,
    Anomaly,
    Strong,
    /// Reserved for verified exact malicious identity matches, not generic suspicion.
    Decisive,
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

/// Compare only verified digests supplied by enrichment, never path identity.
#[allow(dead_code)]
pub fn compare_hashes(previous: &str, current: &str) -> Evidence {
    let valid = |hash: &str| hash.len() == 64 && hash.bytes().all(|b| b.is_ascii_hexdigit());
    if !valid(previous) || !valid(current) {
        return Evidence {
            family: Family::Identity,
            state: ObservationState::Unknown,
            strength: Strength::Context,
            reason: "SHA-256 comparison requires two valid digests".into(),
        };
    }
    if previous.eq_ignore_ascii_case(current) {
        Evidence::known(
            Family::Identity,
            Strength::Context,
            "same verified SHA-256; not a safety verdict",
        )
    } else {
        Evidence::known(
            Family::Identity,
            Strength::Strong,
            "SHA-256 changed at previously observed path",
        )
    }
}

#[derive(Debug, Clone, Default)]
pub struct ParentProfile {
    pub executions: BTreeMap<String, u64>,
}

impl ParentProfile {
    pub fn observe(&mut self, parent: String) {
        *self.executions.entry(parent).or_default() += 1;
    }

    pub fn evidence(&self, parent: &str) -> Evidence {
        let total: u64 = self.executions.values().sum();
        if total < 3 {
            return Evidence {
                family: Family::Lineage,
                state: ObservationState::Unavailable,
                strength: Strength::Context,
                reason: format!("parent baseline insufficient ({total} executions; minimum 3)"),
            };
        }
        let count = self.executions.get(parent).copied().unwrap_or(0);
        let frequency = count as f64 / total as f64;
        let distinct_parents = self.executions.len();
        let dominant_share = self
            .executions
            .values()
            .copied()
            .max()
            .map(|count| count as f64 / total as f64)
            .unwrap_or(0.0);
        // Some legitimate host runtimes (WebView2 is the common case) are
        // intentionally spawned by many unrelated applications. For a mature
        // profile with broad parent diversity, a new/rare parent is weak
        // novelty, not a lineage anomaly by itself.
        let diverse_parent_profile =
            total >= 20 && distinct_parents >= 4 && dominant_share < 0.80;

        let (strength, reason) = if count == 0 && total < 20 {
            (
                Strength::Weak,
                format!("parent not observed in immature profile: {parent} (0/{total} executions)"),
            )
        } else if count == 0 && diverse_parent_profile {
            (
                Strength::Weak,
                format!(
                    "new parent in diverse historical profile: {parent} (0/{total} executions across {distinct_parents} parents)"
                ),
            )
        } else if count == 0 {
            (
                Strength::Anomaly,
                format!(
                    "parent never observed in mature historical profile: {parent} (0/{total} executions)"
                ),
            )
        } else if total >= 20 && frequency < 0.05 && diverse_parent_profile {
            (
                Strength::Weak,
                format!(
                    "rare parent in diverse historical profile: {parent} ({count}/{total} executions, {:.1}%)",
                    frequency * 100.0
                ),
            )
        } else if total >= 20 && frequency < 0.05 {
            (
                Strength::Anomaly,
                format!(
                    "rare historical parent: {parent} ({count}/{total} executions, {:.1}%)",
                    frequency * 100.0
                ),
            )
        } else {
            (
                Strength::Context,
                format!("observed historical parent: {parent} ({count}/{total} executions)"),
            )
        };
        Evidence::known(Family::Lineage, strength, reason)
    }

    pub fn anomaly(&self, parent: &str) -> Option<String> {
        let evidence = self.evidence(parent);
        (evidence.strength >= Strength::Weak).then_some(evidence.reason)
    }
}

/// Collectors must verify the exact digest and the source's validity before
/// supplying ExactBlockedHash. Filename resemblance never raises priority.
#[allow(dead_code)]
#[derive(Debug, Clone, Copy)]
pub enum Reputation {
    ExactBlockedHash,
    NameResemblance,
    NoMatch,
}

#[allow(dead_code)]
impl Reputation {
    pub fn evidence(self, source: &str) -> Evidence {
        let (strength, reason) = match self {
            Self::ExactBlockedHash => (Strength::Decisive, "exact SHA-256 blocklist match"),
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
/// Exact malicious SHA-256 matches are Decisive; generic Strong requires convergence.
pub fn correlate(evidence: &[Evidence]) -> Assessment {
    let mut families = BTreeMap::new();
    let mut reasons = Vec::new();
    let mut limitations = Vec::new();
    let mut performance = false;
    for item in evidence {
        if item.state != ObservationState::Known {
            let limitation = format!("{}: {}", item.reason, item.state.label());
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
    let strong_families = families
        .values()
        .filter(|s| **s >= Strength::Strong)
        .count();
    let classification =
        if families.values().any(|s| *s == Strength::Decisive) || strong_families >= 2 {
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
    fn diverse_parent_profiles_do_not_raise_attention_by_lineage_alone() {
        let mut profile = ParentProfile::default();
        for index in 0..27 {
            profile.observe(format!("host{}.exe", index % 6));
        }
        let evidence = profile.evidence("brand-new-host.exe");
        assert_eq!(evidence.strength, Strength::Weak);
        assert_eq!(correlate(&[evidence]).classification, Classification::Normal);
    }

    #[test]
    fn stable_mature_parent_profiles_still_flag_new_parent() {
        let mut profile = ParentProfile::default();
        for _ in 0..27 {
            profile.observe("stable-host.exe".into());
        }
        let evidence = profile.evidence("unexpected-host.exe");
        assert_eq!(evidence.strength, Strength::Anomaly);
        assert_eq!(correlate(&[evidence]).classification, Classification::Attention);
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
            Reputation::ExactBlockedHash.evidence("local"),
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
            let mut item = Reputation::ExactBlockedHash.evidence("local");
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
