import { useEffect, useMemo, useState } from 'react'
import { ApiError, fetchStatsRows, type StatsRow } from './api'
import { DailyTable } from './components/DailyTable'
import { RankList } from './components/RankList'
import { SummaryCards } from './components/SummaryCards'
import { TimeRangePicker, presetRange, type DateRange, type Preset } from './components/TimeRangePicker'
import { TokenSetting } from './components/TokenSetting'
import { UsageChart } from './components/UsageChart'
import { aggregateByRank, seriesOf, sumUsage, type RankItem } from './stats'

type Dimension = 'global' | 'airport' | 'group'

const TABS: Array<{ key: Dimension; label: string }> = [
  { key: 'global', label: '概览' },
  { key: 'airport', label: '机场' },
  { key: 'group', label: '代理组' },
]

export default function App() {
  const [dimension, setDimension] = useState<Dimension>('global')
  const [preset, setPreset] = useState<Preset>('7d')
  const [range, setRange] = useState<DateRange>(() => presetRange('7d'))
  const [selected, setSelected] = useState<RankItem | null>(null)
  const [grain, setGrain] = useState<'day' | 'month'>('day')
  const [rows, setRows] = useState<StatsRow[] | null>(null)
  const [error, setError] = useState<string | null>(null)
  const [loading, setLoading] = useState(false)

  useEffect(() => {
    let cancelled = false
    setLoading(true)
    setError(null)
    fetchStatsRows(dimension, range.from, range.to)
      .then((items) => !cancelled && setRows(items))
      .catch((cause: unknown) => {
        if (cancelled) return
        if (cause instanceof ApiError && cause.status === 401) {
          setError('查询认证失败：点击右上角「API 密钥」填入服务设置的密钥')
        } else {
          setError(cause instanceof Error ? cause.message : '请求失败')
        }
        setRows(null)
      })
      .finally(() => !cancelled && setLoading(false))
    return () => {
      cancelled = true
    }
  }, [dimension, range.from, range.to])

  // 切换维度后原选中项不再有效，回到聚合视图；清空旧数据避免错误闪现。
  // 点击当前维度不重新请求，否则清空后 effect 依赖未变，不会重新拉取。
  const switchDimension = (next: Dimension) => {
    if (next === dimension) return
    setDimension(next)
    setSelected(null)
    setRows(null)
  }

  // 时间范围未变化时忽略，原因同 switchDimension。
  const applyRange = (nextPreset: Preset, nextRange: DateRange) => {
    if (
      nextPreset === preset &&
      nextRange.from === range.from &&
      nextRange.to === range.to
    ) {
      return
    }
    setPreset(nextPreset)
    setRange(nextRange)
    setRows(null)
  }

  const rank = useMemo(() => (rows ? aggregateByRank(rows) : []), [rows])
  const series = useMemo(
    () => (rows ? seriesOf(rows, dimension === 'global' ? null : selected, grain) : []),
    [rows, dimension, selected, grain],
  )
  const summary = useMemo(() => sumUsage(series), [series])

  return (
    <div className="min-h-screen bg-gray-50 text-gray-900">
      <header className="border-b border-gray-200 bg-white">
        <div className="mx-auto flex max-w-6xl flex-wrap items-center justify-between gap-3 px-4 py-3">
          <h1 className="text-lg font-semibold">Mihomo 流量统计</h1>
          <TokenSetting />
        </div>
        <div className="mx-auto flex max-w-6xl flex-wrap items-center justify-between gap-3 px-4 pb-3">
          <nav className="flex rounded-lg bg-gray-100 p-1">
            {TABS.map(({ key, label }) => (
              <button
                key={key}
                type="button"
                onClick={() => switchDimension(key)}
                className={
                  dimension === key
                    ? 'rounded-md bg-white px-4 py-1.5 text-sm font-medium text-indigo-600 shadow-sm'
                    : 'rounded-md px-4 py-1.5 text-sm text-gray-500 hover:text-gray-800'
                }
              >
                {label}
              </button>
            ))}
          </nav>
          <TimeRangePicker
            preset={preset}
            range={range}
            onPresetChange={applyRange}
          />
        </div>
      </header>

      <main className="mx-auto max-w-6xl space-y-5 px-4 py-6">
        {error && (
          <div className="rounded-lg border border-red-200 bg-red-50 px-4 py-3 text-sm text-red-700">
            {error}
          </div>
        )}

        <SummaryCards usage={summary} />

        <div className="grid grid-cols-1 gap-5 lg:grid-cols-3">
          <section className="rounded-lg border border-gray-200 bg-white p-4 lg:col-span-2">
            <div className="mb-3 flex items-center justify-between">
              <h2 className="text-sm font-medium text-gray-700">
                {selected ? selected.label : dimension === 'airport' ? '机场合计' : dimension === 'group' ? '代理组合计' : '全局'}
                <span className="ml-1 font-normal text-gray-400">
                  {grain === 'day' ? '按日' : '按月'}
                </span>
              </h2>
              <div className="flex rounded-md bg-gray-100 p-0.5">
                {(['day', 'month'] as const).map((key) => (
                  <button
                    key={key}
                    type="button"
                    onClick={() => setGrain(key)}
                    className={
                      grain === key
                        ? 'rounded bg-white px-2.5 py-1 text-xs text-gray-900 shadow-sm'
                        : 'px-2.5 py-1 text-xs text-gray-500 hover:text-gray-800'
                    }
                  >
                    {key === 'day' ? '按日' : '按月'}
                  </button>
                ))}
              </div>
            </div>
            {loading && !rows ? (
              <div className="flex h-80 items-center justify-center text-sm text-gray-400">
                加载中…
              </div>
            ) : (
              <UsageChart data={series} />
            )}
          </section>

          {dimension !== 'global' && (
            <section className="rounded-lg border border-gray-200 bg-white">
              <h2 className="border-b border-gray-100 px-4 py-3 text-sm font-medium text-gray-700">
                {dimension === 'airport' ? '机场' : '代理组'}排行
                <span className="ml-1 font-normal text-gray-400">（点击查看趋势）</span>
              </h2>
              <RankList
                items={rank}
                selected={selected}
                onSelect={(item) => {
                  setSelected(item)
                  setGrain('day')
                }}
              />
            </section>
          )}
        </div>

        <section className="rounded-lg border border-gray-200 bg-white p-4">
          <h2 className="mb-3 text-sm font-medium text-gray-700">
            {grain === 'day' ? '每日' : '每月'}明细
          </h2>
          <DailyTable rows={series} />
        </section>
      </main>
    </div>
  )
}
