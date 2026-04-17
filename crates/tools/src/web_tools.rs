use anyhow::Result;
use async_trait::async_trait;
use serde_json::{json, Value};

use crate::tool::{Tool, ToolContext, ToolResult};

/// Maximum web response size (50KB).
const MAX_RESPONSE_SIZE: usize = 50 * 1024;

/// Web search tool using a search API.
///
/// Falls back to returning a helpful message if no API key is configured.
pub struct WebSearchTool {
    client: reqwest::Client,
}

impl WebSearchTool {
    pub fn new() -> Self {
        Self {
            client: reqwest::Client::builder()
                .timeout(std::time::Duration::from_secs(30))
                .build()
                .unwrap_or_default(),
        }
    }
}

impl Default for WebSearchTool {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl Tool for WebSearchTool {
    fn name(&self) -> &str {
        "web_search"
    }

    fn toolset(&self) -> &str {
        "web"
    }

    fn description(&self) -> &str {
        "Search the web for information. Returns relevant search results with titles, \
        snippets, and URLs."
    }

    fn schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "query": {
                    "type": "string",
                    "description": "The search query"
                },
                "num_results": {
                    "type": "integer",
                    "description": "Number of results to return (default: 5)"
                }
            },
            "required": ["query"]
        })
    }

    fn requires_env(&self) -> &'static [&'static str] {
        &["EXA_API_KEY"]
    }

    async fn execute(&self, args: Value, _ctx: &ToolContext) -> Result<ToolResult> {
        let query = args.get("query")
            .and_then(|v| v.as_str())
            .ok_or_else(|| anyhow::anyhow!("Missing required argument: query"))?;

        let num_results = args.get("num_results")
            .and_then(|v| v.as_u64())
            .unwrap_or(5) as usize;

        // Try Exa API if key is available
        if let Ok(api_key) = std::env::var("EXA_API_KEY") {
            if !api_key.is_empty() {
                return self.search_exa(query, num_results, &api_key).await;
            }
        }

        // Try Tavily API if key is available
        if let Ok(api_key) = std::env::var("TAVILY_API_KEY") {
            if !api_key.is_empty() {
                return self.search_tavily(query, num_results, &api_key).await;
            }
        }

        // Fallback: use a simple web search via scraping or return instructions
        Ok(ToolResult::ok(format!(
            "Web search: no search API configured. \
            Set EXA_API_KEY or TAVILY_API_KEY environment variable to enable web search. \
            Searched for: \"{query}\""
        )))
    }
}

impl WebSearchTool {
    async fn search_exa(&self, query: &str, num_results: usize, api_key: &str) -> Result<ToolResult> {
        #[derive(serde::Deserialize)]
        struct ExaResult {
            title: Option<String>,
            text: Option<String>,
            url: Option<String>,
        }

        #[derive(serde::Deserialize)]
        struct ExaResponse {
            results: Vec<ExaResult>,
        }

        let resp = self.client.post("https://api.exa.ai/search")
            .header("x-api-key", api_key)
            .header("Content-Type", "application/json")
            .json(&json!({
                "query": query,
                "num_results": num_results,
                "use_autoprompt": false,
            }))
            .send()
            .await;

        match resp {
            Ok(resp) if resp.status().is_success() => {
                let body: Result<ExaResponse, _> = resp.json().await;
                match body {
                    Ok(response) => {
                        let results: Vec<String> = response.results
                            .into_iter()
                            .take(num_results)
                            .map(|r| {
                                let title = r.title.as_deref().unwrap_or("(no title)");
                                let url = r.url.as_deref().unwrap_or("");
                                let text = r.text.as_deref().unwrap_or("");
                                format!("[{title}]({url})\n{text}")
                            })
                            .collect();

                        if results.is_empty() {
                            Ok(ToolResult::ok(format!("No results for: {query}")))
                        } else {
                            Ok(ToolResult::ok(format!(
                                "Search results for \"{query}\":\n\n{}",
                                results.join("\n\n---\n\n")
                            )))
                        }
                    }
                    Err(e) => Ok(ToolResult::err(format!("Failed to parse Exa response: {e}"))),
                }
            }
            Ok(resp) => {
                let status = resp.status();
                let body = resp.text().await.unwrap_or_default();
                Ok(ToolResult::err(format!("Exa API error: {status}: {body}")))
            }
            Err(e) => Ok(ToolResult::err(format!("Exa API request failed: {e}"))),
        }
    }

