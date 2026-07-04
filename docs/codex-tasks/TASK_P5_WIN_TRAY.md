# Codex 任务规格 P5:Windows 最小化/关闭到系统托盘(Rust 侧)

日期:2026-07-04 · 执行者:Codex(gpt-5.5)· 工作树:`echoless-tray/`(分支 `phase-2/win-tray` ← main)
范围:**只做 Tauri Rust 侧 + capability + 契约定义**。前端开关 UI 由 UI 重构分支(Claude)按本文档契约接入。

## 需求

- Windows 上最小化 / 关闭窗口时可进系统托盘(按用户偏好),**引擎(sidecar)不中断**。
- 托盘:图标 + tooltip(显示 RUNNING/STOPPED)+ 菜单(显示窗口 / 退出)+ 左键单击恢复窗口。
- 真正退出(托盘菜单退出、或偏好=直接关闭)必须走完整 sidecar 清理(kill + **wait** + cleanup)。
- macOS 行为不变(不注册托盘或注册但默认关闭偏好,见「平台门控」)。

## 现状事实(2026-07-04 核实,file:line 以 `app/src-tauri/src/lib.rs` 为准)

- 窗口**不在** tauri.conf.json 声明(`app.windows: []`),在 `.setup()`(1122-1153)用 `WebviewWindowBuilder` 运行时创建,label `"main"`,1040×640;macOS TitleBarStyle::Overlay + decorum 红绿灯 inset(1151),非 macOS `decorations(false).shadow(true)`(1143)。
- Builder 链(1095-1173):plugins(decorum 1098 / dialog 1099)→ `.manage(RunState)`(1100)→ invoke_handler(1101-1120)→ setup → on_window_event(1156-1170)→ run。
- **sidecar 管理**:`RunChild { child, stopping: Arc<AtomicBool>, config_path }`(25-29),`RunState(Mutex<Option<RunChild>>)`(31),`run_state_guard()`(33-39)。`Some`=运行中,`None`=停止——**这就是 tooltip 状态源**。
- 三条终止路径:`start_run` 先杀残留(953-958);`stop_run`(1084-1093)= take + stopping.store + kill + **wait** + cleanup_run_config(最干净);`on_window_event` CloseRequested(1158-1169)= kill 但**缺 wait()**,且现在**不 prevent_close,关窗即杀引擎退出**。
- `tauri = { version = "2", features = [] }`(Cargo.toml:19)——**无 tray-icon / image feature**。
- capabilities(`capabilities/default.json`):有 minimize/maximize/close/start-dragging;**缺 `core:window:allow-hide` / `allow-show` / `allow-set-focus`**。
- 图标:`icons/icon.ico`(多尺寸,Windows 托盘首选)、`32x32.png` 等;最简取法 `app.default_window_icon()`。
- 事件通道:Rust `app.emit("echoless://...")`(既有 status 1020 / exit 1034 / log 1047);前端 invoke 封装在 `app/src/api.ts`。

## 实现要求

### 1. 依赖与权限

- `Cargo.toml:19`:`tauri` features 加 `"tray-icon"`(用 `default_window_icon()` 可不加 image feature;若从 icon.ico 载入则加 `"image-ico"`)。
- `capabilities/default.json`:补 `core:window:allow-hide`、`core:window:allow-show`、`core:window:allow-set-focus`。

### 2. 抽取 `terminate_run` helper(顺手修 bug)

把 `stop_run`(1084-1093)的清理逻辑抽成 `fn terminate_run(state: &RunState)`:take + `stopping.store(true)` + `child.kill()` + `child.wait()` + `cleanup_run_config()`。三处共用:
- `stop_run` command;
- CloseRequested 真关闭分支(**修复现状缺 wait 的 bug**);
- 托盘「退出」菜单(terminate_run 后 `app.exit(0)`)。

### 3. 托盘偏好状态

