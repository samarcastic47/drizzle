//! Minimal stand-in for `outlayer.testnet` used by the sandbox tests.
//!
//! `request_execution` accepts the live seven-argument ABI, records the input and
//! returns a configurable JSON payload — the same bytes the real OutLayer contract
//! returns to the caller's `.then` callback.
//!
//! `set_empty(true)` makes it return *no* bytes, which Drizzle's callback cannot
//! deserialize; Drizzle then keeps its `in_flight` lock set, which is how the tests
//! simulate an OutLayer worker that never resumes.

use near_sdk::serde_json::Value;
use near_sdk::{env, near, AccountId, PanicOnDefault};

#[near(contract_state)]
#[derive(PanicOnDefault)]
pub struct MockOutlayer {
    /// The JSON text written back to the caller.
    pub response_json: String,
    pub empty: bool,
    pub fail: bool,
    pub last_input: Option<String>,
    pub calls: u32,
}

#[near]
impl MockOutlayer {
    #[init]
    pub fn new() -> Self {
        Self {
            response_json: "null".to_string(),
            empty: false,
            fail: false,
            last_input: None,
            calls: 0,
        }
    }

    /// The seven positional arguments of the live `outlayer.testnet` ABI.
    #[payable]
    pub fn request_execution(
        &mut self,
        source: Value,
        resource_limits: Option<Value>,
        input_data: Option<String>,
        secrets_ref: Option<Value>,
        response_format: Option<String>,
        payer_account_id: Option<AccountId>,
        params: Option<Value>,
    ) {
        let _ = (
            source,
            resource_limits,
            secrets_ref,
            response_format,
            payer_account_id,
            params,
        );
        self.calls += 1;
        self.last_input = input_data;
        if self.fail {
            env::panic_str("mock outlayer: configured failure");
        }
        if !self.empty {
            env::value_return(self.response_json.as_bytes());
        }
    }

    pub fn set_response(&mut self, response: Value) {
        self.response_json = near_sdk::serde_json::to_string(&response)
            .unwrap_or_else(|_| env::abort());
    }

    pub fn set_empty(&mut self, empty: bool) {
        self.empty = empty;
    }

    pub fn set_fail(&mut self, fail: bool) {
        self.fail = fail;
    }

    pub fn get_calls(&self) -> u32 {
        self.calls
    }

    pub fn get_last_input(&self) -> Option<String> {
        self.last_input.clone()
    }

    pub fn get_response(&self) -> String {
        self.response_json.clone()
    }
}