    async fn search_tavily(&self, query: &str, num_results: usize, api_key: &str) -> Result<ToolResult> {
        #[derive(serde::Deserialize)]
        struct TavilyResult {
            title: Option<String>,
            content: Option<String>,
            url: Option<String>,
        }

        #[derive(serde::Deserialize)]
        struct TavilyResponse {
            results: Vec<TavilyResult>,
        }

        let resp = self.client.post("https://api.tavily.com/search")
            .json(&json!({
                "api_key": api_key,
                "query": query,
                "max_results": num_results,
            }))
            .send()
            .await;

        match resp {
            Ok(resp) if resp.status().is_success() => {
                let body: Result<TavilyResponse, _> = resp.json().await;
                match body {
                    Ok(response) => {
                        let results: Vec<String> = response.results
                            .into_iter()
                            .take(num_results)
                            .map(|r| {
                                let title = r.title.as_deref().unwrap_or("(no title)");
                                let url = r.url.as_deref().unwrap_or("");
                                let content = r.content.as_deref().unwrap_or("");
                                format!("[{title}]({url})\n{content}")
                            })
                            .collect();

                        if results.is_empty() {
                            Ok(ToolResult::ok(format!("No results for: {query}")))
                        } else {
                            Ok(ToolResult::ok(format!(
                                "Search results for \"{query}\":\n\n{}",
                                results.join("\n\n---\n\n")
                            )))
                        }
                    }
                    Err(e) => Ok(ToolResult::err(format!("Failed to parse Tavily response: {e}"))),
                }
            }
            Ok(resp) => {
                let status = resp.status();
                let body = resp.text().await.unwrap_or_default();
                Ok(ToolResult::err(format!("Tavily API error: {status}: {body}")))
            }
            Err(e) => Ok(ToolResult::err(format!("Tavily API request failed: {e}"))),
        }
    }
}

/// Web extraction tool for fetching and extracting content from URLs.
pub struct WebExtractTool {
    client: reqwest::Client,
}

impl WebExtractTool {
    pub fn new() -> Self {
        Self {
            client: reqwest::Client::builder()
                .timeout(std::time::Duration::from_secs(30))
                .redirect(reqwest::redirect::Policy::limited(5))
                .build()
                .unwrap_or_default(),
        }
    }
}

impl Default for WebExtractTool {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl Tool for WebExtractTool {
    fn name(&self) -> &str {
        "web_extract"
    }

    fn toolset(&self) -> &str {
        "web"
    }

    fn description(&self) -> &str {
        "Fetch and extract the text content from a URL. Useful for reading web pages, \
        documentation, and articles."
    }

    fn schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "url": {
                    "type": "string",
                    "description": "The URL to fetch and extract content from"
                }
            },
            "required": ["url"]
        })
    }

    fn max_result_size_chars(&self) -> Option<usize> {
        Some(MAX_RESPONSE_SIZE)
    }

    async fn execute(&self, args: Value, _ctx: &ToolContext) -> Result<ToolResult> {
        let url = args.get("url")
            .and_then(|v| v.as_str())
            .ok_or_else(|| anyhow::anyhow!("Missing required argument: url"))?;

        // Validate URL
        let parsed_url: url::Url = match url.parse() {
            Ok(u) => u,
            Err(e) => return Ok(ToolResult::err(format!("Invalid URL: {e}"))),
        };

        // Try Firecrawl API if key is available (better extraction)
        if let Ok(api_key) = std::env::var("FIRECRAWL_API_KEY") {
            if !api_key.is_empty() {
                return self.extract_firecrawl(url, &api_key).await;
            }
        }

        // Fallback: fetch with reqwest and extract text
        self.extract_simple(parsed_url.as_str()).await
    }
}

impl WebExtractTool {
    async fn extract_firecrawl(&self, url: &str, api_key: &str) -> Result<ToolResult> {
        #[derive(serde::Deserialize)]
        struct FirecrawlData {
            markdown: Option<String>,
            content: Option<String>,
        }

        #[derive(serde::Deserialize)]
        struct FirecrawlResponse {
            success: bool,
            data: Option<FirecrawlData>,
        }

        let resp = self.client.post("https://api.firecrawl.dev/v1/scrape")
            .header("Authorization", format!("Bearer {api_key}"))
            .header("Content-Type", "application/json")
            .json(&json!({
                "url": url,
                "formats": ["markdown"],
            }))
            .send()
            .await;

        match resp {
            Ok(resp) if resp.status().is_success() => {
                let body: Result<FirecrawlResponse, _> = resp.json().await;
                match body {
                    Ok(response) if response.success => {
                        if let Some(data) = response.data {
                            let content = data.markdown.or(data.content)
                                .unwrap_or_default();
                            if content.is_empty() {
                                Ok(ToolResult::ok(format!("No content extracted from {url}")))
                            } else {
                                Ok(ToolResult::ok(truncate(&content, MAX_RESPONSE_SIZE)))
                            }
                        } else {
                            Ok(ToolResult::err(format!("Firecrawl returned no data for {url}")))
                        }
                    }
                    Ok(_) => Ok(ToolResult::err(format!("Firecrawl extraction failed for {url}"))),
                    Err(e) => Ok(ToolResult::err(format!("Failed to parse Firecrawl response: {e}"))),
                }
            }
            Ok(resp) => {
                let status = resp.status();
                let body = resp.text().await.unwrap_or_default();
                Ok(ToolResult::err(format!("Firecrawl API error: {status}: {body}")))
            }
            Err(e) => Ok(ToolResult::err(format!("Firecrawl request failed: {e}"))),
        }
    }

