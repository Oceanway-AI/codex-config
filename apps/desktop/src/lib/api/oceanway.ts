import { invoke } from "@tauri-apps/api/core";

export interface ConnectionSummary {
  configured: boolean;
  keyPreview: string | null;
  baseUrl: string;
  configuredApps: string[];
  defaultImageModel: string | null;
  allowedImageModels?: string[];
  appModels?: Record<string, { model: string; models: string[] }>;
}
export interface OceanwayModel {
  id: string;
  supportedProtocols?: string[];
  modelType?: string;
}
export interface ModelCatalog {
  baseUrl: string;
  models: OceanwayModel[];
  checkedAt: string;
}
export interface DetectedApp {
  appId: string;
  name: string;
  installed: boolean;
  desktopInstalled?: boolean;
  configExists: boolean;
  configurable: boolean;
  installAvailable?: boolean;
  reason?: string;
}
export interface ApplyRequest {
  apiKey: string;
  apps: { appId: string; model: string; models?: string[] }[];
  imageModel?: string;
  allowedImageModels?: string[];
}
export interface ApplyResult {
  results: {
    appId: string;
    ok: boolean;
    message: string;
    imageConfigured?: boolean;
    videoConfigured?: boolean;
    imageMessage?: string | null;
  }[];
  restartRequired: boolean;
  backupPath: string;
  imagesConfigured: boolean;
  videosConfigured?: boolean;
}
export interface LaunchResult {
  launched: boolean;
  restartRequired: boolean;
  message: string;
}

export const oceanwayApi = {
  connection: () => invoke<ConnectionSummary>("oceanway_get_connection"),
  checkKey: (apiKey: string) =>
    invoke<ModelCatalog>("oceanway_check_key", { apiKey }),
  detectApps: () => invoke<DetectedApp[]>("oceanway_detect_apps"),
  apply: (request: ApplyRequest) =>
    invoke<ApplyResult>("oceanway_apply_config", { request }),
  launch: (appId: string, restart: boolean) =>
    invoke<LaunchResult>("oceanway_launch_app", { appId, restart }),
};
