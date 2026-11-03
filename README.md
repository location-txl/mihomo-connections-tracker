# Mihomo 连接流量统计

Rust 常驻后端直接采集一个 Mihomo，将数据写入 SQLite，提供只读查询 API，并附带一个由 `web` 子命令启动的网页控制台。不再支持 Master/Agent 或数据同步。

## 编译和安装命令行工具

以下命令均从仓库根目录执行，选择一种安装方式即可，无需 `sudo`。

需要网页控制台时，先构建前端（要求 Node.js 与 pnpm），产物会嵌入二进制：

```bash
cd frontend
pnpm install
pnpm build
```

不执行这一步也能编译命令行工具，但 `web` 子命令没有页面可用；release 编译要求 `frontend/dist` 存在。

### 编译后安装到 ~/.local/bin

```bash
cargo build --manifest-path backend/Cargo.toml --release
mkdir -p "$HOME/.local/bin"
install -m 755 backend/target/release/mihomo-connections-tracker "$HOME/.local/bin/mihomo-connections-tracker"
```

如果已设置 `CARGO_TARGET_DIR`，请从对应的 `release` 目录复制可执行文件。

### 使用 cargo install

从本地源码编译并安装到 `~/.cargo/bin`（设置过 `CARGO_HOME` 时使用其下的 `bin`）：

```bash
cargo install --path ./backend --force
```

也可以让 Cargo 安装到 `~/.local/bin`：

```bash
cargo install --path ./backend --root "$HOME/.local" --force
```

`cargo install` 默认使用 release 配置，`--force` 用于覆盖已有安装。以上命令使用本地源码，无需发布到 crates.io。

### 配置 PATH 并验证

将所选安装目录加入 `PATH`。macOS 默认的 zsh 可在 `~/.zshrc` 中添加对应的一行：

```bash
# 使用 ~/.local/bin 时
export PATH="$HOME/.local/bin:$PATH"
# 使用 Cargo 默认安装目录时
export PATH="${CARGO_HOME:-$HOME/.cargo}/bin:$PATH"
```

保存后执行 `source ~/.zshrc`，确认命令可用：

```bash
command -v mihomo-connections-tracker
mihomo-connections-tracker --version
mihomo-connections-tracker --help
```

安装后可在任意目录使用 `mihomo-connections-tracker service` 管理服务。采集配置只能通过 `service install` 设置，顶层命令不接受配置选项。

## 服务安装参数

以下参数仅供 `mihomo-connections-tracker service install` 使用。密钥也可通过 `MIHOMO_TOKEN` 和 `TRACKER_API_TOKEN` 环境变量在安装时注入；不需要认证时不设置。

| 参数 | 默认值 / 含义 |
|---|---|
| `--database` | 服务目录中的 `tracker.db`，SQLite 文件 |
| `--mihomo-host` | `127.0.0.1`，单个 Mihomo HTTP 控制接口 |
| `--mihomo-port` | `9090` |
| `--mihomo-token` | 可选，优先使用环境变量 `MIHOMO_TOKEN` 注入 |
| `--main-airport` | 必填，空 Provider 节点归属的机场名 |
| `--listen-host` | `127.0.0.1`，查询 API 监听地址 |
| `--listen-port` | `8080` |
| `--api-token` | 可选，也可使用 `TRACKER_API_TOKEN`，保护健康检查以外的查询接口 |

按 Ctrl+C 或收到 SIGTERM 时完成当前数据库事务再退出。连接失败每 5 秒重连；15 秒没有有效快照会重新连接。

## macOS 后台服务

提供用户级 LaunchAgent，无需 `sudo`。登录后自动启动，退出登录后停止；进程退出或崩溃后由 launchd 自动拉起，重启节流间隔为 10 秒。电脑休眠期间不会持续采集。`service` 子命令仅支持 macOS。

完成上面的命令行工具安装后，在已登录的 macOS 用户会话中注册后台服务：

```bash
read -rs 'MIHOMO_TOKEN?Mihomo 密钥: '; echo
export MIHOMO_TOKEN
mihomo-connections-tracker service install \
  --mihomo-host 127.0.0.1 \
  --mihomo-port 9097 \
  --main-airport '主机场'
```

`install` 接受上述服务安装参数，立即启动并开启登录自启。它直接使用当前命令的绝对路径运行服务，不再复制另一份可执行文件。建议先通过 Cargo 或 `install` 安装到固定目录，再注册服务；如果直接从 `target/release` 注册，后续清理编译目录会导致服务无法启动。不指定 `--database` 时使用服务目录中的新数据库；接入已有数据库时传入 `--database /绝对路径/tracker.db`，相对路径也会在安装时转为绝对路径。**必须先停止使用该数据库的前台进程，避免两个采集进程同时写入。** 安装不会自动查找、复制或迁移旧数据库，也不会创建自定义数据库路径的父目录。

```bash
# 使用 PATH 中的命令管理服务
mihomo-connections-tracker service status
mihomo-connections-tracker service stop      # 停止当前会话，保留登录自启设置
mihomo-connections-tracker service start     # 启动当前会话，保留登录自启设置
mihomo-connections-tracker service restart   # 优雅停止后重新加载
mihomo-connections-tracker service disable   # 停止并关闭登录自启
mihomo-connections-tracker service enable    # 开启登录自启并立即启动
mihomo-connections-tracker service uninstall # 保留命令行程序、数据库和日志
```

