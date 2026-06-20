import { useState, useEffect, useCallback, useRef } from "react"
import { api, type GraphStatus, type GraphOverviewRelation, type NeighborMemory, type NeighborRelation, type SuggestedRelation, RELATION_TYPES } from "@/api/client"
import { useAuth } from "@/auth/AuthContext"
import { Badge } from "@/components/ui/badge"
import { Button } from "@/components/ui/button"
import { Input } from "@/components/ui/input"
import { Label } from "@/components/ui/label"
import { cn } from "@/lib/utils"
import {
  ReactFlow, Background, Controls, useNodesState, useEdgesState,
  type Node, type Edge, Position, Handle, type NodeProps,
  BaseEdge, EdgeLabelRenderer, type EdgeProps, getBezierPath,
} from "@xyflow/react"
import "@xyflow/react/dist/style.css"
import { AlertTriangle, CheckCircle, Plus, Trash2, RefreshCw, Search, GitBranch, ArrowLeft } from "lucide-react"

// ---- Radial Layout ----
function radialLayout(centerUuid: string, centerTitle: string, centerCategory: string, neighbors: NeighborRelation[]): { nodes: Node[]; edges: Edge[] } {
  const RADIUS = 240
  const nodes: Node[] = [{ id: centerUuid, type: "memoryNode", position: { x: 0, y: 0 }, data: { label: centerTitle, category: centerCategory, isCenter: true } }]
  const edges: Edge[] = []
  neighbors.forEach((r, i) => {
    const angle = (2 * Math.PI * i) / neighbors.length - Math.PI / 2
    nodes.push({ id: r.memory.memory_uuid, type: "memoryNode", position: { x: RADIUS * Math.cos(angle), y: RADIUS * Math.sin(angle) }, data: { label: r.memory.title_norm, category: r.memory.category, isCenter: false } })
    const source = r.direction === "outgoing" ? centerUuid : r.memory.memory_uuid
    const target = r.direction === "outgoing" ? r.memory.memory_uuid : centerUuid
    edges.push({ id: r.relation_uuid, source, target, type: "labeledEdge", data: { relation_type: r.relation_type, weight: r.weight, note: r.note, relation_uuid: r.relation_uuid } })
  })
  return { nodes, edges }
}

// Why：默认图谱没有中心节点，用环形布局先保证所有当前工作态关系可见。
function overviewLayout(memories: NeighborMemory[], relations: GraphOverviewRelation[]): { nodes: Node[]; edges: Edge[] } {
  const RADIUS = 260
  const nodes = memories.map((memory, i) => {
    const angle = (2 * Math.PI * i) / Math.max(memories.length, 1) - Math.PI / 2
    return { id: memory.memory_uuid, type: "memoryNode", position: { x: RADIUS * Math.cos(angle), y: RADIUS * Math.sin(angle) }, data: { label: memory.title_norm, category: memory.category, isCenter: false } }
  })
  const edges = relations.map((relation) => ({
    id: relation.relation_uuid,
    source: relation.from_memory_uuid,
    target: relation.to_memory_uuid,
    type: "labeledEdge",
    data: { relation_type: relation.relation_type, weight: relation.weight, note: relation.note, relation_uuid: relation.relation_uuid },
  }))
  return { nodes, edges }
}

// ---- Custom Node ----
type MemoryNodeData = { label: string; category: string; isCenter: boolean }
function MemoryNode({ data, selected }: NodeProps) {
  const d = data as unknown as MemoryNodeData
  return (
    <div className={cn(
      "group relative min-w-[150px] max-w-[210px] cursor-pointer rounded-lg border bg-card/95 px-3 py-2 text-left shadow-sm transition-all backdrop-blur",
      selected && "ring-2 ring-foreground/50",
      d.isCenter ? "border-foreground/70 shadow-md" : "border-border/80 hover:-translate-y-0.5 hover:border-foreground/40 hover:shadow-md"
    )}>
      <Handle type="target" position={Position.Top} className="!h-1.5 !w-8 !rounded-full !border-0 !bg-foreground/20" />
      <div className="flex items-center gap-2">
        <span className={cn("h-2.5 w-2.5 rounded-full", d.isCenter ? "bg-foreground" : "bg-muted-foreground/60")} />
        <div className="min-w-0">
          <div className="truncate text-[11px] font-semibold leading-4 text-foreground">{d.label}</div>
          <div className="mt-0.5 text-[9px] uppercase text-muted-foreground">{d.category}</div>
        </div>
      </div>
      <Handle type="source" position={Position.Bottom} className="!h-1.5 !w-8 !rounded-full !border-0 !bg-foreground/20" />
    </div>
  )
}

