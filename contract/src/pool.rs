//! Pure integer pool math for $DRIZZLE.
//!
//! No `near-sdk` dependency: this module is plain Rust so it can be unit
//! tested and cross-checked against `tools/ref_settle.py`. All arithmetic is
//! integer and every division floors, matching the Python reference exactly.

uint::construct_uint! {
    /// 256-bit unsigned integer, used only for wide intermediates in [`mul_div`].
    pub struct U256(4);
}

/// Basis-point denominator.
pub const BPS_DENOM: u128 = 10_000;

/// Shares minted to nobody whenever a pool (re)starts 1:1.
///
/// They keep `pool.shares` from shrinking to dust, so a sole holder cannot inflate
/// the share price by partial uproots and lock the city to new planters. Their
/// value share is unowned: the last holder sweeps it, a reset burns it.
pub const DEAD_SHARES: u128 = 1_000_000_000_000;

/// Round parameters, mirrored from the contract config.
#[derive(Clone, Copy, Debug)]
pub struct Params {
    /// Tokens minted per round, split across rainy cities by weight (18 decimals).
    pub emission: u128,
    /// Evaporation in basis points per dry round.
    pub evap_bps: u16,
    /// Rain below this many tenths of a millimetre does not count.
    pub threshold_tenths: u16,
    /// Weight caps at this many tenths of a millimetre (storm cap).
    pub cap_tenths: u16,
}

/// One city's pool.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Pool {
    pub value: u128,
    pub shares: u128,
    pub pending_total: u128,
    pub jackpot: u128,
    pub last_rain_round: Option<u64>,
    pub last_mm_tenths: u16,
}

/// What settling one city in one round did.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct CityOutcome {
    pub minted: u128,
    pub burned: u128,
    /// `(pending_in, shares_out)` for this round, if anything was pending.
    pub sprout: Option<(u128, u128)>,
    /// The pool was worthless (price below 1e-6, or empty): old shares were
    /// discarded and the remaining dust was burned.
    pub reset: bool,
}

/// `floor(a * b / c)` via a 256-bit intermediate.
///
/// Panics if `c == 0` or the result does not fit in a `u128`.
pub fn mul_div(a: u128, b: u128, c: u128) -> u128 {
    assert!(c != 0, "mul_div: division by zero");
    let prod = U256::from(a) * U256::from(b);
    let res = prod / U256::from(c);
    assert!(res <= U256::from(u128::MAX), "mul_div: result overflows u128");
    res.as_u128()
}

/// Rain weight for a city: `min(mm, cap)` when `mm >= threshold`, else 0.
pub fn weight(mm_tenths: u16, p: &Params) -> u128 {
    if mm_tenths < p.threshold_tenths {
        0
    } else {
        mm_tenths.min(p.cap_tenths) as u128
    }
}

