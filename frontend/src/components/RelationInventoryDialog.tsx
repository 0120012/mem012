import { useEffect, useState } from "react"
import { api, type RelationInventory } from "@/api/client"
import { Button } from "@/components/ui/button"

export function RelationInventoryDialog({ onClose }: { onClose: () => void }) {
  const [page, setPage] = useState(0)
  const [inventory, setInventory] = useState<RelationInventory | null>(null)
  const [error, setError] = useState("")

  useEffect(() => {
    let current = true
    void api.graph.inventory(page).then(
      (result) => { if (current) setInventory(result) },
      (failure: unknown) => { if (current) setError(failure instanceof Error ? failure.message : "盘点加载失败") },
    )
    return () => { current = false }
  }, [page])

  return (
    <div className="fixed inset-0 z-50 grid place-items-center bg-black/50 p-4" onClick={onClose}>
      <section aria-label="历史关系盘点" role="dialog" aria-modal="true" className="max-h-[85vh] w-full max-w-4xl overflow-auto rounded-lg border bg-card p-4" onClick={(event) => event.stopPropagation()}>
        <header className="flex items-center justify-between gap-3">
          <h2 className="font-semibold">历史关系盘点</h2>
          <Button size="sm" variant="outline" onClick={onClose}>关闭</Button>
        </header>
        <p className="my-3 text-sm text-muted-foreground">优先复核项依据当前类型和备注筛选。备注可编辑，请结合待确认变更或已有操作记录核对来源。</p>
        {error && <p role="alert" className="text-destructive">{error}</p>}
        {!inventory && !error && <p>加载中…</p>}
        {inventory && <>
          <p className="mb-2 text-sm">共 {inventory.total} 条 · 优先复核 {inventory.priority_count} 条</p>
          <div className="overflow-x-auto">
            <table className="w-full text-left text-xs">
              <thead><tr><th>关系 ID</th><th>起点</th><th>终点</th><th>当前类型</th><th>复核状态</th></tr></thead>
              <tbody>
                {inventory.relations.map((relation) => <tr key={relation.relation_uuid} className="border-t align-top">
                  <td className="p-2 font-mono">{relation.relation_uuid}</td>
                  <td className="p-2">{relation.source.title_norm}<div className="font-mono text-muted-foreground">{relation.source.memory_uuid}</div></td>
                  <td className="p-2">{relation.target.title_norm}<div className="font-mono text-muted-foreground">{relation.target.memory_uuid}</div></td>
                  <td className="p-2">{relation.relation_type}{relation.note && <div className="text-muted-foreground">{relation.note}</div>}</td>
                  <td className="p-2">{relation.priority_review ? "优先复核" : "保留"}</td>
                </tr>)}
              </tbody>
            </table>
          </div>
          <div className="mt-3 flex items-center gap-2">
            <Button size="sm" variant="outline" disabled={page === 0} onClick={() => { setError(""); setPage(page - 1) }}>上一页</Button>
            <span className="text-sm">第 {page + 1} 页</span>
            <Button size="sm" variant="outline" disabled={(page + 1) * 50 >= inventory.total} onClick={() => { setError(""); setPage(page + 1) }}>下一页</Button>
          </div>
        </>}
      </section>
    </div>
  )
}
