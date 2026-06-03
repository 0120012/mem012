import { useState, useEffect, useCallback, useRef } from "react"
import { Link, useSearchParams } from "react-router-dom"
import { api, type MemoryItem } from "@/api/client"
import { useAuth } from "@/auth/AuthContext"
import { Badge } from "@/components/ui/badge"
import { Skeleton } from "@/components/ui/skeleton"
import { Button } from "@/components/ui/button"
import { Input } from "@/components/ui/input"
import { Label } from "@/components/ui/label"
import { cn } from "@/lib/utils"
import { CalendarDays, ChevronRight, Copy, Folder, Minus, Pencil, Plus } from "lucide-react"

const statusLabel: Record<string, string> = {
  active: "活跃",
  archived: "已归档",
  deleted: "已删除",
}

type MonacoEditor = import("monaco-editor").editor.IStandaloneCodeEditor

function currentMonacoTheme() {
  return document.documentElement.classList.contains("dark") ? "vs-dark" : "vs"
}

function MemoryContentEditor({ readOnly, value, onChange }: { readOnly: boolean; value: string; onChange?: (value: string) => void }) {
  const hostRef = useRef<HTMLDivElement>(null)
  const editorRef = useRef<MonacoEditor | null>(null)

  useEffect(() => {
    // What：在内容区域挂载 Monaco 编辑器实例。
    // Why：Monaco 体积较大，动态加载可避免记忆列表首屏直接拉取编辑器主包。
    let disposed = false
    let observer: MutationObserver | null = null
    let changeListener: { dispose: () => void } | null = null
    void import("monaco-editor").then((monaco) => {
      if (disposed || !hostRef.current) return
      const editor = monaco.editor.create(hostRef.current, {
        value,
        language: "markdown",
        theme: currentMonacoTheme(),
        readOnly,
        automaticLayout: true,
        minimap: { enabled: false },
        scrollBeyondLastLine: false,
        wordWrap: "on",
      })
      changeListener = editor.onDidChangeModelContent(() => onChange?.(editor.getValue()))
      observer = new MutationObserver(() => monaco.editor.setTheme(currentMonacoTheme()))
      observer.observe(document.documentElement, { attributes: true, attributeFilter: ["class"] })
      editorRef.current = editor
    })
    return () => {
      disposed = true
      observer?.disconnect()
      changeListener?.dispose()
      editorRef.current?.dispose()
      editorRef.current = null
    }
  }, [])

  useEffect(() => {
    editorRef.current?.updateOptions({ readOnly })
  }, [readOnly])

  useEffect(() => {
    const editor = editorRef.current
    if (editor && editor.getValue() !== value) {
      editor.setValue(value)
    }
  }, [value])

  return <div id="memory-content" className="min-h-64 flex-1 overflow-hidden rounded-md border" ref={hostRef} />
}

