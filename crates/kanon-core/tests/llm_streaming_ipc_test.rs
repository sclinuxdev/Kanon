//! Integration tests for Core IPC `RequestLLM` streaming RPC.

use std::sync::Arc;
use tokio::sync::mpsc;
use tokio_stream::StreamExt;
use tonic::{async_trait, Request};


use kanon_core::ipc::CoreApiService;
use kanon_llm::error::GatewayError;
use kanon_llm::gateway::types::{ChatChunk, ChatRequest, ChatResponse};
use kanon_llm::gateway::{ChatChunkStream, LlmGateway, LlmProvider};
use kanon_proto::v1::bot_api_service_server::BotApiService;
use kanon_proto::v1::LlmRequest;

struct MockStreamingProvider;

#[async_trait]
impl LlmProvider for MockStreamingProvider {
    async fn chat(&self, _request: &ChatRequest) -> Result<ChatResponse, GatewayError> {
        Ok(ChatResponse {
            content: Some("Full content".to_string()),
            tool_calls: vec![],
            finish_reason: Some("stop".to_string()),
            usage: None,
        })
    }

    async fn chat_stream(&self, _request: &ChatRequest) -> Result<ChatChunkStream, GatewayError> {
        let (tx, rx) = mpsc::channel(3);
        tokio::spawn(async move {
            let _ = tx.send(Ok(ChatChunk::delta("Hello "))).await;
            let _ = tx.send(Ok(ChatChunk::delta("from "))).await;
            let _ = tx.send(Ok(ChatChunk::delta("gRPC stream!"))).await;
            let _ = tx.send(Ok(ChatChunk::done(Some("stop".to_string())))).await;
        });
        Ok(Box::pin(tokio_stream::wrappers::ReceiverStream::new(rx)))
    }
}

#[tokio::test]
async fn test_core_request_llm_streaming_with_gateway() {
    let (event_tx, _event_rx) = mpsc::channel(16);
    let provider = Arc::new(MockStreamingProvider);
    let gateway = Arc::new(LlmGateway::new(provider, "test-model"));

    let service = CoreApiService::new(event_tx).with_gateway(gateway);

    let request = Request::new(LlmRequest {
        prompt: "Tell me a joke".to_string(),
        model: "test-model".to_string(),
        parameters: None,
    });

    let response = service
        .request_llm(request)
        .await
        .expect("RequestLLM failed");

    let mut stream = response.into_inner();

    let mut accumulated = String::new();
    let mut finished = false;

    while let Some(chunk_res) = stream.next().await {
        let chunk = chunk_res.expect("Error in stream chunk");
        accumulated.push_str(&chunk.delta_text);
        if chunk.is_finished {
            finished = true;
        }
    }

    assert!(finished);
    assert_eq!(accumulated, "Hello from gRPC stream!");
}

#[tokio::test]
async fn test_core_request_llm_fallback_without_gateway() {
    let (event_tx, _event_rx) = mpsc::channel(16);
    let service = CoreApiService::new(event_tx); // No gateway configured

    let request = Request::new(LlmRequest {
        prompt: "ping".to_string(),
        model: "default".to_string(),
        parameters: None,
    });

    let response = service
        .request_llm(request)
        .await
        .expect("RequestLLM failed");

    let mut stream = response.into_inner();
    let first = stream.next().await.expect("Stream empty").expect("Chunk error");

    assert_eq!(first.delta_text, "Echo: ping");
    assert!(first.is_finished);
}
