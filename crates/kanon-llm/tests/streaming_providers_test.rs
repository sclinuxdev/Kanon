//! Integration tests for streaming LLM provider protocol implementations (SSE).

use std::net::SocketAddr;
use axum::response::IntoResponse;
use axum::routing::post;
use axum::Router;
use tokio_stream::StreamExt;

use kanon_llm::gateway::providers::{
    AnthropicMessagesProvider, OpenAiChatProvider, OpenAiResponsesProvider,
};
use kanon_llm::gateway::types::{ChatMessage, ChatRequest};
use kanon_llm::gateway::LlmProvider;

#[tokio::test]
async fn test_openai_chat_streaming_sse() {
    let app = Router::new().route(
        "/v1/chat/completions",
        post(|| async {
            let sse_body = "data: {\"choices\":[{\"index\":0,\"delta\":{\"role\":\"assistant\",\"content\":\"Hello\"},\"finish_reason\":null}]}\n\n\
                            data: {\"choices\":[{\"index\":0,\"delta\":{\"content\":\" streaming\"},\"finish_reason\":null}]}\n\n\
                            data: {\"choices\":[{\"index\":0,\"delta\":{\"content\":\" world!\"},\"finish_reason\":\"stop\"}]}\n\n\
                            data: [DONE]\n\n";

            ([("content-type", "text/event-stream")], sse_body).into_response()
        }),
    );

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr: SocketAddr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });

    let provider = OpenAiChatProvider::new(
        format!("http://{addr}/v1"),
        Some("test_key".to_string()),
        "gpt-4o-mini",
    );

    let request = ChatRequest {
        model: "gpt-4o-mini".to_string(),
        messages: vec![ChatMessage::user("Hi")],
        tools: vec![],
        temperature: None,
        max_tokens: None,
    };

    let mut stream = provider.chat_stream(&request).await.expect("Failed to start stream");

    let mut accumulated = String::new();
    let mut finished = false;

    while let Some(chunk_res) = stream.next().await {
        let chunk = chunk_res.expect("Error in chunk stream");
        accumulated.push_str(&chunk.delta_text);
        if chunk.is_finished {
            finished = true;
            assert_eq!(chunk.finish_reason.as_deref(), Some("stop"));
        }
    }

    assert!(finished, "Stream did not signal completion");
    assert_eq!(accumulated, "Hello streaming world!");
}

#[tokio::test]
async fn test_openai_responses_streaming_sse() {
    let app = Router::new().route(
        "/v1/responses",
        post(|| async {
            let sse_body = "event: response.output_text.delta\n\
                            data: {\"type\":\"response.output_text.delta\",\"delta\":\"Modern\"}\n\n\
                            event: response.output_text.delta\n\
                            data: {\"type\":\"response.output_text.delta\",\"delta\":\" responses\"}\n\n\
                            event: response.completed\n\
                            data: {\"type\":\"response.completed\"}\n\n";

            ([("content-type", "text/event-stream")], sse_body).into_response()
        }),
    );

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr: SocketAddr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });

    let provider = OpenAiResponsesProvider::new("test_key")
        .with_base_url(format!("http://{addr}/v1"));

    let request = ChatRequest {
        model: "gpt-4o".to_string(),
        messages: vec![ChatMessage::user("Hi")],
        tools: vec![],
        temperature: None,
        max_tokens: None,
    };

    let mut stream = provider.chat_stream(&request).await.expect("Failed to start stream");

    let mut accumulated = String::new();
    let mut finished = false;

    while let Some(chunk_res) = stream.next().await {
        let chunk = chunk_res.expect("Error in chunk stream");
        accumulated.push_str(&chunk.delta_text);
        if chunk.is_finished {
            finished = true;
        }
    }

    assert!(finished);
    assert_eq!(accumulated, "Modern responses");
}

#[tokio::test]
async fn test_anthropic_messages_streaming_sse() {
    let app = Router::new().route(
        "/v1/messages",
        post(|| async {
            let sse_body = "event: content_block_delta\n\
                            data: {\"type\":\"content_block_delta\",\"index\":0,\"delta\":{\"type\":\"text_delta\",\"text\":\"Claude\"}}\n\n\
                            event: content_block_delta\n\
                            data: {\"type\":\"content_block_delta\",\"index\":0,\"delta\":{\"type\":\"text_delta\",\"text\":\" streaming\"}}\n\n\
                            event: message_delta\n\
                            data: {\"type\":\"message_delta\",\"delta\":{\"stop_reason\":\"end_turn\"}}\n\n\
                            event: message_stop\n\
                            data: {\"type\":\"message_stop\"}\n\n";

            ([("content-type", "text/event-stream")], sse_body).into_response()
        }),
    );

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr: SocketAddr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });

    let provider = AnthropicMessagesProvider::new(
        format!("http://{addr}/v1"),
        Some("test_key".to_string()),
        "claude-3-5-sonnet",
    );

    let request = ChatRequest {
        model: "claude-3-5-sonnet".to_string(),
        messages: vec![ChatMessage::user("Hi")],
        tools: vec![],
        temperature: None,
        max_tokens: None,
    };

    let mut stream = provider.chat_stream(&request).await.expect("Failed to start stream");

    let mut accumulated = String::new();
    let mut finished = false;

    while let Some(chunk_res) = stream.next().await {
        let chunk = chunk_res.expect("Error in chunk stream");
        accumulated.push_str(&chunk.delta_text);
        if chunk.is_finished {
            finished = true;
            assert_eq!(chunk.finish_reason.as_deref(), Some("end_turn"));
        }
    }

    assert!(finished);
    assert_eq!(accumulated, "Claude streaming");
}
