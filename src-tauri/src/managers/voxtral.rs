use anyhow::Result;
use base64::Engine as Base64Engine;
use base64::engine::general_purpose::STANDARD as BASE64;
use futures_util::{SinkExt, StreamExt};
use log::{debug, error, info};
use tokio_tungstenite::tungstenite::client::IntoClientRequest;
use tokio_tungstenite::tungstenite::Message;

pub(crate) const VOXTRAL_WS_BASE: &str =
    "wss://api.mistral.ai/v1/audio/transcriptions/realtime";
const CHUNK_SIZE: usize = 32768; // ~32KB chunks
const TIMEOUT_SECS: u64 = 30;

pub struct VoxtralEngine {
    api_key: String,
    model: String,
}

impl VoxtralEngine {
    pub fn new(api_key: String, model: String) -> Self {
        Self { api_key, model }
    }

    pub async fn transcribe(&self, audio: Vec<f32>) -> Result<String> {
        let pcm = convert_f32_to_s16le(&audio);
        debug!(
            "Voxtral: converted {} f32 samples to {} PCM bytes",
            audio.len(),
            pcm.len()
        );

        // Build WebSocket request with auth header
        let ws_url = format!("{}?model={}", VOXTRAL_WS_BASE, self.model);
        let mut request = ws_url.as_str().into_client_request()?;
        request.headers_mut().insert(
            "Authorization",
            format!("Bearer {}", self.api_key).parse()?,
        );

        let (ws_stream, _response) =
            tokio::time::timeout(
                std::time::Duration::from_secs(TIMEOUT_SECS),
                tokio_tungstenite::connect_async(request),
            )
            .await
            .map_err(|_| anyhow::anyhow!("Voxtral: WebSocket connection timed out"))??;

        let (mut write, mut read) = ws_stream.split();

        // Wait for session.created event
        let mut session_created = false;
        while let Some(msg) = tokio::time::timeout(
            std::time::Duration::from_secs(TIMEOUT_SECS),
            read.next(),
        )
        .await
        .map_err(|_| anyhow::anyhow!("Voxtral: timed out waiting for session.created"))?
        {
            let msg = msg?;
            if let Message::Text(text) = msg {
                let json: serde_json::Value = serde_json::from_str(&text)?;
                if json["type"].as_str() == Some("session.created") {
                    debug!("Voxtral: session created");
                    session_created = true;
                    break;
                }
            }
        }

        if !session_created {
            return Err(anyhow::anyhow!("Voxtral: did not receive session.created"));
        }

        // Send session.update with audio format config
        let session_update = serde_json::json!({
            "type": "session.update",
            "session": {
                "audio_format": {
                    "encoding": "pcm_s16le",
                    "sample_rate": 16000
                }
            }
        });
        write
            .send(Message::Text(session_update.to_string().into()))
            .await?;
        debug!("Voxtral: sent session.update");

        // Split PCM into chunks and send as base64
        for chunk in pcm.chunks(CHUNK_SIZE) {
            let encoded = BASE64.encode(chunk);
            let msg = serde_json::json!({
                "type": "input_audio.append",
                "audio": encoded
            });
            write.send(Message::Text(msg.to_string().into())).await?;
        }
        debug!("Voxtral: sent all audio chunks");

        // Send input_audio.end
        let end_msg = serde_json::json!({
            "type": "input_audio.end"
        });
        write.send(Message::Text(end_msg.to_string().into())).await?;
        debug!("Voxtral: sent input_audio.end");

        // Collect transcription text
        let mut full_text = String::new();

        while let Some(msg) = tokio::time::timeout(
            std::time::Duration::from_secs(TIMEOUT_SECS),
            read.next(),
        )
        .await
        .map_err(|_| anyhow::anyhow!("Voxtral: timed out waiting for transcription"))?
        {
            let msg = msg?;
            match msg {
                Message::Text(text) => {
                    let json: serde_json::Value = serde_json::from_str(&text)?;
                    match json["type"].as_str() {
                        Some("transcription.text.delta") => {
                            if let Some(delta) = json["text"].as_str() {
                                full_text.push_str(delta);
                            }
                        }
                        Some("transcription.done") => {
                            // Use the final text if provided
                            if let Some(final_text) = json["text"].as_str() {
                                full_text = final_text.to_string();
                            }
                            info!("Voxtral: transcription complete");
                            break;
                        }
                        Some("error") => {
                            let err_msg = json["message"]
                                .as_str()
                                .or_else(|| json["error"]["message"].as_str())
                                .unwrap_or("Unknown error");
                            error!("Voxtral API error: {}", err_msg);
                            return Err(anyhow::anyhow!("Voxtral API error: {}", err_msg));
                        }
                        _ => {
                            debug!("Voxtral: received event type: {:?}", json["type"]);
                        }
                    }
                }
                Message::Close(_) => {
                    debug!("Voxtral: WebSocket closed");
                    break;
                }
                _ => {}
            }
        }

        // Close the WebSocket gracefully
        let _ = write.close().await;

        Ok(full_text.trim().to_string())
    }
}

pub(crate) fn convert_f32_to_s16le(samples: &[f32]) -> Vec<u8> {
    let mut bytes = Vec::with_capacity(samples.len() * 2);
    for &sample in samples {
        let clamped = sample.clamp(-1.0, 1.0);
        let val = (clamped * 32767.0) as i16;
        bytes.extend_from_slice(&val.to_le_bytes());
    }
    bytes
}
