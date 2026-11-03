import { useState } from 'react'
import {
  addDays,
  addMonths,
  monthStart,
  toDateKey,
} from '../format'

export type Preset = 'today' | '7d' | '30d' | 'thisMonth' | 'lastMonth' | 'custom'

/** [from, to) 左闭右开，与 API 一致。 */
export interface DateRange {
  from: string
  to: string
}

const PRESETS: Array<{ key: Exclude<Preset, 'custom'>; label: string }> = [
  { key: 'today', label: '今天' },
  { key: '7d', label: '近 7 天' },
  { key: '30d', label: '近 30 天' },
  { key: 'thisMonth', label: '本月' },
  { key: 'lastMonth', label: '上月' },
]

export function presetRange(preset: Exclude<Preset, 'custom'>): DateRange {
  const today = new Date()
  switch (preset) {
    case 'today':
      return { from: toDateKey(today), to: toDateKey(addDays(today, 1)) }
    case '7d':
      return { from: toDateKey(addDays(today, -6)), to: toDateKey(addDays(today, 1)) }
    case '30d':
      return { from: toDateKey(addDays(today, -29)), to: toDateKey(addDays(today, 1)) }
    case 'thisMonth':
      return { from: toDateKey(monthStart(today)), to: toDateKey(addMonths(today, 1)) }
    case 'lastMonth':
      return { from: toDateKey(addMonths(today, -1)), to: toDateKey(monthStart(today)) }
  }
}

interface TimeRangePickerProps {
  preset: Preset
  range: DateRange
  onPresetChange: (preset: Preset, range: DateRange) => void
}

export function TimeRangePicker({ preset, range, onPresetChange }: TimeRangePickerProps) {
  const [customFrom, setCustomFrom] = useState(range.from)
  const [customTo, setCustomTo] = useState(toDateKey(new Date()))

  const pickCustom = (from: string, to: string) => {
    if (from && to && from <= to) {
      // to 含当天，转为排他日期
      onPresetChange('custom', { from, to: toDateKey(addDays(new Date(`${to}T00:00`), 1)) })
    }
  }

  return (
    <div className="flex flex-wrap items-center gap-2">
      {PRESETS.map(({ key, label }) => (
        <button
          key={key}
          type="button"
          onClick={() => onPresetChange(key, presetRange(key))}
          className={
            preset === key
              ? 'rounded-md bg-indigo-600 px-3 py-1.5 text-sm text-white'
              : 'rounded-md border border-gray-200 bg-white px-3 py-1.5 text-sm text-gray-600 hover:border-indigo-300 hover:text-indigo-600'
          }
        >
          {label}
        </button>
      ))}
      <div className="flex items-center gap-1.5">
        <input
          type="date"
          value={preset === 'custom' ? range.from : customFrom}
          onChange={(event) => {
            setCustomFrom(event.target.value)
            pickCustom(event.target.value, customTo)
          }}
          className={
            preset === 'custom'
              ? 'rounded-md border border-indigo-400 bg-white px-2 py-1.5 text-sm text-gray-700'
              : 'rounded-md border border-gray-200 bg-white px-2 py-1.5 text-sm text-gray-600'
          }
        />
        <span className="text-sm text-gray-400">~</span>
        <input
          type="date"
          value={customTo}
          onChange={(event) => {
            setCustomTo(event.target.value)
            pickCustom(customFrom, event.target.value)
          }}
          className={
            preset === 'custom'
              ? 'rounded-md border border-indigo-400 bg-white px-2 py-1.5 text-sm text-gray-700'
              : 'rounded-md border border-gray-200 bg-white px-2 py-1.5 text-sm text-gray-600'
          }
        />
      </div>
    </div>
  )
}
