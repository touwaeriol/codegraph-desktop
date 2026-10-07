use rmcp::model::{CallToolResult, Content};

const TRUNCATED: &str = "\n\n[TRUNCATED: only shown excerpts are available; narrow query or use LSP, not a complete file read. Non-text content, structuredContent and metadata omitted.]";

/// Bound the entire serialized MCP result, including JSON escaping and metadata.
/// The caller must supply a budget of at least 512 Unicode scalar values.
pub(crate) fn bound(result: CallToolResult, max_chars: usize) -> CallToolResult {
    if serialized_chars(&result) <= max_chars {
        return result;
    }

    // A raw prefix longer than the budget cannot fit after JSON serialization.
    // Only copy this bounded prefix, never hidden structured or non-text payloads.
    let mut first = true;
    let text: String = result
        .content
        .iter()
        .filter_map(|content| content.as_text())
        .flat_map(|content| {
            let separator = if first { "" } else { "\n\n" };
            first = false;
            separator.chars().chain(content.text.chars())
        })
        .take(max_chars)
        .collect();
    let boundaries: Vec<usize> = text
        .char_indices()
        .map(|(offset, _)| offset)
        .chain(std::iter::once(text.len()))
        .collect();
    let excerpt = |chars: usize| CallToolResult {
        content: vec![Content::text(format!(
            "{}{}",
            &text[..boundaries[chars]],
            TRUNCATED
        ))],
        structured_content: None,
        is_error: result.is_error,
        meta: None,
    };

    // Prefix size is monotonic even for quotes, backslashes and control chars.
    // The empty excerpt plus notice fits the caller's minimum budget of 512.
    let mut low = 0;
    let mut high = boundaries.len() - 1;
    while low < high {
        let middle = low + (high - low).div_ceil(2);
        if serialized_chars(&excerpt(middle)) <= max_chars {
            low = middle;
        } else {
            high = middle - 1;
        }
    }
    excerpt(low)
}

