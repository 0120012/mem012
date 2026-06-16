import { useState, useEffect, useCallback, useRef } from "react"
import { useLocation } from "react-router-dom"
import "monaco-editor/esm/vs/editor/editor.all.js"
import "monaco-editor/esm/vs/language/json/monaco.contribution.js"
import * as monaco from "monaco-editor/esm/vs/editor/editor.api.js"
import EditorWorker from "monaco-editor/esm/vs/editor/editor.worker?worker"
import JsonWorker from "monaco-editor/esm/vs/language/json/json.worker?worker"
import { api, type ChangeDetail, type ChangeItem, type TrashDetail, type TrashItem } from "@/api/client"
import { useAuth } from "@/auth/AuthContext"
import { Badge } from "@/components/ui/badge"
import { Skeleton } from "@/components/ui/skeleton"
import { Button } from "@/components/ui/button"
import { cn } from "@/lib/utils"
import { ChevronRight } from "lucide-react"

type ReviewItem = ChangeItem | TrashItem
type ReviewDetail = ChangeDetail | TrashDetail
type ReviewState = Record<string, unknown> | null

globalThis.MonacoEnvironment = {
  getWorker: (_workerId, label) => label === "json" ? new JsonWorker() : new EditorWorker(),
}

const actionLabel: Record<string, string> = {
  create: "新增",
  update: "更新",
  delete: "删除",
  restore: "恢复",
}

function formatDateTime(value: string) {
  return new Date(value).toLocaleString("zh-CN")
}

