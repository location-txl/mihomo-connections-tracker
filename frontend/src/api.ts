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
