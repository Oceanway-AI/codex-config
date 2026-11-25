import type { OceanwayModel } from "@/lib/api/oceanway";

export function conversationProtocol(appId: string, defaultModel = ""): string {
  if (appId === "codex") return "openai-responses";
  if (appId.startsWith("claude")) return "anthropic-messages";
  if (appId === "hermes") {
    const family = defaultModel.split("/").pop() ?? defaultModel;
    if (/^(gpt-|o1|o3|o4)/i.test(family)) return "openai-responses";
    if (/^claude-/i.test(family)) return "anthropic-messages";
  }
  return "openai-chat";
}

export function compatibleConversationModels(
  appId: string,
  models: OceanwayModel[],
  defaultModel = "",
): string[] {
  if (!defaultModel) {
    defaultModel = recommendedModel(
      appId,
      conversationModels(
        models.filter(
          (model) =>
            model.supportedProtocols === undefined ||
            model.supportedProtocols.includes(
              conversationProtocol(appId, model.id),
            ),
        ),
      ),
    );
  }
  const protocol = conversationProtocol(appId, defaultModel);
  return conversationModels(
    models.filter(
      (model) =>
        model.supportedProtocols === undefined ||
        model.supportedProtocols.includes(protocol),
    ),
  );
}

// Capability metadata is not consistently supplied by /models. Conservative
// classification keeps media/embedding models out of conversation selectors.
export function isImageModel(id: string): boolean {
  return /(?:gpt-image|dall-e|imagen|flux|stable-diffusion|nano-banana|(?:^|[-_/])image(?:[-_/]|$))|gemini.*image/i.test(
    id,
  );
}
export function isVideoModel(id: string): boolean {
  return /(?:seedance|sora|veo|kling|(?:^|[-_/])video(?:[-_/]|$))/i.test(id);
}
export function conversationModels(models: OceanwayModel[]): string[] {
  return [
    ...new Set(
      models
        .filter((m) => !m.modelType || m.modelType === "text")
        .map((m) => m.id),
    ),
  ]
    .filter(
      (id) =>
        !isImageModel(id) &&
        !isVideoModel(id) &&
        !/embedding|rerank|whisper|tts|transcri|moderation|sora|video|audio|realtime/i.test(
          id,
        ),
    )
    .sort((a, b) => b.localeCompare(a, "en", { numeric: true }));
}
export function imageModels(models: OceanwayModel[]): string[] {
  return [
    ...new Set(
      models
        .filter((m) =>
          m.modelType ? m.modelType === "image" : isImageModel(m.id),
        )
        .map((m) => m.id),
    ),
  ].sort((a, b) => b.localeCompare(a, "en", { numeric: true }));
}

export function recommendedImageModel(models: string[]): string {
  // Prefer the verified Images API models; keep every available model selectable.
  return (
    ["gpt-image-2", "gpt-image-1.5", "gpt-image-1"].find((id) =>
      models.includes(id),
    ) ??
    models.find((id) => !/preview|experimental/i.test(id)) ??
    models[0] ??
    ""
  );
}
export function recommendedModel(appId: string, models: string[]): string {
  const family =
    appId === "codex"
      ? /gpt/i
      : appId.startsWith("claude")
        ? /claude/i
        : appId === "gemini"
          ? /gemini/i
          : appId === "dsh"
            ? /deepseek/i
            : appId === "kimi"
              ? /kimi/i
              : null;
  const preferred = models.filter(
    (id) => !/preview|experimental|latest|mini|nano|haiku/i.test(id),
  );
  // Do not silently select a different family when the expected one is absent.
  return (
    (family
      ? (preferred.find((id) => family.test(id)) ??
        models.find((id) => family.test(id)))
      : (preferred[0] ?? models[0])) ?? ""
  );
}

export function familyModels(appId: string, models: string[]): string[] {
  const family =
    appId === "codex"
      ? /gpt/i
      : appId.startsWith("claude")
        ? /claude/i
        : appId === "gemini"
          ? /gemini/i
          : appId === "dsh"
            ? /deepseek/i
            : appId === "kimi"
              ? /kimi/i
              : null;
  return family ? models.filter((id) => family.test(id)) : models;
}
