# Claude Desktop Managed Workflow (macOS)

YKW Bridge treats Claude Desktop as a companion surface of the Claude workflow on macOS, not as a separate extra app.

## Scope

- macOS only
- official `/Applications/Claude.app`
- managed profile under `~/.ykw-bridge/claude-desktop/profile`
- local HTTPS gateway powered by the built-in Rust proxy
- provider reuse, launch, doctor, certificate install, and desktop-side fallback mapping

## Current Boundaries

- no patching of the official Claude Desktop bundle
- no dependence on a global system proxy
- local gateway and certificate setup must still be explicitly accepted by the user

## How It Works

YKW Bridge writes a managed `claude_desktop_config.json` with:

- `enterpriseConfig.inferenceProvider = "gateway"`
- `enterpriseConfig.inferenceGatewayBaseUrl = https://127.0.0.1:<proxy_port+1>/claude-desktop`
- `enterpriseConfig.inferenceGatewayApiKey = <local secret>`
- `enterpriseConfig.fallbackModels = <desktop model mapping>`

Claude Desktop is launched with:

- `-3p`
- `CLAUDE_USER_DATA_DIR=<managed profile dir>`

## Requirements

Before enabling Claude Desktop takeover or launch:

1. Claude.app must exist.
2. The local HTTPS certificate must be installed explicitly by the user.
3. Proxy HTTP/HTTPS ports must be available.
4. The gateway health check must pass.

## Notes

- The managed profile and gateway files live under YKW Bridge's own data directory.
- If Anthropic changes the hidden 3P gateway behavior, takeover behavior may need to be adjusted.
- Treat this document as an implementation note for the current Claude Desktop companion flow.
