use anyhow::Result;
use base64::engine::general_purpose::STANDARD as BASE64;
use base64::Engine as Base64Engine;
use log::{debug, error, info};

const OPENROUTER_API_URL: &str = "https://openrouter.ai/api/v1/chat/completions";
const TIMEOUT_SECS: u64 = 60;

pub struct OpenRouterEngine {
    api_key: String,
    model: String,
}

impl OpenRouterEngine {
    pub fn new(api_key: String, model: String) -> Self {
        Self { api_key, model }
    }

    pub async fn transcribe(&self, audio: Vec<f32>) -> Result<String> {
        info!(
            "OpenRouter: starting transcription with model '{}', {} samples",
            self.model,
            audio.len()
        );
        let wav_bytes = samples_to_wav(&audio)?;
        debug!(
            "OpenRouter: converted {} f32 samples to {} WAV bytes",
            audio.len(),
            wav_bytes.len()
        );

        let encoded = BASE64.encode(&wav_bytes);

        let body = serde_json::json!({
            "model": self.model,
            "messages": [{
                "role": "user",
                "content": [
                    {
                        "type": "text",
                        "text": "Transcribe this audio exactly as spoken. Output only the transcription text, nothing else."
                    },
                    {
                        "type": "input_audio",
                        "input_audio": {
                            "data": encoded,
                            "format": "wav"
                        }
                    }
                ]
            }]
        });

        let client = reqwest::Client::new();
        let response = tokio::time::timeout(
            std::time::Duration::from_secs(TIMEOUT_SECS),
            client
                .post(OPENROUTER_API_URL)
                .header("Authorization", format!("Bearer {}", self.api_key))
                .header("Content-Type", "application/json")
                .json(&body)
                .send(),
        )
        .await
        .map_err(|_| anyhow::anyhow!("OpenRouter: request timed out"))??;

        let status = response.status();
        let response_text = response.text().await?;

        if !status.is_success() {
            error!("OpenRouter API error ({}): {}", status, response_text);
            // Try to extract a useful error message from the response
            if let Ok(json) = serde_json::from_str::<serde_json::Value>(&response_text) {
                let msg = json["error"]["message"].as_str().unwrap_or(&response_text);
                return Err(anyhow::anyhow!("OpenRouter API error: {}", msg));
            }
            return Err(anyhow::anyhow!(
                "OpenRouter API error ({}): {}",
                status,
                response_text
            ));
        }

        let json: serde_json::Value = serde_json::from_str(&response_text)
            .map_err(|e| anyhow::anyhow!("OpenRouter: failed to parse response: {}", e))?;

        debug!("OpenRouter: raw response: {}", response_text);

        let message = &json["choices"][0]["message"];

        // Try message.content first (standard text response)
        // Then fall back to message.audio.transcript (audio-capable models like gpt-audio-mini)
        let text = message["content"]
            .as_str()
            .filter(|s| !s.is_empty())
            .or_else(|| message["audio"]["transcript"].as_str())
            .unwrap_or("");

        if text.is_empty() {
            error!(
                "OpenRouter: response contained no transcription text. Response: {}",
                response_text
            );
        } else {
            info!("OpenRouter: transcription complete ({} chars)", text.len());
        }

        Ok(text.trim().to_string())
    }
}

/// Convert f32 samples (16kHz mono) to WAV bytes
fn samples_to_wav(samples: &[f32]) -> Result<Vec<u8>> {
    let spec = hound::WavSpec {
        channels: 1,
        sample_rate: 16000,
        bits_per_sample: 16,
        sample_format: hound::SampleFormat::Int,
    };

    let mut cursor = std::io::Cursor::new(Vec::new());
    {
        let mut writer = hound::WavWriter::new(&mut cursor, spec)?;
        for &sample in samples {
            let clamped = sample.clamp(-1.0, 1.0);
            let val = (clamped * 32767.0) as i16;
            writer.write_sample(val)?;
        }
        writer.finalize()?;
    }

    Ok(cursor.into_inner())
}
