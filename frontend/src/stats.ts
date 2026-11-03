import type { StatsRow } from './api'
import { routeKindLabel } from './format'

export interface Usage {
  upload: number
  download: number
  total: number
}

/** 排行项：机场维度的直连/拒绝等行 name 为 null，用 routeKind 区分。 */
export interface RankItem extends Usage {
  name: string | null
  routeKind: string | null
  label: string
}

export interface PeriodUsage extends Usage {
  period: string
}

const usageOf = (row: StatsRow): Usage => ({
  upload: row.upload,
  download: row.download,
  total: row.total,
})

const addTo = (target: Usage, other: Usage) => {
  target.upload += other.upload
  target.download += other.download
  target.total += other.total
}

/** 按 (name, route_kind) 聚合出排行，总量降序。 */
export function aggregateByRank(rows: StatsRow[]): RankItem[] {
  const totals = new Map<string, RankItem>()
  for (const row of rows) {
    const key = `${row.name ?? ''}\0${row.route_kind ?? ''}`
    const item = totals.get(key) ?? {
      name: row.name,
      routeKind: row.route_kind,
      label: row.name ?? routeKindLabel(row.route_kind),
      upload: 0,
      download: 0,
      total: 0,
    }
    addTo(item, usageOf(row))
    totals.set(key, item)
  }
  return [...totals.values()].sort((a, b) => b.total - a.total)
}

/**
 * 时间序列：item 为 null 时汇总全部行（概览/未选择），否则只取该项；
 * grain=month 时按 YYYY-MM 再聚合。
 */
export function seriesOf(
  rows: StatsRow[],
  item: RankItem | null,
  grain: 'day' | 'month',
): PeriodUsage[] {
  const matched = item
    ? rows.filter(
        (row) => row.name === item.name && row.route_kind === item.routeKind,
      )
    : rows
  const totals = new Map<string, Usage>()
  for (const row of matched) {
    const key = grain === 'month' ? row.period.slice(0, 7) : row.period
    const usage = totals.get(key) ?? { upload: 0, download: 0, total: 0 }
    addTo(usage, usageOf(row))
    totals.set(key, usage)
  }
  return [...totals.entries()]
    .sort(([a], [b]) => a.localeCompare(b))
    .map(([period, usage]) => ({ period, ...usage }))
}

export function sumUsage(rows: PeriodUsage[]): Usage {
  const sum: Usage = { upload: 0, download: 0, total: 0 }
  for (const row of rows) addTo(sum, row)
  return sum
}
