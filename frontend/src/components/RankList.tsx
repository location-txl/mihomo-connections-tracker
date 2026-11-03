import { formatBytes } from '../format'
import type { RankItem } from '../stats'

interface RankListProps {
  items: RankItem[]
  selected: RankItem | null
  onSelect: (item: RankItem | null) => void
}

export function RankList({ items, selected, onSelect }: RankListProps) {
  if (items.length === 0) {
    return <div className="py-10 text-center text-sm text-gray-400">该时间段内没有数据</div>
  }
  const max = Math.max(...items.map((item) => item.total))
  return (
    <ul className="divide-y divide-gray-100">
      {items.map((item) => {
        const isSelected =
          selected?.name === item.name && selected?.routeKind === item.routeKind
        return (
          <li key={`${item.name ?? ''}|${item.routeKind ?? ''}`}>
            <button
              type="button"
              onClick={() => onSelect(isSelected ? null : item)}
              className="w-full px-4 py-2.5 text-left hover:bg-gray-50"
            >
              <div className="flex items-baseline justify-between gap-2">
                <span
                  className={
                    isSelected
                      ? 'truncate text-sm font-medium text-indigo-600'
                      : 'truncate text-sm text-gray-700'
                  }
                  title={item.label}
                >
                  {item.label}
                </span>
                <span className="shrink-0 text-xs text-gray-400">
                  ↑ {formatBytes(item.upload)} ↓ {formatBytes(item.download)}
                </span>
              </div>
              <div className="mt-1.5 flex items-center gap-2">
                <div className="h-1.5 flex-1 overflow-hidden rounded-full bg-gray-100">
                  <div
                    className={isSelected ? 'h-full rounded-full bg-indigo-500' : 'h-full rounded-full bg-indigo-300'}
                    style={{ width: `${max > 0 ? (item.total / max) * 100 : 0}%` }}
                  />
                </div>
                <span className="w-20 shrink-0 text-right text-sm tabular-nums text-gray-800">
                  {formatBytes(item.total)}
                </span>
              </div>
            </button>
          </li>
        )
      })}
    </ul>
  )
}
