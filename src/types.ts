export type ProviderCategory =
  | "official" // 官方
  | "cn_official" // 开源官方（原"国产官方"）
  | "cloud_provider" // 云服务商（AWS Bedrock 等）
  | "aggregator" // 聚合网站
  | "third_party" // 第三方供应商
  | "custom"; // 自定义

export interface Provider {
  id: string;
  name: string;
  settingsConfig: Record<string, any>; // 供应商配置对象；当前 Claude 工作流使用 settings.json 结构，历史数据可能保留旧格式
  websiteUrl?: string;
  // 新增：供应商分类（用于差异化提示/能力开关）
  category?: ProviderCategory;
  createdAt?: number; // 添加时间戳（毫秒）
  sortIndex?: number; // 排序索引（用于自定义拖拽排序）
  // 备注信息
  notes?: string;
  // 新增：是否为商业合作伙伴
  isPartner?: boolean;
  // 可选：供应商元数据（仅存于 ~/.ykw-bridge/config.json，不写入 live 配置）
  meta?: ProviderMeta;
  // 图标配置
  icon?: string; // 图标名称（如 "openai", "anthropic"）
  iconColor?: string; // 图标颜色（Hex 格式，如 "#00A67E"）
  // 是否加入故障转移队列
  inFailoverQueue?: boolean;
}

export interface AppConfig {
  providers: Record<string, Provider>;
  current: string;
}

// 自定义端点配置
export interface CustomEndpoint {
  url: string;
  addedAt: number;
  lastUsed?: number;
}

// 端点候选项（用于端点测速弹窗）
export interface EndpointCandidate {
  id?: string;
  url: string;
  isCustom?: boolean;
}

import type { TemplateType } from "./config/constants";

// 用量查询脚本配置
export interface UsageScript {
  enabled: boolean; // 是否启用用量查询
  language: "javascript"; // 脚本语言
  code: string; // 脚本代码（JSON 格式配置）
  timeout?: number; // 超时时间（秒，默认 10）
  templateType?: TemplateType; // 模板类型（用于后端判断验证规则）
  apiKey?: string; // 用量查询专用的 API Key（通用模板使用）
  baseUrl?: string; // 用量查询专用的 Base URL（通用和 NewAPI 模板使用）
  accessToken?: string; // 访问令牌（NewAPI 模板使用）
  userId?: string; // 用户ID（NewAPI 模板使用）
  codingPlanProvider?: string; // Coding Plan 供应商标识（如 "kimi", "zhipu", "minimax"）
  autoQueryInterval?: number; // 自动查询间隔（单位：分钟，0 表示禁用）
  autoIntervalMinutes?: number; // 自动查询间隔（分钟）- 别名字段
  request?: {
    // 请求配置
    url?: string; // 请求 URL
    method?: string; // HTTP 方法
    headers?: Record<string, string>; // 请求头
    body?: any; // 请求体
  };
}

// 单个套餐用量数据
export interface UsageData {
  planName?: string; // 套餐名称（可选）
  extra?: string; // 扩展字段，可自由补充需要展示的文本（可选）
  isValid?: boolean; // 套餐是否有效（可选）
  invalidMessage?: string; // 失效原因说明（可选，当 isValid 为 false 时显示）
  total?: number; // 总额度（可选）
  used?: number; // 已用额度（可选）
  remaining?: number; // 剩余额度（可选）
  unit?: string; // 单位（可选）
}

// 用量查询结果（支持多套餐）
export interface UsageResult {
  success: boolean;
  data?: UsageData[]; // 改为数组，支持返回多个套餐
  error?: string;
}

// 供应商单独的模型测试配置
export interface ProviderTestConfig {
  // 是否启用单独配置（false 时使用全局配置）
  enabled: boolean;
  // 测试用的模型名称（覆盖全局配置）
  testModel?: string;
  // 超时时间（秒）
  timeoutSecs?: number;
  // 测试提示词
  testPrompt?: string;
  // 降级阈值（毫秒）
  degradedThresholdMs?: number;
  // 最大重试次数
  maxRetries?: number;
}

export type AuthBindingSource = "provider_config" | "managed_account";

export interface AuthBinding {
  source: AuthBindingSource;
  authProvider?: string;
  accountId?: string;
}

