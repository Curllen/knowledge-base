# 移动端迁移（Tauri Mobile · iOS + Android）— 任务跟踪

> 目标：在现有 v1.7.1 桌面版（Tauri 2.x + React 19 + rusqlite）基础上，**直接在 master 上**增加 iOS / Android target，复用三层架构与业务 Commands。
> 创建日期：2026-05-04
>
> ## 核心决策（已定）
> | 决策项 | 选择 | 理由 |
> |--------|------|------|
> | 仓库策略 | **同一仓库 + cfg gate**（不另起仓库） | commands 改一次两端生效，schema 不漂移 |
> | 分支策略 | **直接在 master 开发**（不开 mobile 分支） | 减少合并成本；用 cfg gate 在编译时隔离桌面/移动代码，桌面端编译时移动代码完全剥离不进二进制；每次 commit 前必须 `cargo build` 桌面端验证不破 |
> | 兼容策略 | **桌面端继续按 master 出 v1.7.x 版本**，移动端阶段性发版 | 桌面端用户不受影响 |
> | 默认行为 | 闪卡（cards）默认关闭，与 PC 端 `DEFAULT_ENABLED_VIEWS` 保持一致（除 cards 外全部启用） | 复用 PC 端 `FeatureModulesSection` 设计 |
>
> ## 评估结论
> - 后端复用率 **70-75%**（核心业务全保留，砍掉 tray/global-shortcut/autostart/updater/multi-instance/sidecar/PDFium）
> - 前端复用率 **30-40%**（业务组件可用，布局/导航需重写）
> - 总体可行性：⭐⭐⭐⭐ 可行
> - 估算工作量：**一人 4-7 周**

---

## 🔴 执行规则（继续任务前必读）

**每次要开始某条任务前，必须先走三步：**

1. **重新评估必要性**：此刻这条任务是否仍值得做？优先级是否被新情况改变？
2. **给出实现方案**：
   - 涉及哪些文件（lib.rs / commands / services / 前端 layout / capabilities / tauri.conf.json）
   - 具体改什么、新增什么
   - 是否需要 cfg gate / mobile-only crate feature
   - 桌面端是否会受影响 → 如何验证
   - 预计工作量 + 潜在风险点
3. **等待用户确认**后再开始写代码。

⛔ 禁止直接跳过以上三步动手实现。

---

## 🔒 master 直接开发的安全约定

由于不开 mobile 分支，每次 commit 前必须做：

| 检查项 | 命令 | 说明 |
|-------|------|------|
| 桌面端编译通过 | `cd src-tauri && cargo check` | 任何改动后必跑，不破坏现有桌面构建 |
| 桌面端可启动 | `pnpm tauri dev` 启动一次，开主窗+笔记列表 | 验证 cfg gate 隔离没误删桌面代码 |
| 现有功能不退化 | 手测：创建笔记 / 搜索 / AI 对话 / 闪卡 / 同步 | 共享业务代码改动后必测 |
| Schema 兼容性 | 检查是否有 schema 变更，桌面端启动后能跑通迁移 | 移动端加表/字段同样作用于桌面端数据库 |
| `/check` 全栈检查 | `/check` slash command | TypeScript + Rust clippy + 类型对齐 |

每次 commit message 加 scope 标识：`feat(mobile): ...` / `chore(mobile): ...` / `refactor(shared): ...`，便于 git log 区分。

---

## 移动端不兼容功能清单（已盘点）

| 红区（必须 cfg gate 隔离） | 影响代码 |
|---------------------------|---------|
| 系统托盘 | `mod tray`, `tauri = features=["tray-icon"]` |
| 全局快捷键 | `tauri-plugin-global-shortcut`, `services::shortcut`, `commands::shortcut`, `commands::asr` 录音热键 |
| 开机自启 + `--start-minimized` | `tauri-plugin-autostart` |
| 应用自动更新 | `tauri-plugin-updater`（移动端走 App Store / Play） |
| 多开实例 + `--instance` 参数 + 文件锁 | `acquire_instance_lock`, `try_exclusive_lock`, `flock` |
| 多窗口 | `commands::notes::open_note_in_new_window`, `migration-splash` |
| 双击 `.md` 启动 | `extract_md_paths_from_args`, `deliver_md_to_default`, `start_md_deliver_watcher` |
| MCP sidecar（spawn 子进程） | kb-mcp 子 crate, `rmcp transport-child-process` |
| PDFium 动态库加载 | `pdfium-render` + `resources/pdfium/*.dll/dylib/so` |

