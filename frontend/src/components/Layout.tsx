import { Outlet, Link, useNavigate, useLocation } from "react-router-dom"
import { useAuth } from "@/auth/AuthContext"
import { Button } from "@/components/ui/button"
import { Separator } from "@/components/ui/separator"
import {
  DropdownMenu, DropdownMenuContent, DropdownMenuItem, DropdownMenuTrigger,
} from "@/components/ui/dropdown-menu"
import { FileText, Clock, LogOut, Menu, X, Monitor, Moon, Sun, ChevronDown, GitBranch, ShieldCheck, Trash2, Folder } from "lucide-react"
import { useState, useEffect } from "react"
import { cn } from "@/lib/utils"
import { api } from "@/api/client"
import type { ProjectInfo } from "@/api/client"

const navItems = [
  { to: "/memories", icon: FileText, label: "记忆" },
  { to: "/changes", icon: Clock, label: "待确认" },
  { to: "/changes?filter=trash", icon: Trash2, label: "回收站" },
  { to: "/graph", icon: GitBranch, label: "图谱" },
  { to: "/auth", icon: ShieldCheck, label: "授权" },
]

type Theme = "system" | "light" | "dark"
type MobileSidebarState = "closed" | "nav" | "projects"

function getSystemTheme(): "light" | "dark" {
  if (typeof window === "undefined") return "dark"
  return window.matchMedia("(prefers-color-scheme: dark)").matches ? "dark" : "light"
}

const THEME_KEY = "mem_theme"

