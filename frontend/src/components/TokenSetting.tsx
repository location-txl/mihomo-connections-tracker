import { useState } from 'react'
import { getToken, setToken } from '../api'

/** 服务设置了 --api-token 时才需要填写；保存在浏览器 localStorage。 */
export function TokenSetting() {
  const [open, setOpen] = useState(false)
  const [value, setValue] = useState(getToken())

  const save = () => {
    setToken(value.trim())
    setOpen(false)
    location.reload()
  }

  if (!open) {
    return (
      <button
        type="button"
        onClick={() => setOpen(true)}
        className="rounded-md border border-gray-200 px-3 py-1.5 text-sm text-gray-500 hover:border-indigo-300 hover:text-indigo-600"
        title="查询 API 密钥"
      >
        API 密钥
      </button>
    )
  }
  return (
    <div className="flex items-center gap-1.5">
      <input
        type="password"
        value={value}
        placeholder="API 密钥（未设置可留空）"
        onChange={(event) => setValue(event.target.value)}
        onKeyDown={(event) => event.key === 'Enter' && save()}
        className="w-48 rounded-md border border-gray-200 px-2 py-1.5 text-sm"
        autoFocus
      />
      <button
        type="button"
        onClick={save}
        className="rounded-md bg-indigo-600 px-3 py-1.5 text-sm text-white"
      >
        保存
      </button>
    </div>
  )
}
