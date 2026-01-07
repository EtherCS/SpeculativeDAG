use crate::{
    committee::Committee,
    types::{AuthorityIndex, Stake},
};
use std::{cmp::Reverse, fmt, sync::Arc};

/// NodeReputation keeps track of the reputation scores of authorities in the committee.
/// The index of the scores corresponds to the `AuthorityIndex` of the authority.
pub struct NodeReputation {
    /// Reputation scores for each authority in the committee
    scores: Vec<i128>,
    /// Deviation from the threshold score to consider an authority as top authority
    threshold_deviation: u64,
    /// The minimum stake required for reputation
    /// This is a trade-off:
    /// (1) if we set it too large, we risk including malicious nodes;
    /// (2) if we set it too small (>=2f+1), we risk triggering smart_ancestor timeouts and excluding honest leader blocks
    /// We use threshold_deviation to balance the trade-off
    reputation_threshold: Stake,
}

impl NodeReputation {
    pub fn new(
        committee: Arc<Committee>,
        threshold_deviation: u64,
        threshold_numerator: u64,
        threshold_denominator: u64,
        initial_score: i128,
    ) -> Self {
        let authorities = committee.authorities();
        let mut total_stake: Stake = 0;
        for authority in authorities {
            if let Some(stake) = committee.get_stake(authority) {
                total_stake += stake;
            }
        }
        let reputation_threshold = threshold_numerator * total_stake / threshold_denominator + 1; // we currently set it 67%, but we could use 80% here based on the observation in practice
        Self {
            scores: vec![initial_score; committee.len()],
            threshold_deviation,
            reputation_threshold,
        }
    }

    pub fn update_score(&mut self, authority: AuthorityIndex, delta: i128) {
        if let Some(score) = self.scores.get_mut(authority as usize) {
            *score += delta;
        }
    }

    pub fn update_score_batch(&mut self, authorities: Vec<AuthorityIndex>, delta: i128) {
        for authority in authorities {
            self.update_score(authority, delta);
        }
    }

    pub fn get_score(&self, authority: AuthorityIndex) -> i128 {
        self.scores.get(authority as usize).cloned().unwrap_or(0)
    }

    pub fn get_all_scores(&self) -> Vec<i128> {
        self.scores.clone()
    }

    /// Returns the top committee.quorum_threshold authorities with the acceptable reputation scores.
    /// The acceptable reputation scores is in range [reputation_threshold_score - reputation_threshold_score * self.threshold_deviation / 100, highest_score]
    /// If there are ties, returns more than committee.quorum_threshold authorities.
    pub fn top_authorities(&self) -> Vec<AuthorityIndex> {
        let authorities: Vec<_> = (0..self.scores.len())
            .map(|i| AuthorityIndex::from(i as u32))
            .collect();
        let threshold_score = self.threshold_score();
        authorities
            .into_iter()
            .filter(|&a| self.get_score(a) >= threshold_score)
            .collect()
    }

    /// Returns the threshold score
    pub fn threshold_score(&self) -> i128 {
        let mut authorities: Vec<_> = (0..self.scores.len())
            .map(|i| AuthorityIndex::from(i as u32))
            .collect();
        authorities.sort_by_key(|&a| Reverse(self.get_score(a)));
        let reputation_threshold = self.reputation_threshold as usize;
        let reputation_threshold_score = self.get_score(authorities[reputation_threshold - 1]);
        // We will give a chance to authorities with scores that are slightly below the threshold
        // by reducing the threshold by the deviation percentage
        reputation_threshold_score
            - (self.threshold_deviation as i128) * reputation_threshold_score.abs() / 100
    }
}

impl fmt::Debug for NodeReputation {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "NodeReputation {{ scores: {:?} }}", self.scores)
    }
}
