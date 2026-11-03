import { useEffect, useState } from 'react'
import {
  fetchConnections,
  fetchDomains,
  type ConnectionRow,
  type DomainRow,
} from '../api'
import { formatBytes, formatTime } from '../format'

const PAGE_SIZE = 50

type View = 'records' | 'targets'

type PanelData =
  | { view: 'records'; items: ConnectionRow[]; totalCount: number }
  | { view: 'targets'; items: DomainRow[]; totalCount: number }

const STATUS_LABEL: Record<ConnectionRow['status'], string> = {
  active: '活跃',
  unknown: '未知',
  disappeared: '已断开',
}

interface ConnectionsPanelProps {
  month: string
  airport?: string
  group?: string
}

export function ConnectionsPanel({ month, airport, group }: ConnectionsPanelProps) {
  const [view, setView] = useState<View>('records')
  const [page, setPage] = useState(1)
  const [data, setData] = useState<PanelData | null>(null)
  const [error, setError] = useState<string | null>(null)

  useEffect(() => {
    let cancelled = false
    setData(null)
    setError(null)
    const params = {
      month,
      airport,
      group,
      limit: PAGE_SIZE,
      offset: (page - 1) * PAGE_SIZE,
    }
    const request =
      view === 'records'
        ? fetchConnections(params).then(
            (result): PanelData => ({
              view: 'records',
              items: result.items,
              totalCount: result.totalCount,
            }),
          )
        : fetchDomains(params).then(
            (result): PanelData => ({
              view: 'targets',
              items: result.items,
              totalCount: result.totalCount,
            }),
          )
    request
      .then((result) => !cancelled && setData(result))
      .catch((cause: unknown) => {
        if (!cancelled) setError(cause instanceof Error ? cause.message : '请求失败')
      })
    return () => {
      cancelled = true
    }
  }, [view, month, airport, group, page])

  const current = data?.view === view ? data : null
  const totalPages = current ? Math.max(1, Math.ceil(current.totalCount / PAGE_SIZE)) : 1
  const targetName = airport ?? group

  return (
    <div>
      <div className="mb-3 flex justify-end">
        <div className="flex rounded-md bg-gray-100 p-0.5">
          {(
            [
              ['records', '逐条记录'],
              ['targets', '按目标聚合'],
            ] as Array<[View, string]>
          ).map(([key, label]) => (
            <button
              key={key}
              type="button"
              onClick={() => {
                setView(key)
                setPage(1)
              }}
              className={
                view === key
                  ? 'rounded bg-white px-2.5 py-1 text-xs text-gray-900 shadow-sm'
                  : 'px-2.5 py-1 text-xs text-gray-500 hover:text-gray-800'
              }
            >
              {label}
            </button>
          ))}
        </div>
      </div>

      {error ? (
        <div className="rounded-md border border-red-200 bg-red-50 px-4 py-3 text-sm text-red-700">
          {error}
        </div>
      ) : !current ? (
        <div className="py-10 text-center text-sm text-gray-400">加载中…</div>
      ) : current.items.length === 0 ? (
        <div className="py-10 text-center text-sm text-gray-400">
          {month} 内没有「{targetName}」的连接记录
        </div>
      ) : (
        <>
          <div className="overflow-x-auto">
            {current.view === 'records' ? (
              <table className="w-full text-sm">
                <thead>
                  <tr className="border-b border-gray-200 text-left text-gray-500">
                    <th className="px-3 py-2.5 font-medium">域名 / IP</th>
                    <th className="px-3 py-2.5 text-right font-medium">上行</th>
                    <th className="px-3 py-2.5 text-right font-medium">下行</th>
                    <th className="px-3 py-2.5 font-medium">规则</th>
                    <th className="px-3 py-2.5 font-medium">进程</th>
                    <th className="px-3 py-2.5 font-medium">网络</th>
                    <th className="px-3 py-2.5 font-medium">状态</th>
                    <th className="px-3 py-2.5 font-medium">首次观察</th>
                  </tr>
                </thead>
                <tbody>
                  {current.items.map((row) => (
                    <tr key={row.id} className="border-b border-gray-50 tabular-nums">
                      <td
                        className="max-w-56 truncate px-3 py-2 text-gray-800"
                        title={row.domain ?? row.destination_ip ?? ''}
                      >
                        {row.domain ?? row.destination_ip ?? '-'}
                      </td>
                      <td className="px-3 py-2 text-right text-indigo-600">
                        {formatBytes(row.upload)}
                      </td>
                      <td className="px-3 py-2 text-right text-emerald-600">
                        {formatBytes(row.download)}
                      </td>
                      <td className="px-3 py-2 text-gray-600">
                        <span className="text-gray-400">{row.rule}</span>
                        {row.rule_payload && <span className="text-gray-700">:{row.rule_payload}</span>}
                      </td>
                      <td className="max-w-32 truncate px-3 py-2 text-gray-600">
                        {row.process || '-'}
                      </td>
                      <td className="px-3 py-2 text-gray-500">{row.network}</td>
                      <td className="px-3 py-2 text-gray-500">{STATUS_LABEL[row.status]}</td>
                      <td className="px-3 py-2 text-gray-500">{formatTime(row.first_seen)}</td>
                    </tr>
                  ))}
                </tbody>
              </table>
            ) : (
              <table className="w-full text-sm">
                <thead>
                  <tr className="border-b border-gray-200 text-left text-gray-500">
                    <th className="px-3 py-2.5 font-medium">域名 / IP</th>
                    <th className="px-3 py-2.5 text-right font-medium">连接数</th>
                    <th className="px-3 py-2.5 text-right font-medium">上行</th>
                    <th className="px-3 py-2.5 text-right font-medium">下行</th>
                    <th className="px-3 py-2.5 text-right font-medium">合计</th>
                  </tr>
                </thead>
                <tbody>
                  {current.items.map((row) => (
                    <tr key={row.domain ?? ''} className="border-b border-gray-50 tabular-nums">
                      <td
                        className="max-w-72 truncate px-3 py-2 text-gray-800"
                        title={row.domain ?? ''}
                      >
                        {row.domain ?? '（无域名 / IP）'}
                      </td>
                      <td className="px-3 py-2 text-right text-gray-600">{row.connection_count}</td>
                      <td className="px-3 py-2 text-right text-indigo-600">
                        {formatBytes(row.upload)}
                      </td>
                      <td className="px-3 py-2 text-right text-emerald-600">
                        {formatBytes(row.download)}
                      </td>
                      <td className="px-3 py-2 text-right text-gray-900">
                        {formatBytes(row.total)}
                      </td>
                    </tr>
                  ))}
                </tbody>
              </table>
            )}
          </div>
          <div className="mt-3 flex items-center justify-between text-sm text-gray-500">
            <span>
              共 {current.totalCount} 条 · 每页 {PAGE_SIZE} 条
            </span>
            <div className="flex items-center gap-2">
              <button
                type="button"
                disabled={page <= 1 || totalPages <= 1}
                onClick={() => setPage((current) => current - 1)}
                className="rounded-md border border-gray-200 px-3 py-1.5 disabled:cursor-not-allowed disabled:opacity-40 hover:border-indigo-300 hover:text-indigo-600"
              >
                上一页
              </button>
              <span className="tabular-nums">
                {page} / {totalPages}
              </span>
              <button
                type="button"
                disabled={page >= totalPages || totalPages <= 1}
                onClick={() => setPage((current) => current + 1)}
                className="rounded-md border border-gray-200 px-3 py-1.5 disabled:cursor-not-allowed disabled:opacity-40 hover:border-indigo-300 hover:text-indigo-600"
              >
                下一页
              </button>
            </div>
          </div>
        </>
      )}
    </div>
  )
}
