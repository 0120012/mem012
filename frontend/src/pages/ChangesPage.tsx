import { useState, useEffect, useCallback } from "react"
import { useLocation } from "react-router-dom"
import { api, type ChangeDetail, type ChangeItem, type TrashDetail, type TrashItem } from "@/api/client"
import { useAuth } from "@/auth/AuthContext"
import { Badge } from "@/components/ui/badge"
import { Skeleton } from "@/components/ui/skeleton"
import { Button } from "@/components/ui/button"
import { cn } from "@/lib/utils"
import { ChevronDown, ChevronUp } from "lucide-react"

type ReviewItem = ChangeItem | TrashItem
type ReviewDetail = ChangeDetail | TrashDetail

const actionLabel: Record<string, string> = {
  create: "新增",
  update: "更新",
  delete: "删除",
  restore: "恢复",
}

function formatDateTime(value: string) {
  return new Date(value).toLocaleString("zh-CN")
}

// Why：/changes 返回的 before_state / after_state 是结构化 JSON（memory + keywords），
// 不能直接 dump 为文本。拆成可读的字段卡片。
function StateBlock({ state, label }: { state: Record<string, unknown> | null; label: string }) {
  if (!state) return null
  const m = state.memory as Record<string, string | undefined> | undefined
  const keywords = state.keywords as Array<{ keyword_norm: string; weight: number | null }> | undefined
  return (
    <div className="space-y-3">
      <p className="text-xs font-medium text-muted-foreground">{label}</p>
      {m && (
        <div className="rounded-md border bg-muted/30 p-3 space-y-2 text-xs">
          <div className="flex gap-2"><span className="text-muted-foreground shrink-0">标题</span><span className="text-foreground font-medium">{m.title_norm}</span></div>
          <div className="flex gap-2"><span className="text-muted-foreground shrink-0">分类</span><span>{m.category}</span></div>
          <div className="flex gap-2"><span className="text-muted-foreground shrink-0">状态</span><span>{m.status}</span></div>
          <div className="flex gap-2"><span className="text-muted-foreground shrink-0">摘要</span><span>{m.summary || "未填写"}</span></div>
          {m.content && <div className="flex gap-2"><span className="text-muted-foreground shrink-0">内容</span><span className="text-foreground/80">{m.content}</span></div>}
          {m.recall_when && <div className="flex gap-2"><span className="text-muted-foreground shrink-0">召回条件</span><span>{m.recall_when}</span></div>}
        </div>
      )}
      {keywords && keywords.length > 0 && (
        <div>
          <p className="text-xs font-medium text-muted-foreground mb-1">Keywords</p>
          <div className="flex flex-wrap gap-1">{keywords.map((k, i) => <Badge key={i} variant="outline" className="text-xs">{k.keyword_norm}</Badge>)}</div>
        </div>
      )}
    </div>
  )
}