    async fn extract_simple(&self, url: &str) -> Result<ToolResult> {
        let resp = self.client.get(url)
            .header("User-Agent", "Hermes/1.0")
            .send()
            .await;

        match resp {
            Ok(resp) if resp.status().is_success() => {
                let html = resp.text().await?;
                let text = extract_text_from_html(&html);
                if text.is_empty() {
                    Ok(ToolResult::ok(format!("No text content extracted from {url}")))
                } else {
                    Ok(ToolResult::ok(truncate(&text, MAX_RESPONSE_SIZE)))
                }
            }
            Ok(resp) => {
                Ok(ToolResult::err(format!("HTTP error {} fetching {url}", resp.status())))
            }
            Err(e) => Ok(ToolResult::err(format!("Failed to fetch {url}: {e}"))),
        }
    }
}

/// Simple HTML-to-text extraction (strip tags, collapse whitespace).
fn extract_text_from_html(html: &str) -> String {
    let mut text = String::with_capacity(html.len() / 2);
    let mut in_tag = false;
    let mut chars = html.chars().peekable();

    while let Some(c) = chars.next() {
        match c {
            '<' => {
                in_tag = true;
                // Add space before block elements
                if let Some(&next) = chars.peek() {
                    if is_block_element_start(next) {
                        text.push(' ');
                    }
                }
            }
            '>' => {
                in_tag = false;
                // Add space after block elements
                if let Some(&next) = chars.peek() {
                    if next != '<' {
                        text.push(' ');
                    }
                }
            }
            _ if !in_tag => {
                text.push(c);
            }
            _ => {}
        }
    }

    // Clean up whitespace
    let mut result = String::new();
    let mut prev_space = true;
    for c in text.chars() {
        if c.is_whitespace() {
            if !prev_space {
                result.push(' ');
                prev_space = true;
            }
        } else {
            result.push(c);
            prev_space = false;
        }
    }

    result.trim().to_string()
}

fn is_block_element_start(c: char) -> bool {
    matches!(c, 'd' | 'h' | 'p' | 'u' | 'o' | 'l' | 's' | 't' | 'a' | 'n')
}

fn truncate(s: &str, max_bytes: usize) -> String {
    if s.len() <= max_bytes {
        s.to_string()
    } else {
        let truncated = &s[..max_bytes.min(s.len())];
        let boundary = truncated
            .char_indices()
            .last()
            .map(|(i, _)| i + 1)
            .unwrap_or(0);
        format!("{}... [truncated]", &truncated[..boundary])
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extract_text_from_html() {
        let html = "<html><body><h1>Hello</h1><p>World</p></body></html>";
        let text = extract_text_from_html(html);
        assert!(text.contains("Hello"));
        assert!(text.contains("World"));
    }

    #[test]
    fn test_extract_text_nested() {
        let html = "<div><p>Line <strong>one</strong></p><p>Line two</p></div>";
        let text = extract_text_from_html(html);
        assert!(text.contains("Line one"));
        assert!(text.contains("Line two"));
    }

    #[test]
    fn test_extract_text_empty() {
        assert!(extract_text_from_html("").is_empty());
        assert!(extract_text_from_html("<div></div>").trim().is_empty());
    }

    #[tokio::test]
    async fn test_web_search_no_api() {
        // Clear env vars for this test
        unsafe { std::env::remove_var("EXA_API_KEY") };
        unsafe { std::env::remove_var("TAVILY_API_KEY") };

        let tool = WebSearchTool::new();
        let ctx = ToolContext {
            session_id: "test".to_string(),
            task_id: "test".to_string(),
            config: std::sync::Arc::new(h_core::HermesConfig::default()),
            working_dir: std::env::current_dir().unwrap_or_default(),
            clarify: None,
        };

        let result = tool.execute(
            json!({"query": "test query"}),
            &ctx,
        ).await.unwrap();

        assert!(!result.is_error);
        assert!(result.content.contains("no search API configured"));
    }

    #[tokio::test]
    async fn test_web_extract_no_api() {
        unsafe { std::env::remove_var("FIRECRAWL_API_KEY") };

        let tool = WebExtractTool::new();
        let ctx = ToolContext {
            session_id: "test".to_string(),
            task_id: "test".to_string(),
            config: std::sync::Arc::new(h_core::HermesConfig::default()),
            working_dir: std::env::current_dir().unwrap_or_default(),
            clarify: None,
        };

        let result = tool.execute(
            json!({"url": "invalid-url"}),
            &ctx,
        ).await.unwrap();

        assert!(result.is_error);
        assert!(result.content.contains("Invalid URL"));
    }
}
