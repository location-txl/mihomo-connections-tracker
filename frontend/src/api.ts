const TOKEN_KEY = 'tracker-api-token'

export function getToken(): string {
  return localStorage.getItem(TOKEN_KEY) ?? ''
}

export function setToken(token: string): void {
  if (token) localStorage.setItem(TOKEN_KEY, token)
  else localStorage.removeItem(TOKEN_KEY)
}

export interface StatsRow {
  period: string
  name: string | null
  route_kind: string | null
  upload: number
  download: number
  total: number
}

export class ApiError extends Error {
  status: number

  constructor(message: string, status: number) {
    super(message)
    this.status = status
  }
}

async function request<T>(path: string, params: Record<string, string>): Promise<T> {
  const query = new URLSearchParams(params).toString()
  const headers: Record<string, string> = {}
  const token = getToken()
  if (token) headers.Authorization = `Bearer ${token}`
  const response = await fetch(`${path}?${query}`, { headers })
  const body = await response.json().catch(() => null)
  if (!response.ok || body?.status !== 'success') {
    throw new ApiError(body?.message ?? `请求失败（HTTP ${response.status}）`, response.status)
  }
  return body.data as T
}

/** 一次拉取某维度在 [from, to) 内的全部日汇总，排行与下钻序列都从这份数据聚合。 */
export function fetchStatsRows(dimension: string, from: string, to: string) {
  return request<{ items: StatsRow[] }>('/api/v1/stats', {
    dimension,
    period: 'day',
    from,
    to,
  }).then((data) => data.items)
}

export interface ConnectionRow {
  id: string
  first_seen: number
  last_seen: number
  status: 'active' | 'unknown' | 'disappeared'
  node: string | null
  domain: string | null
  destination_ip: string | null
  network: string
  process: string
  rule: string
  rule_payload: string
  upload: number
  download: number
  total: number
}

export interface ConnectionsResult {
  items: ConnectionRow[]
  totalCount: number
}

/** 连接明细分页查询；明细只保留近 12 个月，按月查询。 */
export function fetchConnections(params: {
  month: string
  airport?: string
  group?: string
  limit: number
  offset: number
}) {
  const query: Record<string, string> = {
    month: params.month,
    limit: String(params.limit),
    offset: String(params.offset),
  }
  if (params.airport) query.airport = params.airport
  if (params.group) query.group = params.group
  return request<{
    items: ConnectionRow[]
    total_count: number
  }>('/api/v1/connections', query).then((data) => ({
    items: data.items,
    totalCount: data.total_count,
  }))
}

export interface DomainRow {
  domain: string | null
  connection_count: number
  upload: number
  download: number
  total: number
}

/** 按目标聚合（域名优先，无域名按 IP）的分页查询，总流量降序。 */
export function fetchDomains(params: {
  month: string
  airport?: string
  group?: string
  limit: number
  offset: number
}) {
  const query: Record<string, string> = {
    month: params.month,
    limit: String(params.limit),
    offset: String(params.offset),
  }
  if (params.airport) query.airport = params.airport
  if (params.group) query.group = params.group
  return request<{
    items: DomainRow[]
    total_count: number
  }>('/api/v1/domains', query).then((data) => ({
    items: data.items,
    totalCount: data.total_count,
  }))
}