| 文件 | 位置（相对于用户主目录） |
|---|---|
| 可执行文件 | 注册时使用的命令路径，例如 `.cargo/bin/mihomo-connections-tracker` 或 `.local/bin/mihomo-connections-tracker` |
| 默认数据库 | `Library/Application Support/mihomo-connections-tracker/tracker.db` |
| 安装配置 | `Library/Application Support/mihomo-connections-tracker/service.plist` |
| 登录自启配置 | `Library/LaunchAgents/local.mihomo-connections-tracker.plist` |
| 标准输出/错误日志 | `Library/Logs/mihomo-connections-tracker/stdout.log`、`stderr.log` |

关闭登录自启只移除自启配置，安装配置仍供 `start` 使用。两个配置文件权限均为 `0600`；通过参数或 `MIHOMO_TOKEN`、`TRACKER_API_TOKEN` 传入的配置保存在 `TRACKER_SERVICE_CONFIG` 环境变量中，运行时不放入进程命令行。配置含明文密钥，请勿分享；`service status` 只展示运行状态和文件路径，不输出密钥。服务不继承当前终端的其他环境变量。

### 更新程序

只更新程序时，覆盖原路径下的可执行文件，再重启服务即可，不需要重新输入配置和密钥。改动过前端代码时先在 `frontend` 目录执行 `pnpm build`，再在仓库根目录执行：

```bash
cargo install --path ./backend --root "$HOME/.cargo" --force && \
  "$HOME/.cargo/bin/mihomo-connections-tracker" service restart
```

安装在 `~/.local/bin` 时使用相应的安装方式和路径。旧进程会在重启前继续运行旧版本；重启后 launchd 从同一路径加载新版本，数据库及日志保留。

旧版服务使用顶层启动参数，升级到此版本后必须重新执行一次 `service install` 更新服务配置。

旧版服务若仍指向 `Library/Application Support/mihomo-connections-tracker/bin/` 中的副本，需要用新安装的命令重新执行一次 `service install` 切换路径，之后即可按上述命令更新。

修改配置或更换程序安装路径时，重新执行 `mihomo-connections-tracker service install`，提供完整启动参数和所需密钥，已有配置不会合并；需要保留自定义数据库时须再次指定 `--database`。加载成功不代表采集已连接，请用 `service status`、健康检查及日志确认运行情况：

```bash
tail -f "$HOME/Library/Logs/mihomo-connections-tracker/stderr.log"
curl http://127.0.0.1:8080/api/v1/health
```

日志持续追加，当前未配置自动轮转。Mihomo 尚未启动时沿用每 5 秒重连；配置错误或端口冲突导致的退出也会持续重试，应查看错误日志修正后重新安装配置。卸载后若不再需要数据及日志，可另行手动删除保留目录。

## 统计口径

- 使用官方 `/connections?interval=1000` WebSocket，每秒接收活跃连接快照。使用 `/proxies` 和 `/providers/proxies` 识别代理组、节点类型和 Provider；启动、重连及每 60 秒刷新代理信息。
- 保留原始 `chains`、`providerChains` 顺序。出口节点对应的 Provider 为 `null`、空字符串，或整个 `providerChains` 为 `null` / 缺失时，普通代理归 `--main-airport`；非空 Provider 名作为机场名。
- 按节点类型区分直连、拒绝和普通代理，支持 `DIRECT-IPV4` 这类直连别名。直连、拒绝不归入主机场。
- 每条连接首次观察时固定机场、节点和代理组归属。链上每个代理组各统计一次，全局流量只统计一次；各组总量不可相加当作全局总量。
- 多个终端节点、拨号代理链、节点无法识别或链长度不一致时，机场归属为 `ambiguous`，不向候选机场重复累计。
- 流量按累计值的差计算。首次启动前就存在的连接只建立基线；监控期间新建的连接计入首次观察的累计值。基线持久化，重启不会重复累计。
- 原始计数下降时保存 `counter_reset` 异常并重建基线，该次增量为零。
- 时间戳使用 UTC Unix 毫秒；日/月边界固定为北京时间 UTC+8，自然月统计。增量归接收快照当天，跨午夜存在采样级误差。
- 断采后仍存活连接的增量归恢复当天，断采区间单独保存，不推算间断期间的每日分布。连接消失时间是观察到消失的时间，不是精确关闭时间。
- 单位为字节，`total = upload + download`。不根据节点名中的 `6x` 等字符推测机场计费倍率。

**这是连接采样统计，不是完整 HTTP 请求日志或机场账单。** 采样间隔内完成的短连接和关闭前的尾流可能漏计。连接域名优先使用 `host`，其次 `sniffHost`；没有域名则保留目标 IP，域名为 `null`，不通过反向 DNS 猜测。

## 数据保存