export function Layout() {
  const { activeProject, projects, selectProject, logout } = useAuth()
  const navigate = useNavigate()
  const location = useLocation()
  const [mobileSidebarState, setMobileSidebarState] = useState<MobileSidebarState>("closed")
  const [closedMemoryProjectId, setClosedMemoryProjectId] = useState("")
  const [memoryCategoryState, setMemoryCategoryState] = useState<{ projectId: string; categories: string[] }>({ projectId: "", categories: [] })
  const [pendingChangeState, setPendingChangeState] = useState({ projectId: "", count: 0 })
  const [projectOpen, setProjectOpen] = useState(false)
  const [theme, setTheme] = useState<Theme>(() => {
    if (typeof window === "undefined") return "system"
    return (localStorage.getItem(THEME_KEY) as Theme) || "system"
  })

  // 同步主题到 <html>
  useEffect(() => {
    const root = document.documentElement
    const applied = theme === "system" ? getSystemTheme() : theme
    root.classList.toggle("dark", applied === "dark")
    localStorage.setItem(THEME_KEY, theme)
  }, [theme])

  const activeProjectId = activeProject?.project_id || ""
  const memoryOpen = Boolean(activeProjectId) && closedMemoryProjectId !== activeProjectId
  const memoryCategories = memoryCategoryState.projectId === activeProjectId ? memoryCategoryState.categories : []

  useEffect(() => {
    if (!activeProjectId) return
    void api.memories.list()
      .then((data) => setMemoryCategoryState({
        projectId: activeProjectId,
        categories: Array.from(new Set((data || []).map((m) => m.category).filter(Boolean))).sort((a, b) => a.localeCompare(b, "zh-CN")),
      }))
      .catch(() => setMemoryCategoryState({ projectId: activeProjectId, categories: [] }))
  }, [activeProjectId])

  useEffect(() => {
    if (!activeProjectId) return
    let cancelled = false
    void api.changes.list()
      .then((data) => {
        if (!cancelled) setPendingChangeState({ projectId: activeProjectId, count: (data || []).length })
      })
      .catch(() => {
        if (!cancelled) setPendingChangeState({ projectId: activeProjectId, count: 0 })
      })
    return () => {
      cancelled = true
    }
  }, [activeProjectId, location.pathname, location.search])

  // What：监听 ChangesPage 派发的 changes-updated 事件，刷新待确认计数。
  // Why：approve/reject 不改变 URL，Layout 的 useEffect 不会自动重取，需显式通知。
  useEffect(() => {
    const handler = () => {
      if (!activeProjectId) return
      void api.changes.list()
        .then((data) => setPendingChangeState({ projectId: activeProjectId, count: (data || []).length }))
        .catch(() => setPendingChangeState({ projectId: activeProjectId, count: 0 }))
    }
    window.addEventListener("changes-updated", handler)
    return () => window.removeEventListener("changes-updated", handler)
  }, [activeProjectId])

  // 监听系统主题变化
  useEffect(() => {
    if (theme !== "system") return
    const mq = window.matchMedia("(prefers-color-scheme: dark)")
    const handler = () => {
      document.documentElement.classList.toggle("dark", mq.matches)
    }
    mq.addEventListener("change", handler)
    return () => mq.removeEventListener("change", handler)
  }, [theme])

  const handleLogout = () => {
    logout()
    navigate("/login")
  }

  const openMobileSidebar = () => {
    setMobileSidebarState("nav")
  }

  const closeMobileSidebar = () => {
    setMobileSidebarState("closed")
  }

  const handleMobileProjectSelect = (project: ProjectInfo) => {
    selectProject(project)
    closeMobileSidebar()
  }

  const toggleMobileProjects = () => {
    setMobileSidebarState((state) => state === "projects" ? "nav" : state === "nav" ? "projects" : "closed")
  }

  const handleMemoryToggle = () => {
    const nextOpen = !memoryOpen
    setClosedMemoryProjectId(nextOpen ? "" : activeProjectId)
    if (!nextOpen || !activeProjectId || memoryCategories.length > 0) return
    void api.memories.list()
      .then((data) => setMemoryCategoryState({
        projectId: activeProjectId,
        categories: Array.from(new Set((data || []).map((m) => m.category).filter(Boolean))).sort((a, b) => a.localeCompare(b, "zh-CN")),
      }))
      .catch(() => setMemoryCategoryState({ projectId: activeProjectId, categories: [] }))
  }

  const currentPath = `${location.pathname}${location.search}`
  const categoryFilter = new URLSearchParams(location.search).get("category")?.trim() || ""
  const memoriesActive = location.pathname === "/memories"
  const memoryFilter = memoriesActive ? new URLSearchParams(location.search).get("filter") || "" : ""
  const pageTitle = memoriesActive ? (memoryFilter || categoryFilter || "记忆") : currentPath === "/changes?filter=trash" ? "回收站" : currentPath === "/changes" ? "待确认" : currentPath === "/graph" ? "图谱" : currentPath === "/auth" ? "授权" : "Mem"
  // 当前主题图标组件
  const ThemeIcon = theme === "system" ? Monitor : theme === "dark" ? Moon : Sun
  const mobileSidebarOpen = mobileSidebarState !== "closed"
  const mobileProjectOpen = mobileSidebarState === "projects"
  const pendingChangeCount = pendingChangeState.projectId === activeProjectId ? pendingChangeState.count : 0
  const hasPendingChanges = pendingChangeCount > 0
  const memoryCategoryPath = (category: string) => {
    const params = new URLSearchParams(memoriesActive ? location.search : "")
    params.set("category", category)
    const search = params.toString()
    return `/memories${search ? `?${search}` : ""}`
  }

  return (
    <div className="min-h-screen bg-background flex">
      {/* 左侧栏 */}
      <aside className="hidden sm:flex flex-col w-56 border-r bg-card shrink-0">
        {/* Logo + Workspace */}
        <div className="h-12 border-b flex items-center">
          <Link to="/memories" className="h-12 w-12 shrink-0 border-r flex items-center justify-center text-sm font-bold text-foreground hover:bg-accent/50 transition-colors">Mem</Link>
          <DropdownMenu open={projectOpen} onOpenChange={setProjectOpen}>
            <DropdownMenuTrigger asChild>
              <Button variant="ghost" size="sm" className="h-12 min-w-0 flex-1 rounded-none justify-start px-3 text-sm font-medium">
                <span className="truncate">{activeProject?.display_name || "选择项目"}</span>
                <ChevronDown className="h-3 w-3 text-muted-foreground ml-auto shrink-0" />
              </Button>
            </DropdownMenuTrigger>
            <DropdownMenuContent align="start" className="w-48">
              {projects.map((p: ProjectInfo) => (
                <DropdownMenuItem key={p.project_id} onClick={() => selectProject(p)} className="text-sm">
                  {p.display_name}
                  {activeProject?.project_id === p.project_id && <span className="ml-auto text-muted-foreground text-xs">当前</span>}
                </DropdownMenuItem>
              ))}
            </DropdownMenuContent>
          </DropdownMenu>
        </div>
        <nav className="flex-1 px-2 py-3 space-y-1">
          <div className={cn(
            "flex w-full items-center gap-2 rounded-md px-3 py-2 text-sm transition-colors",
            memoriesActive ? "bg-accent text-foreground font-medium" : "text-muted-foreground hover:text-foreground hover:bg-accent/50"
          )}>
            <Link to="/memories" className="flex min-w-0 flex-1 items-center gap-2">
              <FileText className="h-4 w-4" />
              <span className="flex-1 text-left">记忆</span>
            </Link>
            <button type="button" aria-label={memoryOpen ? "收起类别" : "展开类别"} onClick={handleMemoryToggle} className="shrink-0 rounded-sm p-1 hover:bg-accent">
              <ChevronDown className={cn("h-3 w-3 text-muted-foreground transition-transform", memoryOpen && "rotate-180")} />
            </button>
          </div>
          {memoryOpen && (
            <div className="ml-5 border-l pl-2">
              {memoryCategories.map((category) => (
                <Link key={category} to={memoryCategoryPath(category)} className={cn(
                  "flex w-full items-center gap-2 rounded-md px-3 py-1.5 text-sm transition-colors",
                  categoryFilter === category ? "bg-accent text-foreground font-medium" : "text-muted-foreground hover:text-foreground hover:bg-accent/50"
                )}>
                  <Folder className="h-3.5 w-3.5 shrink-0 text-muted-foreground" />
                  <span className="min-w-0 flex-1 truncate text-left">{category}</span>
                </Link>
              ))}
            </div>
          )}
          {navItems.filter((item) => item.to !== "/memories").map((item) => {
            const active = currentPath === item.to
            return (
              <Link key={item.to} to={item.to} className={cn(
                "flex items-center gap-2 px-3 py-2 rounded-md text-sm transition-colors",
                active ? "bg-accent text-foreground font-medium" : "text-muted-foreground hover:text-foreground hover:bg-accent/50"
              )}>
                <item.icon className="h-4 w-4" />
                <span className="min-w-0 flex-1 truncate">{item.label}</span>
                {item.to === "/changes" && hasPendingChanges && (
                  <span className="h-2 w-2 shrink-0 rounded-full bg-orange-500 ring-2 ring-background" title={`${pendingChangeCount} 条待确认记忆`} aria-label={`${pendingChangeCount} 条待确认记忆`} />
                )}
              </Link>
            )
          })}
        </nav>
        <div className="px-2 py-3 border-t space-y-1">
          <button onClick={handleLogout} className="flex items-center gap-2 px-3 py-2 rounded-md text-sm text-muted-foreground hover:text-foreground hover:bg-accent/50 w-full text-left transition-colors">
            <LogOut className="h-4 w-4" />退出
          </button>
        </div>
      </aside>

      {/* 移动端侧栏 */}
      {mobileSidebarOpen && <div className="fixed inset-0 z-40 bg-black/50 sm:hidden" onClick={closeMobileSidebar} />}
      <aside className={cn(
        "fixed inset-y-0 left-0 z-50 w-56 bg-card border-r flex flex-col sm:hidden transition-transform duration-200",
        mobileSidebarOpen ? "translate-x-0" : "-translate-x-full"
      )}>
        <div className="h-12 flex items-center justify-between px-4 border-b">
          <Button variant="ghost" size="sm" aria-label="切换工作区" aria-expanded={mobileProjectOpen} onClick={toggleMobileProjects} className="h-9 min-w-0 flex-1 justify-start px-2 text-sm font-semibold">
            <span className="truncate">{activeProject?.display_name || "选择项目"}</span>
            <ChevronDown className={cn("h-3 w-3 text-muted-foreground ml-auto shrink-0 transition-transform", mobileProjectOpen && "rotate-180")} />
          </Button>
          <Button variant="ghost" size="icon" onClick={closeMobileSidebar}><X className="h-4 w-4" /></Button>
        </div>
        {mobileProjectOpen && (
          <div className="border-b px-2 py-2">
            {projects.map((p: ProjectInfo) => (
              <button key={p.project_id} type="button" onClick={() => handleMobileProjectSelect(p)} className="flex w-full items-center rounded-md px-3 py-2 text-left text-sm text-muted-foreground hover:bg-accent/50 hover:text-foreground">
                <span className="min-w-0 flex-1 truncate">{p.display_name}</span>
                {activeProject?.project_id === p.project_id && <span className="ml-2 shrink-0 text-xs">当前</span>}
              </button>
            ))}
          </div>
        )}
        <nav className="flex-1 px-2 py-3 space-y-1">
          <div className={cn(
            "flex w-full items-center gap-2 rounded-md px-3 py-2 text-sm transition-colors",
            memoriesActive ? "bg-accent text-foreground font-medium" : "text-muted-foreground hover:text-foreground hover:bg-accent/50"
          )}>
            <Link to="/memories" onClick={closeMobileSidebar} className="flex min-w-0 flex-1 items-center gap-2">
              <FileText className="h-4 w-4" />
              <span className="flex-1 text-left">记忆</span>
            </Link>
            <button type="button" aria-label={memoryOpen ? "收起类别" : "展开类别"} onClick={handleMemoryToggle} className="shrink-0 rounded-sm p-1 hover:bg-accent">
              <ChevronDown className={cn("h-3 w-3 text-muted-foreground transition-transform", memoryOpen && "rotate-180")} />
            </button>
          </div>
          {memoryOpen && (
            <div className="ml-5 border-l pl-2">
              {memoryCategories.map((category) => (
                <Link key={category} to={memoryCategoryPath(category)} onClick={closeMobileSidebar} className={cn(
                  "flex w-full items-center gap-2 rounded-md px-3 py-1.5 text-sm transition-colors",
                  categoryFilter === category ? "bg-accent text-foreground font-medium" : "text-muted-foreground hover:text-foreground hover:bg-accent/50"
                )}>
                  <Folder className="h-3.5 w-3.5 shrink-0 text-muted-foreground" />
                  <span className="min-w-0 flex-1 truncate text-left">{category}</span>
                </Link>
              ))}
            </div>
          )}
          {navItems.filter((item) => item.to !== "/memories").map((item) => {
            const active = currentPath === item.to
            return (
              <Link key={item.to} to={item.to} onClick={closeMobileSidebar} className={cn(
                "flex items-center gap-2 px-3 py-2 rounded-md text-sm transition-colors",
                active ? "bg-accent text-foreground font-medium" : "text-muted-foreground hover:text-foreground hover:bg-accent/50"
              )}>
                <item.icon className="h-4 w-4" />
                <span className="min-w-0 flex-1 truncate">{item.label}</span>
                {item.to === "/changes" && hasPendingChanges && (
                  <span className="h-2 w-2 shrink-0 rounded-full bg-orange-500 ring-2 ring-background" title={`${pendingChangeCount} 条待确认记忆`} aria-label={`${pendingChangeCount} 条待确认记忆`} />
                )}
              </Link>
            )
          })}
        </nav>
        <div className="px-2 py-3 border-t space-y-1">
          <button onClick={handleLogout} className="flex items-center gap-2 px-3 py-2 rounded-md text-sm text-muted-foreground hover:text-foreground hover:bg-accent/50 w-full text-left transition-colors">
            <LogOut className="h-4 w-4" />退出
          </button>
        </div>
      </aside>

      {/* 右侧：顶栏 + 内容 */}
      <div className="flex-1 min-w-0 flex flex-col">
        {/* 顶栏 */}
        <header className="h-12 flex items-center px-4 border-b bg-card shrink-0 gap-3">
          {/* 移动汉堡 */}
          <Button variant="ghost" size="icon" className="sm:hidden" onClick={openMobileSidebar}>
            <Menu className="h-4 w-4" />
          </Button>

          {/* 页面标题 */}
          <span className="text-sm font-semibold text-foreground hidden sm:block">{pageTitle}</span>

          <div className="flex-1" />

          {/* 主题切换 */}
          <Separator orientation="vertical" className="h-4 hidden sm:block" />
          <DropdownMenu>
            <DropdownMenuTrigger asChild>
              <Button variant="ghost" size="icon" className="h-7 w-7 ml-auto sm:ml-0">
                <ThemeIcon className="h-4 w-4" />
              </Button>
            </DropdownMenuTrigger>
            <DropdownMenuContent align="end" className="w-36">
              <DropdownMenuItem onClick={() => setTheme("light")} className="text-sm gap-2">
                <Sun className="h-4 w-4" />浅色
              </DropdownMenuItem>
              <DropdownMenuItem onClick={() => setTheme("dark")} className="text-sm gap-2">
                <Moon className="h-4 w-4" />深色
              </DropdownMenuItem>
              <DropdownMenuItem onClick={() => setTheme("system")} className="text-sm gap-2">
                <Monitor className="h-4 w-4" />系统
              </DropdownMenuItem>
            </DropdownMenuContent>
          </DropdownMenu>
        </header>

        <main className="flex-1 relative">
          <Outlet />
        </main>
      </div>
    </div>
  )
}