/// Settles one round for all cities.
///
/// Panics if `pools.len() != mm_tenths.len()`.
pub fn settle_round(
    pools: &mut [Pool],
    mm_tenths: &[u16],
    round: u64,
    p: &Params,
) -> Vec<CityOutcome> {
    assert_eq!(
        pools.len(),
        mm_tenths.len(),
        "settle_round: pools/mm length mismatch"
    );

    // 1. Weights and their sum.
    let mut weights = Vec::with_capacity(mm_tenths.len());
    let mut total_weight: u128 = 0;
    for &mm in mm_tenths {
        let w = weight(mm, p);
        total_weight += w;
        weights.push(w);
    }

    let mut outcomes = Vec::with_capacity(pools.len());

    // 2. Settle each city in index order.
    for (pool, (&mm, &w)) in pools.iter_mut().zip(mm_tenths.iter().zip(weights.iter())) {
        let mut outcome = CityOutcome::default();
        pool.last_mm_tenths = mm;

        if w > 0 {
            pool.last_rain_round = Some(round);
            if pool.shares > 0 {
                let pot = mul_div(p.emission, w, total_weight) + pool.jackpot;
                pool.value += pot;
                pool.jackpot = 0;
                outcome.minted = pot;
            }
            // shares == 0: mint nothing, leave the jackpot untouched.
        } else {
            let burned = mul_div(pool.value, p.evap_bps as u128, BPS_DENOM);
            pool.value -= burned;
            outcome.burned = burned;
        }

        // Sprout pending at the post-settlement price.
        if pool.pending_total > 0 {
            let pending_in = pool.pending_total;
            let reset = pool.shares == 0
                || pool.value == 0
                || pool.value.saturating_mul(1_000_000) < pool.shares;
            let shares_out = if reset {
                // Empty or worthless pool (price below 1e-6): old shares are dead,
                // so burn the remaining dust and start 1:1 (plus DEAD_SHARES, which
                // keep a sole holder from inflating the price). The contract raises
                // the city epoch so holders of the old shares can no longer uproot.
                outcome.burned += pool.value;
                pool.value = 0;
                pool.shares = DEAD_SHARES;
                pending_in
            } else {
                // Not resetting means `pool.value * 1e6 >= pool.shares`, i.e.
                // `pool.shares / pool.value <= 1e6`. Therefore
                // `shares_out = floor(pending_in * shares / value) <= pending_in * 1e6`.
                // `pending_in` is at most the token supply, so the product stays far
                // below `u128::MAX` (the supply cannot reach 3.4e32 base units); this
                // call cannot panic the settlement callback.
                mul_div(pending_in, pool.shares, pool.value)
            };
            pool.value += pending_in;
            pool.shares += shares_out;
            pool.pending_total = 0;
            outcome.sprout = Some((pending_in, shares_out));
            outcome.reset = reset;
        }

        outcomes.push(outcome);
    }

    outcomes
}

/// Moves value into a city's pending pool (the user side tracks who owns it).
pub fn plant(pool: &mut Pool, amount: u128) {
    pool.pending_total += amount;
}

/// Removes value from a city's pending pool (uprooting unsprouted pending).
///
/// Panics on underflow.
pub fn unplant_pending(pool: &mut Pool, amount: u128) {
    pool.pending_total = pool
        .pending_total
        .checked_sub(amount)
        .expect("unplant_pending: pending_total underflow");
}

/// Adds to the city's jackpot (the caller burns the tokens from the user).
pub fn rain_dance(pool: &mut Pool, amount: u128) {
    pool.jackpot += amount;
}

/// Shares a user receives for `pending`, given the sprout ratio of their round.
pub fn sprout_user(pending: u128, ratio: (u128, u128)) -> u128 {
    mul_div(pending, ratio.1, ratio.0)
}

/// Value of `shares`: `floor(shares * value / pool.shares)`, or 0 for an empty pool.
pub fn shares_value(pool: &Pool, shares: u128) -> u128 {
    if pool.shares == 0 {
        0
    } else {
        mul_div(shares, pool.value, pool.shares)
    }
}

/// Burns `shares` and returns their value.
///
/// Panics if `shares > pool.shares`.
pub fn uproot(pool: &mut Pool, shares: u128) -> u128 {
    assert!(shares <= pool.shares, "uproot: more shares than the pool holds");
    let payout = shares_value(pool, shares);
    pool.value -= payout;
    pool.shares -= shares;
    payout
}

#[cfg(test)]
mod tests {
    use super::*;

    const NC: usize = 12;
    const DEC: u128 = 1_000_000_000_000_000_000; // 10^18
    const SEED: u128 = 1_000 * DEC;
    const USERS: usize = 6;

    fn params() -> Params {
        Params {
            emission: 100 * DEC,
            evap_bps: 10,
            threshold_tenths: 2,
            cap_tenths: 50,
        }
    }

    /// Deterministic LCG so the randomized scenario never needs `rand`.
    struct Lcg(u64);

