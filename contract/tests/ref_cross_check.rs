//! Replays the 48 hours baked into `fixtures/ref_48h.json` through the pure pool
//! API and asserts exact agreement with the Python reference (`tools/ref_settle.py`).

use drizzle::pool::{
    plant, rain_dance, settle_round, sprout_user, unplant_pending, uproot,
    Params, Pool,
};
use serde_json::Value;

const NC: usize = 12;
const DEC: u128 = 1_000_000_000_000_000_000; // 10^18
const E: u128 = 100 * DEC;
const CAPITAL: u128 = 100 * E;

struct User {
    balance: u128,
    pending: [u128; NC],
    pending_round: [u64; NC],
    shares: [u128; NC],
}

impl User {
    fn new() -> Self {
        Self {
            balance: CAPITAL,
            pending: [0; NC],
            pending_round: [0; NC],
            shares: [0; NC],
        }
    }
}

fn dec(v: &Value) -> u128 {
    v.as_str()
        .expect("fixture numbers are decimal strings")
        .parse()
        .expect("valid u128")
}

fn plant_user(user: &mut User, city: usize, amount: u128, pools: &mut [Pool], round: u64) {
    assert!(user.balance >= amount, "plant: insufficient balance");
    user.balance -= amount;
    if user.pending[city] == 0 {
        user.pending_round[city] = round;
    }
    user.pending[city] += amount;
    plant(&mut pools[city], amount);
}

fn dance(user: &mut User, city: usize, amount: u128, pools: &mut [Pool]) {
    assert!(user.balance >= amount, "dance: insufficient balance");
    user.balance -= amount;
    rain_dance(&mut pools[city], amount);
}

fn uproot_user(user: &mut User, city: usize, shares: u128, pools: &mut [Pool]) {
    let pending = user.pending[city];
    if pending > 0 {
        unplant_pending(&mut pools[city], pending);
        user.balance += pending;
        user.pending[city] = 0;
    }
    assert!(shares <= user.shares[city], "uproot: not enough user shares");
    let payout = uproot(&mut pools[city], shares);
    user.shares[city] -= shares;
    user.balance += payout;
}

#[test]
fn matches_python_reference_over_48_hours() {
    let fixture: Value =
        serde_json::from_str(include_str!("fixtures/ref_48h.json")).expect("fixture parses");

    let p = Params {
        emission: dec(&fixture["params"]["emission"]),
        evap_bps: fixture["params"]["evap_bps"].as_u64().unwrap() as u16,
        threshold_tenths: fixture["params"]["threshold_tenths"].as_u64().unwrap() as u16,
        cap_tenths: fixture["params"]["cap_tenths"].as_u64().unwrap() as u16,
    };
    assert_eq!(p.emission, E);
    assert_eq!(p.evap_bps, 10);
    assert_eq!(p.threshold_tenths, 2);
    assert_eq!(p.cap_tenths, 50);

    let mm_rows: Vec<Vec<u16>> = fixture["mm_tenths"]
        .as_array()
        .expect("mm_tenths")
        .iter()
        .map(|row| {
            row.as_array()
                .expect("row")
                .iter()
                .map(|v| v.as_u64().expect("tenths") as u16)
                .collect()
        })
        .collect();
    assert_eq!(mm_rows.len(), 48);
    assert!(mm_rows.iter().all(|row| row.len() == NC));

    let rounds = fixture["rounds"].as_array().expect("rounds");
    assert_eq!(rounds.len(), 48);

    let mut pools: Vec<Pool> = vec![Pool::default(); NC];
    let mut users: Vec<User> = (0..4).map(|_| User::new()).collect();

    for (idx, rec) in rounds.iter().enumerate() {
        let round = idx as u64;
        assert_eq!(rec["round"].as_u64().unwrap(), round);

        // Scripted actions; identical to tools/ref_settle.py::run_actions.
        match round {
            0 => {
                for (i, city) in [1usize, 3, 7, 11].into_iter().enumerate() {
                    plant_user(&mut users[i], city, 50 * E, &mut pools, round);
                }
            }
            10 => {
                dance(&mut users[0], 11, 20 * E, &mut pools);
                plant_user(&mut users[1], 1, 25 * E, &mut pools, round);
            }
            24 => {
                // The brief says "city 3", but user2 holds its shares in city 7
                // (the city it planted in); uproot half of that position.
                let shares = users[2].shares[7];
                assert!(shares > 0, "user2 holds shares in city 7 by round 24");
                uproot_user(&mut users[2], 7, shares / 2, &mut pools);
            }
            _ => {}
        }

        let outcomes = settle_round(&mut pools, &mm_rows[idx], round, &p);

        for c in 0..NC {
            assert_eq!(
                outcomes[c].minted,
                dec(&rec["minted"][c]),
                "round {round} city {c} minted"
            );
            assert_eq!(
                outcomes[c].burned,
                dec(&rec["burned"][c]),
                "round {round} city {c} burned"
            );
            let fp = &rec["pools"][c];
            assert_eq!(pools[c].value, dec(&fp["value"]), "round {round} city {c} value");
            assert_eq!(pools[c].shares, dec(&fp["shares"]), "round {round} city {c} shares");
            assert_eq!(
                pools[c].pending_total,
                dec(&fp["pending_total"]),
                "round {round} city {c} pending_total"
            );
            assert_eq!(
                pools[c].jackpot,
                dec(&fp["jackpot"]),
                "round {round} city {c} jackpot"
            );
        }

        // Lazy sprouting, exactly like the contract: convert pending using the
        // ratio published by the round it was planted for.
        for user in users.iter_mut() {
            for c in 0..NC {
                if user.pending[c] > 0 && user.pending_round[c] == round {
                    let (pin, sout) = outcomes[c]
                        .sprout
                        .expect("pending implies a sprout ratio");
                    user.shares[c] += sprout_user(user.pending[c], (pin, sout));
                    user.pending[c] = 0;
                }
            }
        }
    }

    // Final per-user state.
    let fusers = fixture["users"].as_array().expect("users");
    assert_eq!(fusers.len(), 4);
    for (i, user) in users.iter().enumerate() {
        let fu = &fusers[i];
        assert_eq!(user.balance, dec(&fu["balance"]), "user {i} balance");
        for c in 0..NC {
            assert_eq!(user.shares[c], dec(&fu["shares"][c]), "user {i} city {c} shares");
            assert_eq!(user.pending[c], dec(&fu["pending"][c]), "user {i} city {c} pending");
        }
        // Shares remain a claim on pool value; nothing dormant is left pending.
        assert_eq!(user.pending.iter().sum::<u128>(), 0);
    }
}
