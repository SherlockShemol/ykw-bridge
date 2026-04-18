import { useCallback, useEffect, useMemo, useState } from "react";
import { useTranslation } from "react-i18next";
import { toast } from "sonner";
import {
  Beaker,
  CheckCircle2,
  AlertTriangle,
  Play,
  Link2,
  Shield,
} from "lucide-react";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { ToggleRow } from "@/components/ui/toggle-row";
import { settingsApi } from "@/lib/api";
import type {
  ClaudeDesktopDoctor,
  ClaudeDesktopStatus,
} from "@/lib/api/settings";
import type { SettingsFormState } from "@/hooks/useSettings";
import { isLinux, isWindows } from "@/lib/platform";

interface ClaudeDesktopSettingsProps {
  settings: SettingsFormState;
  onChange: (updates: Partial<SettingsFormState>) => void;
}

function getErrorMessage(error: unknown, fallback: string): string {
  if (error instanceof Error && error.message) {
    return error.message;
  }
  if (typeof error === "string" && error.trim().length > 0) {
    return error;
  }
  return fallback;
}

export function ClaudeDesktopSettings({
  settings,
  onChange,
}: ClaudeDesktopSettingsProps) {
  const { t } = useTranslation();
  const [status, setStatus] = useState<ClaudeDesktopStatus | null>(null);
  const [doctor, setDoctor] = useState<ClaudeDesktopDoctor | null>(null);
  const [busyAction, setBusyAction] = useState<
    "refresh" | "doctor" | "install" | "launch" | null
  >(null);

  const unsupported = isWindows() || isLinux();

  const refreshStatus = useCallback(async () => {
    setBusyAction("refresh");
    try {
      const next = await settingsApi.getClaudeDesktopStatus();
      setStatus(next);
    } catch (error) {
      console.error("[ClaudeDesktopSettings] Failed to load status", error);
    } finally {
      setBusyAction(null);
    }
  }, []);

  useEffect(() => {
    if (!unsupported) {
      void refreshStatus();
    }
  }, [refreshStatus, unsupported]);

  const runDoctor = useCallback(async () => {
    setBusyAction("doctor");
    try {
      const next = await settingsApi.doctorClaudeDesktop();
      setDoctor(next);
      setStatus(next.status);
      if (next.blockers.length === 0) {
        toast.success(
          t("settings.claudeDesktop.doctorOk", {
            defaultValue: "Claude Desktop 已准备就绪",
          }),
        );
      } else {
        toast.error(next.blockers.join("；"));
      }
    } catch (error) {
      console.error("[ClaudeDesktopSettings] Failed to run doctor", error);
      toast.error(
        t("settings.claudeDesktop.doctorFailed", {
          defaultValue: "Claude Desktop 检查失败",
        }),
      );
    } finally {
      setBusyAction(null);
    }
  }, [t]);

  const installCertificate = useCallback(async () => {
    setBusyAction("install");
    try {
      const next = await settingsApi.installClaudeDesktopCertificate();
      setStatus(next);
      toast.success(
        t("settings.claudeDesktop.certificateInstalled", {
          defaultValue: "证书已安装",
        }),
      );
    } catch (error) {
      console.error(
        "[ClaudeDesktopSettings] Failed to install certificate",
        error,
      );
      toast.error(getErrorMessage(error, "Install certificate failed"));
    } finally {
      setBusyAction(null);
    }
  }, [t]);

  const launch = useCallback(async () => {
    setBusyAction("launch");
    try {
      const next = await settingsApi.launchClaudeDesktop();
      setStatus(next);
      toast.success(
        t("settings.claudeDesktop.launched", {
          defaultValue: "Claude Desktop 已启动",
        }),
      );
    } catch (error) {
      console.error("[ClaudeDesktopSettings] Failed to launch", error);
      toast.error(getErrorMessage(error, "Launch failed"));
    } finally {
      setBusyAction(null);
    }
  }, [t]);

  const blockers = useMemo(() => doctor?.blockers ?? [], [doctor]);

  if (unsupported) {
    return null;
  }

  return (
    <section className="space-y-4 rounded-lg border border-border-default p-4">
      <header className="space-y-1">
        <div className="flex items-center gap-2">
          <h3 className="text-sm font-medium">
            {t("settings.claudeDesktop.title", {
              defaultValue: "Claude Desktop",
            })}
          </h3>
          <span className="rounded bg-amber-500/10 px-2 py-0.5 text-[11px] font-medium text-amber-700 dark:text-amber-300">
            Experimental
          </span>
        </div>
        <p className="text-xs text-muted-foreground">
          {t("settings.claudeDesktop.description", {
            defaultValue:
              "Use the official Claude.app with a managed local gateway profile.",
          })}
        </p>
      </header>

      <ToggleRow
        icon={<Link2 className="h-4 w-4 text-sky-500" />}
        title={t("settings.claudeDesktop.launchIntercept", {
          defaultValue: "Intercept direct Claude launches",
        })}
        description={t("settings.claudeDesktop.launchInterceptDescription", {
          defaultValue:
            "Keep ykw-bridge running and normal Claude launches will be reopened with the managed profile.",
        })}
        checked={!!settings.claudeDesktopLaunchWatchdogEnabled}
        onCheckedChange={(value) =>
          onChange({ claudeDesktopLaunchWatchdogEnabled: value })
        }
      />

      <div className="grid gap-3 md:grid-cols-2">
        <div className="space-y-1.5">
          <Label htmlFor="claude-desktop-app-path">
            {t("settings.claudeDesktop.appPath", { defaultValue: "App path" })}
          </Label>
          <Input
            id="claude-desktop-app-path"
            value={settings.claudeDesktopAppPath ?? status?.appPath ?? ""}
            onChange={(event) =>
              onChange({
                claudeDesktopAppPath: event.target.value.trim() || undefined,
              })
            }
            placeholder="/Applications/Claude.app"
          />
        </div>

        <div className="space-y-1.5">
          <Label htmlFor="claude-desktop-profile-dir">
            {t("settings.claudeDesktop.profileDir", {
              defaultValue: "Managed profile",
            })}
          </Label>
          <Input
            id="claude-desktop-profile-dir"
            value={settings.claudeDesktopProfileDir ?? status?.profileDir ?? ""}
            onChange={(event) =>
              onChange({
                claudeDesktopProfileDir: event.target.value.trim() || undefined,
              })
            }
            placeholder="~/.ykw-bridge/claude-desktop/profile"
          />
        </div>
      </div>

      <div className="grid gap-2 text-xs text-muted-foreground md:grid-cols-2">
        <StatusRow
          ok={!!status?.appExists && !!status?.binaryExists}
          label={t("settings.claudeDesktop.appDetected", {
            defaultValue: "Claude.app detected",
          })}
        />
        <StatusRow
          ok={!!status?.certificateInstalled}
          label={t("settings.claudeDesktop.certificateStatus", {
            defaultValue: "Certificate installed",
          })}
        />
        <StatusRow
          ok={!!status?.managedConfigExists}
          label={t("settings.claudeDesktop.profileStatus", {
            defaultValue: "Managed profile written",
          })}
        />
        <StatusRow
          ok={!!settings.claudeDesktopLaunchWatchdogEnabled}
          label={t("settings.claudeDesktop.launchShimStatus", {
            defaultValue: "Direct launches use the managed profile",
          })}
        />
        <StatusRow
          ok={doctor ? doctor.gatewayHealthy : !!status?.proxyRunning}
          label={t("settings.claudeDesktop.gatewayStatus", {
            defaultValue: "Gateway healthy",
          })}
        />
      </div>

      {status?.gatewayBaseUrl ? (
        <p className="text-xs text-muted-foreground break-all">
          Gateway: {status.gatewayBaseUrl}
        </p>
      ) : null}

      <p className="text-xs text-muted-foreground">
        {settings.claudeDesktopLaunchWatchdogEnabled
          ? t("settings.claudeDesktop.launchInterceptHelp", {
              defaultValue:
                "Keep ykw-bridge running in the background. Direct Claude launches may flash once while they are reopened in the managed profile.",
            })
          : t("settings.claudeDesktop.launchShimUnsupportedHelp", {
              defaultValue:
                "Turn on launch interception above, or launch Claude Desktop from here.",
            })}
      </p>

      {blockers.length > 0 ? (
        <div className="rounded-md border border-amber-500/20 bg-amber-500/5 p-3 text-xs text-amber-800 dark:text-amber-200">
          <div className="mb-1 font-medium">
            {t("settings.claudeDesktop.blockers", {
              defaultValue: "Blocking issues",
            })}
          </div>
          <ul className="space-y-1">
            {blockers.map((item) => (
              <li key={item}>{item}</li>
            ))}
          </ul>
        </div>
      ) : null}

      <div className="flex flex-wrap gap-2">
        <Button
          type="button"
          variant="outline"
          size="sm"
          onClick={() => void refreshStatus()}
          disabled={busyAction !== null}
        >
          <Beaker className="mr-2 h-4 w-4" />
          {t("common.refresh", { defaultValue: "Refresh" })}
        </Button>
        <Button
          type="button"
          variant="outline"
          size="sm"
          onClick={() => void runDoctor()}
          disabled={busyAction !== null}
        >
          <AlertTriangle className="mr-2 h-4 w-4" />
          Doctor
        </Button>
        <Button
          type="button"
          variant="outline"
          size="sm"
          onClick={() => void installCertificate()}
          disabled={busyAction !== null}
        >
          <Shield className="mr-2 h-4 w-4" />
          {t("settings.claudeDesktop.installCertificate", {
            defaultValue: "Install Certificate",
          })}
        </Button>
        <Button
          type="button"
          size="sm"
          onClick={() => void launch()}
          disabled={busyAction !== null}
        >
          <Play className="mr-2 h-4 w-4" />
          {t("settings.claudeDesktop.launch", { defaultValue: "Launch" })}
        </Button>
      </div>
    </section>
  );
}

function StatusRow({ ok, label }: { ok: boolean; label: string }) {
  return (
    <div className="flex items-center gap-2">
      {ok ? (
        <CheckCircle2 className="h-4 w-4 text-emerald-500" />
      ) : (
        <AlertTriangle className="h-4 w-4 text-amber-500" />
      )}
      <span>{label}</span>
    </div>
  );
}