    impl Lcg {
        fn next_u64(&mut self) -> u64 {
            self.0 = self
                .0
                .wrapping_mul(6_364_136_223_846_793_005)
                .wrapping_add(1_442_695_040_888_963_407);
            self.0
        }

        fn below(&mut self, n: u64) -> u64 {
            (self.next_u64() >> 33) % n
        }

        fn u128_below(&mut self, n: u128) -> u128 {
            (self.next_u64() as u128) % n
        }
    }

    #[derive(Clone)]
    struct User {
        balance: u128,
        pending: [u128; NC],
        pending_round: [u64; NC],
        shares: [u128; NC],
    }

    impl User {
        fn new() -> Self {
            Self {
                balance: SEED,
                pending: [0; NC],
                pending_round: [0; NC],
                shares: [0; NC],
            }
        }
    }

    /// Lazily converts each user's pending into shares using the outcome of the
    /// round it was planted for.
    fn sprout_users(users: &mut [User], outcomes: &[CityOutcome], round: u64) {
        for u in users.iter_mut() {
            for c in 0..NC {
                if u.pending[c] > 0 && u.pending_round[c] == round {
                    let (pin, sout) = outcomes[c].sprout.expect("pending implies a sprout ratio");
                    u.shares[c] += sprout_user(u.pending[c], (pin, sout));
                    u.pending[c] = 0;
                }
            }
        }
    }

    fn check_supply(pools: &[Pool], users: &[User], supply: u128) {
        let held = users.iter().map(|u| u.balance).sum::<u128>()
            + pools
                .iter()
                .map(|p| p.value + p.pending_total)
                .sum::<u128>();
        assert_eq!(held, supply, "supply invariant broken");
    }

    /// Invariant 1: supply is always balances + pool value + pending, over a long
    /// deterministic scenario that exercises every entry point.
    #[test]
    fn supply_invariant_holds_through_random_scenario() {
        let p = params();
        let mut rng = Lcg(0xD1_22_1E);
        let mut pools: Vec<Pool> = vec![Pool::default(); NC];
        let mut users: Vec<User> = (0..USERS).map(|_| User::new()).collect();
        let mut supply: u128 = SEED * USERS as u128;

        for round in 0..240u64 {
            for i in 0..USERS {
                let action = rng.below(12);
                let city = rng.below(NC as u64) as usize;

                if action < 4 {
                    // plant part of the balance
                    if users[i].balance > 0 {
                        let amount =
                            (1 + rng.u128_below(100 * DEC)).min(users[i].balance);
                        users[i].balance -= amount;
                        if users[i].pending[city] == 0 {
                            users[i].pending_round[city] = round;
                        }
                        users[i].pending[city] += amount;
                        plant(&mut pools[city], amount);
                    }
                } else if action < 6 {
                    // uproot: first unsprouted pending 1:1, then some shares
                    let pending = users[i].pending[city];
                    if pending > 0 {
                        unplant_pending(&mut pools[city], pending);
                        users[i].balance += pending;
                        users[i].pending[city] = 0;
                    }
                    let held = users[i].shares[city];
                    if held > 0 {
                        let part = if rng.below(3) == 0 {
                            held
                        } else {
                            (held / 2).max(1)
                        };
                        let payout = uproot(&mut pools[city], part);
                        users[i].shares[city] -= part;
                        users[i].balance += payout;
                    }
                } else if action == 6 {
                    // rain dance burns from the balance into the (uncounted) jackpot
                    if users[i].balance > 0 {
                        let amount = (1 + rng.u128_below(10 * DEC)).min(users[i].balance);
                        users[i].balance -= amount;
                        rain_dance(&mut pools[city], amount);
                        supply -= amount;
                    }
                }

                check_supply(&pools, &users, supply);
            }

            // settle with deterministic pseudo-rain (tenths of a mm)
            let mm: Vec<u16> = (0..NC).map(|_| rng.below(80) as u16).collect();
            let outcomes = settle_round(&mut pools, &mm, round, &p);
            let minted = outcomes.iter().map(|o| o.minted).sum::<u128>();
            let burned = outcomes.iter().map(|o| o.burned).sum::<u128>();
            supply = supply + minted - burned;
            sprout_users(&mut users, &outcomes, round);
            check_supply(&pools, &users, supply);
        }

        // Sanity: the scenario actually did something.
        assert!(pools.iter().any(|pool| pool.value > 0));
        assert!(users.iter().any(|u| u.shares.iter().any(|&s| s > 0)));
    }