fn serialized_chars(result: &CallToolResult) -> usize {
    serde_json::to_string(result)
        .expect("MCP result contains only JSON-serializable values")
        .chars()
        .count()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn assert_bounded(result: &CallToolResult, budget: usize) -> &str {
        let encoded = serde_json::to_string(result).unwrap();
        assert!(encoded.chars().count() <= budget);
        let decoded: CallToolResult = serde_json::from_str(&encoded).unwrap();
        assert_eq!(&decoded, result);
        assert_eq!(result.content.len(), 1);
        assert!(result.structured_content.is_none());
        assert!(result.meta.is_none());
        let text = &result.content[0].as_text().unwrap().text;
        assert!(text.ends_with(TRUNCATED));
        assert!(result.content[0].as_text().unwrap().meta.is_none());
        text.strip_suffix(TRUNCATED).unwrap()
    }

    #[test]
    fn preserves_every_field_at_and_below_exact_budget() {
        let result: CallToolResult = serde_json::from_value(json!({
            "content": [{"type": "text", "text": "中".repeat(600), "_meta": {"tag": "keep"}}],
            "structuredContent": {"keep": [1, 2, 3]},
            "_meta": {"keep": "metadata"},
            "isError": true
        }))
        .unwrap();
        let exact = serialized_chars(&result);
        assert!(exact >= 512);
        assert_eq!(bound(result.clone(), exact), result);
        assert_eq!(bound(result.clone(), exact + 1), result);
        assert_bounded(&bound(result, exact - 1), exact - 1);
    }

    #[test]
    fn fills_ascii_budget_exactly_at_minimum() {
        let result = bound(
            CallToolResult::success(vec![Content::text("a".repeat(4096))]),
            512,
        );
        assert!(!assert_bounded(&result, 512).is_empty());
        assert_eq!(serialized_chars(&result), 512);
    }

    #[test]
    fn unicode_and_heavy_escaping_use_maximal_safe_prefix() {
        let source = "中🦀é\"\\\n\r\t\0\u{0001}\u{001f}".repeat(200);
        for budget in 512..550 {
            let result = bound(
                CallToolResult::success(vec![Content::text(source.clone())]),
                budget,
            );
            let prefix = assert_bounded(&result, budget);
            assert!(!prefix.is_empty());
            assert!(source.starts_with(prefix));
            let next = source[prefix.len()..].chars().next().unwrap();
            let larger =
                CallToolResult::success(vec![Content::text(format!("{prefix}{next}{TRUNCATED}"))]);
            assert!(serialized_chars(&larger) > budget);
        }
    }

    #[test]
    fn multiple_text_blocks_keep_their_order_and_hide_the_tail() {
        let first = "first ".repeat(10);
        let second = "second ".repeat(300);
        let source = format!("{first}\n\n{second}\n\nHIDDEN_TEXT_TAIL");
        let result = bound(
            CallToolResult::success(vec![
                Content::text(first.clone()),
                Content::image("HIDDEN_IMAGE".repeat(300), "image/png"),
                Content::text(second),
                Content::text("HIDDEN_TEXT_TAIL"),
            ]),
            512,
        );
        let prefix = assert_bounded(&result, 512);
        assert!(prefix.starts_with(&format!("{first}\n\nsecond ")));
        assert!(source.starts_with(prefix));
        let encoded = serde_json::to_string(&result).unwrap();
        assert!(!encoded.contains("HIDDEN_TEXT_TAIL"));
        assert!(!encoded.contains("HIDDEN_IMAGE"));
    }

    #[test]
    fn huge_structured_content_and_metadata_cannot_leak() {
        let result: CallToolResult = serde_json::from_value(json!({
            "content": [{"type": "text", "text": "visible excerpt", "_meta": {"hidden": "BLOCK_SECRET".repeat(1000)}}],
            "structuredContent": {"hidden": "STRUCTURED_SECRET".repeat(10000)},
            "_meta": {"hidden": "META_SECRET".repeat(10000)},
            "isError": false
        }))
        .unwrap();
        let result = bound(result, 512);
        assert_eq!(assert_bounded(&result, 512), "visible excerpt");
        let encoded = serde_json::to_string(&result).unwrap();
        for hidden in ["BLOCK_SECRET", "STRUCTURED_SECRET", "META_SECRET", "_meta"] {
            assert!(!encoded.contains(hidden));
        }
    }

    #[test]
    fn preserves_all_error_flag_states() {
        for is_error in [None, Some(false), Some(true)] {
            let mut input = CallToolResult::success(vec![Content::text("x".repeat(4096))]);
            input.is_error = is_error;
            let result = bound(input, 512);
            assert_eq!(result.is_error, is_error);
            assert_bounded(&result, 512);
        }
    }

    #[test]
    fn no_text_omits_images_and_resources_without_exposing_payloads() {
        let input = CallToolResult::success(vec![
            Content::image("IMAGE_SECRET".repeat(1000), "image/png"),
            Content::embedded_text("file:///HIDDEN_URI", "RESOURCE_SECRET".repeat(1000)),
        ]);
        let result = bound(input, 512);
        assert_eq!(assert_bounded(&result, 512), "");
        let encoded = serde_json::to_string(&result).unwrap();
        for hidden in ["IMAGE_SECRET", "RESOURCE_SECRET", "HIDDEN_URI"] {
            assert!(!encoded.contains(hidden));
        }
    }

    #[test]
    fn empty_content_still_bounds_structured_payload() {
        let mut input = CallToolResult::success(vec![]);
        input.structured_content = Some(json!({"hidden": "SECRET".repeat(1000)}));
        let result = bound(input, 512);
        assert_eq!(assert_bounded(&result, 512), "");
        assert!(!serde_json::to_string(&result).unwrap().contains("SECRET"));
    }
}
