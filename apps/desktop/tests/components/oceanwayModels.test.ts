import { describe, expect, it } from "vitest";
import { conversationModels, familyModels, imageModels, videoModels, recommendedModel } from "@/components/oceanway/models";

describe("Oceanway model modalities", () => {
  it("keeps the media models returned by Oceanway out of conversation choices", () => {
    const models = [
      "gpt-6.1-sol", "claude-opus-5-5", "deepseek-v4-pro",
      "gpt-image-2", "gpt-image-2.5-sunburst", "gpt-image-2.5-flare",
      "gemini-3-pro-image-preview", "gemini-3.1-flash-image",
      "gemini-3.1-flash-image-preview", "gemini-nano-banana-2.1",
      "grok-imagine-image", "grok-imagine-image-2.0",
      "seedance-2.0", "seedance-2.0-fast", "seedance-2.0-mini", "seedance-2.5",
    ].map((id) => ({ id }));
    expect(imageModels(models)).toHaveLength(9);
    expect(videoModels(models)).toHaveLength(4);
    expect(conversationModels(models)).toHaveLength(3);
    expect(familyModels("codex", conversationModels(models))).toEqual(["gpt-6.1-sol"]);
    expect(familyModels("claude", conversationModels(models))).toEqual(["claude-opus-5-5"]);
  });
  it("defaults DeepSeek and Kimi to their own families", () => {
    const models = ["gpt-6.1-sol", "deepseek-v4-pro", "kimi-k2.5", "claude-opus-5-5"];
    expect(recommendedModel("dsh", models)).toBe("deepseek-v4-pro");
    expect(recommendedModel("kimi", models)).toBe("kimi-k2.5");
    expect(familyModels("dsh", models)).toEqual(["deepseek-v4-pro"]);
    expect(familyModels("kimi", models)).toEqual(["kimi-k2.5"]);
    expect(recommendedModel("dsh", ["gpt-6.1-sol"])).toBe("");
  });
});
