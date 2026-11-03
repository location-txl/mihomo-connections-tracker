# Mihomo 连接流量统计

Rust 常驻后端直接采集一个 Mihomo，将数据写入 SQLite，提供只读查询 API。本项目不包含 UI，也不再支持 Master/Agent 或数据同步。

## 启动

```bash
cd backend
cargo build --release
read -rs 'MIHOMO_TOKEN?Mihomo 密钥: '; echo
export MIHOMO_TOKEN
./target/release/mihomo-connections-tracker \
  --mihomo-host 127.0.0.1 \
  --mihomo-port 9097 \
  --main-airport '主机场' \
  --database tracker.db
```

上面的密钥输入示例适用于 zsh；也可通过进程环境注入 `MIHOMO_TOKEN`。不需要认证时不设置此变量。密钥不写入数据库或日志。

| 参数 | 默认值 / 含义 |
|---|---|
| `--database` | `tracker.db`，SQLite 文件 |
| `--mihomo-host` | `127.0.0.1`，单个 Mihomo HTTP 控制接口 |
| `--mihomo-port` | `9090` |
| `--mihomo-token` | 可选，优先使用环境变量 `MIHOMO_TOKEN` 注入 |
| `--main-airport` | 必填，空 Provider 节点归属的机场名 |
| `--listen-host` | `127.0.0.1`，查询 API 监听地址 |
| `--listen-port` | `8080` |
| `--api-token` | 可选，也可使用 `TRACKER_API_TOKEN`，保护健康检查以外的查询接口 |

按 Ctrl+C 时完成当前数据库事务再退出。连接失败每 5 秒重连；15 秒没有有效快照会重新连接。

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
