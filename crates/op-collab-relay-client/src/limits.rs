use std::time::Duration;

pub const DEFAULT_OWNER_LANE_COUNT: usize = 4;
pub const MAX_OWNER_LANE_COUNT: usize = 8;
pub const MAX_RELAY_BINARY_BYTES: usize = 64 * 1024;
pub const MAX_RELAY_CONNECTION_BYTES: u64 = 1024 * 1024 * 1024;

#[derive(Clone, Copy)]
pub(crate) struct RelayLimits {
    pub connect: Duration,
    pub hello: Duration,
    pub pair: Duration,
    /// How long ONE owner lane may sit waiting for a guest before the client
    /// retires it and dials a fresh one.
    ///
    /// The relay hard-closes an unpaired peer after its own waiting window
    /// (`RelayConfig::waiting_timeout`, 60 s and not operator-configurable),
    /// so an owner lane parked on the long [`RelayLimits::pair`] budget never
    /// recycles on its own schedule: it either learns about the close late or,
    /// behind a NAT that silently reaps the idle flow, not at all. Either way
    /// the owner is absent from the relay's waiting queue while it re-dials,
    /// and a guest that registers in that window has no counterpart to pair
    /// with. Staying comfortably under the server window keeps the recycle
    /// client-driven, bounded, and observable.
    pub owner_pair: Duration,
    pub idle: Duration,
    pub lifetime: Duration,
    pub retry: Duration,
    pub stop: Duration,
    pub max_binary_bytes: usize,
    pub max_connection_bytes: u64,
}

impl Default for RelayLimits {
    fn default() -> Self {
        Self {
            connect: Duration::from_secs(10),
            hello: Duration::from_secs(10),
            pair: Duration::from_secs(5 * 60),
            owner_pair: Duration::from_secs(45),
            idle: Duration::from_secs(2 * 60),
            lifetime: Duration::from_secs(24 * 60 * 60),
            retry: Duration::from_secs(1),
            stop: Duration::from_secs(5),
            max_binary_bytes: MAX_RELAY_BINARY_BYTES,
            max_connection_bytes: MAX_RELAY_CONNECTION_BYTES,
        }
    }
}

impl std::fmt::Debug for RelayLimits {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RelayLimits")
            .field("connect", &self.connect)
            .field("hello", &self.hello)
            .field("pair", &self.pair)
            .field("owner_pair", &self.owner_pair)
            .field("idle", &self.idle)
            .field("lifetime", &self.lifetime)
            .field("retry", &self.retry)
            .field("stop", &self.stop)
            .field("max_binary_bytes", &self.max_binary_bytes)
            .field("max_connection_bytes", &self.max_connection_bytes)
            .finish()
    }
}

/// Smallest first-connection waiting budget an owner lane may be given.
///
/// Only reachable with a deliberately tiny `owner_pair` (tests); it keeps a
/// degenerate schedule from turning lane recycling into a reconnect spin.
const MIN_OWNER_LANE_PAIR_BUDGET: Duration = Duration::from_millis(50);

/// First-connection waiting budget for owner lane `index` of `lane_count`.
///
/// Every lane is dialled at session start, so one shared budget expires them
/// all in the same instant and empties the relay's waiting queue for the whole
/// re-dial — precisely the window in which a joining guest finds no owner to
/// pair with. Giving lane `index` a proportional slice of the first window
/// staggers the recycles permanently: after its first cycle each lane runs the
/// full [`RelayLimits::owner_pair`] budget, offset from its neighbours by one
/// slice, so at most one lane is ever re-dialling.
pub(crate) fn owner_lane_first_pair_budget(
    owner_pair: Duration,
    index: usize,
    lane_count: usize,
) -> Duration {
    let lanes = u32::try_from(lane_count.max(1)).unwrap_or(u32::MAX);
    let slot = u32::try_from(index.min(lane_count.saturating_sub(1)).saturating_add(1))
        .unwrap_or(u32::MAX)
        .min(lanes);
    let budget = (owner_pair / lanes).saturating_mul(slot).min(owner_pair);
    budget.max(MIN_OWNER_LANE_PAIR_BUDGET.min(owner_pair))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_owner_pair_budget_stays_under_the_relay_waiting_window() {
        // `op_collab_relay_server::RelayConfig::waiting_timeout` is 60 s and is
        // not operator-configurable, so the client must always recycle first.
        assert!(RelayLimits::default().owner_pair < Duration::from_secs(60));
    }

    #[test]
    fn first_pair_budgets_are_staggered_across_the_default_lane_pool() {
        let owner_pair = RelayLimits::default().owner_pair;
        let budgets: Vec<Duration> = (0..DEFAULT_OWNER_LANE_COUNT)
            .map(|index| owner_lane_first_pair_budget(owner_pair, index, DEFAULT_OWNER_LANE_COUNT))
            .collect();
        assert_eq!(
            budgets,
            vec![
                Duration::from_millis(11_250),
                Duration::from_millis(22_500),
                Duration::from_millis(33_750),
                Duration::from_millis(45_000),
            ]
        );
        // Strictly increasing: no two lanes ever recycle in the same instant.
        assert!(budgets.windows(2).all(|pair| pair[0] < pair[1]));
        assert!(budgets.iter().all(|budget| *budget <= owner_pair));
    }

    #[test]
    fn a_single_lane_pool_keeps_the_whole_budget() {
        let owner_pair = Duration::from_secs(45);
        assert_eq!(owner_lane_first_pair_budget(owner_pair, 0, 1), owner_pair);
    }

    #[test]
    fn a_degenerate_schedule_never_produces_a_zero_budget() {
        let tiny = Duration::from_millis(4);
        for index in 0..8 {
            assert_eq!(owner_lane_first_pair_budget(tiny, index, 8), tiny);
        }
        assert_eq!(
            owner_lane_first_pair_budget(Duration::ZERO, 0, 4),
            Duration::ZERO
        );
    }

    #[test]
    fn an_out_of_range_index_is_clamped_to_the_last_lane() {
        let owner_pair = Duration::from_secs(45);
        assert_eq!(
            owner_lane_first_pair_budget(owner_pair, 99, 4),
            owner_lane_first_pair_budget(owner_pair, 3, 4)
        );
    }
}
