# 云效流水线实时监控（YunxiaoFlowMonitor）

基于 **Tauri v2 + Vue 3 + TypeScript + Rust** 的桌面工具，用于实时查看阿里云云效（Alibaba Cloud DevOps）流水线 Flow 的运行状态与构建日志，并支持运行控制。

界面为两栏布局：左侧是流水线树（每条流水线可展开最近 3 次运行），右侧是运行详情（阶段 → 任务拓扑）与实时日志面板。

| 层 | 选型 |
|---|---|
| 桌面框架 | [Tauri v2](https://tauri.app/) |
| 前端 | Vue 3 + TypeScript + Vite |
| 后端 | Rust（tokio / reqwest / futures） |
| 接口 | 阿里云云效 OpenAPI（Flow，中心版） |
| 平台 | Windows 优先（NSIS 安装包），Linux 构建脚本可用 |

---

## 一、功能清单

### 只读监控
- **流水线总览**：列表分页（云效上限 30 条/页）、按名称搜索、按状态筛选（全部 / 运行中 / 成功 / 失败 / 已取消 / 等待中）
- **实时状态**：后台轮询线程统一调度，状态变化通过 Tauri 事件推送到前端，前端不各自发请求
- **运行历史（内嵌）**：作为流水线的子项展示，默认折叠；展开后只列最近 3 次运行（状态、触发方式、耗时），点击流水线本身则直接展示它最近一次运行的详情
- **运行详情**：阶段（Stage）→ 任务（Job）拓扑，逐节点状态、起止时间、耗时；运行中实例每 5 秒自动刷新
- **实时日志**：按步骤增量拉取，任务运行中每 2 秒追加；可自动跟随正在执行的步骤（步骤 1 结束后自动切到步骤 2）；手动上翻会暂停「跟随尾部」，滚回底部自动恢复
- **日志渲染**：完整解析 ANSI SGR 颜色码（基本色 / 亮色 / 256 色 / 真彩 / 粗体 / 下划线等），并按级别（ERROR / WARN / INFO / DEBUG / SUCCESS）分级配色
- **日志操作**：关键词过滤、行号、折行（默认开启）、平滑滚动跟随、复制纯文本、下载 `.log` 文件、获取官方日志下载地址
- **内存保护**：单步骤日志最多保留 20000 行，超出丢弃头部

### 运行控制（写操作）
| 界面操作 | 对应云效 OpenAPI |
|---|---|
| 运行流水线（支持传 JSON 参数） | `CreatePipelineRun` |
| 终止运行（二次确认） | `UpdatePipelineRun` |
| 重跑任务 | `ExecutePipelineJobRun` |
| 终止任务 | `StopPipelineJobRun` |
| 跳过任务 | `SkipPipelineJobRun` |

> 运行流水线的入口在右侧详情区头部；该流水线还没有运行记录时，空状态里也会提供「运行流水线」按钮。

### 桌面集成
- **系统托盘常驻**：托盘提示实时显示「运行中 / 成功 / 失败」计数；左键单击唤出主窗口
- **托盘菜单**：显示主界面 / 立即刷新 / 开始·暂停轮询 / 退出
- **关闭即最小化**：点窗口关闭按钮默认收进托盘继续监控（可在设置中改为直接退出）
- **桌面通知**：流水线状态跃迁（运行中 → 成功 / 失败 / 取消）时弹系统通知，可分别开关
- **轮询自适应**：无运行中实例时用常规间隔（默认 10s），一旦有实例在跑自动切到加速间隔（默认 3s）
- **开机自启**：设置面板一键开关。Windows 下写入当前用户的 `HKCU\Software\Microsoft\Windows\CurrentVersion\Run`，无需管理员权限；Linux/macOS 走对应平台的登录启动项

---

## 二、云效接口对接说明

**接入版本：中心版（公共云）**，路径形如：

```
https://{domain}/oapi/v1/flow/organizations/{organizationId}/...
```

鉴权统一使用请求头 `x-yunxiao-token: <个人访问令牌>`。

实际调用的接口：

| 方法 | 路径 | OpenAPI 名称 |
|---|---|---|
| GET | `/pipelines` | ListPipelines |
| GET | `/pipelines/{pipelineId}` | GetPipeline |
| GET | `/pipelines/{pipelineId}/runs` | ListPipelineRuns |
| GET | `/pipelines/{pipelineId}/runs/latestPipelineRun` | GetLatestPipelineRun |
| GET | `/pipelines/{pipelineId}/runs/{pipelineRunId}` | GetPipelineRun |
| POST | `/pipelines/{pipelineId}/runs` | CreatePipelineRun |
| PUT | `/pipelines/{pipelineId}/runs/{pipelineRunId}` | UpdatePipelineRun（终止运行） |
| GET | `/pipelines/{id}/pipelineRuns/{runId}/jobs/{jobId}/steps` | GetPipelineJobSteps |
| GET | `.../jobs/{jobId}/step/log` | GetPipelineJobStepLog |
| GET | `.../jobs/{jobId}/step/log/url` | GetPipelineJobStepLogUrl |
| POST | `.../jobs/{jobId}/start` | ExecutePipelineJobRun |
| PUT | `.../jobs/{jobId}/stop` | StopPipelineJobRun |
| PUT | `.../jobs/{jobId}/skip` | SkipPipelineJobRun |

> 日志接口需要 `stepIndex` 和 `buildId` 两个参数，二者都来自 `GetPipelineJobSteps`，所以查看日志前会先取一次步骤列表。

**令牌所需权限**（云效 → 个人设置 → 个人访问令牌）：
- 流水线 → 流水线：**只读**
- 流水线 → 流水线运行实例：**读写**（要用运行/终止功能）
- 流水线 → 流水线运行任务：**读写**（要看重跑/终止/跳过与日志）

只想纯监控的话，后两项给「只读」即可，写操作按钮会返回 403 并给出提示。

### 返回结构兼容说明

官方文档与实际返回存在差异，代码里按真实响应做了兼容（见 `src-tauri/src/api/models.rs`）：

- **GetPipelineJobSteps**：实际返回的是**数组**（一次构建过程一个元素），步骤在 `buildProcessNodes` 里，而文档写的是对象 + `steps`。元素中的 `jobId` 实际等于 `buildId`，因此运行任务 ID 仍使用请求参数。两种形态都已兼容。
- **GetPipelineJobStepLog**：`offset` 的单位是**行号**而不是字符偏移，`limit` 是本次最多返回的**行数**；`last` 表示下一行的位置，`-1` 表示没有更多。步骤尚未产出日志时接口会返回一段 `[ERROR] Log real-time query exception ...` 占位文本，前端会把它过滤掉。
- **GetPipelineJobStepLogUrl**：直接返回**纯文本 URL**（Content-Type 却是 `application/json`），客户端已做纯文本兜底。
- **GetPipelineRun**：`sources[].data.commit` 是一段 JSON 数组文本，提交信息经过百分号编码；解析时会取出 `commitId` 并解码 `commitMsg`。

---

## 三、目录结构

```
yunxiao-flow-monitor/
├── LICENSE                        MIT 许可证
├── .gitattributes                 统一换行符（仓库内 LF）
├── package.json                   前端依赖与脚本
├── vite.config.ts
├── tsconfig.json
├── index.html
├── env.d.ts
├── scripts/
│   ├── gen-icon.mjs               零依赖生成 1024×1024 图标源 PNG（手写 PNG 编码器）
│   ├── build-linux.sh             Linux 编译脚本
│   └── build-windows.ps1          Windows 编译脚本
├── src/                           Vue 3 前端
│   ├── main.ts
│   ├── App.vue                    顶栏 / 两栏布局 / 通知 / 设置入口
│   ├── store.ts                   全局单一状态源 + 所有业务动作
│   ├── types.ts                   与 Rust DTO 对齐的类型定义
│   ├── lib/
│   │   ├── api.ts                 invoke 封装 + 事件订阅 + 错误归一化
│   │   ├── ansi.ts                ANSI 转义解析器
│   │   └── format.ts              状态/时间/时长/触发方式格式化
│   ├── styles/main.css            深色主题设计系统
│   └── components/
│       ├── PipelineList.vue       流水线树（含最近 3 次运行子项）
│       ├── RunDetail.vue          阶段/任务拓扑 + 实时日志 + 触发运行弹窗
│       ├── JobLog.vue             实时日志面板（增量、跟阶段、分级配色、平滑滚动）
│       ├── StatusBadge.vue        状态徽标
│       └── SettingsPanel.vue      设置面板（含开机自启）
└── src-tauri/                     Rust 后端
    ├── Cargo.toml
    ├── tauri.conf.json
    ├── build.rs
    ├── app-icon.png               图标源文件（已生成）
    ├── icons/                     各平台图标（已生成，可直接打包）
    ├── capabilities/default.json
    └── src/
        ├── main.rs                入口
        ├── lib.rs                 Builder 装配 / 插件注册 / 命令注册
        ├── state.rs               AppState 全局共享状态
        ├── config.rs              配置与令牌持久化（含脱敏、0600 权限）
        ├── error.rs               统一错误类型（code + message 送前端）
        ├── commands.rs            全部 Tauri command（含开机自启）
        ├── poller.rs              后台轮询 / 状态跃迁检测 / 事件推送 / 通知
        ├── tray.rs                系统托盘与菜单
        └── api/
            ├── mod.rs
            ├── client.rs          云效 OpenAPI 客户端
            └── models.rs          宽容解析的数据模型
```

---

## 四、开发环境运行

### 前置要求
- **Node.js ≥ 20**
- **Rust ≥ 1.90**（`rustup default stable`；`tauri-plugin-autostart` 要求）
- **Windows**：Visual Studio 2022 生成工具（勾选「使用 C++ 的桌面开发」）+ WebView2 运行时（Win11 已内置）
- **Linux**：`libwebkit2gtk-4.1-dev`、`build-essential`、`libssl-dev`、`libxdo-dev`、`libayatana-appindicator3-dev`、`librsvg2-dev`、`pkg-config`（`scripts/build-linux.sh` 会自动装）

### 启动
```bash
npm install          # 装前端依赖
npm run app:dev      # 开发模式，前端热更新 + Rust 增量编译
```

首次启动后：右上角 **设置** → 填 **组织 ID** 与 **个人访问令牌** → **测试连接** → **保存**，然后点顶栏 **开始轮询**。

> `organizationId` 获取：云效组织管理后台 → 基本信息；也可以从访问链接 `https://devops.aliyun.com/organization/【OrganizationId】` 里直接取。

---

## 五、编译打包

图标已预生成在 `src-tauri/icons/`，可直接编译。若需要换图标：

```bash
npm run icon:gen     # 重新生成 1024×1024 源 PNG
npm run icon:build   # 由 tauri icon 派生 .ico / .icns / 各尺寸 png
```

### Windows
```powershell
.\scripts\build-windows.ps1
```
脚本会依次：检查/安装 Node.js → 检查/安装 Rustup → 检测 MSVC 生成工具（缺失时可一键 winget 安装）→ 检测并安装 WebView2 运行时 → `npm install` → 补图标 → `tauri build --bundles nsis`。

常用参数：
```powershell
.\scripts\build-windows.ps1 -Bundles nsis,msi        # 同时出 NSIS 与 MSI
.\scripts\build-windows.ps1 -Debug                   # 调试构建
.\scripts\build-windows.ps1 -Features secure-store   # 令牌存 Windows 凭据管理器
.\scripts\build-windows.ps1 -SkipDeps                # 跳过依赖自动安装
```

产物：`src-tauri\target\release\bundle\nsis\*.exe`

> 若脚本被执行策略拦截，先在当前会话执行：
> `Set-ExecutionPolicy -Scope Process -ExecutionPolicy Bypass`

### Linux
```bash
chmod +x scripts/build-linux.sh
./scripts/build-linux.sh
```
脚本会自动识别 apt / dnf / zypper / pacman 并安装依赖，然后 `tauri build`。

常用参数：
```bash
./scripts/build-linux.sh --bundles deb           # 只出 deb
./scripts/build-linux.sh --bundles appimage      # 只出 AppImage（不需要 rpmbuild）
./scripts/build-linux.sh --debug                 # 调试构建
./scripts/build-linux.sh --features secure-store # 令牌存 Secret Service（需 libdbus-1-dev）
./scripts/build-linux.sh --skip-deps             # 跳过系统依赖安装
```

产物：`src-tauri/target/release/bundle/{deb,rpm,appimage}/`

> 首次编译需下载并构建全部 Rust 依赖，通常 5–15 分钟。

---

## 六、配置项说明

配置文件位置（Tauri `app_config_dir`）：
- Windows：`%APPDATA%\com.yunxiao.flowmonitor\`
- Linux：`~/.config/com.yunxiao.flowmonitor/`

| 配置 | 默认值 | 说明 |
|---|---|---|
| baseUrl | `https://openapi-rdc.aliyuncs.com` | 中心版服务接入点 |
| organizationId | 空 | 组织 ID，必填 |
| pollIntervalSecs | 10 | 常规轮询间隔，取值范围 3–600 |
| fastPollIntervalSecs | 3 | 有实例运行中时的加速间隔，取值范围 1–60 |
| autoWatchLimit | 30 | 未指定关注项时，自动盯列表首页前 N 条（云效单页上限 30） |
| watchedPipelineIds | 空 | 关注的流水线；在列表点 ☆ 加入 |
| autoStartPolling | true | 启动即轮询（配置齐备时才生效） |
| notifyOnSuccess / Fail / Canceled | true / true / false | 分状态桌面通知开关 |
| minimizeToTrayOnClose | true | 关闭窗口时收进托盘 |

> 「开机自启」保存在操作系统里（Windows 为注册表 Run 项），不属于 `config.json`，因此不占上表字段；设置面板打开时会实时读取当前状态。

### 令牌存储
- **默认**：`credentials.bin`，XOR + Base64 混淆，文件权限 `0600`（Unix）。这是「避免明文落盘」，**不是加密**，请勿在共享机器上使用。
- **推荐**：编译时加 `--features secure-store`，改存系统钥匙串（Windows 凭据管理器 / macOS Keychain / Linux Secret Service）。Linux 需先装 `libdbus-1-dev`；钥匙串不可用时会自动回落到混淆文件，不会导致功能失效。
- 令牌通过接口回传前端时一律脱敏为 `pt-x****xxxx`，界面上永远拿不到原文。

---

## 七、实现要点

**为什么轮询放在 Rust 侧**：如果每个面板各自轮询，N 条流水线 × M 个面板会迅速放大请求量并触发云效限流（HTTP 429）。这里由单一后台任务统一取数、统一节流，再通过 `yunxiao://snapshot` 事件广播；窗口最小化到托盘时监控照常运行。

**状态跃迁检测**：`AppState.last_status` 记录每条流水线「上次的 runId + status」。只有 runId 变了或 status 变了才算跃迁并通知。**首次轮询只写入基线、不发通知**，避免启动时被历史失败记录刷屏。

**日志增量拉取**：`GetPipelineJobStepLog` 的 `offset` 是**行号**。前端维护「已收到的行数」，每轮多回退一行重新拉取，用新内容覆盖可能还没写完的最后一行，避免出现半行或重复行；`limit` 传大值一次取回剩余行。任务结束后会补拉一次，把最后一行补全。

**日志自动跟阶段**：任务运行期间每 2 秒静默刷新步骤状态，优先跟随 `status=running` 的步骤；当前步骤结束后自动切到下一个未结束的步骤。手动在下拉框选步骤会暂停自动跟随，勾回「跟阶段」立即恢复。

**开机自启**：接入官方 `tauri-plugin-autostart`，Windows 写入当前用户 Run 注册表项，Linux 写入 XDG autostart 桌面项，macOS 写入 LaunchAgent；无需管理员权限，也无需额外系统依赖。

**宽容解析**：云效不同接口的包装层与实际返回不完全一致（数组 / 对象、`result` / `data` / `pipelines` 包装、阶段信息平铺或嵌在 `stageInfo` 内），`api/client.rs` 的 `extract_array` / `extract_object` 与 `api/models.rs` 的多键名回退负责统一兜住；字段缺失一律降级为 `None`，不会 panic。步骤与日志接口的具体差异见「二、返回结构兼容说明」。

---

## 八、已知边界

1. **仅对接中心版**。Region 版（专有云）路径不含 `/organizations/{organizationId}`，需要另行适配。
2. **未实现人工卡点审批与部署单查看**（当前范围为「只读 + 运行控制」）。`GetPipelineRun` 返回的 `jobs[].actions` 已经解析出来了，要扩展审批只需按 `action.type` 补对应的 PUT 接口。
3. **限流**：轮询间隔下限已限制为 3 秒，但关注条数过多时仍可能撞上云效限流，界面上会显示 HTTP 429 提示，调大间隔即可。
4. **运行历史只保留最近 3 次**：这是界面设计（避免侧栏过深），更早的记录需要到云效网页端查看。
5. **开机自启的路径差异**：开发模式（`npm run app:dev`）下注册的是 `target/debug` 里的调试程序；安装版注册的是安装目录下的正式程序。切换前建议先在设置里关掉再重新开启。
6. **验证状态**：前端已通过 `vue-tsc --noEmit` 全量类型检查与 `vite build`；Rust 侧已通过 `cargo check` 与 `cargo test`（含步骤/日志/提交解析的 4 个单元测试）。

---

## 九、常用命令速查

```bash
npm install              # 安装前端依赖
npm run app:dev          # 开发模式
npm run dev              # 只跑前端（浏览器，无桌面能力）
npm run build            # 前端产物构建
npm run typecheck        # vue-tsc 类型检查
npm run app:build        # tauri build（等价于打包）
npm run icon:build       # 重新生成全套图标
./scripts/build-linux.sh
.\scripts\build-windows.ps1

# Rust 解析层单测（--no-default-features 可跳过生产构建对 dist/ 的依赖检查）
cd src-tauri && cargo test --no-default-features --lib
```

---

## 十、许可证

本项目基于 [MIT License](LICENSE) 开源。

Copyright (c) 2026 chai