// ---- Custom Edge ----
function LabeledEdge({ id, sourceX, sourceY, targetX, targetY, data }: EdgeProps) {
  const [edgePath, labelX, labelY] = getBezierPath({ sourceX, sourceY, targetX, targetY })
  const d = data as unknown as { relation_type?: string; weight?: number | null } | undefined
  return (
    <>
      <BaseEdge id={id} path={edgePath} className="!stroke-foreground/35" style={{ strokeWidth: d?.weight ? 1.5 + d.weight / 60 : 1.5 }} />
      <EdgeLabelRenderer>
        <div style={{ position: "absolute", transform: `translate(-50%, -50%) translate(${labelX}px,${labelY}px)` }} className="nodrag nopan pointer-events-auto cursor-pointer">
          <div className="flex items-center gap-1 rounded-full border bg-background/95 px-2 py-1 text-[10px] font-medium text-foreground shadow-sm transition-colors hover:border-foreground/50">
            <span>{d?.relation_type}</span>
            {d?.weight != null && <span className="text-muted-foreground">{d.weight}</span>}
          </div>
        </div>
      </EdgeLabelRenderer>
    </>
  )
}

const nodeTypes = { memoryNode: MemoryNode }
const edgeTypes = { labeledEdge: LabeledEdge }

// ---- Page ----
export function GraphPage() {
  const { activeProject } = useAuth()
  const [status, setStatus] = useState<GraphStatus | null>(null)
  const [statusLoading, setStatusLoading] = useState(true)
  const [rebuilding, setRebuilding] = useState(false)
  const autoRebuildAttemptedRef = useRef(false)

  const [searchUuid, setSearchUuid] = useState("")
  const [center, setCenter] = useState<{ uuid: string; title: string; category: string } | null>(null)
  const [neighbors, setNeighbors] = useState<NeighborRelation[]>([])
  const [overviewRels, setOverviewRels] = useState<GraphOverviewRelation[]>([])
  const [loading, setLoading] = useState(false)
  const [error, setError] = useState("")
  const [pendingMsg, setPendingMsg] = useState("")

  const [nodes, setNodes, onNodesChange] = useNodesState<Node>([])
  const [edges, setEdges, onEdgesChange] = useEdgesState<Edge>([])

  // ---- Edit dialog ----
  const [editRel, setEditRel] = useState<NeighborRelation | null>(null)
  const [showEdit, setShowEdit] = useState(false)
  const [formType, setFormType] = useState("related_to")
  const [formWeight, setFormWeight] = useState("")
  const [formNote, setFormNote] = useState("")
  const [formLoading, setFormLoading] = useState(false)
  const [formError, setFormError] = useState("")

  // ---- Add dialog ----
  const [showAdd, setShowAdd] = useState(false)
  const [addTo, setAddTo] = useState("")
  const [addType, setAddType] = useState("related_to")
  const [addWeight, setAddWeight] = useState("")
  const [addNote, setAddNote] = useState("")
  const [addError, setAddError] = useState("")

  // ---- Dialogs ----
  const [showSuggest, setShowSuggest] = useState(false)
  const [suggestions, setSuggestions] = useState<SuggestedRelation[]>([])
  const [suggestLoading, setSuggestLoading] = useState(false)

  // ---- Fetch Status ----
  const fetchStatus = useCallback(async () => {
    try { setStatus(await api.graph.status()) } catch { setStatus(null) }
    setStatusLoading(false)
  }, [])

  // Why：页面默认态要直接展示图谱，不能把 UUID 查询作为进入图谱的前置条件。
  const loadOverview = useCallback(async () => {
    setLoading(true)
    setError("")
    try {
      const res = await api.graph.overview()
      const layout = overviewLayout(res.nodes || [], res.relations || [])
      setCenter(null)
      setNeighbors([])
      setOverviewRels(res.relations || [])
      setNodes(layout.nodes)
      setEdges(layout.edges)
    } catch (e) {
      setError(e instanceof Error ? e.message : "加载失败")
      setNodes([])
      setEdges([])
    }
    setLoading(false)
  }, [setNodes, setEdges])

  useEffect(() => {
    autoRebuildAttemptedRef.current = false
    void Promise.resolve().then(async () => {
      await fetchStatus()
      await loadOverview()
    })
  }, [activeProject, fetchStatus, loadOverview])

  // ---- Load Memory ----
  const loadMemory = useCallback(async (uuid: string) => {
    if (!uuid.trim()) return
    setLoading(true)
    setError("")
    try {
      const res = await api.graph.neighbors(uuid.trim())
      setNeighbors(res.neighbors || [])
      const c = { uuid: res.memory_uuid, title: res.memory.title_norm, category: res.memory.category }
      setCenter(c)
      const layout = radialLayout(res.memory_uuid, c.title, c.category, res.neighbors || [])
      setNodes(layout.nodes)
      setEdges(layout.edges)
    } catch (e) {
      setError(e instanceof Error ? e.message : "加载失败")
      setNodes([])
      setEdges([])
    }
    setLoading(false)
  }, [setNodes, setEdges])

  const rebuildGraph = useCallback(async () => {
    setRebuilding(true)
    try {
      await api.graph.rebuild()
      await fetchStatus()
      if (center) await loadMemory(center.uuid)
      else await loadOverview()
    } catch (e) {
      setError(e instanceof Error ? e.message : "重建失败")
    }
    setRebuilding(false)
  }, [center, fetchStatus, loadMemory, loadOverview])

  useEffect(() => {
    if (status && !status.dirty) autoRebuildAttemptedRef.current = false
  }, [status])

  useEffect(() => {
    if (!status?.dirty || rebuilding || autoRebuildAttemptedRef.current) return
    autoRebuildAttemptedRef.current = true
    void rebuildGraph()
  }, [rebuildGraph, rebuilding, status?.dirty])

  // ---- Node/Edge clicks ----
  const onNodeClick = useCallback((_: React.MouseEvent, node: Node) => {
    if (node.id === center?.uuid) return
    setSearchUuid(node.id)
    loadMemory(node.id)
  }, [center, loadMemory])

  const onEdgeClick = useCallback((_: React.MouseEvent, edge: Edge) => {
    // neighbor 模式：从 neighbors 查找
    const nRel = neighbors.find(r => r.relation_uuid === edge.id)
    if (nRel) {
      setEditRel(nRel)
      setFormType(nRel.relation_type)
      setFormWeight(nRel.weight != null ? String(nRel.weight) : "")
      setFormNote(nRel.note || "")
      setFormError("")
      setShowEdit(true)
      return
    }
    // overview 模式：从 overviewRels 查找
    const oRel = overviewRels.find(r => r.relation_uuid === edge.id)
    if (oRel) {
      // 构造一个 NeighborRelation 风格的 editRel 供 dialog 使用
      const mock: NeighborRelation = {
        relation_uuid: oRel.relation_uuid,
        direction: "outgoing",
        relation_type: oRel.relation_type,
        weight: oRel.weight,
        note: oRel.note,
        memory: { memory_uuid: oRel.to_memory_uuid, category: "", title_norm: "", summary: "", status: "" },
      }
      setEditRel(mock)
      setFormType(oRel.relation_type)
      setFormWeight(oRel.weight != null ? String(oRel.weight) : "")
      setFormNote(oRel.note || "")
      setFormError("")
      setShowEdit(true)
    }
  }, [neighbors, overviewRels])

  // ---- Update relation ----
  const handleUpdate = async () => {
    if (!editRel) return
    const w = formWeight.trim() ? Number(formWeight) : undefined
    const body: Record<string, unknown> = {}
    if (formType !== editRel.relation_type) body.relation_type = formType
    if (w !== undefined && w !== (editRel.weight ?? undefined)) body.weight = w
    if ((formNote || "") !== (editRel.note || "")) body.note = formNote || ""
    if (Object.keys(body).length === 0) { setFormError("至少修改一个字段"); return }
    setFormLoading(true)
    try { await api.graph.updateRelation(editRel.relation_uuid, body); setShowEdit(false); setPendingMsg("Change pending review"); await fetchStatus(); if (center) loadMemory(center.uuid); else loadOverview() }
    catch (e) { setFormError(e instanceof Error ? e.message : "操作失败") }
    setFormLoading(false)
  }

  const handleDelete = async () => {
    if (!editRel || !confirm("确认删除？")) return
    setFormLoading(true)
    try { await api.graph.deleteRelation(editRel.relation_uuid); setShowEdit(false); setPendingMsg("Change pending review"); await fetchStatus(); if (center) loadMemory(center.uuid); else loadOverview() }
    catch (e) { setFormError(e instanceof Error ? e.message : "删除失败") }
    setFormLoading(false)
  }

  // ---- Add relation ----
  const handleAdd = async () => {
    if (!center || !addTo.trim()) { setAddError("目标 UUID 不能为空"); return }
    const w = addWeight.trim() ? Number(addWeight) : undefined
    setFormLoading(true)
    try { await api.graph.addRelation({ from_memory_uuid: center.uuid, to_memory_uuid: addTo.trim(), relation_type: addType, ...(w != null ? { weight: w } : {}), ...(addNote.trim() ? { note: addNote.trim() } : {}) }); setShowAdd(false); setPendingMsg("Change pending review"); await fetchStatus(); loadMemory(center.uuid) }
    catch (e) { setAddError(e instanceof Error ? e.message : "操作失败") }
    setFormLoading(false)
  }

  // ---- Suggestions ----
  const loadSuggestions = async () => {
    if (!center) return
    setSuggestLoading(true)
    try { setSuggestions(await api.graph.suggest(center.uuid) || []) } catch { setSuggestions([]) }
    setSuggestLoading(false)
    setShowSuggest(true)
  }

  const handleAddSuggestion = async (s: SuggestedRelation) => {
    setFormLoading(true)
    try { await api.graph.addRelation({ from_memory_uuid: s.from_memory_uuid, to_memory_uuid: s.to_memory_uuid, relation_type: s.relation_type, ...(s.weight != null ? { weight: s.weight } : {}), ...(s.note ? { note: s.note } : {}) }); setPendingMsg("Change pending review"); await fetchStatus(); if (center) loadMemory(center.uuid); setShowSuggest(false) }
    catch (e) { setError(e instanceof Error ? e.message : "操作失败") }
    setFormLoading(false)
  }

  return (
    <div className="absolute inset-0 top-0 flex flex-col">
      {/* ===== Top Toolbar ===== */}
      <div className="h-12 flex items-center gap-3 px-4 border-b bg-card text-xs shrink-0">
        {statusLoading ? null : status ? (
          <div className="flex items-center gap-3">
            {status.dirty ? (
              <Badge variant="outline" className="text-xs border-destructive text-destructive gap-1"><AlertTriangle className="h-3 w-3" />Needs rebuild</Badge>
            ) : (
              <Badge variant="outline" className="text-xs text-muted-foreground gap-1"><CheckCircle className="h-3 w-3" />Clean</Badge>
            )}
            <span className="text-muted-foreground">{status.memory_count} memories</span>
            <span className="text-muted-foreground">{status.relation_count} relations</span>
            <span className="text-muted-foreground hidden sm:inline">{new Date(status.updated_at).toLocaleString("zh-CN")}</span>
            {status.dirty && (
              <Button size="sm" className="h-6 text-xs" disabled={rebuilding} onClick={rebuildGraph}>
                <RefreshCw className={cn("h-3 w-3 mr-1", rebuilding && "animate-spin")} />Rebuild
              </Button>
            )}
          </div>
        ) : null}
        <div className="flex-1" />
        {center && (
          <Button size="sm" variant="ghost" className="h-7 text-xs" onClick={loadOverview}><ArrowLeft className="h-3 w-3 mr-1" />返回总览</Button>
        )}
        <div className="flex items-center gap-2">
          <Input className="h-7 w-52 text-xs" placeholder="Memory UUID" value={searchUuid} onChange={(e) => setSearchUuid(e.target.value)} onKeyDown={(e) => e.key === "Enter" && loadMemory(searchUuid)} />
          <Button size="sm" className="h-7 text-xs" onClick={() => loadMemory(searchUuid)} disabled={loading}><Search className="h-3 w-3 mr-1" />查询</Button>
          {center && (
            <>
              <Button size="sm" variant="outline" className="h-7 text-xs" onClick={() => { setShowAdd(true); setAddTo(""); setAddType("related_to"); setAddWeight(""); setAddNote(""); setAddError("") }}><Plus className="h-3 w-3 mr-1" />Add</Button>
              <Button size="sm" variant="outline" className="h-7 text-xs" onClick={loadSuggestions} disabled={formLoading}>Suggest</Button>
            </>
          )}
        </div>
        {pendingMsg && (
          <span className="text-destructive hidden sm:inline">{pendingMsg}</span>
        )}
      </div>

      {/* ===== Graph Canvas ===== */}
      <div className="relative min-h-0 flex-1 bg-[radial-gradient(circle_at_20%_20%,hsl(var(--accent))_0,transparent_26%),linear-gradient(135deg,hsl(var(--background)),hsl(var(--muted)))]">
        <ReactFlow
          nodes={nodes}
          edges={edges}
          onNodesChange={onNodesChange}
          onEdgesChange={onEdgesChange}
          onNodeClick={onNodeClick}
          onEdgeClick={onEdgeClick}
          nodeTypes={nodeTypes}
          edgeTypes={edgeTypes}
          fitView
          fitViewOptions={{ padding: 0.4 }}
          minZoom={0.15}
          maxZoom={2}
          proOptions={{ hideAttribution: true }}
          className="[&_.react-flow__node]:!bg-transparent"
        >
          <Background color="hsl(var(--muted-foreground))" gap={24} size={0.45} />
          <Controls className="!bg-card !border !rounded-md !shadow-sm [&>button]:!border-border [&>button]:!bg-card [&>button]:!text-foreground [&>button:hover]:!bg-accent" />
        </ReactFlow>

        {error && !loading && (
          <div className="absolute top-3 right-3 max-w-md rounded-md border bg-card px-3 py-2 text-xs text-destructive shadow-sm">
            {error}
          </div>
        )}

        {/* Empty state */}
        {!loading && !error && nodes.length === 0 && (
          <div className="absolute inset-0 flex items-center justify-center pointer-events-none">
            <div className="text-center space-y-2">
              <GitBranch className="h-10 w-10 text-muted-foreground/30 mx-auto" />
              <p className="text-sm text-muted-foreground">
                {!center ? "暂无图谱数据" : "该 Memory 没有邻居关系"}
              </p>
            </div>
          </div>
        )}

        {/* Loading overlay */}
        {loading && (
          <div className="absolute inset-0 flex items-center justify-center bg-background/50">
            <RefreshCw className="h-6 w-6 animate-spin text-muted-foreground" />
          </div>
        )}

        {/* Current memory label */}
        {center && (
          <div className="absolute top-3 left-3 bg-card border rounded-md px-3 py-1.5 text-xs shadow-sm">
            <span className="font-medium">{center.title}</span>
            <span className="text-muted-foreground ml-2">{neighbors.length} neighbors</span>
          </div>
        )}
      </div>

      {/* ===== Edit Relation Dialog ===== */}
      {showEdit && editRel && (
        <div className="fixed inset-0 z-50 flex items-center justify-center bg-black/50" onClick={() => setShowEdit(false)}>
          <div className="bg-card border rounded-lg shadow-lg w-80 p-4 space-y-3" onClick={(e) => e.stopPropagation()}>
            <p className="text-sm font-semibold">编辑关系</p>
            <p className="text-xs text-muted-foreground">{editRel.memory.title_norm}</p>
            <div className="space-y-2">
              <Label className="text-xs">Type</Label>
              <select className="flex h-7 w-full rounded-md border bg-background px-2 text-xs" value={formType} onChange={(e) => setFormType(e.target.value)}>{RELATION_TYPES.map(t => <option key={t} value={t}>{t}</option>)}</select>
              <Label className="text-xs">Weight</Label>
              <Input className="h-7 text-xs" type="number" min={0} max={100} value={formWeight} onChange={(e) => setFormWeight(e.target.value)} />
              <Label className="text-xs">Note</Label>
              <Input className="h-7 text-xs" value={formNote} onChange={(e) => setFormNote(e.target.value)} />
              {formError && <p className="text-xs text-destructive">{formError}</p>}
            </div>
            <div className="flex gap-2">
              <Button size="sm" className="flex-1 text-xs" onClick={handleUpdate} disabled={formLoading}>更新</Button>
              <Button size="sm" variant="destructive" className="text-xs" onClick={handleDelete} disabled={formLoading}><Trash2 className="h-3 w-3" /></Button>
              <Button size="sm" variant="ghost" className="text-xs" onClick={() => setShowEdit(false)}>取消</Button>
            </div>
          </div>
        </div>
      )}

      {/* ===== Add Relation Dialog ===== */}
      {showAdd && (
        <div className="fixed inset-0 z-50 flex items-center justify-center bg-black/50" onClick={() => setShowAdd(false)}>
          <div className="bg-card border rounded-lg shadow-lg w-80 p-4 space-y-3" onClick={(e) => e.stopPropagation()}>
            <p className="text-sm font-semibold">新增关系</p>
            <div className="space-y-2">
              <Label className="text-xs">From</Label>
              <Input className="h-7 text-xs" value={center?.uuid || ""} disabled />
              <Label className="text-xs">To UUID</Label>
              <Input className="h-7 text-xs" value={addTo} onChange={(e) => setAddTo(e.target.value)} placeholder="目标 UUID" />
              <Label className="text-xs">Type</Label>
              <select className="flex h-7 w-full rounded-md border bg-background px-2 text-xs" value={addType} onChange={(e) => setAddType(e.target.value)}>{RELATION_TYPES.map(t => <option key={t} value={t}>{t}</option>)}</select>
              <Label className="text-xs">Weight</Label>
              <Input className="h-7 text-xs" type="number" min={0} max={100} value={addWeight} onChange={(e) => setAddWeight(e.target.value)} />
              <Label className="text-xs">Note</Label>
              <Input className="h-7 text-xs" value={addNote} onChange={(e) => setAddNote(e.target.value)} />
              {addError && <p className="text-xs text-destructive">{addError}</p>}
            </div>
            <div className="flex gap-2">
              <Button size="sm" className="flex-1 text-xs" onClick={handleAdd} disabled={formLoading}>创建</Button>
              <Button size="sm" variant="ghost" className="text-xs" onClick={() => setShowAdd(false)}>取消</Button>
            </div>
          </div>
        </div>
      )}

      {/* ===== Suggestions Dialog ===== */}
      {showSuggest && (
        <div className="fixed inset-0 z-50 flex items-center justify-center bg-black/50" onClick={() => setShowSuggest(false)}>
          <div className="bg-card border rounded-lg shadow-lg w-96 max-h-[70vh] overflow-y-auto p-4 space-y-3" onClick={(e) => e.stopPropagation()}>
            <p className="text-sm font-semibold">Suggested Relations</p>
            {suggestLoading ? <p className="text-xs text-muted-foreground">加载中...</p> : suggestions.length === 0 ? <p className="text-xs text-muted-foreground">暂无候选</p> : (
              <div className="space-y-1">
                {suggestions.map((s, i) => (
                  <div key={i} className="flex items-center gap-2 text-xs p-2 rounded border">
                    <div className="flex-1 min-w-0">
                      <div className="font-medium truncate">{s.candidate.title_norm}</div>
                      <div className="text-muted-foreground">{s.relation_type} · shared keywords: {s.candidate.shared_keywords}</div>
                    </div>
                    <Button size="sm" className="h-6 text-xs shrink-0" onClick={() => handleAddSuggestion(s)} disabled={formLoading}><Plus className="h-3 w-3 mr-1" />Add</Button>
                  </div>
                ))}
              </div>
            )}
          </div>
        </div>
      )}
    </div>
  )
}
