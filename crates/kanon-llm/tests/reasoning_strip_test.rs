//! Tests for the user-visible half of model reasoning output.
//!
//! Backends with a reasoning channel (DeepSeek `reasoning_content`) have that channel folded into
//! the completion text as a `<think>` block so the management console can render it separately.
//! That encoding must never reach a chat platform, otherwise users read the model's internal
//! chain of thought — these tests pin the stripping contract.

use kanon_llm::strip_reasoning_tags;

#[test]
fn strips_a_complete_reasoning_block() {
    let completion =
        "<think>\nThe user greeted me; I should greet back.\n</think>\n\n你好！我是黑猪AI。";
    assert_eq!(strip_reasoning_tags(completion), "你好！我是黑猪AI。");
}

#[test]
fn leaves_a_plain_answer_untouched() {
    let completion = "你好！我是黑猪AI。";
    assert_eq!(strip_reasoning_tags(completion), completion);
}

#[test]
fn handles_reasoning_only_and_truncated_blocks() {
    // A reasoning-only completion carries no answer for the user.
    assert_eq!(
        strip_reasoning_tags("<think>\nstill thinking\n</think>"),
        ""
    );

    // A stream cut off mid-reasoning must not leak the partial chain of thought.
    assert_eq!(strip_reasoning_tags("<think>\nstill thinking"), "");
}

#[test]
fn does_not_strip_a_block_that_is_not_leading() {
    // Only the leading block is the reasoning channel; a literal mention inside an answer stays.
    let completion = "我来解释 <think> 标签：它是推理标记。";
    assert_eq!(strip_reasoning_tags(completion), completion);
}