export function MemoriesPage() {
  const { activeProject } = useAuth()
  const [searchParams] = useSearchParams()
  const [memories, setMemories] = useState<MemoryItem[]>([])
  const [loading, setLoading] = useState(true)
  const [error, setError] = useState("")
  const [selectedUuid, setSelectedUuid] = useState<string | null>(null)
  const [isEditing, setIsEditing] = useState(false)
  const [copiedUuid, setCopiedUuid] = useState("")
  const titleInputRef = useRef<HTMLInputElement>(null)
  const summaryInputRef = useRef<HTMLTextAreaElement>(null)
  const recallWhenInputRef = useRef<HTMLTextAreaElement>(null)
  const [editorKeywords, setEditorKeywords] = useState<string[]>([])
  const [contentDraft, setContentDraft] = useState("")
  const [keywordDraft, setKeywordDraft] = useState("")
  const [keywordInputOpen, setKeywordInputOpen] = useState(false)
  const [keywordError, setKeywordError] = useState("")
  const categoryFilter = searchParams.get("category")?.trim() || ""
  const keywordFilter = searchParams.get("keyword")?.trim() || ""
  const visibleMemories = memories.filter((m) =>
    (!categoryFilter || m.category === categoryFilter) &&
    (!keywordFilter || !m.keywords || m.keywords.includes(keywordFilter))
  )
  const selectedMemory = visibleMemories.find((m) => m.memory_uuid === selectedUuid) || null

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

  useEffect(() => {
    const timer = window.setTimeout(() => {
      void fetchMemories()
    }, 0)
    return () => window.clearTimeout(timer)
  }, [activeProject, fetchMemories])

  useEffect(() => {
    setIsEditing(false)
    setCopiedUuid("")
    setEditorKeywords(selectedMemory?.keywords || [])
    setContentDraft(selectedMemory?.content || "")
    setKeywordDraft("")
    setKeywordInputOpen(false)
    setKeywordError("")
  }, [selectedUuid])

  const toggleCard = (uuid: string) => {
    setSelectedUuid(selectedUuid === uuid ? null : uuid)
  }

  const copySelectedUuid = async () => {
    if (!selectedMemory) return
    try {
      await navigator.clipboard.writeText(selectedMemory.memory_uuid)
      setCopiedUuid(selectedMemory.memory_uuid)
      window.setTimeout(() => setCopiedUuid((current) => current === selectedMemory.memory_uuid ? "" : current), 1200)
    } catch {
      setCopiedUuid("")
    }
  }

  const addKeyword = () => {
    const keyword = keywordDraft.trim()
    if (!keyword) {
      setKeywordError("关键词不能为空")
      return
    }
    if (editorKeywords.includes(keyword)) {
      setKeywordError("关键词已存在")
      return
    }
    setEditorKeywords((current) => [...current, keyword])
    setKeywordDraft("")
    setKeywordInputOpen(false)
    setKeywordError("")
  }

  const removeKeyword = (keyword: string) => {
    if (editorKeywords.length <= 1) {
      setKeywordError("关键词至少保留一个")
      return
    }
    setEditorKeywords((current) => current.filter((item) => item !== keyword))
    setKeywordError("")
  }

  const cancelEditing = () => {
    setEditorKeywords(selectedMemory?.keywords || [])
    setContentDraft(selectedMemory?.content || "")
    setKeywordDraft("")
    setKeywordInputOpen(false)
    setKeywordError("")
    setIsEditing(false)
  }

  const saveSelectedMemory = async () => {
    if (!selectedMemory) return
    const title = titleInputRef.current?.value.trim() || ""
    if (!title || !contentDraft.trim() || editorKeywords.length === 0) {
      setKeywordError("标题、内容和关键词不能为空")
      return
    }
    const summary = summaryInputRef.current?.value.trim() || ""
    const recallWhen = recallWhenInputRef.current?.value.trim() || ""
    try {
      await api.memories.update(selectedMemory.memory_uuid, {
        title_norm: title,
        summary: summary || null,
        recall_when: recallWhen || null,
        content: contentDraft,
        keywords: editorKeywords,
      })
      await fetchMemories()
      setIsEditing(false)
    } catch (error) {
      setKeywordError(error instanceof Error ? error.message : "保存失败")
    }
  }

  return (
    <div className="mx-auto max-w-7xl p-4 sm:p-6">
      <div className="flex items-center justify-between mb-4">
        <h1 className="text-lg font-semibold text-foreground">{keywordFilter || categoryFilter || "记忆"}</h1>
        <Button variant="outline" size="sm" onClick={fetchMemories} disabled={loading}>刷新</Button>
      </div>

      {loading ? (
        <div className="grid grid-cols-1 gap-3 sm:grid-cols-2 xl:grid-cols-4">
          {Array.from({ length: 8 }).map((_, i) => <Skeleton key={i} className="h-48 w-full rounded-md" />)}
        </div>
      ) : error ? (
        <div className="text-center py-12">
          <p className="text-destructive mb-3">{error}</p>
          <Button variant="outline" size="sm" onClick={fetchMemories}>重试</Button>
        </div>
      ) : memories.length === 0 ? (
        <p className="text-muted-foreground text-center py-12">暂无记忆</p>
      ) : visibleMemories.length === 0 && keywordFilter ? (
        <p className="text-muted-foreground text-center py-12">该关键词暂无记忆</p>
      ) : visibleMemories.length === 0 ? (
        <p className="text-muted-foreground text-center py-12">该分类暂无记忆</p>
      ) : (
        <div className="grid grid-cols-1 gap-3 sm:grid-cols-2 xl:grid-cols-4">
          {visibleMemories.map((m) => {
            const isSelected = selectedUuid === m.memory_uuid
            return (
              <div
                key={m.memory_uuid}
                className={cn(
                  "group flex h-48 min-w-0 flex-col overflow-hidden rounded-md border bg-card shadow-sm transition-[background-color,border-color,box-shadow] hover:border-primary/30 hover:shadow-md",
                  isSelected && "border-primary/70 bg-accent/20 shadow-md ring-1 ring-primary/15"
                )}
              >
                <div
                  className="flex min-h-0 flex-1 cursor-pointer flex-col gap-3 p-3"
                  onClick={() => toggleCard(m.memory_uuid)}
                >
                  <div className="flex min-w-0 items-start gap-2">
                    <span className="min-w-0 flex-1 truncate text-sm font-semibold leading-5 text-foreground">{m.title_norm}</span>
                    <Badge variant={m.status === "active" ? "default" : "secondary"} className="shrink-0 px-1.5 py-0 text-[10px]">
                      {statusLabel[m.status] || m.status}
                    </Badge>
                  </div>
                  <p className="line-clamp-3 min-h-[3.75rem] border-l-2 border-border pl-2 text-xs leading-5 text-muted-foreground">{m.summary || "暂无摘要"}</p>
                  <div className="flex min-h-5 flex-wrap gap-1 overflow-hidden">
                    {m.keywords?.slice(0, 3).map((keyword) => (
                      <Badge key={keyword} variant="outline" className="max-w-24 truncate px-1.5 py-0 text-[10px] font-normal">
                        {keyword}
                      </Badge>
                    ))}
                    {m.has_open_change && (
                      <Link to="/changes" onClick={(e) => e.stopPropagation()}>
                        <Badge variant="outline" className="text-[10px] border-destructive text-destructive">变更</Badge>
                      </Link>
                    )}
                  </div>
                  <div className="mt-auto flex items-center justify-between gap-2 rounded-md border bg-muted/20 px-2 py-1.5 text-[11px] text-muted-foreground">
                    <span className="flex min-w-0 items-center gap-1.5">
                      <Folder className="h-3.5 w-3.5 shrink-0 text-primary/70" />
                      <span className="truncate font-medium text-foreground/80">{m.category}</span>
                    </span>
                    <span className="flex shrink-0 items-center gap-1.5">
                      <CalendarDays className="h-3.5 w-3.5" />
                      <span>{new Date(m.updated_at).toLocaleDateString("zh-CN", { year: "numeric", month: "2-digit", day: "2-digit" })}</span>
                    </span>
                  </div>
                </div>
              </div>
            )
          })}
        </div>
      )}
      {selectedMemory && (
        <>
          <button
            type="button"
            aria-label="关闭编辑框"
            className="fixed inset-0 z-30 cursor-default bg-black/5"
            onClick={() => setSelectedUuid(null)}
          />
          <aside className="fixed inset-y-0 right-0 z-40 w-full max-w-xl border-l bg-background shadow-xl sm:w-[520px]">
            <button
              type="button"
              aria-label="收回编辑框"
              className="absolute left-0 top-1/2 flex h-11 w-8 -translate-x-full -translate-y-1/2 items-center justify-center rounded-l-md border bg-background text-muted-foreground shadow-sm hover:bg-accent"
              onClick={() => setSelectedUuid(null)}
            >
              <ChevronRight className="h-4 w-4" />
            </button>
            <div key={`${selectedMemory.memory_uuid}-${isEditing}`} className="flex h-full flex-col gap-4 overflow-auto p-5">
              <div className="flex items-center justify-between gap-3">
                <h2 className="text-sm font-semibold">{isEditing ? "记忆编辑" : "记忆详情"}</h2>
                <div className="flex items-center gap-2">
                  {isEditing ? (
                    <>
                      <Button type="button" variant="outline" size="sm" onClick={cancelEditing}>取消</Button>
                      <Button type="button" size="sm" onClick={saveSelectedMemory}>确认</Button>
                    </>
                  ) : (
                    <>
                      <Button type="button" variant="outline" size="sm" className="w-28 justify-center gap-2" onClick={copySelectedUuid}>
                        <Copy className="h-4 w-4" />
                        {copiedUuid === selectedMemory.memory_uuid ? "已复制" : "复制 UUID"}
                      </Button>
                      <Button type="button" size="sm" className="w-28 justify-center gap-2" onClick={() => setIsEditing(true)}>
                        <Pencil className="h-4 w-4" />
                        编辑
                      </Button>
                    </>
                  )}
                </div>
              </div>
              {isEditing ? (
                <>
                  <div className="grid gap-2">
                    <Label htmlFor="memory-title">标题</Label>
                    <Input id="memory-title" ref={titleInputRef} defaultValue={selectedMemory.title_norm} />
                  </div>
                  <div className="grid gap-2">
                    <Label htmlFor="memory-summary">摘要</Label>
                    <textarea
                      id="memory-summary"
                      ref={summaryInputRef}
                      defaultValue={selectedMemory.summary || ""}
                      className="min-h-24 w-full rounded-md border border-input bg-transparent px-3 py-2 text-sm shadow-sm focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-ring"
                    />
                  </div>
                  <div className="grid gap-2">
                    <Label htmlFor="memory-recall-when">召回时机</Label>
                    <textarea
                      id="memory-recall-when"
                      ref={recallWhenInputRef}
                      defaultValue={selectedMemory.recall_when || ""}
                      className="min-h-20 w-full rounded-md border border-input bg-transparent px-3 py-2 text-sm shadow-sm focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-ring"
                    />
                  </div>
                  <div className="grid gap-2">
                    <div className="flex items-center justify-between gap-2">
                      <Label htmlFor="memory-keywords">关键词</Label>
                      <Button
                        type="button"
                        size="icon"
                        variant="outline"
                        onClick={keywordInputOpen ? addKeyword : () => {
                          setKeywordInputOpen(true)
                          setKeywordError("")
                        }}
                      >
                        <Plus className="h-4 w-4" />
                      </Button>
                    </div>
                    {keywordInputOpen && (
                      <Input
                        id="memory-keywords"
                        autoFocus
                        value={keywordDraft}
                        onChange={(event) => {
                          setKeywordDraft(event.target.value)
                          setKeywordError("")
                        }}
                        onKeyDown={(event) => {
                          if (event.key === "Enter") {
                            event.preventDefault()
                            addKeyword()
                          }
                        }}
                        placeholder="输入关键词后按 Enter"
                      />
                    )}
                    {keywordError && <p className="text-xs text-destructive">{keywordError}</p>}
                    <div className="flex min-h-8 flex-wrap gap-2">
                      {editorKeywords.map((keyword) => (
                        <Badge key={keyword} variant="outline" className="gap-1 py-1 pr-1">
                          <span>{keyword}</span>
                          <button
                            type="button"
                            className="rounded-sm p-0.5 hover:bg-accent"
                            onClick={() => removeKeyword(keyword)}
                          >
                            <Minus className="h-3 w-3" />
                          </button>
                        </Badge>
                      ))}
                    </div>
                  </div>
                  <div className="flex min-h-0 flex-1 flex-col gap-2">
                    <Label htmlFor="memory-content">内容</Label>
                    <MemoryContentEditor readOnly={false} value={contentDraft} onChange={setContentDraft} />
                  </div>
                </>
              ) : (
                <>
                  <div className="space-y-1">
                    <p className="text-xs text-muted-foreground">标题</p>
                    <p className="text-sm font-medium text-foreground">{selectedMemory.title_norm}</p>
                  </div>
                  <div className="space-y-1">
                    <p className="text-xs text-muted-foreground">摘要</p>
                    <p className="whitespace-pre-wrap break-words text-sm leading-6 text-foreground">{selectedMemory.summary || "暂无摘要"}</p>
                  </div>
                  <div className="space-y-1">
                    <p className="text-xs text-muted-foreground">召回时机</p>
                    <p className="whitespace-pre-wrap break-words text-sm leading-6 text-foreground">{selectedMemory.recall_when || "未填写"}</p>
                  </div>
                  <div className="space-y-2">
                    <p className="text-xs text-muted-foreground">关键词</p>
                    <div className="flex min-h-8 flex-wrap gap-2">
                      {(selectedMemory.keywords || []).map((keyword) => (
                        <Badge key={keyword} variant="outline">{keyword}</Badge>
                      ))}
                      {(!selectedMemory.keywords || selectedMemory.keywords.length === 0) && (
                        <span className="text-sm text-muted-foreground">暂无关键词</span>
                      )}
                    </div>
                  </div>
                  <div className="flex min-h-0 flex-1 flex-col gap-2">
                    <p className="text-xs text-muted-foreground">内容</p>
                    <div className="min-h-64 flex-1 overflow-auto rounded-md border bg-muted/20 p-3 text-sm leading-6 text-foreground">
                      <div className="whitespace-pre-wrap break-words">{selectedMemory.content || "暂无内容"}</div>
                    </div>
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
