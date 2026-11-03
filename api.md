# 后端查询 API

基础地址：`http://127.0.0.1:8080/api/v1`。只提供查询，不包含 UI、设备管理或同步接口。

## 公共约定

除 `/health` 外，设置 `--api-token` 或 `TRACKER_API_TOKEN` 后均要求 `Authorization: Bearer <token>`。

```json
{"status":"success","data":{},"message":null}
```

错误使用相同结构，`status=error`、`data=null`，并返回 400（参数）、401（认证）、404（接口）、405（方法）或 500（数据库/内部错误）。空结果正常返回空数组，不掩盖查询失败。

- 字节字段是 64 位整数，`total = upload + download`。
- 时间戳为 UTC Unix 毫秒，日期按北京时间 UTC+8 划分。
- 日期范围为 `[from, to)`；`from/to` 使用 `YYYY-MM-DD`，默认当月第一天至下月第一天。
- 分页 `limit` 默认 50，允许 1..500；`offset` 默认 0，必须非负。
- 域名忽略大小写与末尾的点；机场、代理组、规则载荷按精确值筛选，URL 中应编码中文和特殊字符。
- `route_kind` 为 `proxy`、`direct`、`reject`、`ambiguous`。机场名称为 `null` 时，通过这个字段区分直连、拒绝和归属不明确。

## GET /health

无需认证，返回服务的采集状态。HTTP 200 表示查询服务可访问，是否连接 Mihomo 以 `connected` 判断。

```json
{"status":"success","data":{"connected":true,"last_success_at":1793700000000,"last_error":null,"timezone":"Asia/Shanghai","sampling_interval_ms":1000},"message":null}
```

`last_success_at` 为空表示尚未成功采集；`last_error` 是最近断采原因。断采时原有连接状态变为 `unknown`，收到完整有效快照后才判定哪些连接已消失。

## GET /stats

| 参数 | 含义 |
|---|---|
| `dimension` | `global`（默认）、`airport`、`group` |
| `period` | `day`（默认）或 `month` |
| `name` | 可选，机场或代理组精确名称，`global` 不接受此参数 |
| `from`、`to` | 左闭右开的日期范围 |

```text
/stats?dimension=airport&period=month&from=2026-11-01&to=2026-12-01
/stats?dimension=group&period=day&name=ChatGPT&from=2026-11-01&to=2026-12-01
```

```json
{"status":"success","data":{"dimension":"airport","period":"month","from":"2026-11-01","to":"2026-12-01","timezone":"Asia/Shanghai","items":[{"period":"2026-11","name":"宝可梦专线","route_kind":"proxy","upload":7485,"download":9929,"total":17414}]},"message":null}
```

机场维度还会返回名称为 `null` 的直连、拒绝、归属不明确记录，用 `route_kind` 区分。全局和代理组维度的 `route_kind` 为 `null`，表示合并各类型。结果按日期、名称排序，不补齐没有数据的日期。月统计只累计所选范围内的日汇总；获取完整月须传入完整自然月范围。

代理组之间存在重叠，不能相加得到全局用量。全局、机场维度各自完整分区，每笔流量仅计入一次；`ambiguous` 不会分摊到候选机场。

## GET /connections

查询某月被观察到的连接，包含零流量连接及跨月连接。参数：

| 参数 | 含义 |
|---|---|
| `month` | `YYYY-MM`，默认当前月 |
| `airport`、`group` | 精确匹配机场或链上代理组，可同时使用 |
| `domain` | 精确匹配规范化域名 |
| `rule` | 精确匹配 `rulePayload`，例如 `openai`、`twiiter` |
| `limit`、`offset` | 分页 |

```text
/connections?month=2026-11&group=ChatGPT&domain=chatgpt.com&limit=50&offset=0
```

`data` 包含 `items`、`total_count`、`limit`、`offset`、`from`、`to`。按首次观察时间降序、连接 ID 排序。每项包含：

- `id`、`start`、`first_seen`、`last_seen`、`disappeared_at`、`status`。
- `airport`、`route_kind`、`node`、`groups`、原始顺序的 `chains`、`provider_chains`。
- `domain`、`destination_ip`、`network`、`process`、`rule`、`rule_payload`。
- `metadata`：完整 Mihomo 元数据 JSON，保留未知字段。
- `upload`、`download`、`total`：**所选月份**观察到的字节数。
- `observed_upload`、`observed_download`：该连接从开始监控起累计观察到的字节数。
- `last_raw_upload`、`last_raw_download`：Mihomo 最近返回的原始累计计数，可能包含监控启动前流量。

`status` 为 `active`、`unknown`、`disappeared`。`disappeared_at` 为观察到连接消失的时间，不能据此推断精确结束时刻。

明细只覆盖当前月及此前 11 个自然月，过期查询返回空集合；历史汇总仍可使用 `/stats` 查询。

## GET /domains

接受与 `/connections` 相同的参数，按域名汇总保留期内的连接日志。`data.items` 中每项包含 `domain`、`connection_count`、`upload`、`download`、`total`，另返回分页及日期范围。按总流量降序、域名排序。

连接数按所选月份内被观察到的连接去重，不是 HTTP 请求数。无域名的连接归入 `domain=null`，目标 IP 不冒充域名。代理组筛选使用成员关系，不因嵌套组重复统计。

## GET /anomalies

参数：可选 `connection_id`、`reason`，以及 `limit`、`offset`。按最近观察时间降序、异常 ID 降序返回，`data` 包含 `items` 和分页信息。

每项包含：`id`、`connection_id`、`reason`、`first_seen`、`last_seen`、`occurrences`、`first_raw_connection`、`latest_raw_connection`、`raw_context`。

原始数据以 JSON 对象返回，不是二次编码的字符串。`raw_context` 保存首次异常使用的完整代理、Provider、版本响应和拓扑获取时间，可与连接原文一起用于重放诊断；最近原文可能来自后续拓扑版本，首次上下文不被覆盖。

| reason | 含义 |
|---|---|
| `ambiguous_multihop` | 多终端链或存在拨号代理，无法确定唯一机场 |
| `chain_length_mismatch` | `chains` 与非空 `providerChains` 数组长度不一致 |
| `missing_terminal` | 代理链为空或只有代理组，无法找到终端 |
| `unresolved_node` | 在相应 Provider 或主配置中找不到节点 |
| `unresolved_terminal` | 终端策略不能识别为可归属的代理 |
| `counter_reset` | 同一连接原始累计计数下降，该帧重建基线不计增量 |

空 Provider 按主机场正常处理，不生成异常。异常默认长期保留，不随连接明细删除。

## GET /collection-gaps

接受 `from`、`to` 日期范围，返回与范围重叠的断采区间：`id`、`started_at`、`ended_at`、`reason`。`ended_at=null` 表示尚未恢复有效采集。

包括首次启动等待、服务停止、连接失败、WebSocket 断开、快照超时或无效快照等原因。不对断采期间按机场/代理组的漏计流量进行推算。