| 黄区（需适配） | 处理方案 |
|---------------|---------|
| `tauri-plugin-dialog` 多选/目录选 | 改单文件选择 |
| 自定义数据目录 / KB_DATA_DIR | 移动端沙盒强制，整功能砍 |
| WebDAV / S3 同步后台保活 | iOS 后台限制，改"手动按钮 + 通知重启" |
| `commands::import::scan_markdown_folder`（OB 整库导入） | 改 ZIP 上传 / 单文件导入 |

---

## 任务列表

### Phase 0 · 移动端原型设计（已完成 ✅）

#### T-M000 · 移动端 17 页 HTML 原型

- **状态**：`completed` · 完成日期：2026-05-04
- **价值**：⭐⭐⭐⭐⭐  成本：低
- **产物位置**：`output/UI原型/2026-05-04_知识库移动端App/`
- **包含**：
  - [x] `index.html` — 入口页（设计系统 + 流程图 + iframe 实时预览）
  - [x] 5 个 Tab 主页：00-home / 01-notes / 06-ai / 08-tasks / 10-me（统一 5 格 Tab）
  - [x] 9 个二级页：02-note-edit / 03-search / 04-daily / 05-tags / 07-ai-chat / 09-cards / 11-sync / 12-task-edit / 13-trash
  - [x] 2 个特殊页：14-quick-create（新建）/ 15-quick-capture（闪念）
  - [x] 1 个功能开关页：16-feature-toggle（含底部 Tab 配置 + Dashboard 显示项 + 8 个可选模块开关）
- **设计要点**：
  - 主色 `#1677FF`（沿用 PC 端 Ant Design 蓝）+ AI 强调橙 `#FA8C16`
  - 5 Tab 统一：主页 / 笔记 / AI / 待办 / 我的
  - 浮动 FAB 全局指向 `14-quick-create`
  - 闪卡（cards）默认关闭，与 PC 端 `DEFAULT_ENABLED_VIEWS` 一致
  - 所有页面顶栏统一为「← 返回 + 居中标题 + 右按钮」（参考 04-daily 模板）

---

### Phase 1 · 探针（预计 1 周）

#### T-M001 · master 上 Tauri Android 初始化

- **状态**：`pending`
- **价值**：⭐⭐⭐⭐⭐  成本：中
- **目标**：跑起来一个能编译的 Android 工程，主窗口能渲染（不要求功能可用）
- **前置工具链**（用户当前 Windows 环境**裸**，需先安装）：
  - [ ] JDK 17+（已有 OpenJDK 21 ✅，但 `JAVA_HOME` 未设置）
  - [ ] Android SDK + Android Studio（含命令行工具）
  - [ ] Android NDK 25+
  - [ ] 配置环境变量：`JAVA_HOME` / `ANDROID_HOME` / `NDK_HOME`
  - [ ] `rustup target add aarch64-linux-android armv7-linux-androideabi i686-linux-android x86_64-linux-android`
- **子任务**：
  - [ ] `pnpm tauri android init` → 生成 `src-tauri/gen/android/`
  - [ ] 整理 `.gitignore`：commit Gradle 模板，忽略 `.gradle/` `build/` `local.properties`
  - [ ] 提交：`chore(mobile): tauri android init`
- **注意**：`pnpm tauri android dev` 在 T-M002 完成前**必然失败**（红区依赖编译不过），不强求验证 dev 启动
- **风险**：
  - 工具链下载量大（~2 GB），网络慢需配国内镜像
  - JAVA_HOME 配置错误会让 Gradle 报错

#### T-M002 · `lib.rs` 桌面专属代码 cfg gate 隔离

- **状态**：`pending`
- **价值**：⭐⭐⭐⭐⭐  成本：中
- **目标**：让 `cargo check --target aarch64-linux-android` 能过编译，且**桌面端 `cargo check` 仍能通过**
- **子任务**：
  - [ ] `mod tray;` 加 `#[cfg(desktop)]`
  - [ ] `tauri = features=["tray-icon"]` → 改为条件 feature 或 cfg gate 注册逻辑
  - [ ] `tauri-plugin-global-shortcut` 注册 + `services::shortcut::register_all` 加 `#[cfg(desktop)]`
  - [ ] `tauri-plugin-autostart` 注册 加 `#[cfg(desktop)]`
  - [ ] `tauri-plugin-updater` 注册 加 `#[cfg(desktop)]`
  - [ ] 多开实例锁逻辑（`acquire_instance_lock` / `try_exclusive_lock` / `parse_instance_arg` / `early_app_data_dir`）加 `#[cfg(desktop)]`
  - [ ] `.md` 投递（`deliver_md_to_default` / `start_md_deliver_watcher`）加 `#[cfg(desktop)]`
  - [ ] MCP sidecar：保留 in-memory `setup_internal_mcp`，砍 `rmcp transport-child-process`（本就 desktop only）
  - [ ] PDFium 动态加载 加 `#[cfg(desktop)]`
  - [ ] `Cargo.toml` 桌面专属依赖加 `[target.'cfg(any(target_os = "windows", target_os = "macos", target_os = "linux"))'.dependencies]`
  - [ ] 桌面端回归：`cd src-tauri && cargo build` 通过 + `pnpm tauri dev` 启动验证笔记 CRUD/搜索/AI 不退化
