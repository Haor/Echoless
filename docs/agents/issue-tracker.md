# 任务跟踪器：GitHub

Echoless 使用 GitHub 任务单跟踪产品工作、缺陷、产品需求文档和实现任务：

- 仓库：`Haor/Echoless`
- 仓库地址：`https://github.com/Haor/Echoless`
- 拉取请求不是主要需求入口。

## 命令

在 Echoless 仓库中使用 GitHub CLI：

```sh
gh issue list
gh issue view <number>
gh issue create
gh issue edit <number>
gh issue close <number>
```

向任务跟踪器发布计划、产品需求文档、缺陷报告或实现任务时，除非用户明确要求其他产物，
否则创建 GitHub 任务单。

读取任务时使用 `gh issue view`，并把任务单正文、评论、标签和关联依赖作为当前任务上下文。

## 任务拆分

拆分工作时：

- 使用一个 GitHub 任务单表示顶层目标。
- 使用相互关联的 GitHub 任务单表示可以独立执行的子任务。
- GitHub 提供原生关系时优先使用原生关系。
- 无原生关系时，在任务单正文中明确记录父任务、子任务和阻塞关系。
- 只有任务不再缺少产品决策或外部依赖、可以直接执行时，才添加 `ready-for-agent`。
- 需要人工判断、凭据、硬件、安装包运行验证或用户验收时，添加 `ready-for-human`。
- 任务单正在执行期间移除就绪标签。
- 只有验收标准已经验证后才关闭任务单。
