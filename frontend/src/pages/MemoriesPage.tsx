import { useState, useEffect, useCallback } from "react"
import { Link } from "react-router-dom"
import { api, type MemoryItem } from "@/api/client"
import { useAuth } from "@/auth/AuthProvider"
import { Badge } from "@/components/ui/badge"
import { Skeleton } from "@/components/ui/skeleton"
import { Button } from "@/components/ui/button"
import { cn } from "@/lib/utils"
import { ChevronDown, ChevronUp } from "lucide-react"

const statusLabel: Record<string, string> = {
  active: "活跃",
  archived: "已归档",
  deleted: "已删除",
}

export function MemoriesPage() {
  const { activeProject } = useAuth()
  const [memories, setMemories] = useState<MemoryItem[]>([])
  const [loading, setLoading] = useState(true)
  const [error, setError] = useState("")
  const [expandedUuid, setExpandedUuid] = useState<string | null>(null)

  const fetchMemories = useCallback(async () => {
    setLoading(true)
    setError("")
    try {
      const data = await api.memories.list()
      setMemories(data || [])
    } catch (e) {
      setMemories([])
      setError(e instanceof Error ? e.message : "加载失败")
    }
    setLoading(false)
  }, [])

  useEffect(() => { fetchMemories() }, [activeProject, fetchMemories])

  const toggleRow = (uuid: string) => {
    setExpandedUuid(expandedUuid === uuid ? null : uuid)
  }

  return (
    <div className="max-w-4xl mx-auto p-4 sm:p-6">
      <div className="flex items-center justify-between mb-4">
        <h1 className="text-lg font-semibold text-foreground">记忆</h1>
        <Button variant="outline" size="sm" onClick={fetchMemories} disabled={loading}>刷新</Button>
      </div>

      {loading ? (
        <div className="space-y-1">
          {Array.from({ length: 5 }).map((_, i) => <Skeleton key={i} className="h-12 w-full rounded-md" />)}
        </div>
      ) : error ? (
        <div className="text-center py-12">
          <p className="text-destructive mb-3">{error}</p>
          <Button variant="outline" size="sm" onClick={fetchMemories}>重试</Button>
        </div>
      ) : memories.length === 0 ? (
        <p className="text-muted-foreground text-center py-12">暂无记忆</p>
      ) : (
        <div className="rounded-lg border">
          {memories.map((m, idx) => {
            const isExpanded = expandedUuid === m.memory_uuid
            return (
              <div key={m.memory_uuid} className={cn(
                "transition-colors",
                idx > 0 && "border-t"
              )}>
                {/* 行头 */}
                <div
                  className="flex items-center gap-3 px-4 py-3 cursor-pointer hover:bg-accent/30 transition-colors min-h-[44px]"
                  onClick={() => toggleRow(m.memory_uuid)}
                >
                  <div className="flex-1 min-w-0">
                    <div className="flex items-center gap-2">
                      <span className="text-sm font-medium text-foreground truncate">{m.title_norm}</span>
                      <Badge variant={m.status === "active" ? "default" : "secondary"} className="text-[10px] shrink-0">
                        {statusLabel[m.status] || m.status}
                      </Badge>
                    </div>
                  </div>
                  <div className="hidden sm:flex items-center gap-3 text-xs text-muted-foreground shrink-0">
                    <span className="w-20 truncate">{m.category}</span>
                    <span className="w-16 text-right">{new Date(m.updated_at).toLocaleDateString("zh-CN")}</span>
                  </div>
                  <div className="flex items-center gap-1 shrink-0">
                    {m.has_open_change && (
                      <Link to="/changes" onClick={(e) => e.stopPropagation()}>
                        <Badge variant="outline" className="text-[10px] border-destructive text-destructive">变更</Badge>
                      </Link>
                    )}
                    {isExpanded ? <ChevronUp className="h-4 w-4 text-muted-foreground" /> : <ChevronDown className="h-4 w-4 text-muted-foreground" />}
                  </div>
                </div>

                {/* 移动端次级信息 */}
                <div className="sm:hidden px-4 pb-2 flex items-center gap-2 text-xs text-muted-foreground">
                  <span>{m.category}</span>
                  <span>·</span>
                  <span>{new Date(m.updated_at).toLocaleDateString("zh-CN")}</span>
                </div>

                {/* 展开详情 */}
                <div className={cn(
                  "overflow-hidden transition-all duration-200 ease-in-out",
                  isExpanded ? "max-h-[500px] opacity-100" : "max-h-0 opacity-0"
                )}>
                  <div className="px-4 pb-4">
                    <div className="bg-muted/30 rounded-md p-3 space-y-3 text-xs">
                      <div>
                        <p className="text-muted-foreground mb-1">摘要</p>
                        <p className="text-foreground/80 leading-relaxed">{m.summary}</p>
                      </div>
                      <div className="flex flex-wrap gap-x-6 gap-y-1">
                        <div><span className="text-muted-foreground">分类 </span><span>{m.category}</span></div>
                        <div><span className="text-muted-foreground">状态 </span><span>{statusLabel[m.status] || m.status}</span></div>
                        <div><span className="text-muted-foreground">创建 </span><span>{new Date(m.created_at).toLocaleDateString("zh-CN")}</span></div>
                        <div><span className="text-muted-foreground">更新 </span><span>{new Date(m.updated_at).toLocaleDateString("zh-CN")}</span></div>
                      </div>
                    </div>
                  </div>
                </div>
              </div>
            )
          })}
        </div>
      )}
    </div>
  )
}