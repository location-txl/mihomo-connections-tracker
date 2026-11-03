import { formatBytes } from '../format'
import type { PeriodUsage, Usage } from '../stats'

interface DailyTableProps {
  rows: PeriodUsage[]
}

export function DailyTable({ rows }: DailyTableProps) {
  if (rows.length === 0) {
    return <div className="py-10 text-center text-sm text-gray-400">该时间段内没有数据</div>
  }
  const total: Usage = rows.reduce(
    (sum, row) => ({
      upload: sum.upload + row.upload,
      download: sum.download + row.download,
      total: sum.total + row.total,
    }),
    { upload: 0, download: 0, total: 0 },
  )
  return (
    <div className="overflow-x-auto">
      <table className="w-full text-sm">
        <thead>
          <tr className="border-b border-gray-200 text-left text-gray-500">
            <th className="px-4 py-2.5 font-medium">期间</th>
            <th className="px-4 py-2.5 text-right font-medium">上行</th>
            <th className="px-4 py-2.5 text-right font-medium">下行</th>
            <th className="px-4 py-2.5 text-right font-medium">合计</th>
          </tr>
        </thead>
        <tbody>
          {rows.map((row) => (
            <tr key={row.period} className="border-b border-gray-50 tabular-nums">
              <td className="px-4 py-2 text-gray-700">{row.period}</td>
              <td className="px-4 py-2 text-right text-indigo-600">{formatBytes(row.upload)}</td>
              <td className="px-4 py-2 text-right text-emerald-600">{formatBytes(row.download)}</td>
              <td className="px-4 py-2 text-right text-gray-900">{formatBytes(row.total)}</td>
            </tr>
          ))}
          <tr className="tabular-nums">
            <td className="px-4 py-2.5 font-medium text-gray-900">合计</td>
            <td className="px-4 py-2.5 text-right font-medium text-indigo-600">
              {formatBytes(total.upload)}
            </td>
            <td className="px-4 py-2.5 text-right font-medium text-emerald-600">
              {formatBytes(total.download)}
            </td>
            <td className="px-4 py-2.5 text-right font-medium text-gray-900">
              {formatBytes(total.total)}
            </td>
          </tr>
        </tbody>
      </table>
    </div>
  )
}
