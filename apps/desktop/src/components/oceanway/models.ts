import type { OceanwayModel } from "@/lib/api/oceanway";

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
export function videoModels(models: OceanwayModel[]): string[] {
  return [...new Set(models.map((m) => m.id).filter(isVideoModel))].sort(
    (a, b) => b.localeCompare(a, "en", { numeric: true }),
  );
}
export function conversationModels(models: OceanwayModel[]): string[] {
  return [...new Set(models.map((m) => m.id))]
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
  return [...new Set(models.map((m) => m.id).filter(isImageModel))].sort(
    (a, b) => b.localeCompare(a, "en", { numeric: true }),
  );
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
          : null;
  return family ? models.filter((id) => family.test(id)) : models;
}
