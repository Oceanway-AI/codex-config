import { describe, expect, it } from "vitest";
import {
  conversationModels,
  compatibleConversationModels,
  familyModels,
  imageModels,
  recommendedModel,
} from "@/components/oceanway/models";

describe("Oceanway model modalities", () => {
  it("only offers models supported by the agent connection, including compatible other families", () => {
    const catalog = [
      { id: "gpt-6.1-sol", supportedProtocols: ["openai-responses"] },
      { id: "gpt-6-sol", supportedProtocols: ["openai-responses"] },
      { id: "claude-sonnet-5-5", supportedProtocols: ["anthropic-messages"] },
      {
        id: "deepseek-v4.1-flash",
        supportedProtocols: ["openai-chat", "openai-responses"],
      },
      {
        id: "kimi-k3-1",
        supportedProtocols: ["openai-chat", "openai-responses"],
      },
      { id: "unpublished", supportedProtocols: [] },
    ];
    expect(compatibleConversationModels("codex", catalog)).toEqual(
      expect.arrayContaining([
        "gpt-6.1-sol",
        "gpt-6-sol",
        "deepseek-v4.1-flash",
        "kimi-k3-1",
      ]),
    );
    expect(compatibleConversationModels("codex", catalog)).not.toContain(
      "claude-sonnet-5-5",
    );
    expect(compatibleConversationModels("claude", catalog)).toEqual([
      "claude-sonnet-5-5",
    ]);
    expect(compatibleConversationModels("kimi", catalog)).toHaveLength(2);
    expect(compatibleConversationModels("dsh", catalog)).not.toContain(
      "gpt-6.1-sol",
    );
    expect(
      compatibleConversationModels("hermes", catalog, "gpt-6.1-sol"),
    ).not.toContain("claude-sonnet-5-5");
    expect(
      compatibleConversationModels("hermes", catalog, "claude-sonnet-5-5"),
    ).toEqual(["claude-sonnet-5-5"]);
    expect(compatibleConversationModels("hermes", catalog)).not.toContain(
      "unpublished",
    );
  });
  it("keeps the media models returned by Oceanway out of conversation choices", () => {
    const models = [
      "gpt-6.1-sol",
      "claude-opus-5-5",
      "deepseek-v4-pro",
      "gpt-image-2",
      "gpt-image-2.5-sunburst",
      "gpt-image-2.5-flare",
      "gemini-3-pro-image-preview",
      "gemini-3.1-flash-image",
      "gemini-3.1-flash-image-preview",
      "gemini-nano-banana-2.1",
      "grok-imagine-image",
      "grok-imagine-image-2.0",
      "seedance-2.0",
      "seedance-2.0-fast",
      "seedance-2.0-mini",
      "seedance-2.5",
    ].map((id) => ({ id }));
    expect(imageModels(models)).toHaveLength(9);
    expect(imageModels(models)).not.toContain("seedance-2.0");
    expect(conversationModels(models)).toHaveLength(3);
    expect(familyModels("codex", conversationModels(models))).toEqual([
      "gpt-6.1-sol",
    ]);
    expect(familyModels("claude", conversationModels(models))).toEqual([
      "claude-opus-5-5",
    ]);
  });
  it("defaults DeepSeek and Kimi to their own families", () => {
    const models = [
      "gpt-6.1-sol",
      "deepseek-v4-pro",
      "kimi-k2.5",
      "claude-opus-5-5",
    ];
    expect(recommendedModel("dsh", models)).toBe("deepseek-v4-pro");
    expect(recommendedModel("kimi", models)).toBe("kimi-k2.5");
    expect(familyModels("dsh", models)).toEqual(["deepseek-v4-pro"]);
    expect(familyModels("kimi", models)).toEqual(["kimi-k2.5"]);
    expect(recommendedModel("dsh", ["gpt-6.1-sol"])).toBe("");
  });
});
