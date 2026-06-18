import { useState, useEffect, useCallback } from "react"
import { useLocation } from "react-router-dom"
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

const actionLabel: Record<string, string> = {
  create: "新增",
  update: "更新",
  delete: "删除",
  restore: "恢复",
}

function formatDateTime(value: string) {
  return new Date(value).toLocaleString("zh-CN")
}

function extractFields(state: ReviewState): Record<string, string> {
  if (!state) return {}
  const memory = state.memory
  if (!memory || typeof memory !== "object") return { "原始数据": JSON.stringify(state, null, 2) }
  const fields = memory as Record<string, unknown>
  const text = (key: string) => typeof fields[key] === "string" ? fields[key] as string : ""
  const keywords = (Array.isArray(state?.keywords) ? state.keywords : [])
    .map((i) => i && typeof i === "object" ? (i as Record<string, unknown>).keyword_norm : i)
    .filter((v): v is string => typeof v === "string" && v.trim() !== "")
    .join(", ")
  return {
    "分类": text("category"),
    "标题": text("title_norm"),
    "状态": text("status"),
    "摘要": text("summary"),
    "召回时机": text("recall_when"),
    "关键词": keywords,
    "内容": text("content"),
  }
}

/** What：找出 before 和 after 的共同首尾，标记中间差异子串。 */
/** Why：在红绿分栏中精确高亮修改的字符，替代 Monaco 在窄空间下无法两栏的问题。 */
function diffParts(before: string, after: string): {
  prefix: string; oldMid: string; newMid: string; suffix: string;
} {
  let start = 0;
  while (start < before.length && start < after.length && before[start] === after[start]) start++;
  let endB = before.length - 1;
  let endA = after.length - 1;
  while (endB >= start && endA >= start && before[endB] === after[endA]) { endB--; endA--; }
  return {
    prefix: before.slice(0, start),
    oldMid: before.slice(start, endB + 1),
    newMid: after.slice(start, endA + 1),
    suffix: before.slice(endB + 1),
  };
}

function FieldDiff({ beforeState, afterState }: { beforeState: ReviewState; afterState: ReviewState }) {
  const b = extractFields(beforeState)
  const a = extractFields(afterState)
  const changed = Object.keys({ ...b, ...a })
    .filter((k) => b[k] !== a[k])
    .map((k) => ({ field: k, before: b[k] ?? "—", after: a[k] ?? "—" }))

  return (
    <div className="flex flex-1 flex-col min-h-0">
      <div className="flex-1 min-h-0 overflow-auto space-y-2">
        {changed.length === 0 ? (
          <p className="text-xs text-muted-foreground">无变更</p>
        ) : (
          changed.map((e, i) => {
            /* What：按 memories 编辑框风格排版，Label + 左右双格。 */
            /* Why：短字段单行对比即可，长内容逐行 + 行号 + 字符高亮。 */
            const isLong = e.field === "内容" || e.before.includes("\n") || e.after.includes("\n")
            return (
              <div key={i} className="grid gap-2">
                <div className="text-sm font-semibold">{e.field}</div>
                {isLong ? (
                  <FieldLines before={e.before} after={e.after} />
                ) : (
                  <FieldInline before={e.before} after={e.after} />
                )}
              </div>
            )
          })
        )}
      </div>
    </div>
  )
}

function FieldInline({ before, after }: { before: string; after: string }) {
  const dp = diffParts(before, after)
  return (
    <div className="grid grid-cols-2 gap-px text-sm rounded-sm overflow-hidden border">
      <div className="bg-red-50 dark:bg-red-950/30 px-3 py-2 line-through text-muted-foreground break-all">
        {dp.prefix}<mark className="bg-red-200 dark:bg-red-800/40 text-red-800 dark:text-red-200 rounded-sm px-0.5">{dp.oldMid}</mark>{dp.suffix}
      </div>
      <div className="bg-green-50 dark:bg-green-950/30 px-3 py-2 break-all">
        {dp.prefix}<mark className="bg-green-200 dark:bg-green-800/40 text-green-800 dark:text-green-200 rounded-sm px-0.5">{dp.newMid}</mark>{dp.suffix}
      </div>
    </div>
  )
}

function FieldLines({ before, after }: { before: string; after: string }) {
  const beforeLines = before.split("\n")
  const afterLines = after.split("\n")
  const maxLen = Math.max(beforeLines.length, afterLines.length)
  const rows = Array.from({ length: maxLen }, (_, li) => {
    const bl = beforeLines[li] ?? ""
    const al = afterLines[li] ?? ""
    const same = bl === al
    const dp = same ? null : diffParts(bl, al)
    return { li, bl, al, same, dp }
  })
  return (
    <div className="grid grid-cols-2 gap-px text-sm font-mono leading-5 rounded-sm overflow-hidden border">
      <div className="bg-red-50 dark:bg-red-950/30">
        {rows.map((r) => (
          <div key={r.li} className={cn("flex", r.same && "line-through text-muted-foreground")}>
            <span className="w-8 shrink-0 text-right pr-2 text-muted-foreground/40 select-none">{r.li + 1}</span>
            <span className="flex-1 pr-2 whitespace-pre-wrap break-all">
              {r.dp ? <><span className="text-muted-foreground">{r.dp.prefix}</span><mark className="bg-red-200 dark:bg-red-800/40 text-red-800 dark:text-red-200">{r.dp.oldMid}</mark><span className="text-muted-foreground">{r.dp.suffix}</span></> : r.bl}
            </span>
          </div>
        ))}
      </div>
      <div className="bg-green-50 dark:bg-green-950/30">
        {rows.map((r) => (
          <div key={r.li} className="flex">
            <span className="w-8 shrink-0 text-right pr-2 text-muted-foreground/40 select-none">{r.li + 1}</span>
            <span className="flex-1 pr-2 whitespace-pre-wrap break-all">
              {r.dp ? <><span>{r.dp.prefix}</span><mark className="bg-green-200 dark:bg-green-800/40 text-green-800 dark:text-green-200">{r.dp.newMid}</mark><span>{r.dp.suffix}</span></> : r.al}
            </span>
          </div>
        ))}
      </div>
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
      const data = await (trashOnly ? api.trash.list() : api.changes.list()) || []
      setChanges(data)
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
      window.dispatchEvent(new CustomEvent("changes-updated"))
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
                  <div className="flex items-center justify-between gap-3 shrink-0">
                    <Badge variant="outline" className="text-[10px]">{actionLabel[detail.action] || detail.action}</Badge>
                  </div>
                  {trashOnly && "trashed_at" in detail && (
                    <div className="rounded-md border bg-muted/30 p-3 space-y-1 text-xs text-muted-foreground">
                      <div>进入回收站：{formatDateTime(detail.trashed_at)}</div>
                      <div>预计永久删除：{formatDateTime(detail.expires_at)}</div>
                    </div>
                  )}
                  <FieldDiff beforeState={detail.before_state} afterState={detail.after_state} />
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