function StateBlock({ beforeState, afterState, label }: { beforeState: ReviewState; afterState: ReviewState; label: string }) {
  const hostRef = useRef<HTMLDivElement>(null)

  useEffect(() => {
    if (!hostRef.current) return
    const editor = monaco.editor.createDiffEditor(hostRef.current, {
      automaticLayout: true,
      minimap: { enabled: false },
      originalEditable: false,
      readOnly: true,
      scrollBeyondLastLine: false,
      useInlineViewWhenSpaceIsLimited: true,
      theme: document.documentElement.classList.contains("dark") ? "vs-dark" : "vs",
    })
    const original = monaco.editor.createModel(JSON.stringify(beforeState ?? {}, null, 2), "json")
    const modified = monaco.editor.createModel(JSON.stringify(afterState ?? {}, null, 2), "json")
    editor.setModel({ original, modified })

    return () => {
      editor.dispose()
      original.dispose()
      modified.dispose()
    }
  }, [beforeState, afterState])

  return (
    <div className="space-y-3">
      <p className="text-xs font-medium text-muted-foreground">{label}</p>
      {/* What：用 Monaco DiffEditor 对比 before_state / after_state。 */}
      {/* Why：后端返回的是嵌套 JSON，统一 diff 比字段卡片更容易审阅真实变更。 */}
      <div ref={hostRef} className="h-[420px] overflow-hidden rounded-md border" />
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
  const isThisDetail = detail?.memory_uuid === expandedUuid

  return (
    <div className="mx-auto max-w-6xl p-4 sm:p-6">
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
        <div className="grid grid-cols-1 gap-3 md:grid-cols-2">
          {visibleChanges.map((c) => {
            const isExpanded = expandedUuid === c.memory_uuid
            return (
              <div
                key={c.memory_uuid}
                className={cn(
                  "flex min-h-40 min-w-0 flex-col rounded-md border bg-card shadow-sm transition-[background-color,border-color,box-shadow] hover:border-primary/30 hover:shadow-md",
                  isExpanded && "border-primary/70 bg-accent/20 shadow-md ring-1 ring-primary/15"
                )}
              >
                <div
                  className="flex min-h-0 flex-1 cursor-pointer flex-col gap-3 p-4"
                  onClick={() => toggleCard(c.memory_uuid)}
                >
                  <div className="flex min-w-0 items-start gap-2">
                    <span className="min-w-0 flex-1 truncate text-sm font-semibold leading-5 text-foreground">{c.title_norm}</span>
                    <Badge variant="outline" className="shrink-0 px-1.5 py-0 text-[10px]">{actionLabel[c.action] || c.action}</Badge>
                  </div>
                  <p className="line-clamp-3 min-h-[3.75rem] border-l-2 border-border pl-2 text-xs leading-5 text-muted-foreground">{c.summary || "未填写摘要"}</p>
                  {trashOnly && "trashed_at" in c && (
                    <p className="text-xs text-muted-foreground">
                      进入回收站 {formatDateTime(c.trashed_at)} · 预计永久删除 {formatDateTime(c.expires_at)}
                    </p>
                  )}
                  <div className="mt-auto flex items-center justify-between gap-2 rounded-md border bg-muted/20 px-2 py-1.5 text-[11px] text-muted-foreground">
                    <span>创建 {new Date(c.created_at).toLocaleDateString("zh-CN", { year: "numeric", month: "2-digit", day: "2-digit" })}</span>
                    <ChevronRight className="h-3.5 w-3.5 shrink-0" />
                  </div>
                </div>
              </div>
            )
          })}
        </div>
      )}
      {expandedUuid && (
        <>
          <button
            type="button"
            aria-label="关闭变更详情"
            className="fixed inset-0 z-30 cursor-default bg-black/5"
            onClick={() => { setExpandedUuid(null); setDetail(null) }}
          />
          <aside className="fixed inset-y-0 right-0 z-40 w-full max-w-3xl border-l bg-background shadow-xl sm:w-[720px]">
            <button
              type="button"
              aria-label="收回变更详情"
              className="absolute left-2 top-1/2 z-10 flex h-11 w-8 -translate-y-1/2 items-center justify-center rounded-md border bg-background text-muted-foreground shadow-sm hover:bg-accent sm:left-0 sm:-translate-x-full sm:rounded-l-md"
              onClick={() => { setExpandedUuid(null); setDetail(null) }}
            >
              <ChevronRight className="h-4 w-4" />
            </button>
            <div className="flex h-full flex-col gap-4 overflow-auto p-5">
              {detailLoading || !isThisDetail || !detail ? (
                <div className="rounded-md bg-muted/30 p-3 space-y-2">
                  <Skeleton className="h-4 w-3/4" />
                  <Skeleton className="h-4 w-1/2" />
                  <Skeleton className="h-80 w-full" />
                </div>
              ) : (
                <>
                  <div className="flex items-center justify-between gap-3">
                    <h2 className="text-sm font-semibold">{trashOnly ? "删除详情" : "变更详情"}</h2>
                    <Badge variant="outline" className="text-[10px] shrink-0">{actionLabel[detail.action] || detail.action}</Badge>
                  </div>
                  <p className="text-xs text-muted-foreground">{detail.summary || "未填写摘要"}</p>
                  {trashOnly && "trashed_at" in detail && (
                    <div className="rounded-md border bg-muted/30 p-3 space-y-1 text-xs text-muted-foreground">
                      <div>进入回收站：{formatDateTime(detail.trashed_at)}</div>
                      <div>预计永久删除：{formatDateTime(detail.expires_at)}</div>
                    </div>
                  )}
                  <StateBlock beforeState={detail.before_state} afterState={detail.after_state} label={trashOnly ? "删除 Diff" : "修改 Diff"} />
                  <div className="flex gap-2 pt-2">
                    <Button variant="outline" size="sm" className="flex-1" onClick={() => handleAction(detail.memory_uuid, "reject")} disabled={actionLoading}>{trashOnly ? "恢复" : "拒绝"}</Button>
                    <Button variant={trashOnly ? "destructive" : "default"} size="sm" className="flex-1" onClick={() => handleAction(detail.memory_uuid, "approve")} disabled={actionLoading}>{trashOnly ? "确认删除" : "批准"}</Button>
                  </div>
                </>
              )}
            </div>
          </aside>
        </>
      )}
    </div>
  )
}