- **风险**：误删桌面专属代码导致桌面端 panic / 启动崩溃

#### T-M003 · Capabilities 移动端版本

- **状态**：`pending`
- **价值**：⭐⭐⭐⭐  成本：低
- **目标**：`src-tauri/capabilities/mobile.json` 只包含移动端可用权限
- **子任务**：
  - [ ] 新建 `capabilities/mobile.json`：删除 tray / autostart / global-shortcut / updater / opener:allow-reveal-item-in-dir
  - [ ] `default.json` 加 `"platforms": ["windows", "macOS", "linux"]`
  - [ ] `mobile.json` 加 `"platforms": ["android", "iOS"]`
  - [ ] `tauri.conf.json` 增加 `bundle.android.minSdkVersion`（建议 26+）

#### T-M004 · Android 探针验证：笔记 CRUD + 搜索能跑通

- **状态**：`pending`
- **价值**：⭐⭐⭐⭐⭐  成本：低
- **目标**：在 Android 模拟器上能创建笔记、列出、搜索（验证 rusqlite + 三层架构在移动端工作）
- **子任务**：
  - [ ] 启动 Android 模拟器，`pnpm tauri android dev` 安装应用
  - [ ] 验证 `commands::notes::list_notes` 返回空数组
  - [ ] 调 `commands::notes::create_note` 创建 1 条
  - [ ] 调 `commands::search::search_notes` 验证 FTS 搜索
  - [ ] 验证数据持久化（杀进程重启后数据还在）

#### T-M005 · iOS 初始化（独立任务，需 macOS）

- **状态**：`pending` · 阻塞中（需 Mac 设备）
- **价值**：⭐⭐⭐⭐  成本：高
- **前置**：macOS + Xcode 16+ + Apple Developer Program 账号（$99/年，TestFlight/App Store 必须）
- **子任务**：
  - [ ] 在 Mac 上 `pnpm tauri ios init` → 生成 `src-tauri/gen/apple/`
  - [ ] commit `gen/apple/` 进 master（CI 后续 build 依赖）
  - [ ] 配置 iOS Bundle ID（建议 `com.agilefr.kb.mobile`）
  - [ ] `tauri.conf.json` 增加 `bundle.iOS.minimumSystemVersion`（建议 16+）

---

### Phase 2 · 移植绿区业务（预计 2-3 周）

#### T-M006 · 前端响应式布局 / 双布局决策 + 移动端布局实现

- **状态**：`pending`
- **价值**：⭐⭐⭐⭐⭐  成本：高
- **方案候选**：
  - 方案 A：响应式 Ant Design + Tailwind（一套代码两端用）
  - 方案 B：antd-mobile 双布局（移动端独立组件库）
- **子任务**：
  - [ ] 决策：写 ADR `docs/architecture/decisions/ADR-XXX-mobile-ui.md`
  - [ ] 新建 `src/components/layout/MobileLayout.tsx`（底部 5 Tab + Drawer + FAB）
  - [ ] 新建 `useIsMobile()` hook（基于 `@tauri-apps/plugin-os` 检测平台）
  - [ ] `App.tsx` 根据平台分发布局
  - [ ] **桌面端不受影响**：`MobileLayout` 只在 `isMobile===true` 时渲染

#### T-M007 · 砍 / 隐藏桌面专属设置项

- **状态**：`pending`
- **价值**：⭐⭐⭐  成本：低
- **依赖**：T-M015 完成（功能开关页要先有 isMobile 判断）
- **子任务**：
  - [ ] 设置页（PC）的"全局快捷键 / 开机启动 / 自定义数据目录 / 多开实例 / 应用更新"分页加 `isMobile` 判断隐藏
  - [ ] `migration-splash` / `emergency-reminder` / `popout-note` 三类窗口在移动端改 Modal / 路由跳转

#### T-M008 · Dashboard 主页（00-home 对应代码）