// 供应商元数据（字段名与后端一致，保持 snake_case）
export interface ProviderMeta {
  // 自定义端点：以 URL 为键，值为端点信息
  custom_endpoints?: Record<string, CustomEndpoint>;
  // 是否在切换/同步到 live 时应用通用配置片段
  commonConfigEnabled?: boolean;
  // 用量查询脚本配置
  usage_script?: UsageScript;
  // 请求地址管理：测速后自动选择最佳端点
  endpointAutoSelect?: boolean;
  // 是否为官方合作伙伴
  isPartner?: boolean;
  // 合作伙伴促销 key（用于后端识别 PackyCode 等）
  partnerPromotionKey?: string;
  // 供应商单独的模型测试配置
  testConfig?: ProviderTestConfig;
  // 供应商成本倍率
  costMultiplier?: string;
  // 供应商计费模式来源
  pricingModelSource?: string;
  // Claude API 格式（仅 Claude 供应商使用）
  // - "anthropic": 原生 Anthropic Messages API 格式，直接透传
  // - "openai_chat": OpenAI Chat Completions 格式，需要格式转换
  // - "openai_responses": OpenAI Responses API 格式，需要格式转换
  apiFormat?: "anthropic" | "openai_chat" | "openai_responses";
  // 通用认证绑定
  authBinding?: AuthBinding;
  // Claude 认证字段名
  apiKeyField?: ClaudeApiKeyField;
  // 是否将 base_url 视为完整 API 端点（代理直接使用此 URL，不拼接路径）
  isFullUrl?: boolean;
  // Prompt cache key for OpenAI Responses-compatible endpoints (improves cache hit rate)
  promptCacheKey?: string;
  // 供应商类型（用于识别 Copilot 等特殊供应商）
  providerType?: string;
  // GitHub Copilot 关联账号 ID（旧字段，保留兼容读取）
  githubAccountId?: string;
}

// Skill 同步方式
export type SkillSyncMethod = "auto" | "symlink" | "copy";

// Skill 存储位置
export type SkillStorageLocation = "cc_switch" | "unified";

// Claude API 格式类型
// - "anthropic": 原生 Anthropic Messages API 格式，直接透传
// - "openai_chat": OpenAI Chat Completions 格式，需要格式转换
// - "openai_responses": OpenAI Responses API 格式，需要格式转换
export type ClaudeApiFormat = "anthropic" | "openai_chat" | "openai_responses";

// Claude 认证字段类型
export type ClaudeApiKeyField = "ANTHROPIC_AUTH_TOKEN" | "ANTHROPIC_API_KEY";

// 主页面显示的应用配置
export interface VisibleApps {
  claude: boolean;
  claude_desktop: boolean;
}

// WebDAV 同步状态
export interface WebDavSyncStatus {
  lastSyncAt?: number | null;
  lastError?: string | null;
  lastErrorSource?: string | null;
  lastRemoteEtag?: string | null;
  lastLocalManifestHash?: string | null;
  lastRemoteManifestHash?: string | null;
}

// WebDAV 同步配置
export interface WebDavSyncSettings {
  enabled?: boolean;
  autoSync?: boolean;
  baseUrl?: string;
  username?: string;
  password?: string;
  remoteRoot?: string;
  profile?: string;
  status?: WebDavSyncStatus;
}

export type RemoteSnapshotLayout = "current" | "legacy";

// 远端快照信息（下载前预览）
export interface RemoteSnapshotInfo {
  deviceName: string;
  createdAt: string;
  snapshotId: string;
  version: number;
  protocolVersion: number;
  dbCompatVersion?: number | null;
  compatible: boolean;
  artifacts: string[];
  layout: RemoteSnapshotLayout;
  remotePath: string;
}

// 应用设置类型（用于设置对话框与 Tauri API）
// 存储在本地 ~/.ykw-bridge/settings.json，不随数据库同步
export interface Settings {
  // ===== 设备级 UI 设置 =====
  // 是否在系统托盘（macOS 菜单栏）显示图标
  showInTray: boolean;
  // 点击关闭按钮时是否最小化到托盘而不是关闭应用
  minimizeToTrayOnClose: boolean;
  // 是否启用应用级窗口控制按钮（最小化/最大化/关闭）
  useAppWindowControls?: boolean;
  // 启用 Claude 插件联动（写入 ~/.claude/config.json 的 primaryApiKey）
  enableClaudePluginIntegration?: boolean;
  // 跳过 Claude Code 初次安装确认（写入 ~/.claude.json 的 hasCompletedOnboarding）
  skipClaudeOnboarding?: boolean;
  // 是否开机自启
  launchOnStartup?: boolean;
  // 静默启动（程序启动时不显示主窗口）
  silentStartup?: boolean;
  // 是否启用主页面本地代理功能（默认关闭）
  enableLocalProxy?: boolean;
  // User has confirmed the local proxy first-run notice
  proxyConfirmed?: boolean;
  // User has confirmed the usage query first-run notice
  usageConfirmed?: boolean;
  // User has confirmed the stream check first-run notice
  streamCheckConfirmed?: boolean;
  // Whether to show the failover toggle independently on the main page
  enableFailoverToggle?: boolean;
  // User has confirmed the failover toggle first-run notice
  failoverConfirmed?: boolean;
  // User has confirmed the first-run welcome notice
  firstRunNoticeConfirmed?: boolean;
  // User has confirmed the auto-sync traffic warning
  autoSyncConfirmed?: boolean;
  // User has confirmed the common config first-run notice
  commonConfigConfirmed?: boolean;
  // 首选语言（可选，默认中文）
  language?: "en" | "zh" | "ja";

