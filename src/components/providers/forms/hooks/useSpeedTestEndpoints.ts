import { useMemo } from "react";
import type { AppId } from "@/lib/api";
import type { ProviderPreset } from "@/config/claudeProviderPresets";
import type { ProviderMeta, EndpointCandidate } from "@/types";

type PresetEntry = {
  id: string;
  preset: ProviderPreset;
};

interface UseSpeedTestEndpointsProps {
  appId: AppId;
  selectedPresetId: string | null;
  presetEntries: PresetEntry[];
  baseUrl: string;
  initialData?: {
    settingsConfig?: Record<string, unknown>;
    meta?: ProviderMeta;
  };
}

/**
 * 收集端点测速弹窗的初始端点列表
 *
 * 收集来源：
 * 1. 当前选中的 Base URL
 * 2. 编辑模式下的初始数据 URL
 * 3. 预设中的 endpointCandidates
 *
 * 注意：已保存的自定义端点通过 getCustomEndpoints API 在 EndpointSpeedTest 组件中加载，
 * 不在此处读取，避免重复导入。
 */
export function useSpeedTestEndpoints({
  appId,
  selectedPresetId,
  presetEntries,
  baseUrl,
  initialData,
}: UseSpeedTestEndpointsProps) {
  return useMemo<EndpointCandidate[]>(() => {
    if (appId !== "claude" && appId !== "claude_desktop") {
      return [];
    }

    const map = new Map<string, EndpointCandidate>();
    // 候选端点标记为 isCustom: false，表示来自预设或配置
    // 已保存的自定义端点会在 EndpointSpeedTest 组件中通过 API 加载
    const add = (url?: string, isCustom = false) => {
      if (!url) return;
      const sanitized = url.trim().replace(/\/+$/, "");
      if (!sanitized || map.has(sanitized)) return;
      map.set(sanitized, { url: sanitized, isCustom });
    };

    // 1. 当前 Base URL
    if (baseUrl) {
      add(baseUrl);
    }

    // 2. 编辑模式：初始数据中的 URL
    if (initialData && typeof initialData.settingsConfig === "object") {
      const configEnv = initialData.settingsConfig as {
        env?: { ANTHROPIC_BASE_URL?: string; GOOGLE_GEMINI_BASE_URL?: string };
      };
      const envUrls = [
        configEnv.env?.ANTHROPIC_BASE_URL,
        configEnv.env?.GOOGLE_GEMINI_BASE_URL,
      ];
      envUrls.forEach((u) => {
        if (typeof u === "string") add(u);
      });
    }

    // 3. 预设中的 endpointCandidates
    if (selectedPresetId && selectedPresetId !== "custom") {
      const entry = presetEntries.find((item) => item.id === selectedPresetId);
      if (entry) {
        const preset = entry.preset;
        const presetEnv = preset.settingsConfig as {
          env?: {
            ANTHROPIC_BASE_URL?: string;
          };
        };
        add(presetEnv?.env?.ANTHROPIC_BASE_URL);
        // 添加预设的候选端点
        if (preset.endpointCandidates) {
          preset.endpointCandidates.forEach((url) => add(url));
        }
      }
    }

    return Array.from(map.values());
  }, [appId, baseUrl, initialData, selectedPresetId, presetEntries]);
}
