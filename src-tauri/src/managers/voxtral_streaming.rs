use anyhow::Result;
use base64::engine::general_purpose::STANDARD as BASE64;
use base64::Engine as Base64Engine;
use futures_util::{SinkExt, StreamExt};
use log::{debug, error, info};
use tokio::sync::{mpsc, oneshot};
use tokio_tungstenite::tungstenite::client::IntoClientRequest;
use tokio_tungstenite::tungstenite::Message;

use super::voxtral::{convert_f32_to_s16le, VOXTRAL_WS_BASE};

const TIMEOUT_SECS: u64 = 30;

pub enum StreamingAudioMsg {
    AudioChunk(Vec<f32>),
    EndOfAudio,
    Cancel,
}

pub enum StreamingTextEvent {
    Delta(String),
    Done,
}

pub struct VoxtralStreamingSession {
    audio_tx: mpsc::UnboundedSender<StreamingAudioMsg>,
    result_rx: Option<oneshot::Receiver<Result<String>>>,
    text_rx: Option<mpsc::UnboundedReceiver<StreamingTextEvent>>,
    task_handle: Option<tauri::async_runtime::JoinHandle<()>>,
}

impl VoxtralStreamingSession {
    pub fn start(api_key: String, model: String) -> Result<Self> {
        let (audio_tx, audio_rx) = mpsc::unbounded_channel::<StreamingAudioMsg>();
        let (result_tx, result_rx) = oneshot::channel::<Result<String>>();
        let (text_tx, text_rx) = mpsc::unbounded_channel::<StreamingTextEvent>();

        let task_handle =
            tauri::async_runtime::spawn(Self::run(api_key, model, audio_rx, result_tx, text_tx));

        Ok(Self {
            audio_tx,
            result_rx: Some(result_rx),
            text_rx: Some(text_rx),
            task_handle: Some(task_handle),
        })
    }

    async fn run(
        api_key: String,
        model: String,
        mut audio_rx: mpsc::UnboundedReceiver<StreamingAudioMsg>,
        result_tx: oneshot::Sender<Result<String>>,
        text_tx: mpsc::UnboundedSender<StreamingTextEvent>,
    ) {
        let result = Self::run_inner(&api_key, &model, &mut audio_rx, &text_tx).await;
        let _ = result_tx.send(result);
    }

