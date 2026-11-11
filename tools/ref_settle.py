#!/usr/bin/env python3
"""Drizzle settlement reference implementation (stdlib only).

A literal integer port of `contract/src/pool.rs::settle_round` plus the
ledger-side actions of the 48-hour replay. Running it regenerates
`contract/tests/fixtures/ref_48h.json`, which `contract/tests/ref_cross_check.rs`
replays through the Rust API and compares field by field.

Usage:
    python3 tools/ref_settle.py [rain_60d.json]

All divisions floor. Rain values are exact decimal tenths of a millimetre.
"""
import json
import sys
from decimal import Decimal
from pathlib import Path

# Contract city order (index 0..11).
CITY_ORDER = [
    "Bergen", "Sao Paulo", "Lagos", "Singapore", "Tokyo", "Jakarta",
    "Mumbai", "London", "Lisbon", "Seattle", "Cairo", "Dubai",
]
# The source JSON serialises cities in a different order; we reorder by name.

NC = 12
E = 100 * 10 ** 18          # emission per round
EVAP_BPS = 10
THRESHOLD_TENTHS = 2        # 0.2 mm
CAP_TENTHS = 50             # 5.0 mm
BPS_DENOM = 10_000
DEAD_SHARES = 1_000_000_000_000  # unowned shares minted on every 1:1 (re)start
START = 288                 # time[288] == 2026-09-24T00:00
ROUNDS = 48
U128_MAX = 2 ** 128 - 1

REPO_ROOT = Path(__file__).resolve().parent.parent
FIXTURE_PATH = REPO_ROOT / "contract" / "tests" / "fixtures" / "ref_48h.json"

# Each user is seeded with this much unplanted balance (faucet capital).
CAPITAL = 100 * E


def mul_div(a, b, c):
    assert c != 0, "mul_div: division by zero"
    result = (a * b) // c
    assert result <= U128_MAX, "mul_div: result overflows u128"
    return result


def weight(mm):
    return 0 if mm < THRESHOLD_TENTHS else min(mm, CAP_TENTHS)


def new_pool():
    return {
        "value": 0, "shares": 0, "pending_total": 0, "jackpot": 0,
        "last_rain_round": None, "last_mm_tenths": 0,
    }


def settle_round(pools, mm_row, round_no):
    """Settle all 12 cities for one round; returns per-city outcomes."""
    weights = [weight(mm) for mm in mm_row]
    total_weight = sum(weights)
    outcomes = []
    for c in range(NC):
        pool = pools[c]
        mm = mm_row[c]
        w = weights[c]
        outcome = {"minted": 0, "burned": 0, "sprout": None, "reset": False}
        pool["last_mm_tenths"] = mm

        if w > 0:
            pool["last_rain_round"] = round_no
            if pool["shares"] > 0:
                pot = mul_div(E, w, total_weight) + pool["jackpot"]
                pool["value"] += pot
                pool["jackpot"] = 0
                outcome["minted"] = pot
            # shares == 0: mint nothing, jackpot stays.
        else:
            burned = mul_div(pool["value"], EVAP_BPS, BPS_DENOM)
            pool["value"] -= burned
            outcome["burned"] = burned

        if pool["pending_total"] > 0:
            pending_in = pool["pending_total"]
            reset = (
                pool["shares"] == 0
                or pool["value"] == 0
                or pool["value"] * 1_000_000 < pool["shares"]
            )
            if reset:
                # Empty or worthless pool (price below 1e-6): old shares are dead,
                # so burn the remaining dust and sprout 1:1 plus DEAD_SHARES.
                outcome["burned"] += pool["value"]
                pool["value"] = 0
                pool["shares"] = DEAD_SHARES
                shares_out = pending_in
            else:
                # Not resetting implies value * 1e6 >= shares.
                shares_out = mul_div(pending_in, pool["shares"], pool["value"])
            pool["value"] += pending_in
            pool["shares"] += shares_out
            pool["pending_total"] = 0
            outcome["sprout"] = (pending_in, shares_out)
            outcome["reset"] = reset

        outcomes.append(outcome)
    return outcomes


class User:
    def __init__(self):
        self.balance = CAPITAL
        self.pending = [0] * NC
        self.pending_round = [0] * NC
        self.shares = [0] * NC


def plant(user, city, amount, pools, round_no):
    assert user.balance >= amount, "plant: insufficient balance"
    user.balance -= amount
    if user.pending[city] == 0:
        user.pending_round[city] = round_no
    user.pending[city] += amount
    pools[city]["pending_total"] += amount


def uproot(user, city, shares, pools):
    """Settle unsprouted pending 1:1, then burn `shares` and credit the payout."""
    pool = pools[city]
    pending = user.pending[city]
    if pending:
        assert pool["pending_total"] >= pending
        pool["pending_total"] -= pending
        user.balance += pending
        user.pending[city] = 0
    assert shares <= user.shares[city], "uproot: not enough user shares"
    assert shares <= pool["shares"]
    payout = shares * pool["value"] // pool["shares"] if pool["shares"] else 0
    pool["value"] -= payout
    pool["shares"] -= shares
    user.shares[city] -= shares
    user.balance += payout
    return payout