    /// Invariant 2: a planter earns nothing from the round they plant in.
    #[test]
    fn planter_earns_nothing_from_their_own_round() {
        let p = params();
        let mut pools = [Pool::default()];
        // An existing holder establishes the pool and its share price.
        plant(&mut pools[0], 500 * DEC);
        settle_round(&mut pools[0..1], &[30], 0, &p);

        let amount = 50 * DEC;
        let before_shares = pools[0].shares;
        plant(&mut pools[0], amount);
        let outcome = settle_round(&mut pools[0..1], &[30], 1, &p);
        let (pin, sout) = outcome[0].sprout.expect("pending sprouted");
        assert_eq!(pin, amount);
        assert!(pools[0].shares > before_shares);

        let shares = sprout_user(amount, (pin, sout));
        let value = shares_value(&pools[0], shares);
        assert!(value <= amount, "planter earned money from their own round");
        assert!(value > 0);

        // Had they been in the pool for this round they would have earned the pot.
        assert!(pools[0].value > amount);
    }

    /// Invariant 3: rain on a shareless city mints nothing and keeps the jackpot.
    #[test]
    fn rain_without_shares_keeps_the_jackpot() {
        let p = params();
        let mut pools = [Pool::default()];
        rain_dance(&mut pools[0], 20 * DEC);

        let outcome = settle_round(&mut pools[0..1], &[30], 7, &p);

        assert_eq!(outcome[0].minted, 0);
        assert_eq!(outcome[0].burned, 0);
        assert_eq!(pools[0].jackpot, 20 * DEC);
        assert_eq!(pools[0].value, 0);
        assert_eq!(pools[0].last_rain_round, Some(7));
        assert_eq!(pools[0].last_mm_tenths, 30);
    }

    /// Invariant 4: a dry round evaporates exactly `floor(value * evap_bps / 10000)`.
    #[test]
    fn dry_round_evaporates_exactly() {
        let p = params();
        let mut pools = [Pool::default()];
        plant(&mut pools[0], 1_000 * DEC);
        settle_round(&mut pools[0..1], &[30], 0, &p);

        let before = pools[0].value;
        assert!(before > 0);
        let expected = before * 10 / 10_000;
        let outcome = settle_round(&mut pools[0..1], &[0], 1, &p);

        assert_eq!(outcome[0].burned, expected);
        assert_eq!(outcome[0].minted, 0);
        assert_eq!(pools[0].value, before - expected);

        // Below-threshold rain counts as dry.
        let before = pools[0].value;
        let outcome = settle_round(&mut pools[0..1], &[1], 2, &p);
        assert_eq!(outcome[0].burned, before * 10 / 10_000);
        assert_eq!(pools[0].last_mm_tenths, 1);
    }

    #[test]
    fn mul_div_is_floor_and_handles_huge_intermediates() {
        assert_eq!(mul_div(7, 3, 2), 10);
        assert_eq!(mul_div(0, u128::MAX, 1), 0);
        assert_eq!(mul_div(10u128.pow(30), 10u128.pow(18), 10u128.pow(30)), DEC);
        assert_eq!(mul_div(u128::MAX, u128::MAX, u128::MAX), u128::MAX);
        assert_eq!(
            mul_div(u128::MAX, 10u128.pow(18), u128::MAX),
            10u128.pow(18)
        );
        // 1e30 * 1e30 = 1e60 needs 256 bits; the quotient still fits in u128.
        assert_eq!(
            mul_div(10u128.pow(30), 10u128.pow(30), 10u128.pow(30)),
            10u128.pow(30)
        );
    }

