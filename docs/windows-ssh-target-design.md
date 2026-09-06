# Windows-as-SSH-target 支持方案（讨论稿）

背景：Ctrl+W 连接 Windows 主机（如 192.168.1.6，已可 SSH 登录）报 `remote port probe failed`。
根因：SSH bootstrap 全流程（端口探测、安装、凭证、启动、守护检查）全部是 POSIX shell
单行脚本，通过 SSH exec 在目标机执行；Windows 目标机的默认 shell 是 PowerShell，
脚本无法解析。这不只 probe 一处——安装命令下载的是 linux tarball，故无法靠换 shell 绕过。

注意区分边界：本方案解决"**通过 SSH 把 waitagent 装进一台 Windows 目标机**"。
"waitagent 运行在 Windows 上"（阶段 1-8）已完成；"Windows 目标机已手动装好 waitagent
后的直连复用"（`try_reuse_existing_connection`，`remote_host_connect_runtime.rs:159`）
是跨平台的、已可用，不在本方案范围。

## 现有管线解剖（每个远程命令都是 POSIX 单行）

| 步骤 | 代码位置 | 现状 |
|---|---|---|
| 端口探测 | `remote_port_probe.rs:202` `remote_probe_command` | `for p in $(seq 7474 7574); do ss -ltn \| grep …` |
| 版本检查 | `ssh_remote_host_bootstrapper.rs:631` | `command -v waitagent && waitagent --version \| grep -q <ver>` |
| ensure home | :639 | `mkdir -p $HOME/.waitagent` |
| 安装 preflight（代理） | :713-734 | `curl -fsSL --connect-timeout 5 … install.sh` |
| 安装/更新 | :577-587 | `curl -fsSL install.sh \| bash`；install.sh 默认装到 **`/usr/local/bin`**（非 root 时这步用 sudo，sudo 只用于安装这一步） |
| 凭证生成 | :643 | `waitagent --port P --node-key-path K --node-cert-path C __generate-node-credentials`，输出 `WAITAGENT_CREDENTIALS<pin>:<port>` |
| operator 公钥 | `install_operator_public_key`（同文件） | 写入 `$HOME/.waitagent/authorized_operators/` |
| 守护检查 | :692 | `ps -eo args \| grep -F -- …` |
| 启动 | :60-77 | `nohup waitagent --port P --node-id ID --node-key-path K --node-cert-path C __ratatui-node-server >/tmp/waitagent-P.log 2>&1 </dev/null &` + `/dev/tcp` 等待端口就绪 |
| 端点 preflight（inbound 模式） | :751 | nc / python3 / bash+timeout 三选一 TCP 探测 |

所有命令由**控制端生成字符串**，经 SSH exec 送到目标机 shell 执行。输出契约只有三种：
`port=N` / `WAITAGENT_CREDENTIALS…` / 退出码。这为"按目标平台生成不同脚本"提供了清晰的
切分点——业务解析逻辑（`parse_probe_output`、`parse_credentials_output`）完全不用动。

## 方案概览

引入 `RemoteShellKind { Posix, Windows }`，在 connect 流程开头探测一次，后续所有命令
生成器按 kind 出 POSIX 或 PowerShell 版本。输出契约、错误处理、重试、复用路径不变。

### 1. 平台探测（新增，一次 SSH exec）

```
命令: uname -s
- 退出码 0 且输出不是 MSYS/MINGW/CYGWIN 内核名 → Posix（Linux/macOS/WSL 都覆盖）
- 退出码 0 且输出是 MSYS_NT-…/MINGW*_NT-…/CYGWIN_NT-… → Windows（见下）
- 失败/找不到     → Windows
```

风险与缓解：目标机的 `uname` 可能是 Git for Windows / Cygwin 提供的（native sshd +
系统 PATH 含 `C:\Program Files\Git\usr\bin` 是开发机常见配置），此时退出码也是 0，
输出为 `MSYS_NT-…`——必须按 **输出内容** 分类为 Windows（真 POSIX 内核名不会含这些
token）。真实装了 Cygwin/MSYS **sshd** 且默认 shell 为 bash 的 Windows 仍会被归为
Windows 但命令经 bash 传递会被展开破坏，属于已知限制，文档标注；缓解是 connect
runtime 在端口探测失败时用新分类重探测一次并修正缓存（自愈旧缓存）。WSL 不受影响
（`uname` 返回 Linux，正确）。探测结果可选缓存进
`RemoteHostProfile`（新增 `remote_shell` 字段），后续 connect 跳过探测。

Windows 侧执行统一显式调 `powershell -NoProfile -NonInteractive -Command <脚本>`
（或 `-EncodedCommand` + UTF-16LE base64，彻底避开引号地狱），**不依赖目标机 sshd 的
DefaultShell 配置**。

### 2. PowerShell 版命令生成器（与 POSIX 版并列，纯函数可单测）

