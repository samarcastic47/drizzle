use std::io::{self, Read, Write};

use drizzle_oracle::{build_url, format_hour, parse_input, parse_response, RoundOutput};

/// Read the round input, perform the single batched Open-Meteo request, and
/// return the serialized stdout payload.
fn run() -> Result<String, String> {
    let mut input = String::new();
    io::stdin()
        .read_to_string(&mut input)
        .map_err(|e| format!("failed to read stdin: {e}"))?;

    let round = parse_input(&input)?;
    let hour_str = format_hour(round.hour)?;
    let url = build_url(&hour_str, &round.cities);

    let (status, body) = http_get(&url)?;
    if !(200..300).contains(&status) {
        let snippet: String = body.chars().take(200).collect();
        return Err(format!("open-meteo HTTP {status}: {snippet}"));
    }

    let mm_tenths = parse_response(&body, round.cities.len(), &hour_str)?;

    let outlayer_request_id = std::env::var("NEAR_REQUEST_ID")
        .ok()
        .filter(|value| !value.is_empty() && value.parse::<u64>().is_ok());

    serde_json::to_string(&RoundOutput {
        hour: round.hour,
        mm_tenths,
        source: "open-meteo",
        outlayer_request_id,
    })
    .map_err(|e| format!("failed to serialize output: {e}"))
}

fn main() {
    match run() {
        Ok(output) => {
            print!("{output}");
            let _ = io::stdout().flush();
        }
        Err(message) => {
            let quoted =
                serde_json::to_string(&message).unwrap_or_else(|_| "\"internal error\"".to_string());
            print!("{{\"error\":{quoted}}}");
            let _ = io::stdout().flush();
            std::process::exit(1);
        }
    }
}

/// GET with a 10 s connect timeout. Only available on wasm32-wasip2; the
/// host build exists solely so `cargo test` can compile this binary target.
#[cfg(target_os = "wasi")]
fn http_get(url: &str) -> Result<(u16, String), String> {
    use std::time::Duration;
    use wasi_http_client::Client;

    let response = Client::new()
        .get(url)
        .connect_timeout(Duration::from_secs(10))
        .send()
        .map_err(|e| format!("open-meteo request failed: {e}"))?;

    let status = response.status();
    let body = response
        .body()
        .map_err(|e| format!("failed to read open-meteo response body: {e}"))?;
    let body = String::from_utf8(body)
        .map_err(|e| format!("open-meteo response is not valid UTF-8: {e}"))?;

    Ok((status, body))
}

#[cfg(not(target_os = "wasi"))]
fn http_get(_url: &str) -> Result<(u16, String), String> {
    Err("HTTP is only supported on the wasm32-wasip2 target".to_string())
}