- **状态**：`pending`
- **价值**：⭐⭐⭐⭐⭐  成本：中
- **依赖**：T-M006 完成
- **子任务**：
  - [ ] 新建 `src/pages/home/MobileHome.tsx`（按 `00-home.html` 原型实现）
  - [ ] 复用 `commands::system::get_dashboard_stats` + `get_writing_trend`
  - [ ] 30 天写作热力图（移动端版，缩小 cell）
  - [ ] 4 个数据卡片 + 4 个快捷操作 + 今日待办速览 + 最近 2 条笔记

#### T-M009 · 新建抽屉 + 闪念捕获页

- **状态**：`pending`
- **价值**：⭐⭐⭐⭐⭐  成本：中
- **子任务**：
  - [ ] 新建 `src/pages/quick-create/index.tsx`（按 `14-quick-create.html` 原型）
  - [ ] 新建 `src/pages/quick-capture/index.tsx`（按 `15-quick-capture.html` 原型）
  - [ ] 闪念捕获实现自动保存草稿到 `app_config`（key=`quick_capture_draft`）
  - [ ] 集成剪贴板检测（`tauri-plugin-clipboard-manager`）

#### T-M010 · 任务详情页（12-task-edit 对应代码）

- **状态**：`pending`
- **价值**：⭐⭐⭐⭐  成本：中
- **子任务**：
  - [ ] 新建 `src/pages/tasks/TaskDetail.tsx`（按 `12-task-edit.html` 原型）
  - [ ] 复用 `commands::tasks::*`（update / list_subtasks / add_link / snooze_reminder）
  - [ ] 子任务拖拽排序（react-dnd 移动端兼容）

#### T-M011 · 图谱页移动端简化版

- **状态**：`pending` · 优先级 P2
- **价值**：⭐⭐⭐  成本：中
- **风险**：当前桌面图谱触屏交互（pan/zoom/pinch）需重写
- **子任务**：
  - [ ] 评估：移动端图谱是否做？（用户决定）
  - [ ] 若做：换轻量图谱库（如 react-flow），桌面继续用现有方案

---

### Phase 3 · 黄区适配（预计 1-2 周）

#### T-M012 · PDFium 替换 / 移动端砍 PDF 解析

- **状态**：`pending`
- **价值**：⭐⭐⭐  成本：中
- **子任务**：
  - [ ] 评估：移动端是否保留 PDF 导入？
  - [ ] 若保留：`pdf-extract` + 字体兜底，砍 `pdfium-render` cfg gate
  - [ ] 测试中文 CMap PDF 提取效果

#### T-M013 · 文件导入改单文件模式

- **状态**：`pending`
- **价值**：⭐⭐⭐⭐  成本：中
- **子任务**：
  - [ ] `commands::import::scan_markdown_folder` 在移动端改 `import_single_file`
  - [ ] OB 整库导入：移动端改 ZIP 上传 → 服务端解压 → 逐文件导入
  - [ ] PDF / 附件导入：用 `tauri-plugin-dialog` 单文件选择

#### T-M014 · 同步功能移动端 UX

- **状态**：`pending`
- **价值**：⭐⭐⭐⭐⭐  成本：中（核心：PC ↔ 手机数据互通）
- **子任务**：
  - [ ] 后台调度器（`sync_scheduler` / `sync_v1_scheduler`）加 `#[cfg(desktop)]`
  - [ ] 移动端：改"手动同步按钮" + "前台运行时定期触发"
  - [ ] 测试 WebDAV 在 iOS 后台限制下的可靠性
  - [ ] 测试 S3 上传大文件在移动网络下的稳定性

#### T-M015 · 功能模块开关页（16-feature-toggle 对应代码）

- **状态**：`pending`
- **价值**：⭐⭐⭐⭐⭐  成本：中（PC 与移动端共用，需双向兼容）
- **依赖**：T-M006
- **现状**：PC 端已有 `src/components/settings/FeatureModulesSection.tsx`，闪卡默认关闭已实现
- **子任务**：
  - [ ] 移动端复用 PC 端 `FeatureModulesSection` 组件，加移动端样式（Card → 全屏列表）
  - [ ] **新增移动端独有**：底部 Tab 配置（最多 5 个 Tab + 自动占第 5 格"我的"）
  - [ ] **新增移动端独有**：主页 Dashboard 显示项开关（5 个卡片单独控制）
  - [ ] 持久化到 `app_config`（沿用现有 `enabled_views` key）
  - [ ] 同步：`enabled_views` 通过 V1 同步推送到所有设备
  - [ ] **桌面端验证**：原 `FeatureModulesSection` 在 PC 上行为不变（不破现有功能）

---

### Phase 4 · 平台特化 + 发布（预计 1-2 周）

#### T-M016 · iOS Share Extension（接受外部 .md / 链接剪藏）

