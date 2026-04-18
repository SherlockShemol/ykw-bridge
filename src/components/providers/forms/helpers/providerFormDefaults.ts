import type { PricingModelSourceOption } from "../ProviderAdvancedConfig";

export const CLAUDE_DEFAULT_CONFIG = JSON.stringify({ env: {} }, null, 2);

export const normalizePricingSource = (
  value?: string,
): PricingModelSourceOption =>
  value === "request" || value === "response" ? value : "inherit";
