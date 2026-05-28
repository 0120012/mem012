import { useNavigate } from "react-router-dom"
import { useAuth } from "@/auth/AuthContext"
import { Card, CardContent } from "@/components/ui/card"
import { Button } from "@/components/ui/button"
import { Skeleton } from "@/components/ui/skeleton"
import type { ProjectInfo } from "@/api/client"

export function ProjectsPage() {
  const { projects, isLoading, selectProject } = useAuth()
  const navigate = useNavigate()

  const handleSelect = (project: ProjectInfo) => {
    selectProject(project)
    navigate("/memories")
  }

  return (
    <div className="px-4 py-8 max-w-md mx-auto">
      <h1 className="text-xl font-semibold mb-4 text-foreground">选择项目</h1>
      {isLoading ? (
        <div className="flex flex-col gap-3">
          {Array.from({ length: 3 }).map((_, i) => <Skeleton key={i} className="h-20 w-full rounded-xl" />)}
        </div>
      ) : projects.length === 0 ? (
        <p className="text-muted-foreground text-center py-12">暂无可用项目</p>
      ) : (
        <div className="flex flex-col gap-3">
          {projects.map((p) => (
            <Card key={p.project_id} className="cursor-pointer hover:bg-accent/50 transition-colors min-h-[44px]" onClick={() => handleSelect(p)}>
              <CardContent className="p-4">
                <div className="flex items-center justify-between">
                  <div>
                    <p className="font-medium text-foreground">{p.display_name}</p>
                    <p className="text-sm text-muted-foreground">{p.db_scope}</p>
                  </div>
                  <Button variant="outline" size="sm">进入</Button>
                </div>
              </CardContent>
            </Card>
          ))}
        </div>
      )}
    </div>
  )
}