def rain_dance(user, city, amount, pools):
    assert user.balance >= amount, "rain_dance: insufficient balance"
    user.balance -= amount
    pools[city]["jackpot"] += amount


def sprout_user(pending, ratio):
    pending_in, shares_out = ratio
    return mul_div(pending, shares_out, pending_in)


def sprout_users(users, outcomes, round_no):
    for user in users:
        for c in range(NC):
            if user.pending[c] > 0 and user.pending_round[c] == round_no:
                ratio = outcomes[c]["sprout"]
                assert ratio is not None, "pending implies a sprout ratio"
                user.shares[c] += sprout_user(user.pending[c], ratio)
                user.pending[c] = 0


def run_actions(round_no, users, pools, danced):
    """Scripted actions, identical in contract/tests/ref_cross_check.rs."""
    if round_no == 0:
        for i, city in enumerate([1, 3, 7, 11]):
            plant(users[i], city, 50 * E, pools, round_no)
    elif round_no == 10:
        rain_dance(users[0], 11, 20 * E, pools)
        danced[0] += 20 * E
        plant(users[1], 1, 25 * E, pools, round_no)
    elif round_no == 24:
        # NOTE: the brief says "user2 uproots half its shares in city 3", but
        # user2 holds its shares in city 7 (the city it planted in). City 3 is
        # held by user1 and the action targets "its shares", so we uproot half
        # of user2's city-7 position to keep the action meaningful.
        shares = users[2].shares[7]
        assert shares > 0, "user2 should hold shares in city 7 by round 24"
        uproot(users[2], 7, shares // 2, pools)


def load_rain(path):
    with open(path, encoding="utf-8") as f:
        data = json.load(f)
    times = data["time"]
    assert len(times) == 1440, len(times)
    assert times[START] == "2026-09-24T00:00", times[START]

    series = data["cities"]
    missing = [name for name in CITY_ORDER if name not in series]
    assert not missing, f"missing cities in rain data: {missing}"

    rows = []
    for h in range(START, START + ROUNDS):
        row = []
        for name in CITY_ORDER:
            mm = series[name][h]
            dec = Decimal(str(mm))
            assert dec.as_tuple().exponent >= -1, f"{name} h{h}: {mm} has >1 decimal"
            tenths = int(dec * 10)
            assert Decimal(tenths) == dec * 10, f"{name} h{h}: {mm} not exact"
            assert 0 <= tenths <= 0xFFFF
            row.append(tenths)
        rows.append(row)
    return rows


def main(argv):
    if len(argv) > 1:
        arg = Path(argv[1])
        path = arg if arg.exists() else REPO_ROOT / arg
    else:
        path = REPO_ROOT / "rain_60d.json"

    mm_rows = load_rain(path)

    pools = [new_pool() for _ in range(NC)]
    users = [User() for _ in range(4)]
    danced = [0]
    supply = CAPITAL * len(users)

    rounds = []
    for round_no in range(ROUNDS):
        danced_before = danced[0]
        run_actions(round_no, users, pools, danced)
        supply -= danced[0] - danced_before  # rain dance burns tokens

        outcomes = settle_round(pools, mm_rows[round_no], round_no)
        minted = [o["minted"] for o in outcomes]
        burned = [o["burned"] for o in outcomes]
        supply += sum(minted) - sum(burned)
        sprout_users(users, outcomes, round_no)

        held = sum(u.balance for u in users) + sum(
            p["value"] + p["pending_total"] for p in pools
        )
        assert held == supply, f"supply invariant broken at round {round_no}"

        rounds.append({
            "round": round_no,
            "pools": [
                {
                    "value": str(p["value"]),
                    "shares": str(p["shares"]),
                    "pending_total": str(p["pending_total"]),
                    "jackpot": str(p["jackpot"]),
                }
                for p in pools
            ],
            "minted": [str(x) for x in minted],
            "burned": [str(x) for x in burned],
        })

    fixture = {
        "params": {
            "emission": str(E),
            "evap_bps": EVAP_BPS,
            "threshold_tenths": THRESHOLD_TENTHS,
            "cap_tenths": CAP_TENTHS,
        },
        "cities": CITY_ORDER,
        "mm_tenths": mm_rows,
        "rounds": rounds,
        "users": [
            {
                "balance": str(u.balance),
                "shares": [str(s) for s in u.shares],
                "pending": [str(x) for x in u.pending],
            }
            for u in users
        ],
    }

    FIXTURE_PATH.parent.mkdir(parents=True, exist_ok=True)
    with open(FIXTURE_PATH, "w", encoding="utf-8") as f:
        json.dump(fixture, f, indent=1)
        f.write("\n")

    total_minted = sum(int(x) for r in rounds for x in r["minted"])
    total_burned = sum(int(x) for r in rounds for x in r["burned"])
    print(f"wrote {FIXTURE_PATH}")
    print(
        f"rounds={ROUNDS} supply={supply / 1e18:.6f} "
        f"minted={total_minted / 1e18:.6f} burned={total_burned / 1e18:.6f} "
        f"danced={sum(danced) / 1e18:.6f}"
    )


if __name__ == "__main__":
    main(sys.argv)