    #[test]
    #[should_panic(expected = "division by zero")]
    fn mul_div_panics_on_zero_denominator() {
        mul_div(1, 1, 0);
    }

    #[test]
    #[should_panic(expected = "result overflows u128")]
    fn mul_div_panics_when_quotient_does_not_fit() {
        mul_div(u128::MAX, 2, 1);
    }

    #[test]
    fn weight_edges() {
        let p = Params {
            emission: 0,
            evap_bps: 0,
            threshold_tenths: 2,
            cap_tenths: 50,
        };
        assert_eq!(weight(0, &p), 0);
        assert_eq!(weight(1, &p), 0);
        assert_eq!(weight(2, &p), 2);
        assert_eq!(weight(50, &p), 50);
        assert_eq!(weight(51, &p), 50);
        assert_eq!(weight(u16::MAX, &p), 50);
    }

    #[test]
    fn sole_holder_uproot_returns_full_value() {
        let mut pool = Pool {
            value: 12_345,
            shares: 1_000,
            ..Default::default()
        };
        assert_eq!(uproot(&mut pool, 1_000), 12_345);
        assert_eq!(pool.value, 0);
        assert_eq!(pool.shares, 0);
    }

    #[test]
    fn sprout_into_empty_pool_is_one_to_one() {
        let mut pool = Pool::default();
        plant(&mut pool, 777);
        let outcome = settle_round(std::slice::from_mut(&mut pool), &[2], 0, &params());
        assert_eq!(outcome[0].sprout, Some((777, 777)));
        assert_eq!(pool.shares, DEAD_SHARES + 777);
        assert_eq!(pool.value, 777);
    }

    #[test]
    fn worthless_pool_resets_old_shares_on_sprout() {
        // shares > 0 but value == 0: old shares are worthless; new pending is 1:1.
        let mut pool = Pool {
            shares: 500,
            ..Default::default()
        };
        plant(&mut pool, 100);
        let outcome = settle_round(std::slice::from_mut(&mut pool), &[0], 0, &params());
        assert_eq!(outcome[0].sprout, Some((100, 100)));
        assert_eq!(pool.shares, DEAD_SHARES + 100);
        assert_eq!(pool.value, 100);
    }

    #[test]
    #[should_panic(expected = "length mismatch")]
    fn settle_round_length_mismatch_panics() {
        let mut pools = [Pool::default(), Pool::default()];
        settle_round(&mut pools, &[0], 0, &params());
    }
    /// A pool whose price falls below 1e-6 is reset: dust is burned, old shares
    /// are discarded and the new pending sprouts 1:1.
    #[test]
    fn worthless_pool_resets_and_burns_its_dust() {
        let p = params();
        let mut pools = vec![Pool::default(); NC];
        pools[0].value = 1_000_000_000_001; // 1e12 + 1
        pools[0].shares = DEC;
        let mm = [0u16; NC];

        let out = settle_round(&mut pools, &mm, 0, &p);
        assert!(!out[0].reset, "price is just above the threshold");
        assert!(pools[0].value.saturating_mul(1_000_000) < pools[0].shares);

        let value_before = pools[0].value;
        pools[0].pending_total = DEC;
        let out = settle_round(&mut pools, &mm, 1, &p);
        assert!(out[0].reset);
        // The evaporation burn plus the reset burn add up to the whole value.
        assert_eq!(out[0].burned, value_before);
        assert_eq!(pools[0].value, DEC);
        assert_eq!(pools[0].shares, DEAD_SHARES + DEC);
        assert_eq!(out[0].sprout, Some((DEC, DEC)));
    }

}