export function ChangesPage() {
  const { activeProject } = useAuth()
  const location = useLocation()
  const [changes, setChanges] = useState<ReviewItem[]>([])
  const [loading, setLoading] = useState(true)
  const [error, setError] = useState("")
  const [expandedUuid, setExpandedUuid] = useState<string | null>(null)
  const [detail, setDetail] = useState<ReviewDetail | null>(null)
  const [detailLoading, setDetailLoading] = useState(false)
  const [actionLoading, setActionLoading] = useState(false)
  const trashOnly = new URLSearchParams(location.search).get("filter") === "trash"

  const fetchChanges = useCallback(async () => {
    setLoading(true)
    setError("")
    try {
      setChanges(await (trashOnly ? api.trash.list() : api.changes.list()) || [])
    } catch (e) {
      setChanges([])
      setError(e instanceof Error ? e.message : "加载失败")
    }
    setLoading(false)
  }, [trashOnly])

  useEffect(() => {
    const timer = window.setTimeout(() => {
      void fetchChanges()
    }, 0)
    return () => window.clearTimeout(timer)
  }, [activeProject, fetchChanges])

  const toggleCard = async (uuid: string) => {
    if (expandedUuid === uuid) {
      setExpandedUuid(null)
      setDetail(null)
      return
    }
    setDetailLoading(true)
    setExpandedUuid(uuid)
    const data = await (trashOnly ? api.trash.detail(uuid) : api.changes.detail(uuid)) || null
    setDetail(data)
    setDetailLoading(false)
  }

  const handleAction = async (uuid: string, action: "approve" | "reject") => {
    if (trashOnly && action === "approve" && !window.confirm("确认永久删除这条记忆？此操作不可恢复。")) return
    setActionLoading(true)
    setError("")
    try {
      if (trashOnly) {
        if (action === "approve") await api.trash.delete(uuid)
        else await api.trash.restore(uuid)
      } else if (action === "approve") await api.changes.approve(uuid)
      else await api.changes.reject(uuid)
      setExpandedUuid(null)
      setDetail(null)
      await fetchChanges()
    } catch (e) {
      setError(e instanceof Error ? e.message : "操作失败")
    } finally {
      setActionLoading(false)
    }
  }

  const visibleChanges = trashOnly ? changes.filter((change) => change.action === "delete") : changes

  return (
    <div className="max-w-4xl mx-auto p-4 sm:p-6">
      <div className="flex items-center justify-between mb-4">
        <h1 className="text-xl font-semibold text-foreground">{trashOnly ? "回收站" : "待确认"}</h1>
        <Button variant="outline" size="sm" onClick={fetchChanges} disabled={loading}>刷新</Button>
      </div>
      {loading ? (
        <div className="flex flex-col gap-3">
          {Array.from({ length: 3 }).map((_, i) => <Skeleton key={i} className="h-20 w-full rounded-xl" />)}
        </div>
      ) : error ? (
        <div className="text-center py-12">
          <p className="text-destructive mb-3">{error}</p>
          <Button variant="outline" size="sm" onClick={fetchChanges}>重试</Button>
        </div>
      ) : visibleChanges.length === 0 ? (
        <p className="text-muted-foreground text-center py-12">{trashOnly ? "没有待处理的删除" : "没有待确认的变更"}</p>
      ) : (
        <div className="rounded-lg border">
          {visibleChanges.map((c, idx) => {
            const isExpanded = expandedUuid === c.memory_uuid
            const isThisDetail = isExpanded && detail?.memory_uuid === c.memory_uuid
            return (
              <div key={c.memory_uuid} className={cn("transition-colors", idx > 0 && "border-t")}>
                <div
                  className="flex items-center gap-3 px-4 py-3 cursor-pointer hover:bg-accent/30 transition-colors min-h-[44px]"
                  onClick={() => !isExpanded && toggleCard(c.memory_uuid)}
                >
                  <div className="flex-1 min-w-0">
                    <div className="flex items-center gap-2">
                      <span className="text-sm font-medium text-foreground truncate">{c.title_norm}</span>
                      <Badge variant="outline" className="text-[10px] shrink-0">{actionLabel[c.action] || c.action}</Badge>
                    </div>
                    {trashOnly && "trashed_at" in c && (
                      <p className="mt-1 text-xs text-muted-foreground truncate">
                        进入回收站 {formatDateTime(c.trashed_at)} · 预计永久删除 {formatDateTime(c.expires_at)}
                      </p>
                    )}
                  </div>
                  <div className="hidden sm:block text-xs text-muted-foreground shrink-0">
                    {trashOnly && "expires_at" in c ? new Date(c.expires_at).toLocaleDateString("zh-CN") : new Date(c.updated_at).toLocaleDateString("zh-CN")}
                  </div>
                  {isExpanded ? <ChevronUp className="h-4 w-4 text-muted-foreground" /> : <ChevronDown className="h-4 w-4 text-muted-foreground" />}
                </div>

                {/* 展开详情 */}
                <div className={cn(
                  "overflow-hidden transition-all duration-200 ease-in-out",
                  isExpanded ? "max-h-[800px] opacity-100" : "max-h-0 opacity-0"
                )}>
                  <div className="px-4 pb-4">
                    {detailLoading || !isThisDetail ? (
                      <div className="bg-muted/30 rounded-md p-3 space-y-2">
                        <Skeleton className="h-4 w-3/4" />
                        <Skeleton className="h-4 w-1/2" />
                        <Skeleton className="h-20 w-full" />
                      </div>
                    ) : detail ? (
                      <div className="space-y-4">
                        <p className="text-xs text-muted-foreground">{detail.summary || "未填写摘要"}</p>
                        {trashOnly && "trashed_at" in detail && (
                          <div className="rounded-md border bg-muted/30 p-3 space-y-1 text-xs text-muted-foreground">
                            <div>进入回收站：{formatDateTime(detail.trashed_at)}</div>
                            <div>预计永久删除：{formatDateTime(detail.expires_at)}</div>
                          </div>
                        )}
                        <StateBlock state={detail.before_state} label={trashOnly ? "删除前" : "修改前"} />
                        <StateBlock state={detail.after_state} label={trashOnly ? "删除后" : "修改后"} />
                        <div className="flex gap-2 pt-2">
                          <Button variant="outline" size="sm" className="flex-1"
                            onClick={(e) => { e.stopPropagation(); handleAction(detail.memory_uuid, "reject") }}
                            disabled={actionLoading}>{trashOnly ? "恢复" : "拒绝"}</Button>
                          <Button variant={trashOnly ? "destructive" : "default"} size="sm" className="flex-1"
                            onClick={(e) => { e.stopPropagation(); handleAction(detail.memory_uuid, "approve") }}
                            disabled={actionLoading}>{trashOnly ? "确认删除" : "批准"}</Button>
                        </div>
                      </div>
                    ) : null}
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