- 新增 `TrayPrefs { minimize_to_tray: AtomicBool, close_to_tray: AtomicBool }`,`.manage()` 注册。
- **Rust 侧默认值 = false/false**(保守:前端启动时会把持久化偏好推下来;推下来之前维持旧行为)。
- 新增 command `set_tray_prefs(minimize_to_tray: bool, close_to_tray: bool)`:写入状态;加进 invoke_handler。
- 平台门控:托盘注册与偏好生效仅 `#[cfg(target_os = "windows")]`……**例外**:代码结构上允许全平台编译(tray API 跨平台),但**非 Windows 不注册托盘、偏好强制视为 false**,macOS 现行为零变化。

### 4. 托盘注册(在 `.setup()` 建窗后)

`TrayIconBuilder`:
- id 固定(如 `"main-tray"`),icon = `app.default_window_icon().cloned()`,tooltip 初始 `"Echoless — STOPPED"`。
- 菜单:`显示窗口`(show + unminimize + set_focus)、分隔、`退出`(terminate_run → app.exit(0))。菜单文案用英文 `Show` / `Quit`(i18n 由前端层负责的部分不在此;托盘菜单先英文,后续跟随 i18n 再议)。
- `on_tray_icon_event`:左键 Click(Up)→ show + unminimize + set_focus。
- 保存 `TrayIcon` handle(manage 或 setup 里存 state),供 tooltip 更新。

### 5. 窗口事件拦截(改 `.on_window_event`)

- `CloseRequested`:若 windows && close_to_tray → `api.prevent_close()` + `window.hide()`(**绝不碰 RunState,引擎继续跑**);否则 `terminate_run` + 放行关闭。
- 最小化:Tauri 2 无独立 Minimized 事件,用 `WindowEvent::Resized` 时查 `window.is_minimized()`;windows && minimize_to_tray && is_minimized → `window.unminimize()` + `window.hide()`(先 unminimize 再 hide,避免恢复时窗口仍处最小化态)。注意防抖:hide 本身可能再触发 Resized,用状态位防重入。

### 6. tooltip 状态同步

- 引擎状态变化点:`start_run` 成功 spawn 后 → `RUNNING`;`stop_run`/`terminate_run`/stdout reader 检测到退出(emit `echoless://exit` 处,1034)→ `STOPPED`。
- 在这些点调用 helper `update_tray_tooltip(app, running: bool)`(内部拿 TrayIcon handle,`set_tooltip(Some("Echoless — RUNNING"))`);非 Windows no-op。

### 7. 前端契约(本任务只定义,不实现 UI)

- command:`set_tray_prefs { minimizeToTray: bool, closeToTray: bool }`(注意 tauri 参数命名转换,Rust snake_case ↔ 前端 camelCase,与既有 command 风格一致)。
- 前端持久化 key(UI 分支实现):`echoless.trayPrefs.v1`(JSON `{minimizeToTray, closeToTray}`),启动 useEffect 里读取并 invoke 同步,变更时再 invoke。
- 在 `docs/frontend/FRONTEND_STATE_HANDOFF.md` 追加一节记录此契约。

## 验收标准

1. `cargo build`(app/src-tauri)+ `cargo clippy -- -D warnings` 通过;`pnpm build` 前端不需改动也应仍通过。
2. Windows 行为(自测清单,可用 `cargo tauri dev` 描述验证步骤;无 Windows 环境则给出手测清单):
   - 偏好关(默认):行为与现状完全一致(关窗杀引擎退出)。
   - close_to_tray=true:关窗 → 窗口隐没托盘,引擎不断(RunState 仍 Some,音频继续);托盘左键/「显示窗口」恢复;「退出」→ 引擎清理(含 wait)后进程退出,无 echoless sidecar 残留。
   - minimize_to_tray=true:最小化 → 进托盘;恢复正常。
   - tooltip 随 start/stop 在 RUNNING/STOPPED 间切换。
3. macOS:零行为变化(不出现托盘/菜单栏图标,关窗行为同现状)。
4. CloseRequested 真关闭路径含 `wait()`(修复项)。
5. 输出:diff 摘要 + 契约段(command 签名/参数)+ Windows 手测清单。
