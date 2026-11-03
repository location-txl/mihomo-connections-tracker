import {
  Bar,
  BarChart,
  CartesianGrid,
  Legend,
  ResponsiveContainer,
  Tooltip,
  XAxis,
  YAxis,
} from 'recharts'
import { formatBytes } from '../format'
import type { PeriodUsage } from '../stats'

const UP_COLOR = '#6366f1'
const DOWN_COLOR = '#10b981'

interface UsageChartProps {
  data: PeriodUsage[]
}

export function UsageChart({ data }: UsageChartProps) {
  if (data.length === 0) {
    return <div className="flex h-80 items-center justify-center text-sm text-gray-400">该时间段内没有数据</div>
  }
  return (
    <div className="h-80">
      <ResponsiveContainer width="100%" height="100%">
        <BarChart data={data} margin={{ top: 8, right: 8, left: 8, bottom: 0 }}>
          <CartesianGrid strokeDasharray="3 3" vertical={false} stroke="#f0f0f2" />
          <XAxis dataKey="period" tick={{ fontSize: 12, fill: '#6b7280' }} tickLine={false} />
          <YAxis
            tickFormatter={(value) => formatBytes(Number(value))}
            tick={{ fontSize: 12, fill: '#6b7280' }}
            tickLine={false}
            width={84}
          />
          <Tooltip
            formatter={(value) => formatBytes(Number(value))}
            labelStyle={{ color: '#111827' }}
            contentStyle={{ borderRadius: 8, border: '1px solid #e5e7eb' }}
          />
          <Legend />
          <Bar dataKey="upload" name="上行" fill={UP_COLOR} radius={[3, 3, 0, 0]} />
          <Bar dataKey="download" name="下行" fill={DOWN_COLOR} radius={[3, 3, 0, 0]} />
        </BarChart>
      </ResponsiveContainer>
    </div>
  )
}
