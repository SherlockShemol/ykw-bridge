import { cn } from "@/lib/utils";
import { ProviderHealthStatus } from "@/types/proxy";
import { useTranslation } from "react-i18next";

interface ProviderHealthBadgeProps {
  consecutiveFailures: number;
  className?: string;
}

/**
 * 供应商健康状态徽章
 * 根据连续失败次数显示不同颜色的状态指示器
 */
export function ProviderHealthBadge({
  consecutiveFailures,
  className,
}: ProviderHealthBadgeProps) {
  const { t } = useTranslation();

  const getStatus = () => {
    if (consecutiveFailures === 0) {
      return {
        labelKey: "health.operational",
        labelFallback: "正常",
        status: ProviderHealthStatus.Healthy,
        dotColor: "bg-sky-500",
        bgColor: "bg-sky-50 dark:bg-sky-950/30",
        textColor: "text-sky-700 dark:text-sky-300",
      };
    } else if (consecutiveFailures < 5) {
      return {
        labelKey: "health.degraded",
        labelFallback: "降级",
        status: ProviderHealthStatus.Degraded,
        dotColor: "bg-amber-500",
        bgColor: "bg-amber-50 dark:bg-amber-950/30",
        textColor: "text-amber-700 dark:text-amber-300",
      };
    }

    return {
      labelKey: "health.circuitOpen",
      labelFallback: "熔断",
      status: ProviderHealthStatus.Failed,
      dotColor: "bg-red-500",
      bgColor: "bg-red-50 dark:bg-red-950/30",
      textColor: "text-red-700 dark:text-red-300",
    };
  };

  const statusConfig = getStatus();
  const label = t(statusConfig.labelKey, {
    defaultValue: statusConfig.labelFallback,
  });

  return (
    <div
      className={cn(
        "inline-flex items-center gap-1.5 px-2 py-1 rounded-full text-xs font-medium",
        statusConfig.bgColor,
        statusConfig.textColor,
        className,
      )}
      title={t("health.consecutiveFailures", {
        count: consecutiveFailures,
        defaultValue: `连续失败 ${consecutiveFailures} 次`,
      })}
    >
      <div className={cn("w-2 h-2 rounded-full", statusConfig.dotColor)} />
      <span>{label}</span>
    </div>
  );
}
