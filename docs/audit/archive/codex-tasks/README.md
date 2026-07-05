# Codex 任务包(phase-2)

每份 TASK_*.md 都是自包含规格:目标、2026-07-04 核实过的现状锚点、改动点、验收标准。
派发时把整份文档作为任务喂给 Codex,不要只给摘要。

| 任务 | 规格 | 工作树 | 分支 | 模型 | 前置 |
|---|---|---|---|---|---|
| P3 内化改名 | `TASK_P3_AEC3_INTERNALIZE.md` | `../echoless-aec3/` | `phase-2/aec3-internalize` | gpt-5.5(bulk) | 无,可立即开始 |
| P4 延迟魔改 | `TASK_P4_AEC3_DELAY_MOD.md` | P3 合入后重建 | `phase-2/aec3-delay-mod` | gpt-5.5 xhigh | **P3 合入 main** |
| P5 托盘 Rust 侧 | `TASK_P5_WIN_TRAY.md` | `../echoless-tray/` | `phase-2/win-tray` | gpt-5.5 | 无,可与 P3 并行 |

## 派发方式

走 `/codex:rescue`(companion 共享运行时),workspace-write,`--cd` 指向对应 worktree。
提示词模板:

```
按 docs/codex-tasks/TASK_XX.md 的规格完整执行。先通读规格全文与其引用的背景方案,
再按执行顺序分批 commit。每批 commit 后运行规格中对应的构建/测试命令并报告结果。
全部完成后输出:commit 列表、验收标准逐项勾选、遗留问题。
不要超出规格范围改动;发现规格与代码不符时停下来报告,不要自行发挥。
```

⚠️ 本会话历史坑:环境注入的 `HTTP_PROXY=127.0.0.1:17890` 会让 Codex 连不上 OpenAI;
`.claude/settings.json` 已设 NO_PROXY(新会话生效),老会话调用前需 `env -u HTTP_PROXY -u HTTPS_PROXY -u ALL_PROXY ...`。

## 合并纪律

- 后端分支从 main 切、PR 合回 main;UI 分支(`phase-2/ui-refactor`)定期 merge main。
- P4 的 diff 必须落在 P3 改名后的命名空间上,两者严禁混在同一批提交。
- 验收由 Claude 复核后再合并(P3:grep 归零 + 别名冒烟;P4:vendor 单测 + 长跑数据;P5:清理路径含 wait + macOS 零变化)。
