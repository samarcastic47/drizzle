# OutLayer integration research — Drizzle ($DRIZZLE) oracle

All facts below were fetched live on 2026-11-11. Sources are cited inline as `URL §section` or `repo-path:line`. Items I could not observe directly are marked **[INFERENCE]**.

---

## Summary

* **The deployed `outlayer.testnet` contract does *not* have the argument shape most of the docs show.** The live ABI (read from chain via NearBlocks' schema endpoint) exposes exactly seven positional JSON args: `source, resource_limits, input_data, secrets_ref, response_format, payer_account_id, params`. There is **no** `code_source`, no `WasmHash` variant, and no flat `{"repo","commit"}` object. `ExecutionSource` is `{"GitHub":{repo,commit,build_target}} | {"WasmUrl":{url,hash,build_target}} | {"Project":{project_id,version_key}}`.
* **The callback receives the guest's stdout directly — not an `OutLayerResponse { success, result, error }` wrapper.** On any failure the contract returns the JSON literal `null`. `Result<Option<T>, PromiseError>` is the right callback type; `Err(PromiseError)` means the OutLayer call itself failed (or was never resumed → the callback never fires at all).
* **`resource_limits: null` means compile-only** — a request with no limits runs nothing. Always pass it, even `{}`.
* **The request id is never delivered to the calling contract.** It exists in the `execution_requested` event payload (chain logs) and in `get_request(request_id)`; a contract cannot read it, so RoundLog must be filled off-chain by the keeper from the tx logs.
* Attestation pages live at `https://app.outlayer.ai/attestation/{job_id}` (verified live); discovery is via the `/executions` dashboard list.
* Deposit is `estimate_execution_cost(resource_limits, input_len)`; for a 5 B-instruction / 30 s / tiny-input job that is ≈0.0045 NEAR. Time dominates because `per_ms_fee` is 1e17 yocto **per millisecond**.
* Open-Meteo: multi-location → JSON **array** (first element has no `location_id`, later ones do); single-location → **object**; `/v1/forecast` returns **HTTP 400** beyond the ~92-day past window; `/v1/archive` returns a whole 24-hour day, so index by timestamp, not `[0]`.

---

## 1. Exact current `request_execution` on `outlayer.testnet`

### 1.1 What is actually deployed (verified on chain)

`outlayer.testnet` exists (account created at `block_timestamp 1761155457426997785`, ≈2025-10-22) — `https://api-testnet.nearblocks.io/v3/accounts/outlayer.testnet`.

Its on-chain ABI (`https://api-testnet.nearblocks.io/v3/accounts/outlayer.testnet/contract/schema`, `schema_version 0.4.0`, built `rustc 1.85.0` / `cargo-near-build 0.11.5`) contains, at JSON lines 2246–2333:

```json
{ "name": "request_execution", "kind": "call", "modifiers": ["payable"],
  "params": { "serialization_type": "json", "args": [
    { "name": "source",            "type_schema": { "$ref": "#/definitions/ExecutionSource" } },
    { "name": "resource_limits",   "type_schema": { "anyOf": [ {"$ref":"#/definitions/ResourceLimits"}, {"type":"null"} ] } },
    { "name": "input_data",        "type_schema": { "type": ["string","null"] } },
    { "name": "secrets_ref",       "type_schema": { "anyOf": [ {"$ref":"#/definitions/SecretsReference"}, {"type":"null"} ] } },
    { "name": "response_format",   "type_schema": { "anyOf": [ {"$ref":"#/definitions/ResponseFormat"}, {"type":"null"} ] } },
    { "name": "payer_account_id",  "type_schema": { "type": ["string","null"] } },
    { "name": "params",            "type_schema": { "anyOf": [ {"$ref":"#/definitions/RequestParams"}, {"type":"null"} ] } }
  ] } }
```

and the variant definitions (same document, lines 3782–3888, 4112–4183, 4212–4230, 4384–4405):

| Type | JSON shape (exactly as deployed) |
|---|---|
| `ExecutionSource` | `{"GitHub":{"repo":string,"commit":string,"build_target":string\|null}}` **or** `{"WasmUrl":{"url":string,"hash":string,"build_target":string\|null}}` **or** `{"Project":{"project_id":string,"version_key":string\|null}}` (`additionalProperties:false`; `repo`+`commit` / `url`+`hash` / `project_id` required) |
| `ResourceLimits` | `{"max_instructions":u64\|null,"max_memory_mb":u32\|null,"max_execution_seconds":u64\|null}` — all optional keys, all nullable |
| `SecretsReference` | `{"account_id":AccountId,"profile":string}` (both **required**) |
| `ResponseFormat` | `"Bytes"` \| `"Text"` \| `"Json"` (bare string, default `Text`) |
| `RequestParams` | `{"force_rebuild":bool,"store_on_fastfs":bool,"compile_only":bool,"project_uuid":string\|null,"attached_usd":U128-string\|null,"use_bound_identity":bool}` — all defaulted |

This matches the repo source byte-for-byte: `contract/src/lib.rs:150` (`enum ExecutionSource`), `:192` (`struct RequestParams`), `:241` (`enum ResponseFormat`), `:259` (`struct ResourceLimits`), `:374` (`struct SecretsReference`) and `contract/src/execution.rs:36-43` (`pub fn request_execution(&mut self, source, resource_limits, input_data, secrets_ref, response_format, payer_account_id, params)`).

### 1.2 What the docs say, and which one is current

| Doc (live) | Shape it shows | Verdict |
|---|---|---|
| `https://app.outlayer.ai/docs/near-integration` §"Method: request_execution" | `source` with **Project** or **WasmUrl** variants, `resource_limits` (**"Always pass it, even as `{}`: a request without it runs nothing"**), `input_data` string, `secrets_ref`, `response_format`, `payer_account_id`, `params` (`attached_usd`, `use_bound_identity`) | **Current.** Matches chain. Note it omits the `GitHub` variant even though the contract still accepts it. |
| `https://github.com/out-layer/weather-example` README §"Step 3: Request Execution" | `source.Project{project_id, version_key}`, plus `secrets_ref`, `resource_limits`, `response_format`, `input_data` — omits `payer_account_id`/`params` | **Current.** |
| `llms-full.txt` §"Proxy Contracts Tutorial" (lines 3267–3290) and `contract/README.md` §`request_execution` | `code_source` as a free-form `serde_json::Value`, flat `{"repo","commit","build_target"}`; no `params` | **Stale.** `code_source` is not a parameter name in the deployed ABI; the flat repo object is not a valid `ExecutionSource`. |
| `llms-full.txt` §"Code Source Formats" (lines 3412–3450) | `{"GitHub":{"url","hash"}}`, `{"WasmHash":{"hash"}}` | **Stale / wrong.** Live `GitHub` takes `repo`+`commit`, not `url`+`hash`; there is **no `WasmHash` variant** — a pre-built file is `WasmUrl{url,hash}`. |
| `llms-full.txt` §"NEAR Contract Integration" line 739–752 | `code_source: {repo, commit, build_target}` | **Stale.** |
| `https://github.com/out-layer/private-dao-example` `dao-contract/src/lib.rs:51-62` | `source` as `Value`, `Option<…>` for the rest, **no `params`** | Works only if `params` may be omitted; the ABI lists it as nullable, so `null`/omission is accepted **[INFERENCE: near-sdk treats `Option<T>` args as optional; the doc example at docs/near-integration passes only 3 of 7 args]**. Its callback typing (`Result<Option<OutLayerResponse>,…>`, `:718`, `:775`) is **stale** — see §2. |

Why: the `llms-full.txt` bundle predates the current contract (its own Contents list, lines 18–40, lacks docs that `https://app.outlayer.ai/llms.txt` lists, e.g. *Pricing*, *TEE Attestation*, *Projects*). The chain ABI and `/docs/near-integration` agree with each other and with `contract/src/execution.rs` on `main`.

### 1.3 Ready-to-paste Rust (near-sdk 5.x)

```rust
use near_sdk::{ext_contract, near, AccountId, Gas, NearToken, Promise, PromiseError};
use near_sdk::serde_json::json;

pub const OUTLAYER: &str = "outlayer.testnet"; // "outlayer.near" on mainnet
pub const OUTLAYER_GAS: Gas = Gas::from_tgas(200);
pub const WEATHER_CALLBACK_GAS: Gas = Gas::from_tgas(60);

/// Mirrors the deployed `ExecutionSource` (externally tagged).
#[near(serializers = [json])]
#[derive(Clone, Debug)]
pub enum ExecutionSource {
    GitHub { repo: String, commit: String, build_target: Option<String> },
    WasmUrl { url: String, hash: String, build_target: Option<String> },
    Project { project_id: String, version_key: Option<String> },
}

#[ext_contract(ext_outlayer)]
pub trait OutLayer {
    fn request_execution(
        &mut self,
        source: serde_json::Value,
        resource_limits: Option<serde_json::Value>,
        input_data: Option<String>,
        secrets_ref: Option<serde_json::Value>,
        response_format: Option<String>,
        payer_account_id: Option<AccountId>,
        params: Option<serde_json::Value>,
    );
}
```

Call site (project source, JSON output, refunds to the keeper):

```rust
let src = json!({ "Project": { "project_id": self.cfg.oracle.project_id, "version_key": null } });
let limits = json!({ "max_instructions": 5_000_000_000u64, "max_memory_mb": 64, "max_execution_seconds": 30 });

ext_outlayer::ext(OUTLAYER.parse().unwrap())
    .with_attached_deposit(NearToken::from_millinear(20)) // 0.02 NEAR
    .with_static_gas(OUTLAYER_GAS)
    .with_unused_gas_weight(1)
    .request_execution(
        src,
        Some(limits),
        Some(input_json),          // the stdin bytes for the WASI module
        None,                      // no secrets for Open-Meteo
        Some("Json".to_string()),
        Some(keeper.clone()),      // refunds go here on failure
        None,                      // params
    )
    .then(ext_self::ext(env::current_account_id())
        .with_static_gas(WEATHER_CALLBACK_GAS)
        .on_weather(round, keeper))
```

> Dropping `&mut self` from the `#[ext_contract]` trait is also valid in near-sdk 5; the shipped docs and the DAO example both include it, so either compiles.

### 1.4 Ready-to-paste `near` CLI (near-cli-rs)

```bash
near contract call-function as-transaction outlayer.testnet request_execution \
  json-args '{
    "source": { "Project": { "project_id": "drizzle-dev.testnet/drizzle-oracle", "version_key": null } },
    "resource_limits": { "max_instructions": 5000000000, "max_memory_mb": 64, "max_execution_seconds": 30 },
    "input_data": "{\"hour\":1789605600,\"cities\":[[603900,53200],[-235500,-466300]]}",
    "secrets_ref": null,
    "response_format": "Json",
    "payer_account_id": null,
    "params": null
  }' \
  prepaid-gas '100 Tgas' attached-deposit '0.02 NEAR' \
  sign-as drizzle-dev.testnet network-config testnet sign-with-keychain send
```

(The `near contract call-function as-transaction … json-args … prepaid-gas … attached-deposit … sign-as … network-config … sign-with-keychain send` ordering is the one used throughout `contract/README.md` §"Deploy with init" / §"Set operator account". A raw-WASM dry run instead of a project: `"source": {"WasmUrl": {"url": "https://test.fastfs.io/<you>/outlayer.testnet/<sha256>.wasm", "hash": "<sha256>", "build_target": "wasm32-wasip2"}}` — cf. `https://app.outlayer.ai/docs/near-integration` §"FastFS Workflow".)

---

## 2. How the result reaches the caller contract

### 2.1 Mechanism (yield/resume)

`request_execution` is a **payable, non-returning** method: it builds a `request_data` JSON, calls `env::promise_yield_create("on_execution_response", request_data, response_gas(input_len), GasWeight::default(), DATA_ID_REGISTER)` and then `env::promise_return(promise_idx)` — `contract/src/execution.rs:323-330` and `:355`. It stores an `ExecutionRequest{request_id, data_id, payer_account_id, payment, …}` in `pending_requests` (`execution.rs:333-355`).

The worker later calls `resolve_execution(request_id, response)` / `submit_execution_output_and_resolve(...)` (operator-only, `execution.rs:395-430`), which calls `env::promise_yield_resume(&data_id, &serde_json::to_vec(&response))` (`execution.rs:1072`). The resume payload is an `ExecutionResponse` (`contract/src/lib.rs:350-367`): `{success: bool, output: Option<ExecutionOutput>, error: Option<String>, resources_used: {instructions, time_ms, compile_time_ms}, compilation_note, refund_usd}`.

`on_execution_response` (`execution.rs:451`, `#[private]`) is then run **by the OutLayer contract**, refunds the payer, emits `execution_completed`, and terminates with `env::value_return(...)`. **That returned value is what the caller's `.then()` callback receives as the promise result.**

### 2.2 What bytes the callback actually gets

From `output_return_bytes` (`execution.rs:889-920`) — the authority on this question:

| Run outcome | Bytes returned to your `.then()` |
|---|---|
| success, `response_format:"Json"` | the guest's stdout **verbatim** (e.g. `{"hour":…,"mm_tenths":[…]}`) |
| success, `"Text"` (default) | that text as **one JSON string** (`"…"`) |
| success, `"Bytes"` | base64 as one JSON string |
| success but empty/no output | `null` |
| `success:false` (guest error, timeout, hash mismatch, …) | `null` (`execution.rs:757`) |
| `Err(PromiseError)` inside `on_execution_response` (the worker-resume promise itself failed) | `null` (`execution.rs:788`) |
| unknown `request_id` | `null` (`execution.rs:797`) |

So: **there is no `OutLayerResponse { success, result, error }` wrapper on chain.** The wrapper documented at `llms-full.txt` §"Advanced: OutLayerResponse Wrapper" (lines 3445–3460) and used by `private-dao-example/dao-contract/src/lib.rs:718` is stale; code that deserializes it would fail on the current contract (the object is the guest's payload, which has no `success` key). Defensive parsing is cheap if you want belt-and-braces: if the object has exactly `success`/`result`/`error` keys, unwrap `result`, else treat it as the payload.

Because `response_format` defaults to **`Text`**, a JSON-emitting module must set `"Json"`; otherwise the callback value is a JSON *string*.

### 2.3 Ready-to-paste callback declaration

```rust
use near_sdk::{near, AccountId, PromiseError};
use near_sdk::serde_json::Value;

#[near(serializers = [json])]
#[derive(Clone, Debug)]
pub struct RainReading {
    pub hour: u64,
    pub mm_tenths: Vec<u16>,
    pub source: String,
}

#[near]
impl Contract {
    #[private]
    pub fn on_weather(
        &mut self,
        round: u64,
        keeper: AccountId,
        #[callback_result] result: Result<Option<Value>, PromiseError>,
    ) {
        let reading: RainReading = match result {
            Ok(Some(v)) => match near_sdk::serde_json::from_value(v) {
                Ok(r) => r,
                Err(e) => return self.fail_round(round, keeper, &format!("bad payload: {e}")),
            },
            Ok(None) => return self.fail_round(round, keeper, "outlayer returned null"),
            Err(_)  => return self.fail_round(round, keeper, "outlayer call failed / never resumed"),
        };
        // ... validate reading.hour == expected hour, reading.mm_tenths.len() == 12 ...
    }
}
```

`Result<Option<RainReading>, PromiseError>` (typed) works identically — `Option<T>` deserializes the literal `null` to `None`.

### 2.4 Failure and timeout semantics

* **Guest failure / worker-reported error** → `Ok(None)` (value is `null`). Your callback runs normally, refund already issued.
* **OutLayer call itself failing** (bad args, insufficient deposit, `project not found`, `Not enough gas` at yield creation) → the receipt fails, `.then()` runs with `Err(PromiseError)`. Immediate (same block chain).
* **Worker never resolves** → **nothing happens.** There is no timeout on the yield inside `request_execution`; the promise stays pending and your `.then()` is never called. `EXECUTION_TIMEOUT` is 600 s = 10 min (`contract/src/lib.rs:86`), and `cancel_stale_execution(request_id)` (`execution.rs:819`) can then be called **by the original sender only** (`execution.rs:830-834`) to get a refund — but that path removes the pending request and refunds **without resuming the yield** (`execution.rs:836-870`), so the caller's `.then()` still never fires. Consequence for Drizzle: the `in_flight` lock **must** self-expire on the contract side (HANDOFF §2.2 already specifies 10 min) — do not rely on getting a callback at all.

### 2.5 Refunds — who gets what

* `payer_account_id` defaults to `predecessor_account_id()` (`execution.rs:399`); it is stored, non-optional, in `ExecutionRequest.payer_account_id`.
* Success: `cost = calculate_cost(used resources)`, capped at `payment − input_storage`; `refund = payment − cost` transferred to `payer_account_id` (`execution.rs:640-690`).
* Failure (`success:false`): same formula on the resources the worker reported (`execution.rs:700-740`) — **so a failed run is partially charged, not "not refunded"** as `https://app.outlayer.ai/docs/pricing` §"Refund Policy" claims. Source wins.
* Resume-promise failure: refund is `payment − base_fee` (`execution.rs:762-770`).
* Stale cancel: keeps base fee (or the compute estimate if output was already delivered), refunds the rest (`execution.rs:840-860`).
* The stablecoin side (`attached_usd`) is always returned to `sender_id` (= the predecessor), never to `payer_account_id` (`execution.rs:930-945`).

---

## 3. Request id, logs, and the attestation link

* **Returned to the callback: no.** `request_execution` returns nothing (`execution.rs:36`), and the id is not part of the callback argument list of `on_execution_response` (`execution.rs:451-458` includes `request_id`, but that is OutLayer's *own* callback; the caller only receives the `value_return` bytes).
* **Emitted on chain: yes.** The `execution_requested` NEP-297 event's `data[0].request_data` is a **JSON string** containing `"request_id": <u64>`, plus `sender_id`, `predecessor_id`, `code_source`, `resource_limits`, `input_data`, `secrets_ref`, `response_format`, `payment`, `attached_usd`, `timestamp`, `project_uuid`, `project_id` (`execution.rs:301-320`; emitter `contract/src/events.rs:42-52`, standard `"near-outlayer"`). `input_data` is blanked in the event when ≥ 10 000 bytes (`contract/src/lib.rs:100`, `execution.rs:404-415`).
* **Readable later: yes.** `get_request(request_id)` view (`contract/README.md` §"View Functions"; in the deployed ABI), and `get_pending_output` / `has_pending_output`.
* **Public attestation page: `https://app.outlayer.ai/attestation/{job_id}`.** Verified live: `https://app.outlayer.ai/attestation/123` returns HTTP 200 with `<title>TEE Attestation — Job #123 · OutLayer</title>` and body "Loading attestation #123…". The list of jobs with its "View Attestation" link is `https://app.outlayer.ai/executions` (`https://app.outlayer.ai/docs/near-integration` §"TEE Attestation": *"View attestations at /executions → 'View Attestation'"*). Note `/executions/{id}` is **404** — the detail route is `/attestation/{id}`.
* **What the page shows** (`https://app.outlayer.ai/docs/tee-attestation` §"Execution Attestation" and §"Task Hash"): `${wasm_hash}$ (SHA256 of the exact bytes), input hash, output hash, worker measurement (TDX MRTD+RTMR0-3), and the full Intel TDX quote; the browser verifies the DCAP chain to Intel's SGX Root CA, checks measurements against `worker.outlayer.testnet`, and recomputes the task hash from REPORTDATA[568:600]. The task-hash preimage is documented literally as `task_type + task_id(i64 LE) + repo_url? + commit_hash? + build_target? + wasm_hash? + input_hash? + output_hash + block_height?(u64 LE)`.
* **[OPEN]** Whether `{job_id}` equals the on-chain `request_id` is not documented and I could not observe a job id on a real execution. The dashboard table has separate "ID" and "TX" columns (`https://app.outlayer.ai/executions`), which suggests the job id is the coordinator's, discovered by tx hash — treat the mapping as unknown until observed.

**Practical answer for Drizzle:** the keeper (off-chain) reads the `tick()` tx outcome via RPC (`EXPERIMENTAL_tx_status`), pulls `request_id` out of the `execution_requested` event, and stores the mapping `round → {request_id, tx_hash}` for the UI/Verify drawer. The contract itself cannot know it.

---

## 4. Gas

| Quantity | Value | Source |
|---|---|---|
| Gas the OutLayer contract reserves for **its own** settlement callback | **50 Tgas** minimum, +25 Mgas per byte of `input_data` above 32 KiB | `contract/src/lib.rs:28`, `:49`, `:75`; `response_gas()` `:52-58` |
| Unused-gas distribution into that callback | `GasWeight::default()` (= 0) — i.e. only the reserved amount, unused gas is not shared | `execution.rs:327` |
| Docs' example allocation | `Gas::from_tgas(100)` for `request_execution`, `Gas::from_tgas(10)` for the callback | `https://app.outlayer.ai/docs/near-integration` §"Example Contract Call" |
| JS/TS example | 300 Tgas total | `llms-full.txt:2658` |
| Test task in the contract repo | `prepaid_gas(Gas::from_tgas(300))` | `contract/src/execution.rs` mod `who_the_on_chain_door_judges` |

Recommendation for a `tick()` inside the 300 Tgas ceiling:

```
total prepaid            300 Tgas
  request_execution      200 Tgas   (>= 50 reserved + ~10 admission; with_unused_gas_weight(1))
  on_weather callback     60 Tgas   (12 mints + ~12 storage writes + 1 event)
  left for tick() body    ~40 Tgas
```

If you attach less than ~60 Tgas to `request_execution`, `promise_yield_create` panics for lack of gas and your `.then()` gets `Err(PromiseError)` — cheap but useless. `estimate_execution_cost` is a view, so the keeper can pre-compute the deposit without any gas risk.

---

## 5. Deposit / pricing

**Live testnet pricing** (`https://testnet-api.outlayer.ai/public/pricing`, fetched now — fields mirror `get_pricing`):

| Field | YoctoNEAR | NEAR |
|---|---|---|
| `base_fee` | 1 000 000 000 000 000 000 000 | 0.001 |
| `per_instruction_fee` (= per **million** instructions) | 100 000 000 000 000 | 0.0001 / 1M |
| `per_ms_fee` | 100 000 000 000 000 000 | 0.0001 **per ms** |
| `per_compile_ms_fee` | 100 000 000 000 000 000 | 0.0001 per ms |
| `max_instructions` / `max_execution_seconds` / `max_compilation_seconds` | 500e9 / 180 / 300 | — |

Formula (`contract/src/lib.rs:863-880` `estimate_cost`, and `calculate_cost` just above): `base + (max_instructions/1_000_000)*per_million_instructions_fee + (max_execution_seconds*1000)*per_ms_fee` (+ `compile_time_ms*per_compile_ms_fee` at settlement). Note the source comment calls `per_ms_fee` "NEAR per second" (`lib.rs:660` region, init) while the arithmetic multiplies by **milliseconds** — the arithmetic is what runs, so time is the dominant term.

**Worked example** for `{max_instructions: 5_000_000_000, max_execution_seconds: 30, max_memory_mb: 64}`, tiny input:
`0.001 + 5000·0.0001 (…= 0.0005) + 30 000×1e17 yocto (0.003)` = **0.0045 NEAR** required. Input storage adds `0.00001 NEAR/byte` above 32 KiB only. Attach 0.02 NEAR → at settlement the charge is recomputed from *used* resources (e.g. ~1.5 s, 300 M instructions → ≈0.0012 NEAR), so the refund is ≈0.0187 NEAR.

There is **no fixed minimum**: the contract's assert is `payment >= estimated_cost + input_storage` (`execution.rs:470-483`), and it panics with the exact required figure. The "Minimum 0.01 NEAR" line in `llms-full.txt:3360-3390` §"Deposit Requirements" is a convention from an example, not an enforcement.

Empirical cross-check: testnet `/public/stats` reports `total_executions: 55908`, `total_near_paid_yocto: "1734509469245670666994608804"` → **≈0.031 NEAR average per execution** (includes compiles).

Failed executions are still charged for reported usage (`execution.rs:700-740`), contradicting `https://app.outlayer.ai/docs/pricing` §"Refund Policy" ("failed executions are not refunded") — the contract refunds everything above the charge.

---

## 6. OutLayer CLI

| Item | Detail | Source |
|---|---|---|
| Install | `cargo install --git https://github.com/out-layer/outlayer-cli` (or `cd outlayer-cli && cargo install --path .`) | `llms-full.txt:355-363` (§CLI §Install) |
| Login testnet | `outlayer login testnet` → prompts Account ID + ed25519 private key; creds in `~/.outlayer/{network}/credentials.json` + OS keychain; `outlayer whoami`, `outlayer logout` | `llms-full.txt:365-405` |
| Upload + hash | `outlayer upload ./target/wasm32-wasip2/release/x.wasm` → prints **SHA256** and a FastFS URL (`https://<you>.near.fastfs.io/outlayer.near/<sha>.wasm`; testnet: `https://test.fastfs.io/<you>/outlayer.testnet/<sha>.wasm`, receiver `outlayer.testnet`). Files >1 MB chunked. | `llms-full.txt:530-540`; `https://app.outlayer.ai/docs/near-integration` §"FastFS Workflow" |
| Project create/deploy | `outlayer create <name> [--template contract]`, `outlayer deploy <name>` (from git origin+HEAD), `outlayer deploy <name> <wasm-url>`, `--no-activate`; `outlayer projects [account]`; `outlayer status [call_id]` | `llms-full.txt:407-430` |
| Test execution | `outlayer run <owner>/<project> '<json>'`, `--input file`, `--async`, `--deposit`, `--compute-limit`, `--version <hash>`, `--secrets-profile/--secrets-account`; raw sources: `outlayer run --github github.com/u/r [--commit X] '…'`, `outlayer run --wasm <url> [--hash …] '…'` | `llms-full.txt:425-445` |
| Pinned hash from chain | `near view outlayer.testnet list_versions '{"project_id":"…"}'` / `get_version` return `wasm_hash` in `VersionView` (`contract/src/lib.rs` §`VersionView`), or read `Project.active_version` via `get_project` | `contract/src/lib.rs:520-545`; deployed ABI method list |
| Network/HTTP access | **Not gated for ordinary projects**: the worker's egress table says outbound network is *"open"* for a normal project, and *"only the hosts your manifest declares"* for **connectors** (`https://app.outlayer.ai/docs/connectors` §comparison table line ~115, §"Network: you declare it, the worker enforces it"). Nothing in `ResourceLimits` controls networking. | as cited |

`outlayer` CLI availability/version: not independently verified on this machine — **[OPEN]** (only the install command is documented).

---

## 7. `weather-example` (the fork base)

Repo: https://github.com/out-layer/weather-example, default branch `main`, tree SHA `c059f7f3dc7756f27dc5c5e8094f96e9a2f8357f`.

| Aspect | Fact | Source |
|---|---|---|
| Layout | `.gitignore`, `Cargo.lock`, `Cargo.toml`, `LICENSE-APACHE`, `LICENSE-MIT`, `README.md`, `build.sh` (0755), `src/main.rs` (3 477 B). **No `rust-toolchain.toml`.** | GitHub trees API for the repo |
| Crate | single binary: `[[bin]] name="weather-example", path="src/main.rs"`; `edition 2021`; `[profile.release] opt-level="z", lto=true, strip=true` | `Cargo.toml` |
| Deps | `serde 1` (derive), `serde_json 1`, **`wasi-http-client = "0.2"`** (resolved `0.2.1`, pulling `wasi 0.13.3+wasi-0.2.2`), `urlencoding 2.1` | `Cargo.toml`; `Cargo.lock` packages `wasi-http-client 0.2.1` |
| HTTP | `use wasi_http_client::Client;` → `Client::new().get(&url).connect_timeout(Duration::from_secs(10)).send()?`, then `response.status()`, `response.body()`; non-2xx → `Err(format!("Weather API error ({}): {}", …))` | `src/main.rs` |
| Input | **stdin**: `io::stdin().read_to_string(&mut input_string)` then `serde_json::from_str::<Input>` | `src/main.rs` |
| Secrets | `std::env::var("OPENWEATHER_API_KEY")`, error if absent | `src/main.rs` |
| Output | **stdout**: `serde_json::to_string(&output)` + `print!` + `io::stdout().flush()`; on `Err` from `main` the process exits non-zero (Rust `main() -> Result`) and prints the error to stderr | `src/main.rs` |
| Build | `./build.sh` → installs `wasm32-wasip2` if missing, `cargo build --target wasm32-wasip2 --release`, copies to `./weather-example.wasm`, prints size | `build.sh` |
| Limits | README: *"Output is always < 900 bytes (NEAR limit)"*, *"Timeout: 10 seconds for HTTP request"* (the code's `connect_timeout(10s)`); no other size. Note the 1024-byte large-output threshold in the contract (`contract/src/execution.rs:398` docs, ✓ `submit_execution_output` for >1024 B) is what the README's 900 is hedging | README §Notes; `contract/README.md` §`submit_execution_output_and_resolve` |
| License | README says **MIT**; `Cargo.toml` and the `LICENSE-*` pair say **MIT OR Apache-2.0** — dual. Treat as dual ([contradiction], §9) | README §License vs `Cargo.toml:4` |
| Console test | `echo '{"city":"Tokyo","units":"metric"}' \| wasmtime --env OPENWEATHER_API_KEY=… target/wasm32-wasip2/release/weather-example.wasm` | README §Testing Locally |

For Drizzle: fork, delete the OpenWeather key path (Open-Meteo needs none), swap `api.openweathermap.org/data/2.5/find` for the Open-Meteo URL built from the 12 city pairs, and read the city table from `input_data` rather than the hard-coded `city` string.

---

## 8. Open-Meteo — actual measured behaviour (2026-11-11)

| # | Request | Status | Observed |
|---|---|---|---|
| a | `/v1/forecast?latitude=60.39,-23.55&longitude=5.32,-46.63&hourly=precipitation&start_hour=2026-09-24T14:00&end_hour=2026-09-24T14:00&timezone=UTC` | **200** | **JSON array of 2 objects.** Each: `hourly.time = ["2026-09-24T14:00"]`, `hourly.precipitation = [0]`, `hourly_units.precipitation="mm"`, `utc_offset_seconds:0`, `timezone:"GMT"`. First element: `latitude 60.390038, longitude 5.3183136, elevation 17` (**no `location_id`**); second: `latitude -23.514938, longitude -46.610504, elevation 737, "location_id": 1`. |
| b | same, `start_hour=2026-11-11T10:00` (current hour) | **200** | array; Bergen `2.1`, São Paulo `0`; `hourly.time=["2026-11-11T10:00"]`. |
| c | `/v1/forecast?latitude=60.39&longitude=5.32&…&start_hour=2026-07-01T14:00&end_hour=2026-07-01T14:00` (133 days back) | **HTTP 400** | Body not retrievable through the fetch tool. Documented cause: the forecast API's `past_days` is limited to **0–92** (`https://open-meteo.com/en/docs`, forecast parameter table, `past_days Integer (0-92)`). Older hours must come from the archive API. **[INFERENCE on the exact error text]** |
| d | `/v1/archive?latitude=60.39&longitude=5.32&start_date=2026-09-24&end_date=2026-09-24&hourly=precipitation&timezone=UTC` | **200** | **JSON object** (single location). `hourly.time` = **24** entries `2026-09-24T00:00 … 23:00`; `hourly.precipitation` = 24 zeros. `latitude 60.42179, longitude 5.294118, elevation 17`. |
| e | single-location `/v1/forecast?latitude=60.39&longitude=5.32&…start_hour=2026-11-11T10:00…` | **200** | **object**, not array: `{latitude, longitude, generationtime_ms, utc_offset_seconds, timezone, timezone_abbreviation, elevation, hourly_units, hourly:{time:["2026-11-11T10:00"], precipitation:[2.1]}}` — no `location_id`. |

Answers to the specific questions:

* **Array for multi-location?** Yes — top-level JSON **array**, one object per comma-separated coordinate pair, in request order. `location_id` is present only from the second element on (0-based index), so **index by array position, never by `location_id`**.
* **Is `hourly.precipitation[0]` the requested hour?** Yes for both APIs **when** `start_hour == end_hour`: `hourly.time` is exactly `[requested]`. For the archive API (date-range semantics) `[0]` is midnight — you must find the index whose `time` equals `YYYY-MM-DDTHH:00`.
* **How far back does `/v1/forecast` go?** ≈92 days (`past_days` max 92, docs table above). 2026-09-24 (48 days back) still works; 2026-07-01 (133 days) returns 400.
* **Single-location → object instead of array?** Yes. The module must branch on `Value::is_array()` or always request ≥2 points. Drizzle sends 12 points, so it always gets an array — but the local `wasmtime` fixture with one city would silently break a naive parser.
* Precision note: values are `mm` with one decimal (e.g. `2.1`), which matches `mm_tenths = round(mm*10)` exactly; no float rounding drift at this granularity.

---

## 9. Open questions / contradictions

1. **`OutLayerResponse` wrapper.** `llms-full.txt` §"Advanced: OutLayerResponse Wrapper" (lines 3445-3540) and `private-dao-example` (`dao-contract/src/lib.rs:718`, `:775`) expect `{success, result, error}`. The deployed contract returns the guest payload verbatim or `null` (`execution.rs:889-920`). I trust the chain + source; the docs/DAO example are stale. Drizzle should parse raw JSON (optionally tolerant of the wrapper).
2. **`code_source` vs `source`.** Docs (proxy tutorial, `contract/README.md`, weather README's sibling pages) still say `code_source`; the live ABI says `source`. `code_source` will fail deserialization.
3. **`WasmHash` variant.** Documented in three places, absent from the enum (`lib.rs:150-172`). Use `WasmUrl{url,hash}`.
4. **`GitHub` field names.** Docs show `{url, hash}`; the enum wants `repo` + `commit` (and `build_target`).
5. **`resource_limits` optionality.** ABI says nullable; but `None` ⇒ **compile-only** (`execution.rs:180`), not "defaults". `/docs/near-integration` states this explicitly; every older doc shows it as optional-with-defaults. Always pass it.
6. **"Failed executions are not refunded"** (`/docs/pricing` §Refund Policy) vs the contract charging only actual usage and refunding the rest (`execution.rs:700-740`). Source wins.
7. **Attestation job id ↔ on-chain `request_id`** — undocumented; whether `/attestation/{n}` accepts the on-chain id is unverified. The keeper must record `(tx_hash, request_id)` and the UI should link the dashboard, not construct an id.
8. **`per_ms_fee` unit wording** — the contract comment says "per second" while `estimate_cost` multiplies by milliseconds (making a 30 s budget cost 0.003 NEAR). Follow the arithmetic.
9. **weather-example license** — README says MIT; `Cargo.toml`/`LICENSE-*` say MIT OR Apache-2.0.
10. **No public URL for a single execution other than `/attestation/{id}`**; `/executions/{id}` is 404, and no public list API exists (`/public/executions`, `/public/attestation/{id}` both 404 on `testnet-api.outlayer.ai`). Programmatic access is `GET /calls/{call_id}` (documented at `llms-full.txt:242-250`, auth-free), but `call_id` is a UUID, not the on-chain id.
11. **Method availability on the deployed contract** was confirmed from the ABI; I could not execute `view` calls (the fetch path here is GET-only, and NEAR RPC is POST-only). Prices therefore come from `/public/pricing` (documented as the on-chain mirror) plus the contract source defaults, not from a direct `get_pricing` call.
---

## 10. Follow-up: guest environment variables and WasmUrl hash checks

**Yes — the worker injects a fixed, documented set of environment variables into the guest's `WasiCtx`, including the on-chain `request_id` as `NEAR_REQUEST_ID` (decimal string). And yes, `WasmUrl.hash` must be the lowercase-or-any-case sha256 hex of the exact bytes, checked before any secret is decrypted or any code runs.**

---

## 1. How the env reaches the guest

| Step | Evidence |
|---|---|
| The authoritative allowlist is a constant in the worker: `pub const SYSTEM_ENV_VARS: &[&str]` — *"Every environment variable the WORKER owns… This is the authoritative list, and two tests keep it that way"* | `worker/src/main.rs:1317-1371` |
| Every name in that list is **removed from the caller's secrets first**, then written with the worker's value — so a stored secret named `NEAR_REQUEST_ID` cannot forge it (`for key in SYSTEM_ENV_VARS { env_vars.remove(*key); }`) | `worker/src/main.rs:1432-1443` |
| NEAR-mode values are written from `ExecutionContext` + task fields; correlation var written unconditionally: `env_vars.insert("NEAR_REQUEST_ID".to_string(), request_id.to_string())` | `worker/src/main.rs:1547`; HTTPS branch blanks it: `:1476` |
| The map is handed to wasmtime one key at a time — WASI **P2**: `wasi_builder.env(&key, &value)` | `worker/src/executor/wasi_p2.rs:669-672` (ctx built at `:639-642`) |
| WASI **P1**: same, `wasi_builder.env(&key, &value)` | `worker/src/executor/wasi_p1.rs:109-112` |
| There is **no `inherit_env()` anywhere** — the host process environment is not leaked; only this map is visible | absence of `inherit_env` in both executor files; only `.env(k,v)` calls present |
| Docs table (matches the code): `https://app.outlayer.ai/llms-full.txt` §"WASM Environment Variables" (lines 2277-2452) | cited sections below |

## 2. Exact names (NEAR on-chain mode)

Values are **strings** — the guest always parses (`parse::<u64>()`, etc.). Source column = where the worker writes it.

| Variable | Type / format | Value in a NEAR `request_execution` run | Source |
|---|---|---|---|
| `NEAR_REQUEST_ID` | decimal string of `u64` | **the contract's `request_id`** | `main.rs:1547` (`request_id: u64` param, `:1400`) |
| `NEAR_SENDER_ID` | AccountId string | transaction **signer** | `main.rs:1500-1502` |
| `NEAR_USER_ACCOUNT_ID` | AccountId string | billing identity (payment-key owner in HTTPS) | `main.rs:1528-1530` |
| `NEAR_PREDECESSOR_ID` | AccountId string | the contract that called OutLayer | `main.rs:1517-1519` |
| `NEAR_SIGNER_PUBLIC_KEY` | `ed25519:…` string | signer key | `main.rs:1520-1522` |
| `NEAR_RELAYER_ID` | AccountId string | `""` unless a meta-tx was relayed | `main.rs:1526-1529` |
| `NEAR_NETWORK_ID` | `"testnet"` \| `"mainnet"` | `"testnet"` for `outlayer.testnet` | `main.rs:1445` |
| `OUTLAYER_EXECUTION_TYPE` | `"NEAR"` \| `"HTTPS"` | `"NEAR"` | `main.rs:1446-1449` |
| `NEAR_CONTRACT_ID` | AccountId string | `outlayer.testnet` | `main.rs:1506-1508` |
| `NEAR_BLOCK_HEIGHT` | decimal string | block of the request | `main.rs:1509-1511` |
| `NEAR_BLOCK_TIMESTAMP` | decimal string, **nanoseconds** | request block ts | `main.rs:1512-1514` |
| `NEAR_RECEIPT_ID` | base58 string | `request_execution` receipt | `main.rs:1515-1517` |
| `NEAR_TRANSACTION_HASH` | base58 string | the caller's tx hash | `main.rs:1545-1547` |
| `NEAR_GAS_BURNT` | decimal string | gas burnt by the request | `main.rs:1523-1525` |
| `NEAR_PAYMENT_YOCTO` | decimal string (yoctoNEAR) | attached deposit | `main.rs:1531-1533` |
| `ATTACHED_USD` | decimal string (micro-USD) | `0` unless `attached_usd` | `main.rs:1538-1542` |
| `USD_PAYMENT` | decimal string | `"0"` in NEAR mode | `main.rs:1550` |
| `NEAR_MAX_INSTRUCTIONS` / `NEAR_MAX_MEMORY_MB` / `NEAR_MAX_EXECUTION_SECONDS` | decimal strings | the limits you asked for | `main.rs:1554-1556` |
| `OUTLAYER_PROJECT_ID` / `_OWNER` / `_NAME` / `_UUID` | strings | **only** for `Project` sources | `main.rs:1559-1572`, `:1576-1578` |
| `OUTLAYER_SECRETS_ROWS` | **JSON string**: `[{"owner","profile","keys":[…]}]` (names only, never values) | absent when no secret row was opened | `main.rs:1578-1585`; test `:5314-5321` |
| `OUTLAYER_SECRETS_OWNER` / `OUTLAYER_SECRETS_PROFILE` | strings | first row's owner/profile | `main.rs:1579-1580` |
| `WALLET_ID` | string | only when custody wallet attached | `main.rs:3137-3140` |
| `NEAR_RPC_PROXY_AVAILABLE` | `"1"` | written straight onto the P2 builder, only when the RPC proxy exists | `wasi_p2.rs:676-679` |
| `OUTLAYER_CALL_ID` | `""` in NEAR mode; UUID over HTTPS | `""` | `main.rs:1475-1476`, `:1552` |

Docs coverage: `llms-full.txt` §"Execution Type", §"User Identity", §"Project Variables", §"Payment Variables", §"Blockchain Context (NEAR Mode Only)" (lists `NEAR_REQUEST_ID` as *"Internal request ID"*), §"HTTPS-Specific Variables", §"Resource Limits". The docs table **omits** `NEAR_RELAYER_ID`, `OUTLAYER_SECRETS_*`, `WALLET_ID`, `NEAR_RPC_PROXY_AVAILABLE` — the code list (`main.rs:1317-1371`) is the complete one.

## 3. Can the guest read the on-chain `request_id`? — **yes**

**Variable: `NEAR_REQUEST_ID`, type: decimal string of a `u64`.**

Provenance is the contract's own id, not something the caller supplies:

1. Contract puts `"request_id": request_id` into the `execution_requested` event payload (`contract/src/execution.rs:301-320`, emitted `contract/src/events.rs:42-52`).
2. Worker parses that event into `pub struct RequestData { pub request_id: u64, … }` (`worker/src/event_monitor.rs:192-196`) and forwards it into the task: `CreateTaskParams { request_id: request_data.request_id, … }` (`worker/src/event_monitor.rs:2204-2206`). The worker also re-reads the chain's `get_request` for that id before running (`event_monitor.rs:1473-1477`, `:2648-2651`).
3. `process_task` passes it into the env merge (`worker/src/main.rs:3115-3119`) → `main.rs:1547`.

Caveats worth knowing for Drizzle:

* It is a **`u64` rendered as base-10 text**; a JSON echo must keep it a string or parse it — `{"request_id":"123"}` vs `123`.
* It is in `SYSTEM_ENV_VARS`, so it is stripped from secrets before writing → **not caller-forgeable** (`main.rs:1432-1443`, test `every_injected_variable_is_declared_a_system_name` at `:5102-5105`).
* Echoing it in stdout means it enters the **attested output hash** (`main.rs:3690-3719`), which is fine and actually useful for the Verify drawer.
* The on-chain contract still cannot *learn* the id from the callback (§earlier report) — the echo is for off-chain correlation only. If you want RoundLog.request_id on chain, the keeper must still read it from the tx event logs; the echo just makes the attestation page and the round line up.
* `OUTLAYER_SECRETS_*`, `WALLET_ID`, project vars are **absent** (not empty) when not applicable — the code deliberately preserves absent-vs-empty (`main.rs:1434-1442`); `NEAR_REQUEST_ID` is always present in NEAR mode.

Guest-side read (works in the fork of weather-example, unchanged toolchain):

```rust
// WASI P2 guest, std only
let request_id: u64 = std::env::var("NEAR_REQUEST_ID")
    .ok()
    .and_then(|s| s.parse().ok())
    .unwrap_or(0);
// echo it back so the attestation output hash covers the correlation
println!("{{\"hour\":{hour},\"mm_tenths\":{mm},\"source\":\"open-meteo\",\"outlayer_request_id\":\"{request_id}\"}}");
```

(`OUTLAYER_EXECUTION_TYPE` / `NEAR_NETWORK_ID` are the right guards if the same module is also called over HTTPS, where `NEAR_REQUEST_ID` is `""`: `llms-full.txt` §"HTTPS-Specific Variables".)

## 4. `WasmUrl.hash` — must be sha256-hex of the bytes, and it **is** verified

| Fact | Evidence |
|---|---|
| The worker measures the bytes it loaded: `let executed_wasm_sha256 = { use sha2::{Digest, Sha256}; hex::encode(Sha256::digest(&wasm_bytes)) };` | `worker/src/main.rs:2419-2422` |
| For a `WasmUrl` source the contract-named hash is compared to that measurement, **case-insensitively**, and a mismatch aborts the job with *"The WASM file does not match the hash the project names."* | `worker/src/main.rs:2428-2434` (`if !executed_wasm_sha256.eq_ignore_ascii_case(hash)`) |
| The check sits **before** any secret decryption (`decrypt_secrets_by_project` at `:2744`, `:2811`) and before the executor is invoked (`:3290-3297`) — pinned by the test `wasm_url_bytes_must_hash_to_the_projects_hash` (`:5806-5818`) which also asserts `return Ok(())` follows the refusal | `worker/src/main.rs` as cited |
| Comment stating the intent: *"A `wasm_url` project names the sha256 of its file on chain, and that is the key the cache serves it under. Bytes that hash otherwise are not the project's… refused before any secret is read and before anything runs."* | `worker/src/main.rs:2424-2427` |
| Contract side: `CodeSource::WasmUrl { url, hash, build_target }`, `hash` is documented as *"SHA256 hash for verification (hex encoded)"* | `contract/src/lib.rs:154-162` |
| Contract-side prose: *"the worker refuses a file whose bytes do not match the sha256"*; *"A project version's source is always a WasmUrl"* | `https://app.outlayer.ai/docs/near-integration` §"Method: request_execution"; `https://app.outlayer.ai/docs/projects` §"Storage Security Model" |
| Distinct thing to not confuse: for `GitHub` sources the *checksum* passed around is `sha256(repo:commit:target)` (coordinates, **not** bytes); the byte hash is always measured separately | `worker/src/main.rs:3281-3288` |

So: 64 hex chars, case-insensitive comparison, of the exact `.wasm` you uploaded (`outlayer upload` prints it), and the bytes are re-hashed in the enclave on every cold run — the cached path is covered because the measurement is taken from the buffer the engine will run (`main.rs:2360-2372`, `:2419`).

---

**Two practical notes for the Drizzle oracle:** (1) add `outlayer_request_id` to the module's JSON so the attested output hash commits to it, but keep writing `RoundLog.outlayer_request_id` from the keeper's tx-log parse — the contract can't read it; (2) `NEAR_BLOCK_TIMESTAMP`/`NEAR_BLOCK_HEIGHT` are available if you ever want the module to refuse stale blocks, but per `https://app.outlayer.ai/docs/tee-attestation` §"Data Freshness" OutLayer does not enforce source freshness — the hour check must stay in the WASM or in `on_weather`.

> **Orchestrator note:** the guest echoes `NEAR_REQUEST_ID` in its stdout, and the stdout is exactly what `on_weather` receives. So the contract **does** learn the request id from the payload, and `RoundLog.outlayer_request_id` is filled on-chain.
