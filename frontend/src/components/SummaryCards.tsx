import { formatBytes } from '../format'
import type { Usage } from '../stats'

interface SummaryCardsProps {
  usage: Usage
}

const CARDS: Array<{ key: keyof Usage; label: string; className: string }> = [
  { key: 'upload', label: '上行', className: 'text-indigo-600' },
  { key: 'download', label: '下行', className: 'text-emerald-600' },
  { key: 'total', label: '合计', className: 'text-gray-900' },
]

export function SummaryCards({ usage }: SummaryCardsProps) {
  return (
    <div className="grid grid-cols-1 gap-4 sm:grid-cols-3">
      {CARDS.map(({ key, label, className }) => (
        <div key={key} className="rounded-lg border border-gray-200 bg-white px-5 py-4">
          <div className="text-sm text-gray-500">{label}</div>
          <div className={`mt-1 text-2xl font-semibold tabular-nums ${className}`}>
            {formatBytes(usage[key])}
          </div>
        </div>
      ))}
    </div>
  )
}