| 表 | 用途与保留策略 |
|---|---|
| `connections` | 连接元数据、固定归属、原始计数基线、已观察流量、最后观察状态 |
| `connection_groups` | 连接经过的代理组，去重保存 |
| `connection_daily_usage` | 每连接每日用量，保留当前月及此前 11 个自然月 |
| `daily_totals` | 全局、机场和代理组的每日上下行汇总，长期保留 |
| `attribution_anomalies` | 归属/计数异常及原始诊断证据，长期保留 |
| `collection_gaps` | 断采区间，长期保留 |
| `collector_state` | 采集状态及最后成功时间 |

启动以及采集跨日时清理过期明细；活跃或断采中状态未知的连接保留基线。已消失且没有保留期内明细的连接元数据随之清理。每日汇总、异常及断采记录不受明细清理影响。

异常按 `(connection_id, reason)` 去重，保留首次和最近的完整连接 JSON（包括未知字段）、首次异常的原始 `/proxies`、`/providers/proxies`、Mihomo 版本和拓扑获取时间，以及首次/最近观察时间和出现次数。不会为每秒重复保存整份拓扑。异常不关联连接明细外键，因此明细清理不会删除诊断证据。

单个采集任务顺序处理快照，在同一事务里提交计数基线、连接日用量、每日汇总和异常；数据库失败会结束服务并报告错误。SQLite 使用 WAL。数据库应只由一个采集进程写入。

## 查询和验证

### 命令行查询流量

`usage` 子命令直接查询数据库并输出格式化后的上下行流量，不需要服务正在运行：

```bash
mihomo-connections-tracker usage                 # 最近 7 天全部流量
mihomo-connections-tracker usage --day           # 今天，也可 --day 2026-11-01
mihomo-connections-tracker usage --month         # 本月，也可 --month 2026-10
mihomo-connections-tracker usage --last 30       # 最近 30 天

mihomo-connections-tracker usage --airport       # 列出所有机场及各自流量
mihomo-connections-tracker usage --airport 飞鸟  # 某个机场
mihomo-connections-tracker usage --group         # 列出所有代理组
mihomo-connections-tracker usage --group 节点选择 --month 2026-10
```

时间参数 `--day` / `--month` / `--last` 三选一，都不传默认最近 7 天；日/月边界为北京时间。`--airport` / `--group` 二选一，不传值时列出该维度全部名称（按流量降序），用于先确认名称再精确查询。单位按 1024 进制格式化为 B/KB/MB/GB。默认读取服务数据库 `Library/Application Support/mihomo-connections-tracker/tracker.db`，前台运行等其他库通过 `--database` 指定。

### Web 控制台

`web` 子命令在前台启动网页控制台，同时提供页面与查询 API，不依赖采集服务是否运行（直接只读数据库，服务运行中也可安全共用）：

```bash
mihomo-connections-tracker web                                  # 默认 http://127.0.0.1:8081/
mihomo-connections-tracker web --listen-port 9000 --database /path/to/tracker.db
mihomo-connections-tracker web --api-token 密钥                  # 可选，保护查询接口
```

- 页面提供概览、机场、代理组三个视图：时间范围预设（今天/近 7 天/近 30 天/本月/上月）与自定义区间，机场/代理组排行可点击查看单个名称的每日上下行柱状图、汇总卡片与明细表，支持按日/按月聚合。机场维度中直连、拒绝、归属不明确的流量单独成行。
- 设置了 `--api-token` 时，首次打开页面点击右上角「API 密钥」填入即可。
- 端口默认 `8081`，避开采集服务的查询 API 默认端口 `8080`，两者可以同时运行。密钥也可通过 `TRACKER_API_TOKEN` 环境变量传入。
- 仅前台运行，Ctrl+C 或 SIGTERM 退出；不提供、也不接入任何后台服务或开机自启方式。数据库文件不存在时直接报错退出，不会创建空库。

前端开发模式：终端 1 运行 `cargo run --manifest-path backend/Cargo.toml -- web`，终端 2 在 `frontend` 目录运行 `pnpm dev`，开发服务器会把 `/api` 代理到 `127.0.0.1:8081`。

### 查询 API

```bash
curl 'http://127.0.0.1:8080/api/v1/stats?dimension=airport&period=month'
curl 'http://127.0.0.1:8080/api/v1/stats?dimension=group&period=day'
curl 'http://127.0.0.1:8080/api/v1/connections?month=2026-11&domain=chatgpt.com'
curl 'http://127.0.0.1:8080/api/v1/anomalies'

cd backend
cargo fmt --check
cargo check
cargo test
```

设置查询密钥时携带 `Authorization: Bearer <查询密钥>`。参数、字段和示例见 [API 文档](api.md)。

## 旧版迁移边界

`master`、`agent` 子命令以及 `/sync`、`/agents` 等接口已移除。使用新的数据库路径启动；检测到旧版数据库的 `sync_state` 表或 `agent_id` 列时会拒绝打开，不自动迁移或拆分旧累计流量。旧文件保留供原版本读取，不能从旧累计值还原历史每日用量。

数据源依据：[Mihomo 官方 API](https://wiki.metacubex.one/en/api/#connections)。文档与当前版本返回值存在差异时，以实际响应保留的原始数据作为诊断依据。