| 步骤 | PowerShell 版要点 |
|---|---|
| 端口探测 | 遍历 7474..7574，`[Net.NetworkInformation.IPGlobalProperties]::GetIPGlobalProperties().GetActiveTcpListeners()` 按 Port 匹配；输出 `port=N`（契约不变） |
| 版本检查 | 固定安装路径 `$env:USERPROFILE\.waitagent\bin\waitagent.exe` 存在且 `& $exe --version` 匹配（Windows 不依赖 PATH） |
| ensure home | `New-Item -ItemType Directory -Force $env:USERPROFILE\.waitagent` |
| 安装 preflight | `curl.exe -fsSL --connect-timeout 5 <release zip url>`（curl.exe 是 Win10+ 系统自带）；代理 = 在脚本块内 `$env:ALL_PROXY=…; $env:HTTPS_PROXY=…` 后调用 curl.exe |
| 安装/更新 | `curl.exe -fsSL -o tmp.zip <waitagent-<ver>-x86_64-windows.zip>` + `tar.exe -xf tmp.zip -C <installdir>`（tar.exe 即 bsdtar，Win10+ 自带、支持 zip）。装到 **`%LOCALAPPDATA%\Programs\waitagent\`**（与 Windows 本地 irm 安装器的目的地一致，写 `version.txt` 作为版本标记）。该目录即 `/usr/local/bin` 的 Windows 对应物：Linux 是"系统级、单一路径"，Windows 上按用户安装、与本地安装合并为同一路径，免去双份检测；无需管理员 |
| 凭证生成 | 与 POSIX **完全相同**（跑我们自己的 exe 子命令），仅路径换 Windows 形式；exe 的该子命令本身是跨平台的 |
| operator 公钥 | `New-Item -Force authorized_operators 目录` + `Set-Content` 追加公钥（需确认 `operator_auth` 的路径解析在 Windows 下指向 `$env:USERPROFILE\.waitagent\`，应已是 home-based） |
| 守护检查 | `Get-CimInstance Win32_Process -Filter "Name='waitagent.exe'"` 按 CommandLine 匹配 `--port P`/`--node-id ID`/`__ratatui-node-server`（同用户下 CommandLine 可读） |
| 启动 | 见第 3 节（关键差异点） |
| 端点 preflight | PowerShell `New-Object Net.Sockets.TcpClient` + `BeginConnect` 5s 超时 |
| 端口就绪等待 | PowerShell 循环 `TcpClient` 试连 127.0.0.1:P，~10s 上限 |

### 3. 启动与进程存活（本方案最大的技术点）

POSIX 用 `nohup … &` 脱离 SSH 会话。Windows 的对应问题：**OpenSSH for Windows 的
sshd 把每个 exec 会话跑在带 kill-on-close 的 Job Object 里，SSH 断开后子进程会被
一并杀掉**——`Start-Process -WindowStyle Hidden` 不足以存活。

候选：

- **A. 自守护化（推荐）**：给 Windows 的 `__ratatui-node-server` 入口加一个前台/守护
  分叉——检测到 stdin 是 SSH 会话时，用阶段 3 已有的 `spawn_detached`
  （`platform/process.rs`，`CREATE_NEW_PROCESS_GROUP | DETACHED_PROCESS`）**补加
  `CREATE_BREAKAWAY_FROM_JOB`** 重新拉起自身，父进程等端口就绪后退出。效果与
  `nohup &` 对齐，启动命令几乎不变，且复用我们自己的进程设施。需验证/补上
  BREAKAWAY 标志（当前阶段 3 实现未含）。
- B. schtasks 一次性任务：标准 trick、能活，但引入任务计划程序依赖与残留任务，
  错误路径难排查。
- C. 装成 Windows 服务：需要管理员 + SCM 逻辑，最重，本轮不做。

选定 A。日志重定向到 `%TEMP%\waitagent-<port>.log`。

### 4. 改动文件清单（预估）

- `src/host/ssh/remote_port_probe.rs`：`remote_probe_command` 按 kind 分发（PowerShell 版）
- `src/host/ssh/ssh_remote_host_bootstrapper.rs`：全部 `*_command` 生成器按 kind 双实现；
  `ensure_waitagent_and_start` 透传 kind；`install_operator_public_key` Windows 版
- `src/host/ssh/remote_host_connect_runtime.rs`：connect 开头探测 kind（含缓存读写）、
  透传给 probe/bootstrapper
- `src/host/ssh/remote_host_history_store.rs`：profile 增加 `remote_shell` 可选字段
  （TOML 向后兼容）
- `src/platform/process.rs`：`spawn_detached` Windows 分支补 `CREATE_BREAKAWAY_FROM_JOB`
- node server 入口（`__ratatui-node-server`）：Windows 下自守护化分叉 + 端口就绪等待
- `operator_auth`：核对 Windows 下 authorized_operators 目录解析
- 测试：命令生成器纯函数单测（与现有 POSIX 单测同构）；真机 E2E（192.168.1.6）

### 5. 验证计划

1. 每步与既有 POSIX 路径同标准：Linux `cargo test --release`、`clippy -D warnings`、
   `fmt`、windows-gnu check 全绿。
2. 命令生成器单测：PowerShell 版探测/安装/启动脚本的输出契约（`port=N`、
   `WAITAGENT_CREDENTIALS`）与 POSIX 一致。
3. 真机 E2E：控制端（Windows 或 WSL）Ctrl+W → 192.168.1.6（JJ 密码登录）→ 自动
   下载安装 Windows zip → 启动 → 建 session → TUI 出输出、可输入。再测复用路径
   （目标机 waitagent 已在跑时重启控制端，应走 reuse dial 不重装）。

### 6. 讨论结论（2026-09-06 确认）

1. **进程存活**：选 A（自守护化 + `CREATE_BREAKAWAY_FROM_JOB`）。
2. **目标机 SSH 要求**：只支持 Windows 自带的 native OpenSSH（Win32-OpenSSH，可选功能）；
   MSYS/Cygwin **sshd** 为已知不支持，文档标注。native sshd + Git for Windows 在
   PATH（`uname` 回答 `MSYS_NT-…` 但登录 shell 是 cmd）按输出内容归类为 Windows，
   正常使用。探测失败时 connect runtime 会用新分类重探测并修正缓存（自愈）。
3. **安装目录**：`%LOCALAPPDATA%\Programs\waitagent\`（与本地 irm 安装器一致，按用户、免管理员）。
4. **密钥登录**：与密码登录同在验证范围（E2E 两种都测）。
5. **提权等价物**：Linux 上 sudo 仅用于"装到 /usr/local/bin"这一步；Windows 按用户安装
   无需提权，本轮不做"以管理员运行 waitagent"（机器级 `C:\Program Files` 安装留作未来选项）。
6. **无守护监管**：nohup 只免疫 SIGHUP（SSH 断开），不负责崩溃/重启后拉起——Linux 与
   Windows 行为一致：进程死了就是死了，靠下次 connect 的 `daemon_running_check` /
   reuse-dial 失败 → 重新 SSH bootstrap 来恢复。不在本轮引入 supervisor/service。

### 7. 遗留开放问题

- 复用探测结果是否缓存进 profile（`remote_shell` 字段）：建议做，每次 connect 省一次
  SSH exec；TOML 向后兼容。
- Windows 机器重启后 waitagent 不自启（与 Linux 行为一致）。是否需要"注册到用户
  启动项/服务"作为后续增强？本轮不做。

### 8. 任务拆分

每步结束均须：Linux `cargo test --release`、`cargo clippy -- -D warnings`、
`cargo fmt --check` 全绿；涉及 Windows 代码另跑 mingw 交叉 check。

- **T1 平台探测**：`RemoteShellKind { Posix, Windows }`；connect 开头一次
  `uname -s` exec 探测；结果缓存进 `RemoteHostProfile.remote_shell`（TOML 向后兼容）。
- **T2 PowerShell 命令生成器**（核心工作量）：`remote_port_probe.rs` 与
  `ssh_remote_host_bootstrapper.rs` 的全部 `*_command` 生成器按 kind 双实现——
  端口探测、版本检查、ensure home、安装、安装 preflight（含代理 env）、凭证路径、
  operator 公钥、守护检查、端点 preflight、端口就绪等待。输出契约（`port=N` /
  `WAITAGENT_CREDENTIALS` / 退出码）不变；全部纯函数 + 与 POSIX 同构的单测。
- **T3 Windows 安装器**：curl.exe 下载 release zip + tar.exe 解包到
  `%LOCALAPPDATA%\Programs\waitagent\`；`version.txt` 版本标记；`curl.exe`/
  `tar.exe` 缺失时给出明确错误。
- **T4 进程存活**：`platform/process.rs` 的 `spawn_detached` 补
  `CREATE_BREAKAWAY_FROM_JOB`；`__ratatui-node-server` 在 Windows 下自守护化
  （拉起脱离 Job Object 的自身副本，父进程等端口就绪后退出）；日志到
  `%TEMP%\waitagent-<port>.log`。
- **T5 路径核对**：`operator_auth` authorized_operators、`NodeCredentialPaths`
  在 Windows 目标上的解析正确（home-based 路径）。
- **T6 接线**：`remote_host_connect_runtime.rs` 传 kind 给 probe/bootstrapper；
  `ensure_waitagent_and_start` 按 kind 分支；sudo 字段与 MSYS 场景忽略并标注。
- **T7 README**：`Remote Machines` 节增加"Windows 作为连接目标"小节——启用
  OpenSSH Server 的步骤（`Add-WindowsCapability -Online -Name OpenSSH.Server~~~~0.0.1.0`、
  `Start-Service sshd`、防火墙规则、`Set-Service -StartupType Automatic`、密码/密钥
  认证说明）与限制（需 native OpenSSH、按用户免管理员安装、无开机自启）。
- **T8 验证**：全部单测 + mingw check；真机 E2E（192.168.1.6）：密码登录与密钥登录
  各一次完整流程（安装→启动→建 session→输出/输入）；复用路径（目标已装时重启
  控制端应走 reuse dial、不重装）。

依赖：T1 → T2/T3/T4 可并行 → T5/T6 → T7 → T8。
