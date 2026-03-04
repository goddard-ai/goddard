export const Models = {
  AnthropicClaude37Sonnet: "anthropic/claude-3-7-sonnet-20250219",
  AnthropicClaudeSonnet45: "anthropic/claude-sonnet-4-5",
  AnthropicClaudeSonnet46: "anthropic/claude-sonnet-4-6",
  AnthropicClaudeOpus46: "anthropic/claude-opus-4-6",
  OpenAiO3Mini: "openai/o3-mini",
  OpenAiO3Pro: "openai/o3-pro",
  OpenAiGpt5Codex: "openai/gpt-5-codex",
  OpenAiGpt51Codex: "openai/gpt-5.1-codex",
  OpenAiGpt52Codex: "openai/gpt-5.2-codex",
  OpenAiGpt53Codex: "openai/gpt-5.3-codex",
} as const;

export type Model = (typeof Models)[keyof typeof Models] | (string & {});
