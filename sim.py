"""Drizzle supply simulator.

Replays real hourly precipitation (rain_60d.json, Open-Meteo, 12 cities, 60 days)
through the $DRIZZLE mechanic and reports supply and per-strategy returns.

Usage: python3 sim.py [rain_60d.json]
"""
import json
import random
import sys

RAIN_MM = 0.2      # an hour counts as rainy at or above this
CAP_MM = 5.0       # storm cap: weight per city per hour is min(mm, CAP_MM)
PLAYERS = 1000
SEED = 100.0       # one-time faucet per account
REBALANCE_H = 24   # mobile strategies re-plant once a day


def load(path):
    d = json.load(open(path))
    return list(d["cities"].keys()), [list(x) for x in zip(*d["cities"].values())]


def run(cities, hours, emission, evap, mix, dances=0.0, rng_seed=7):
    """mix: {strategy: share of players}. Returns metrics dict."""
    rng = random.Random(rng_seed)
    n = len(cities)
    # stake[strategy][city]; wallet[strategy] = unplanted balance
    strategies = list(mix)
    count = {s: round(PLAYERS * mix[s]) for s in strategies}
    stake = {s: [0.0] * n for s in strategies}
    wallet = {s: 0.0 for s in strategies}
    for s in strategies:
        if s == "loyal":
            for _ in range(count[s]):
                stake[s][rng.randrange(n)] += SEED
        elif s == "idle":
            wallet[s] = count[s] * SEED
        else:
            wallet[s] = count[s] * SEED
    jackpot = [0.0] * n
    minted = burned = 0.0
    supply0 = PLAYERS * SEED
    supply_curve = []
    warm = {s: [0.0] * n for s in strategies}  # stake that is still warming up
    history = []

    for h, mm in enumerate(hours):
        # daily rebalance for mobile strategies (uses only the past: persistence forecast)
        if h % REBALANCE_H == 0:
            past = history[-168:] or [[0.0] * n]
            rainy = [sum(1 for row in past if row[c] >= RAIN_MM) for c in range(n)]
            total = [sum(stake[s][c] for s in strategies) for c in range(n)]
            for s in ("herd", "forecaster"):
                if s not in stake:
                    continue
                stake_prev = stake[s][:]
                bal = wallet[s] + sum(stake[s])
                stake[s] = [0.0] * n
                if s == "herd":
                    best = max(range(n), key=lambda c: rainy[c])
                    stake[s][best] = bal
                    warm[s] = stake[s][:]
                else:
                    # spread over the 3 cities with the best rain-per-stake ratio
                    # expected share of emission per staked coin: rainy hours / crowd size
                    own = [bal / 3] * n
                    ratio = sorted((c for c in range(n) if rainy[c] > 0),
                                   key=lambda c: -rainy[c] / (total[c] - stake_prev[c] + own[c]))[:3]
                    if not ratio:  # no rain history yet: sit out
                        wallet[s] = bal
                        continue
                    for c in ratio:
                        stake[s][c] = bal / len(ratio)
                    warm[s] = stake[s][:]
                wallet[s] = 0.0
        # dancers burn into Dubai's jackpot every day
        if dances and h % 24 == 0:
            d = dances
            jackpot[cities.index("Dubai")] += d
            burned += d
            minted -= 0  # burn already counted; jackpot mints back only if it rains

        weights = [min(x, CAP_MM) if x >= RAIN_MM else 0.0 for x in mm]
        wsum = sum(weights)
        for c in range(n):
            eligible = sum(stake[s][c] - warm[s][c] for s in strategies)
            if weights[c] > 0:
                pot = emission * weights[c] / wsum + jackpot[c]
                if eligible <= 0:
                    continue  # nobody planted: emission is simply not minted
                jackpot[c] = 0.0
                minted += pot
                for s in strategies:
                    share = (stake[s][c] - warm[s][c]) / eligible
                    stake[s][c] += pot * share  # mint auto-compounds into the plot
            else:
                for s in strategies:
                    loss = stake[s][c] * evap
                    stake[s][c] -= loss
                    burned += loss
        warm = {s: [0.0] * n for s in strategies}
        history.append(mm)
        if h % 24 == 23:
            supply_curve.append(supply0 + minted - burned)

    final = {s: (wallet[s] + sum(stake[s])) / max(count[s], 1) for s in strategies}
    return {
        "supply_end": supply0 + minted - burned,
        "minted": minted,
        "burned": burned,
        "per_player_end": final,
        "curve_weekly": [round(x) for x in supply_curve[6::7]],
        "curve_daily": supply_curve,
    }


def main():
    cities, hours = load(sys.argv[1] if len(sys.argv) > 1 else "rain_60d.json")
    any_rain = sum(1 for mm in hours if any(x >= RAIN_MM for x in mm))
    print(f"{len(hours)} hours, {len(cities)} cities, hours with rain somewhere: {any_rain}")
    scenarios = {
        "all loyal (random city, never move)": {"loyal": 1.0},
        "herd (everyone chases the wettest city)": {"herd": 0.8, "loyal": 0.2},
        "mixed (40 forecaster / 30 herd / 20 loyal / 10 idle)":
            {"forecaster": 0.4, "herd": 0.3, "loyal": 0.2, "idle": 0.1},
    }
    for emission in (50, 100, 200):
        for evap in (0.0005, 0.001, 0.0025):
            print(f"\n=== emission {emission}/h, evaporation {evap*100:.2f}%/dry h ===")
            for name, mix in scenarios.items():
                r = run(cities, hours, emission, evap, mix)
                pp = ", ".join(f"{k} {v:.0f}" for k, v in r["per_player_end"].items())
                print(f"  {name:52} supply {r['supply_end']:9.0f} "
                      f"(+{r['minted']:7.0f} / -{r['burned']:6.0f})  per player: {pp}")
    print("\nweekly supply, emission 100, evap 0.10%, mixed:")
    print(run(cities, hours, 100, 0.001, scenarios["mixed (40 forecaster / 30 herd / 20 loyal / 10 idle)"])["curve_weekly"])
    print("\nDubai rain-dance: 200/day burned into Dubai jackpot (it never rains there in this data):")
    r = run(cities, hours, 100, 0.001, {"forecaster": 0.4, "herd": 0.3, "loyal": 0.3}, dances=200)
    print(f"  supply {r['supply_end']:.0f}, burned {r['burned']:.0f} -> the dance is a pure sink until it rains")


if __name__ == "__main__":
    main()