  // 主页面显示的应用（默认全部显示）
  visibleApps?: VisibleApps;

  // ===== 设备级目录覆盖 =====
  // 覆盖 Claude Code 配置目录（可选）
  claudeConfigDir?: string;
  // Claude Desktop 官方 App 路径（可选）
  claudeDesktopAppPath?: string;
  // Claude Desktop 受管 profile 目录（可选）
  claudeDesktopProfileDir?: string;
  // 当 ykw-bridge 运行时，拦截直接启动的 Claude Desktop 并改为受管 3P 启动
  claudeDesktopLaunchWatchdogEnabled?: boolean;

  // ===== 当前供应商 ID（设备级）=====
  // 当前 Claude 供应商 ID（优先于数据库 is_current）
  currentProviderClaude?: string;
  // 兼容旧字段：Claude Desktop 现与 Claude 共用当前供应商
  currentProviderClaudeDesktop?: string;

  // ===== Skill 同步设置 =====
  // Skill 同步方式：auto（默认，优先 symlink）、symlink、copy
  skillSyncMethod?: SkillSyncMethod;
  // Skill 存储位置：cc_switch（默认）或 unified（~/.agents/skills/）
  skillStorageLocation?: SkillStorageLocation;

  // ===== WebDAV v2 同步设置 =====
  webdavSync?: WebDavSyncSettings;

  // ===== 备份策略设置 =====
  // Auto-backup interval in hours (0=disabled, default 24)
  backupIntervalHours?: number;
  // Maximum backup files to retain (default 10)
  backupRetainCount?: number;

  // ===== 终端设置 =====
  // 首选终端应用（可选，默认使用系统默认终端）
  // macOS: "terminal" | "iterm2" | "warp" | "alacritty" | "kitty" | "ghostty" | "wezterm" | "kaku"
  // Windows: "cmd" | "powershell" | "wt"
  // Linux: "gnome-terminal" | "konsole" | "xfce4-terminal" | "alacritty" | "kitty" | "ghostty"
  preferredTerminal?: string;
}

export interface SessionMeta {
  providerId: string;
  sessionId: string;
  title?: string;
  summary?: string;
  projectDir?: string | null;
  createdAt?: number;
  lastActiveAt?: number;
  sourcePath?: string;
  resumeCommand?: string;
}

export interface SessionMessage {
  role: string;
  content: string;
  ts?: number;
}

// MCP 服务器连接参数（宽松：允许扩展字段）
export interface McpServerSpec {
  // 可选：社区常见 .mcp.json 中 stdio 配置可不写 type
  type?: "stdio" | "http" | "sse";
  // stdio 字段
  command?: string;
  args?: string[];
  env?: Record<string, string>;
  cwd?: string;
  // http 和 sse 字段
  url?: string;
  headers?: Record<string, string>;
  // 通用字段
  [key: string]: any;
}

// v3.7.0: MCP 服务器应用启用状态
export interface McpApps {
  claude: boolean;
}

// MCP 服务器条目（v3.7.0 统一结构）
export interface McpServer {
  id: string;
  name: string;
  server: McpServerSpec;
  apps: McpApps; // v3.7.0: 标记应用到哪些客户端
  description?: string;
  tags?: string[];
  homepage?: string;
  docs?: string;
  // 兼容旧字段（v3.6.x 及以前）
  enabled?: boolean; // 已废弃，v3.7.0 使用 apps 字段
  source?: string;
  [key: string]: any;
}

// MCP 服务器映射（id -> McpServer）
export type McpServersMap = Record<string, McpServer>;

// MCP 配置状态
export interface McpStatus {
  userConfigPath: string;
  userConfigExists: boolean;
  serverCount: number;
}

// 新：来自 config.json 的 MCP 列表响应
export interface McpConfigResponse {
  configPath: string;
  servers: Record<string, McpServer>;
}