    async fn run_inner(
        api_key: &str,
        model: &str,
        audio_rx: &mut mpsc::UnboundedReceiver<StreamingAudioMsg>,
        text_tx: &mpsc::UnboundedSender<StreamingTextEvent>,
    ) -> Result<String> {
        // Build WebSocket request with auth header
        let ws_url = format!("{}?model={}", VOXTRAL_WS_BASE, model);
        let mut request = ws_url.as_str().into_client_request()?;
        request.headers_mut().insert(
            "Authorization",
            format!("Bearer {}", api_key).parse()?,
        );

        let (ws_stream, _response) = tokio::time::timeout(
            std::time::Duration::from_secs(TIMEOUT_SECS),
            tokio_tungstenite::connect_async(request),
        )
        .await
        .map_err(|_| anyhow::anyhow!("Voxtral streaming: WebSocket connection timed out"))??;

        let (mut ws_write, mut ws_read) = ws_stream.split();

        // Wait for session.created
        let mut session_created = false;
        while let Some(msg) = tokio::time::timeout(
            std::time::Duration::from_secs(TIMEOUT_SECS),
            ws_read.next(),
        )
        .await
        .map_err(|_| anyhow::anyhow!("Voxtral streaming: timed out waiting for session.created"))?
        {
            let msg = msg?;
            if let Message::Text(text) = msg {
                let json: serde_json::Value = serde_json::from_str(&text)?;
                if json["type"].as_str() == Some("session.created") {
                    debug!("Voxtral streaming: session created");
                    session_created = true;
                    break;
                }
            }
        }

        if !session_created {
            return Err(anyhow::anyhow!(
                "Voxtral streaming: did not receive session.created"
            ));
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
        ws_write
            .send(Message::Text(session_update.to_string().into()))
            .await?;
        debug!("Voxtral streaming: sent session.update");

        // Main loop: forward audio chunks and listen for transcription events
        let mut full_text = String::new();
        let mut audio_ended = false;

        loop {
            tokio::select! {
                msg = audio_rx.recv(), if !audio_ended => {
                    match msg {
                        Some(StreamingAudioMsg::AudioChunk(samples)) => {
                            let pcm = convert_f32_to_s16le(&samples);
                            let encoded = BASE64.encode(&pcm);
                            let msg = serde_json::json!({
                                "type": "input_audio.append",
                                "audio": encoded
                            });
                            if let Err(e) = ws_write.send(Message::Text(msg.to_string().into())).await {
                                error!("Voxtral streaming: failed to send audio chunk: {}", e);
                                return Err(anyhow::anyhow!("Voxtral streaming: WebSocket write failed: {}", e));
                            }
                        }
                        Some(StreamingAudioMsg::EndOfAudio) => {
                            let end_msg = serde_json::json!({
                                "type": "input_audio.end"
                            });
                            if let Err(e) = ws_write.send(Message::Text(end_msg.to_string().into())).await {
                                error!("Voxtral streaming: failed to send end signal: {}", e);
                                return Err(anyhow::anyhow!("Voxtral streaming: WebSocket write failed: {}", e));
                            }
                            debug!("Voxtral streaming: sent input_audio.end");
                            audio_ended = true;
                        }
                        Some(StreamingAudioMsg::Cancel) => {
                            debug!("Voxtral streaming: cancelled");
                            let _ = ws_write.close().await;
                            return Err(anyhow::anyhow!("Voxtral streaming: cancelled by user"));
                        }
                        None => {
                            // Channel closed unexpectedly — treat as end of audio
                            debug!("Voxtral streaming: audio channel closed");
                            audio_ended = true;
                        }
                    }
                }
                ws_msg = ws_read.next() => {
                    match ws_msg {
                        Some(Ok(Message::Text(text))) => {
                            let json: serde_json::Value = serde_json::from_str(&text)?;
                            match json["type"].as_str() {
                                Some("transcription.text.delta") => {
                                    if let Some(delta) = json["text"].as_str() {
                                        full_text.push_str(delta);
                                        let _ = text_tx.send(StreamingTextEvent::Delta(delta.to_string()));
                                    }
                                }
                                Some("transcription.done") => {
                                    if let Some(final_text) = json["text"].as_str() {
                                        full_text = final_text.to_string();
                                    }
                                    let _ = text_tx.send(StreamingTextEvent::Done);
                                    info!("Voxtral streaming: transcription complete");
                                    break;
                                }
                                Some("error") => {
                                    let err_msg = json["message"]
                                        .as_str()
                                        .or_else(|| json["error"]["message"].as_str())
                                        .unwrap_or("Unknown error");
                                    error!("Voxtral streaming API error: {}", err_msg);
                                    return Err(anyhow::anyhow!("Voxtral streaming API error: {}", err_msg));
                                }
                                _ => {
                                    debug!("Voxtral streaming: received event type: {:?}", json["type"]);
                                }
                            }
                        }
                        Some(Ok(Message::Close(_))) => {
                            if audio_ended {
                                // Server closed before sending transcription.done —
                                // return what we have from accumulated deltas
                                info!("Voxtral streaming: server closed connection after audio ended, using accumulated text");
                                break;
                            }
                            debug!("Voxtral streaming: WebSocket closed by server");
                            break;
                        }
                        Some(Err(e)) => {
                            error!("Voxtral streaming: WebSocket read error: {}", e);
                            return Err(anyhow::anyhow!("Voxtral streaming: WebSocket read error: {}", e));
                        }
                        None => {
                            if audio_ended {
                                // Stream ended after we sent end signal —
                                // return what we have from accumulated deltas
                                info!("Voxtral streaming: stream ended after audio ended, using accumulated text");
                                break;
                            }
                            debug!("Voxtral streaming: WebSocket stream ended unexpectedly");
                            break;
                        }
                        _ => {}
                    }
                }
            }
        }

        let _ = ws_write.close().await;
        Ok(full_text.trim().to_string())
    }

    pub fn audio_sender(&self) -> mpsc::UnboundedSender<StreamingAudioMsg> {
        self.audio_tx.clone()
    }

    pub fn take_text_receiver(&mut self) -> Option<mpsc::UnboundedReceiver<StreamingTextEvent>> {
        self.text_rx.take()
    }

    pub fn send_end(&self) {
        let _ = self.audio_tx.send(StreamingAudioMsg::EndOfAudio);
    }

    pub fn cancel(&self) {
        let _ = self.audio_tx.send(StreamingAudioMsg::Cancel);
    }

    pub async fn await_result(mut self) -> Result<String> {
        let rx = self
            .result_rx
            .take()
            .ok_or_else(|| anyhow::anyhow!("Result already consumed"))?;

        rx.await
            .map_err(|_| anyhow::anyhow!("Voxtral streaming: result channel closed"))?
    }
}

impl Drop for VoxtralStreamingSession {
    fn drop(&mut self) {
        // If we're being dropped without consuming the result, cancel the task
        if let Some(handle) = self.task_handle.take() {
            handle.abort();
        }
    }
}
