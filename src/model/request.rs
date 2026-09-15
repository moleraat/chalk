const MAX_ATTEMPTS: u32 = 5;

pub fn handle_prompt(api_key: &str, prompt: &str) -> Result<String, Box<dyn std::error::Error>> {
    let mut attempt: u32 = 0;
    loop {
        attempt = attempt.saturating_add(1);
        let (status, body) = curl_request(api_key, prompt)?;

        if status < 400 {
            return Ok(body);
        }
        if attempt >= MAX_ATTEMPTS || !retryable(status) {
            return Err(format!("Request failed with status {status}: {body}").into());
        }

        let delay_ms = backoff_delay_ms(attempt);
        eprintln!(
            "Request failed with status {status}, retrying in {delay_ms}ms (attempt {attempt}/{MAX_ATTEMPTS})"
        );
        std::thread::sleep(std::time::Duration::from_millis(delay_ms));
    }
}

// 0 covers curl transport failures (DNS, connection refused, etc.), where `%{http_code}` reports "000".
const fn retryable(status: u16) -> bool {
    status == 429 || status == 0 || (status >= 500 && status < 600)
}

const fn backoff_delay_ms(attempt: u32) -> u64 {
    match attempt {
        1 => 500,
        2 => 1_000,
        3 => 2_000,
        4 => 4_000,
        _ => 8_000,
    }
}

fn curl_request(api_key: &str, prompt: &str) -> Result<(u16, String), Box<dyn std::error::Error>> {
    const URL: &str = "https://generativelanguage.googleapis.com/v1beta/interactions";
    const CONTENT_HEADER: &str = "Content-Type: application/json";
    let api_header = format!("x-goog-api-key: {api_key}");
    let payload = serde_json::json!({"model": "gemini-3.8-flash", "input": prompt});

    let output = std::process::Command::new("curl")
        .arg("-s")
        .arg("-X")
        .arg("POST")
        .arg(URL)
        .arg("-H")
        .arg(api_header)
        .arg("-H")
        .arg(CONTENT_HEADER)
        .arg("-d")
        .arg(payload.to_string())
        .arg("-w")
        .arg("\n%{http_code}")
        .output()?;
    let raw = String::from_utf8(output.stdout)?;

    let (body, status) = raw
        .rsplit_once('\n')
        .ok_or("Malformed curl output: missing status code")?;
    let status: u16 = status.trim().parse()?;
    Ok((status, body.to_string()))
}
