use serde::Deserialize;
use serde::de::DeserializeOwned;

const MAX_ATTEMPTS: u32 = 5;
const REQUEST_INTERVAL_S: u64 = 3;

pub fn handle_prompt(api_key: &str, prompt: &str) -> Result<String, Box<dyn std::error::Error>> {
    println!("\t(ᴗ˳ᴗ)ᶻ𝗓𐰁 for {REQUEST_INTERVAL_S}s");
    std::thread::sleep(std::time::Duration::from_millis(REQUEST_INTERVAL_S * 1000));

    let mut attempt: u32 = 0;
    loop {
        println!("\t˗ˏˋ ꒰ ✉︎ ꒱ ˎˊ˗ to model...");
        attempt = attempt.saturating_add(1);
        let (status, body) = curl_request(api_key, prompt)?;

        if status < 400 {
            return Ok(body);
        }
        if attempt >= MAX_ATTEMPTS || !retryable(status) {
            return Err(format!("\tRequest failed with status {status}: {body}").into());
        }

        let delay_ms = backoff_delay_ms(attempt);
        eprintln!(
            "\t.‸. failed with {status}, retrying in {delay_ms}ms ({attempt}/{MAX_ATTEMPTS})"
        );
        std::thread::sleep(std::time::Duration::from_millis(delay_ms));
    }
}

fn curl_request(api_key: &str, prompt: &str) -> Result<(u16, String), Box<dyn std::error::Error>> {
    const URL: &str = "https://openrouter.ai/api/v1/chat/completions";
    const CONTENT_HEADER: &str = "Content-Type: application/json";
    let api_header = format!("Authorization: Bearer {api_key}");
    let payload = serde_json::json!({
        "model": "qwen/qwen3.7-flash",
        "messages": [{"role": "user", "content": prompt}],
        "response_format": {"type": "json_object"}
    });

    let output = std::process::Command::new("curl")
        .arg(URL)
        .arg("-H")
        .arg(CONTENT_HEADER)
        .arg("-H")
        .arg(api_header)
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

pub fn parse_model_output<T: DeserializeOwned>(
    raw_output: &str,
) -> Result<(T, Usage), Box<dyn std::error::Error>> {
    let model_response = serde_json::from_str::<ModelResponse>(raw_output)?;
    if model_response.choices.len() != 1 {
        return Err("Expected only 1 choice in the response".into());
    }
    let choice = model_response
        .choices
        .into_iter()
        .next()
        .ok_or("Should never happen, validated choices len")?;

    let our_response = serde_json::from_str::<T>(&choice.message.content)?;
    Ok((our_response, model_response.usage))
}

#[derive(Deserialize, Debug)]
struct ModelResponse {
    choices: Vec<Choice>,
    usage: Usage,
}

#[derive(Deserialize, Debug)]
pub struct Usage {
    pub prompt_tokens: u32,
    pub total_tokens: u32,
    pub cost: f64,
    pub completion_tokens_details: CompletionTokensDetails,
}

#[derive(Deserialize, Debug)]
pub struct CompletionTokensDetails {
    pub reasoning_tokens: u32,
}

#[derive(Deserialize, Debug)]
struct Choice {
    message: Message,
}

#[derive(Deserialize, Debug)]
struct Message {
    content: String,
}