- **状态**：`pending` · 阻塞中（依赖 T-M005 iOS init）
- **价值**：⭐⭐⭐⭐  成本：高
- **目标**：从 Safari / 其他 App 分享文本/链接/图片到知识库
- **子任务**：
  - [ ] Xcode 工程添加 Share Extension target
  - [ ] Swift 桥接：把分享内容写入应用沙盒 + 主程序检测启动

#### T-M017 · Android Intent Filter（接受 .md 文件 / 文本分享）

- **状态**：`pending`
- **价值**：⭐⭐⭐⭐  成本：中
- **子任务**：
  - [ ] `AndroidManifest.xml` 加 `<intent-filter>` for `text/markdown` `text/plain`
  - [ ] Rust 侧通过 deeplink 接收数据（替代 PC 端的 `extract_md_paths_from_args`）

#### T-M018 · iOS 签名 + TestFlight 发布

- **状态**：`pending` · 阻塞中（依赖 Apple Developer Program）
- **价值**：⭐⭐⭐⭐⭐  成本：中
- **子任务**：
  - [ ] Apple Developer Program 注册（$99/年）
  - [ ] 创建 App ID + Provisioning Profile
  - [ ] 导入证书 `.p12` 到 GitHub Secrets
  - [ ] CI 跑 `tauri ios build` + 上传 TestFlight

#### T-M019 · Android 签名 + APK / AAB 打包

- **状态**：`pending`
- **价值**：⭐⭐⭐⭐⭐  成本：中
- **子任务**：
  - [ ] 生成 Android keystore（`keytool -genkey`）
  - [ ] keystore 存 GitHub Secrets
  - [ ] CI 跑 `tauri android build --aab` 出 Google Play 上架包
  - [ ] 自签 APK 同时输出，方便用户直接侧载

#### T-M020 · CI 扩展：`release.yml` 加 Android / iOS job

- **状态**：`pending`
- **价值**：⭐⭐⭐⭐  成本：中
- **子任务**：
  - [ ] `.github/workflows/release.yml` matrix 增加 `platform: ubuntu-22.04` + Android target 节点
  - [ ] 新增 `macos-latest` + iOS target 节点
  - [ ] 桌面端 4 个 job 不动，确保不破坏现有发布流程
  - [ ] tag `v*-mobile.*.*` 触发移动端 CI（与桌面 tag 区分）

---

## 阶段进度看板

| Phase | 任务数 | 完成数 | 状态 |
|-------|-------|--------|------|
| Phase 0 原型设计 | 1 | 1 | ✅ `completed` |
| Phase 1 探针 | 5 | 0 | `pending` |
| Phase 2 移植绿区 | 6 | 0 | `pending` |
| Phase 3 黄区适配 | 4 | 0 | `pending` |
| Phase 4 平台特化 | 5 | 0 | `pending` |
| **合计** | **21** | **1** | — |

---

## 任务依赖图

```
Phase 0 (T-M000) ✅
   └── Phase 1
        ├── T-M001 Android init ─┐
        ├── T-M002 cfg gate ─────┼─→ T-M004 探针验证
        ├── T-M003 Capabilities ─┘
        └── T-M005 iOS init (独立，需 Mac)

Phase 2 (依赖 T-M002 + T-M004)
   ├── T-M006 布局决策 ──→ T-M007 砍设置项
   │                  ├── T-M008 Dashboard
   │                  ├── T-M009 新建抽屉 + 闪念
   │                  └── T-M010 任务详情
   └── T-M011 图谱（P2 可选）

Phase 3 (依赖 Phase 2)
   ├── T-M012 PDFium
   ├── T-M013 文件导入
   ├── T-M014 同步 UX
   └── T-M015 功能模块开关 (依赖 T-M006)

Phase 4 (依赖 Phase 3)
   ├── T-M016 iOS Share Ext (依赖 T-M005)
   ├── T-M017 Android Intent
   ├── T-M018 iOS 签名 + TestFlight (依赖 T-M005 + Apple Dev)
   ├── T-M019 Android 签名
   └── T-M020 CI 扩展
```

---

## 历史记录

- **2026-05-04** 创建任务跟踪文件，定义 4 Phase + 14 任务
- **2026-05-04** 完成 T-M000 原型设计（17 页 HTML，已对齐 PC 端功能模块清单）
- **2026-05-04** 决策：放弃 mobile 分支策略，改为 master 直接开发 + cfg gate 隔离 + commit 前桌面端回归验证
- **2026-05-04** 扩展为 21 个任务（增加 T-M005 iOS init 独立任务、T-M015 功能模块开关、T-M016/T-M020 平台特化任务）
